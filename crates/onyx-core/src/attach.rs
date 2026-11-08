//! The files a note embeds or links to, set after its words when it is
//! exported or printed, as Craft sets a document's pages after it: a PDF
//! by its pages, a note as it reads, any other file the preview reads as
//! markdown (code, a table, a Word document, a notebook) as that. Each
//! comes once, in the order the note first names it, and the place that
//! names it leads there and back.
//!
//! - Only the note's own references are followed: a note set after it
//!   brings none of its own, though one of its links that names a file
//!   already set here leads there.
//! - A picture is shown in the words already, and a video or a sound has
//!   nothing to set on paper; neither comes after.
//! - Nothing is read for a note that names no file. A PDF is read once
//!   for every layout of the export sheet (`forget` lets it go when the
//!   sheet closes), and as Typst's own bytes, so the next layout with the
//!   same file costs nothing more.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use pulldown_cmark::{Event, LinkType, Parser, Tag};
use typst::foundations::Bytes;

use crate::pdf::Note;
use crate::{html, preview, vault};

/// The most bytes of a PDF set after a note, as the preview reads no more.
pub const PDF_BYTES: u64 = 120_000_000;

/// A file set after the note.
#[derive(Clone)]
pub struct Attached {
    /// Where it is, as `resolve` finds it.
    pub path: PathBuf,
    /// Its file's name, with its extension.
    pub name: String,
    /// What the way back and the place in the words call it: a note by its
    /// name, any other file by its file's name.
    pub title: String,
    pub body: Body,
}

#[derive(Clone)]
pub enum Body {
    /// A PDF's bytes and how many pages it has.
    Pdf { bytes: Bytes, pages: usize },
    /// A note's text.
    Note(String),
    /// What another file reads as, in markdown.
    File(String),
}

impl Attached {
    /// The text set as a note is, for a note or a file the preview reads.
    pub fn text(&self) -> Option<&str> {
        match &self.body {
            Body::Note(t) | Body::File(t) => Some(t),
            Body::Pdf { .. } => None,
        }
    }
}

/// Every place the note names a file, in order: what it names and
/// whether it is embedded (`![..](x)`, `![[x]]`, `<embed>`, `<img>`)
/// rather than linked.
fn references(text: &str, out: &mut Vec<(String, bool)>) {
    // In parts, as the note is read: a line under a picture is markdown
    // (`html::parts`).
    let options = crate::reading::options();
    for ev in html::parts(text, options).into_iter().flat_map(|r| Parser::new_ext(&text[r], options)) {
        match ev {
            Event::Start(Tag::Image { dest_url, .. }) => out.push((dest_url.to_string(), true)),
            Event::Start(Tag::Link { link_type, dest_url, .. }) if !matches!(link_type, LinkType::Email) => out.push((dest_url.to_string(), false)),
            Event::InlineHtml(s) => match html::read_tag(&s) {
                html::Tag::Image { src, .. } => out.push((html::entities(&src), true)),
                // `<a href>`, a card's among them.
                html::Tag::Open { style: html::Style::Link(href), .. } => out.push((href, false)),
                _ => {}
            },
            Event::Html(s) => {
                // A block of HTML reads as the markdown it becomes, as it
                // is set; its lines come one at a time and are read whole.
                if let html::Tag::Image { src, .. } = html::read_tag(s.trim()) {
                    out.push((html::entities(&src), true));
                    continue;
                }
                let reads = html::to_markdown(&s);
                if !reads.markdown.is_empty() {
                    references(&reads.markdown, out);
                }
            }
            _ => {}
        }
    }
}

/// A note's text as the writers read it: a card's line (`html::card_line`)
/// is the file it shows embedded there, `![words](href)`, which each writer
/// sets as a card of its own; a card of a page of the web, its link.
pub fn cards_as_embeds(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("data-card") {
        return std::borrow::Cow::Borrowed(text);
    }
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| match html::card_line(line) {
            Some(c) => {
                let lead = &line[..line.len() - line.trim_start().len()];
                let words = html::entities(&line[c.words]);
                let link = html::link_markup(&c.href, &words);
                if crate::link_card::is_web(&c.href) { format!("{lead}{link}") } else { format!("{lead}!{link}") }
            }
            None => line.to_string(),
        })
        .collect();
    std::borrow::Cow::Owned(lines.join("\n"))
}

/// What a card says of a file under its name: its kind and its size,
/// `PDF · 2.4 MB`, as the editor's cards say it.
pub fn card_meta(path: &Path) -> String {
    let kind = path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
    let bytes = std::fs::metadata(path).map_or(0, |m| m.len());
    [kind, size_words(bytes)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// A file's size as a person reads it.
pub fn size_words(bytes: u64) -> String {
    let b = bytes as f64;
    match bytes {
        0 => String::new(),
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.0} KB", b / 1024.),
        n if n < 1024 * 1024 * 1024 => format!("{:.1} MB", b / 1024. / 1024.),
        _ => format!("{:.1} GB", b / 1024. / 1024. / 1024.),
    }
}

/// The start of a document's words, as a card shows them: its marks at
/// the lines' heads, its fences and front matter left out, up to `most`
/// characters.
pub fn plain_start(text: &str, most: usize) -> String {
    let mut out = String::new();
    let mut lines = text.lines().peekable();
    if lines.peek().is_some_and(|l| l.trim() == "---") {
        lines.next();
        for l in lines.by_ref() {
            if l.trim() == "---" {
                break;
            }
        }
    }
    for line in lines {
        let l = line.trim().trim_start_matches(['#', '>', '-', '*', '+', '|', ' ']).trim();
        if l.is_empty() || l.starts_with("```") || l.starts_with("~~~") {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(l);
        if out.chars().count() >= most {
            break;
        }
    }
    out.chars().take(most).collect()
}

impl Attached {
    /// A few of its words for its card: a note's or a document's start;
    /// none for a PDF, whose first page stands beside them.
    pub fn card_words(&self) -> Option<String> {
        self.text().map(|t| plain_start(t, 240)).filter(|s| !s.is_empty())
    }
}

/// The file a note's reference names, if it is one of the vault's: as
/// `vault::find_file` finds it, else a note by its name alone, as a
/// wikilink names one. The path is the file's own, so two ways of naming
/// it are one file.
pub fn resolve(note: &Note, dest: &str) -> Option<PathBuf> {
    if crate::reading::live_address(dest) {
        return None;
    }
    let found = vault::find_file(note.vault, note.path, dest).or_else(|| {
        let root = note.vault.or_else(|| note.path.parent())?;
        vault::resolve_link(root, note.path, &vault::decoded(dest))
    })?;
    Some(found.canonicalize().unwrap_or(found))
}

/// Every file the note embeds or links to that can be set after it, once
/// each, in the order it is first named.
pub fn gather(note: &Note) -> Vec<Attached> {
    let mut refs = Vec::new();
    references(note.text, &mut refs);
    if refs.is_empty() {
        return Vec::new();
    }
    let own = note.path.canonicalize().unwrap_or_else(|_| note.path.to_path_buf());
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out = Vec::new();
    for (dest, _) in refs {
        let Some(path) = resolve(note, &dest) else { continue };
        if path == own || seen.contains(&path) {
            continue;
        }
        seen.push(path.clone());
        if let Some(a) = read(&path) {
            out.push(a);
        }
    }
    out
}

/// A file as it is set after the note; none when it is a picture, a
/// video or a sound, or reads as nothing.
fn read(path: &Path) -> Option<Attached> {
    if !path.is_file() || matches!(preview::kind(path), preview::Kind::Picture | preview::Kind::Media) {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().to_string();
    let body = if vault::is_pdf(path) {
        let (bytes, pages) = pdf_bytes(path)?;
        Body::Pdf { bytes, pages }
    } else if vault::is_note(path) {
        Body::Note(vault::read(path).ok()?)
    } else {
        match preview::read(path) {
            preview::Preview::Markdown { text, .. } => Body::File(text),
            _ => return None,
        }
    };
    let title = if vault::is_note(path) { vault::title(path) } else { name.clone() };
    Some(Attached { path: path.to_path_buf(), name, title, body })
}

type Held = HashMap<PathBuf, (Option<SystemTime>, u64, Bytes, usize)>;

/// The PDFs read, by path, with the time and size they were read at.
static READ: LazyLock<Mutex<Held>> = LazyLock::new(Default::default);

/// A PDF's bytes and its page count; none when it is too large, locked,
/// or has no page.
fn pdf_bytes(path: &Path) -> Option<(Bytes, usize)> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > PDF_BYTES {
        return None;
    }
    let when = meta.modified().ok();
    let mut held = READ.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, len, bytes, pages)) = held.get(path) {
        if *t == when && *len == meta.len() {
            return Some((bytes.clone(), *pages));
        }
    }
    let bytes = Bytes::new(std::fs::read(path).ok()?);
    let pages = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.clone())).ok().map(|pdf| pdf.pages().len()))).ok().flatten().filter(|n| *n > 0)?;
    held.insert(path.to_path_buf(), (when, meta.len(), bytes.clone(), pages));
    Some((bytes, pages))
}

/// The PDFs read let go, as the export sheet closes.
pub fn forget() {
    READ.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// A PDF's pages as PNG pictures `width` pixels wide, for the writers
/// that cannot hold a PDF's page (Word and the web page): each with its
/// size in pixels. Read once for all its pages. A page that cannot be
/// drawn is left out.
pub fn pages_as_pictures(bytes: &Bytes, width: u32) -> Vec<(Vec<u8>, (u32, u32))> {
    pages_drawn(bytes, width, usize::MAX)
}

/// A PDF's first page as a PNG picture, for its card.
pub fn first_page(bytes: &Bytes, width: u32) -> Option<(Vec<u8>, (u32, u32))> {
    pages_drawn(bytes, width, 1).into_iter().next()
}

/// A PDF's first page as its card shows it: the top of the page, `width`
/// pixels wide and two thirds as tall, as a PNG. Cut here rather than by
/// the card's edge: Typst sets what overflows a box in its middle.
pub fn card_picture(bytes: &Bytes, width: u32) -> Option<(Vec<u8>, (u32, u32))> {
    let (png, (w, h)) = first_page(bytes, width)?;
    let page = image::load_from_memory(&png).ok()?;
    let tall = (w * 2 / 3).clamp(1, h);
    let top = page.crop_imm(0, 0, w, tall);
    let mut out = Vec::new();
    top.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some((out, (w, tall)))
}

fn pages_drawn(bytes: &Bytes, width: u32, most: usize) -> Vec<(Vec<u8>, (u32, u32))> {
    use hayro::vello_cpu::color::palette::css::WHITE;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Ok(pdf) = hayro::hayro_syntax::Pdf::new(std::sync::Arc::new(bytes.clone())) else { return Vec::new() };
        let cache = hayro::RenderCache::new();
        let settings = hayro::hayro_interpret::InterpreterSettings::default();
        let mut out = Vec::new();
        for page in pdf.pages().iter().take(most) {
            let (w, h) = page.render_dimensions();
            if !(w.is_finite() && h.is_finite() && w > 0. && h > 0.) {
                continue;
            }
            let width = width.clamp(1, 4096);
            let scale = width as f32 / w;
            let height = ((h * scale).round() as u32).clamp(1, 16_384);
            let render = hayro::RenderSettings { x_scale: scale, y_scale: scale, width: Some(width as u16), height: Some(height as u16), bg_color: WHITE };
            let pixmap = hayro::render(page, &cache, &settings, &render);
            let (pw, ph) = (pixmap.width() as u32, pixmap.height() as u32);
            let Some(img) = image::RgbaImage::from_raw(pw, ph, pixmap.data_as_u8_slice().to_vec()) else { continue };
            let mut png = Vec::new();
            if image::DynamicImage::ImageRgba8(img).to_rgb8().write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok() {
                out.push((png, (pw, ph)));
            }
        }
        out
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_come_in_order_with_their_kind() {
        let mut out = Vec::new();
        references("A [[Other]] and ![](a.pdf).\n\n<p align=\"center\"><embed src=\"b.pdf\" title=\"B\"></p>\n\n[c](c.py) <img src=\"d.png\">\n", &mut out);
        let got: Vec<(&str, bool)> = out.iter().map(|(d, e)| (d.as_str(), *e)).collect();
        assert_eq!(got, vec![("Other", false), ("a.pdf", true), ("b.pdf", true), ("c.py", false), ("d.png", true)]);
        // A line under a picture is markdown, and a card names its file.
        let mut out = Vec::new();
        references("<img src=\"a.png\">\n![x](x.pdf)\n<a href=\"y.pdf\" data-card=\"small\">y</a>\n", &mut out);
        let got: Vec<&str> = out.iter().map(|(d, _)| d.as_str()).collect();
        assert_eq!(got, vec!["a.png", "x.pdf", "y.pdf"]);
    }

    #[test]
    fn a_card_is_set_as_its_file_embedded() {
        let text = "Before\n  <a href=\"x y.pdf\" data-card=\"large\">Plan</a>\n<a href=\"https://a.b\" data-card=\"small\">Site</a>\n";
        assert_eq!(cards_as_embeds(text), "Before\n  ![Plan](<x y.pdf>)\n[Site](https://a.b)\n");
        assert_eq!(plain_start("---\nt: x\n---\n# Head\n\n```py\n- one\n", 100), "Head one");
        assert_eq!(size_words(2_500_000), "2.4 MB");
    }
}
