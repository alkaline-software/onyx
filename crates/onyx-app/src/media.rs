//! A video or a sound played by the system's own player: a web view
//! (WKWebView on a Mac, WebView2 on Windows) set in an asset's page
//! as a child of the window, holding a page of ours with a `<video>` or
//! an `<audio>` on the page's ground. After Emaki's `html_web.rs`.
//!
//! The view is the system's, not gpui's, so it is drawn over whatever
//! gpui draws where it stands; it is put inside the page with a margin,
//! clear of the card's round corners. Its file comes through a scheme of
//! ours a range at a time, as a player asks, so a long video is never
//! read whole and seeking works; nothing else on the disk is handed out.
//!
//! Linux has no web view that can be set in a gpui window, and there
//! `open` gives none.

use std::path::Path;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::path::PathBuf;

use gpui::{Bounds, Pixels, Window};

/// The scheme the file is asked for under.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const SCHEME: &str = "onyxmedia";

/// Where the file is, as the web view is told it. WebView2 has no
/// schemes of its own making and is given them as hosts.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn url_of(file: &Path) -> String {
    let mut path = String::new();
    for part in file.to_string_lossy().replace('\\', "/").split('/') {
        if !path.is_empty() || part.is_empty() {
            path.push('/');
        }
        for b in part.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => path.push(b as char),
                _ => path.push_str(&format!("%{b:02X}")),
            }
        }
    }
    let path = path.trim_start_matches('/');
    if cfg!(target_os = "windows") { format!("http://{SCHEME}.localhost/{path}") } else { format!("{SCHEME}://localhost/{path}") }
}

/// The file a request's path names: its escapes undone, and on Windows
/// the slash before the drive taken off.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn file_of(path: &str) -> PathBuf {
    let bytes = path.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push(hi * 16 + lo);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    let text = String::from_utf8_lossy(&out).into_owned();
    if cfg!(target_os = "windows") { PathBuf::from(text.trim_start_matches('/')) } else { PathBuf::from(text) }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn mime_of(file: &Path) -> &'static str {
    match file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "aif" | "aiff" => "audio/aiff",
        _ => "application/octet-stream",
    }
}

/// The page that holds the player: the file in the middle, as large as
/// the page lets it be, on the page's ground (as hex). A video shows
/// its first frame before it plays: WebKit paints none for a video only
/// its metadata is read of.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn player_page(src: &str, video: bool, ground: &str) -> Vec<u8> {
    let ground: String = ground.chars().filter(|c| c.is_ascii_hexdigit()).take(8).collect();
    let player = if video { format!(r#"<video src="{src}#t=0.001" controls playsinline preload="auto"></video>"#) } else { format!(r#"<audio src="{src}" controls preload="metadata"></audio>"#) };
    format!(r#"<!doctype html><html><head><meta charset="utf-8"><style>html,body{{margin:0;height:100%;background:#{ground};}}body{{display:flex;align-items:center;justify-content:center;}}video{{max-width:100%;max-height:100%;border-radius:8px;}}audio{{width:min(520px,88%);}}</style></head><body>{player}</body></html>"#).into_bytes()
}

/// The bytes a `Range` header asks for (`bytes=a-b`, `bytes=a-`,
/// `bytes=-n`), held to the file's length; None for the whole file.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn range_of(header: Option<&str>, len: u64) -> Option<(u64, u64)> {
    /// The most one answer holds of a range left open at its end: a
    /// player asks again for what follows.
    const STEP: u64 = 4 << 20;
    let spec = header?.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let (from, to) = match (a.trim().parse::<u64>().ok(), b.trim().parse::<u64>().ok()) {
        (Some(a), Some(b)) => (a, b.min(len.saturating_sub(1))),
        (Some(a), None) => (a, (a + STEP - 1).min(len.saturating_sub(1))),
        (None, Some(n)) => (len.saturating_sub(n), len.saturating_sub(1)),
        (None, None) => return None,
    };
    (from <= to && from < len).then_some((from, to))
}

/// The web view with the player in it.
pub struct Player {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    view: std::rc::Rc<wry::WebView>,
    placed: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    shown: bool,
}

impl Player {
    /// A player in `window` for `file`, which hands out that one file and
    /// no other. None where the system has no web view to give.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn open(window: &Window, file: &Path, video: bool, ground: &str) -> Option<Player> {
        use std::borrow::Cow;
        use std::io::{Read as _, Seek as _};
        use wry::http::Response;
        let only = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        let answer = |status: u16, kind: &str, body: Vec<u8>| Response::builder().status(status).header("Content-Type", kind).header("Cache-Control", "no-store").body(Cow::Owned(body)).unwrap_or_else(|_| Response::new(Cow::Owned(Vec::new())));
        let page = format!("{}?play={}&ground={ground}", url_of(file), if video { "video" } else { "audio" });
        let view = wry::WebViewBuilder::new()
            .with_custom_protocol(SCHEME.to_string(), move |_, request| {
                let asked = file_of(request.uri().path());
                let Some(real) = asked.canonicalize().ok().filter(|real| *real == only) else { return answer(404, "text/plain", Vec::new()) };
                let query = request.uri().query().unwrap_or("");
                let field = |name: &str| query.split('&').find_map(|kv| kv.strip_prefix(name).and_then(|v| v.strip_prefix('=')));
                if let Some(kind) = field("play") {
                    return answer(200, "text/html; charset=utf-8", player_page(&url_of(&asked), kind == "video", field("ground").unwrap_or("000000")));
                }
                let Ok(mut f) = std::fs::File::open(&real) else { return answer(404, "text/plain", Vec::new()) };
                let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                let range = request.headers().get("Range").and_then(|v| v.to_str().ok());
                match range_of(range, len) {
                    Some((from, to)) => {
                        let mut bytes = vec![0; (to - from + 1) as usize];
                        if f.seek(std::io::SeekFrom::Start(from)).is_err() || f.read_exact(&mut bytes).is_err() {
                            return answer(416, "text/plain", Vec::new());
                        }
                        Response::builder()
                            .status(206)
                            .header("Content-Type", mime_of(&real))
                            .header("Accept-Ranges", "bytes")
                            .header("Content-Range", format!("bytes {from}-{to}/{len}"))
                            .header("Content-Length", bytes.len().to_string())
                            .header("Cache-Control", "no-store")
                            .body(Cow::Owned(bytes))
                            .unwrap_or_else(|_| Response::new(Cow::Owned(Vec::new())))
                    }
                    None => {
                        let mut bytes = Vec::new();
                        match f.read_to_end(&mut bytes) {
                            Ok(_) => answer(200, mime_of(&real), bytes),
                            Err(_) => answer(404, "text/plain", Vec::new()),
                        }
                    }
                }
            })
            // The player's page is the only one it shows.
            .with_navigation_handler(|url| url.starts_with(SCHEME) || url.starts_with("http://onyxmedia.localhost/") || url.starts_with("about:") || url.starts_with("blob:"))
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_focused(false)
            .with_visible(false)
            .with_url(page)
            .build_as_child(window)
            .ok()?;
        Some(Player { view: std::rc::Rc::new(view), placed: Default::default(), shown: false })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn open(_: &Window, _: &Path, _: bool, _: &str) -> Option<Player> {
        None
    }

    /// What puts it where the page has its room, to be called as the
    /// page is laid out: in that same frame, so it does not trail it.
    pub fn placer(&self) -> impl Fn(Bounds<Pixels>) + 'static {
        let placed = self.placed.clone();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let view = self.view.clone();
        move |at| {
            if placed.replace(Some(at)) == Some(at) {
                return;
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                use wry::dpi::{LogicalPosition, LogicalSize};
                let _ = view.set_bounds(wry::Rect {
                    position: LogicalPosition::new(f32::from(at.origin.x) as f64, f32::from(at.origin.y) as f64).into(),
                    size: LogicalSize::new(f32::from(at.size.width).max(1.) as f64, f32::from(at.size.height).max(1.) as f64).into(),
                });
            }
        }
    }

    pub fn is_placed(&self) -> bool {
        self.placed.get().is_some_and(|at| at.size.width > gpui::px(2.) && at.size.height > gpui::px(2.))
    }

    /// Shown, or put out of sight while the window draws over it.
    pub fn show(&mut self, shown: bool) {
        if self.shown != shown {
            self.shown = shown;
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let _ = self.view.set_visible(shown);
        }
    }

    /// How far the player has read its file: its ready state, its
    /// picture's size, its length and any error, on stderr. For probes.
    pub fn print_state(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let _ = self.view.evaluate_script_with_callback(
            "(() => { const m = document.querySelector('video, audio'); return m ? [m.readyState, m.videoWidth || 0, m.videoHeight || 0, m.duration, m.error ? m.error.code : 0].join(',') : 'no player'; })()",
            |answer| eprintln!("media {answer}"),
        );
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_what_was_asked_held_to_the_file() {
        assert_eq!(range_of(Some("bytes=0-99"), 1000), Some((0, 99)));
        assert_eq!(range_of(Some("bytes=900-2000"), 1000), Some((900, 999)));
        assert_eq!(range_of(Some("bytes=-100"), 1000), Some((900, 999)));
        assert_eq!(range_of(Some("bytes=0-"), 10 << 20), Some((0, (4 << 20) - 1)));
        assert_eq!(range_of(Some("bytes=1000-"), 1000), None);
        assert_eq!(range_of(None, 1000), None);
    }

    #[test]
    fn a_path_goes_to_the_view_and_back_the_same() {
        let file = Path::new("/Users/a/Vault/.assets/旅行 video (1).mp4");
        let url = url_of(file);
        let path = url.split("localhost").nth(1).unwrap();
        assert_eq!(file_of(path), file);
    }
}
