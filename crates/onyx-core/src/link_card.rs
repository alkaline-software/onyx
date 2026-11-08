//! What a web link's card shows (`docs/editor.md`, Cards): the page's
//! title, a few words of it, its site, its icon and its picture, read
//! from the head of the page as Craft's rich links read it (Open Graph,
//! then the page's own `<title>` and `description`). Read once over the
//! network and kept in the cache (`paths::cache_dir`), never in the note:
//! the note holds the link alone, and a card with no network shows the
//! link's words and its site.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::html;
use crate::paths;

/// How long a card's reading is kept before it is read again.
const KEEP: Duration = Duration::from_secs(7 * 24 * 3600);
/// The most of a page read for its head, of a picture, of an icon.
const PAGE_MOST: u64 = 1 << 20;
const PICTURE_MOST: u64 = 6 << 20;
const ICON_MOST: u64 = 1 << 19;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct LinkCard {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub site: Option<String>,
    /// The icon and the picture, as files of the cache.
    pub icon: Option<PathBuf>,
    pub image: Option<PathBuf>,
    /// When it was read, in seconds since 1970.
    pub read: u64,
}

impl LinkCard {
    /// Read long enough ago to be read again.
    pub fn stale(&self) -> bool {
        now().saturating_sub(self.read) > KEEP.as_secs()
    }
}

/// What a page's head says of it, before anything is fetched for it.
#[derive(Debug, Default, PartialEq)]
pub struct Head {
    pub title: Option<String>,
    pub description: Option<String>,
    pub site: Option<String>,
    pub icon: Option<String>,
    pub image: Option<String>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn folder() -> PathBuf {
    paths::cache_dir().join("links")
}

fn key(url: &str) -> String {
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// The site a link is on, as a card names it: its host without `www.`.
pub fn host(link: &str) -> String {
    url::Url::parse(link).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_else(|| link.to_string())
}

/// Whether a link leads to a page on the web.
pub fn is_web(link: &str) -> bool {
    let l = link.trim_start().to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("http://")
}

/// The card kept for a link, read or not long ago.
pub fn cached(link: &str) -> Option<LinkCard> {
    let v = paths::read_json(&folder().join(format!("{}.json", key(link))))?;
    serde_json::from_value(v).ok()
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(8)
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(12))
        .user_agent(&format!("Mozilla/5.0 (compatible; Onyx/{}; link preview)", env!("CARGO_PKG_VERSION")))
        .build()
}

fn get(agent: &ureq::Agent, link: &str, most: u64) -> Result<(String, Vec<u8>), String> {
    let response = agent.get(link).call().map_err(|e| e.to_string())?;
    let at = response.get_url().to_string();
    let mut bytes = Vec::new();
    response.into_reader().take(most).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok((at, bytes))
}

/// A link's card read from its page over the network, its icon and
/// picture fetched beside it, and kept. What could not be read stays
/// out; a page that cannot be reached at all is an error.
pub fn fetch(link: &str) -> Result<LinkCard, String> {
    let agent = agent();
    let (at, bytes) = get(&agent, link, PAGE_MOST)?;
    let base = url::Url::parse(&at).map_err(|e| e.to_string())?;
    let head = read_head(&String::from_utf8_lossy(&bytes), &base);
    let dir = folder();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = key(link);
    let keep = |from: &Option<String>, what: &str, most: u64| -> Option<PathBuf> {
        let (_, bytes) = get(&agent, from.as_deref()?, most).ok()?;
        let format = image::guess_format(&bytes).ok()?;
        let ext = format.extensions_str().first().copied().unwrap_or("img");
        let file = dir.join(format!("{name}-{what}.{ext}"));
        paths::write_atomic(&file, &bytes).ok()?;
        Some(file)
    };
    let image = keep(&head.image, "image", PICTURE_MOST);
    let icon = keep(&head.icon, "icon", ICON_MOST).or_else(|| keep(&base.join("/favicon.ico").ok().map(|u| u.to_string()), "icon", ICON_MOST));
    let card = LinkCard { url: link.to_string(), title: head.title, description: head.description, site: head.site, icon, image, read: now() };
    if let Ok(v) = serde_json::to_value(&card) {
        let _ = paths::write_json(&dir.join(format!("{name}.json")), &v);
    }
    Ok(card)
}

/// What a page's head says of it: Open Graph's words first, then the
/// page's own, and its icon, the largest it names that a card can draw.
pub fn read_head(page: &str, base: &url::Url) -> Head {
    let lower = page.to_ascii_lowercase();
    let end = lower.find("</head").or_else(|| lower.find("<body")).unwrap_or(page.len());
    let page = &page[..end];
    let mut head = Head::default();
    let (mut og_title, mut og_desc, mut og_image, mut title) = (None, None, None, None);
    let (mut desc, mut tw_image) = (None, None);
    let mut icons: Vec<(i32, String)> = Vec::new();
    let mut i = 0;
    while let Some(lt) = page[i..].find('<') {
        let at = i + lt;
        let Some(gt) = page[at..].find('>') else { break };
        let tag = &page[at + 1..at + gt];
        i = at + gt + 1;
        let (name, from) = html::name_of(tag);
        let list = html::attrs(tag, from);
        let get = |n: &str| html::attr(tag, &list, n).map(html::entities).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        match name.as_str() {
            "title" if title.is_none() => {
                let close = page[i..].to_ascii_lowercase().find("</title").map_or(page.len(), |e| i + e);
                let words = html::entities(&page[i..close]).split_whitespace().collect::<Vec<_>>().join(" ");
                title = Some(words).filter(|w| !w.is_empty());
                i = close;
            }
            "meta" => {
                let what = get("property").or_else(|| get("name")).unwrap_or_default().to_ascii_lowercase();
                let Some(content) = get("content") else { continue };
                match what.as_str() {
                    "og:title" => og_title = og_title.or(Some(content)),
                    "og:description" => og_desc = og_desc.or(Some(content)),
                    "description" | "twitter:description" => desc = desc.or(Some(content)),
                    "og:image" | "og:image:url" | "og:image:secure_url" => og_image = og_image.or(Some(content)),
                    "twitter:image" | "twitter:image:src" => tw_image = tw_image.or(Some(content)),
                    "og:site_name" => head.site = head.site.or(Some(content)),
                    _ => {}
                }
            }
            "link" => {
                let rel = get("rel").unwrap_or_default().to_ascii_lowercase();
                let Some(href) = get("href") else { continue };
                if !rel.split_whitespace().any(|r| r == "icon" || r == "apple-touch-icon" || r == "apple-touch-icon-precomposed") || href.to_ascii_lowercase().ends_with(".svg") {
                    continue;
                }
                let size = get("sizes").and_then(|s| s.split(['x', 'X']).next().and_then(|n| n.parse::<i32>().ok())).unwrap_or(if rel.contains("apple") { 180 } else { 32 });
                // A card draws its icon small, at twice its points: the
                // nearest to 64 pixels, one too small for that last.
                icons.push((if size >= 48 { (size - 64).abs() } else { 100 + 64 - size }, href));
            }
            _ => {}
        }
    }
    let join = |s: Option<String>| s.and_then(|s| base.join(&s).ok()).map(|u| u.to_string());
    head.title = og_title.or(title);
    head.description = og_desc.or(desc);
    head.image = join(og_image.or(tw_image));
    icons.sort_by_key(|(d, _)| *d);
    head.icon = join(icons.into_iter().next().map(|(_, h)| h));
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_says_what_a_card_shows() {
        let base = url::Url::parse("https://www.craft.do/blog/post").unwrap();
        let page = r#"<html><head><title> Craft &amp; Blog </title>
            <meta name="description" content="Plain words">
            <meta property="og:description" content="Open Graph words">
            <meta property="og:image" content="/cover.png">
            <meta property="og:site_name" content="Craft">
            <link rel="icon" href="favicon-16.png" sizes="16x16">
            <link rel="apple-touch-icon" href="/touch.png">
            <link rel="icon" href="/icon.svg">
            </head><body><meta property="og:title" content="Not this"></body></html>"#;
        let head = read_head(page, &base);
        assert_eq!(head.title.as_deref(), Some("Craft & Blog"));
        assert_eq!(head.description.as_deref(), Some("Open Graph words"));
        assert_eq!(head.site.as_deref(), Some("Craft"));
        assert_eq!(head.image.as_deref(), Some("https://www.craft.do/cover.png"));
        assert_eq!(head.icon.as_deref(), Some("https://www.craft.do/touch.png"));
        assert_eq!(host("https://www.craft.do/blog"), "craft.do");
        assert!(is_web("https://x.y") && !is_web("x.pdf"));
    }
}
