//! A note set as a PDF, by Typst compiled into the app, so nothing else
//! has to be installed. pulldown-cmark reads the note with the options
//! the editor reads it with, `Walk` writes it as Typst's markup after
//! `pdf/template.typ`, and `lay_out` gives a `Laid` document: the export
//! sheet's preview draws its pages (`Laid::draw`) and the file is written
//! from the same document (`Laid::pdf`), so what shows is what is made.
//!
//! - A note cannot run Typst. Its words go in as strings, its math through
//!   MiTeX, and the world hands out the template and the pictures the
//!   note names, nothing else, and has no network.
//! - One bad part does not lose the note. A formula Typst refuses is
//!   printed as code and a picture it cannot read is its alt text: the
//!   whole is tried first, and only when that fails is each part tried
//!   alone (`Bad`), so a note with hundreds of formulas costs nothing.
//! - A line break in a note is a line break here, as the editor shows it.
//! - A panic in Typst stays on `lay_out`'s thread and comes back as an
//!   error, so it cannot take notes with changes down with it.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

use crate::config::PdfConfig;
use crate::html::{self, Across, Rgb, Style};
use crate::rhythm;

const TEMPLATE: &str = include_str!("pdf/template.typ");
const THEME: &[u8] = include_bytes!("pdf/onyx.tmTheme");
const SCOPE: &[(&str, &str)] = &[("/mitex/mod.typ", include_str!("pdf/mitex/mod.typ")), ("/mitex/prelude.typ", include_str!("pdf/mitex/prelude.typ")), ("/mitex/latex/standard.typ", include_str!("pdf/mitex/latex/standard.typ"))];
/// The face of math. No note's words are set in it.
const MATH_FACE: &[u8] = include_bytes!("pdf/NewCMMath-Regular.otf");

/// The stack Typst works on: deep lists and quotes go deep in it.
const STACK: usize = 32 << 20;

/// The papers on offer, each with its width and height in points.
pub const PAPERS: &[(&str, f64, f64)] = &[("a3", 841.89, 1190.55), ("a4", 595.28, 841.89), ("a5", 419.53, 595.28), ("letter", 612., 792.), ("legal", 612., 1008.), ("tabloid", 792., 1224.)];

/// Where Letter is what a printer holds.
const LETTER_REGIONS: &[&str] = &["US", "CA", "MX", "PR", "PH", "CL", "CO", "CR", "DO", "GT", "PA", "PE", "SV", "VE"];

/// The paper a locale (`en-US`, `zh-Hans-CN`) prints on.
pub fn paper_of_locale(locale: &str) -> &'static str {
    let letter = locale.split(['-', '_']).skip(1).any(|part| LETTER_REGIONS.iter().any(|r| r.eq_ignore_ascii_case(part)));
    if letter { "letter" } else { "a4" }
}

/// The ranges of a run of text that are `==highlighted==`, marks and
/// all, as the editor reads them: the parser does not know them.
pub fn highlights(s: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = s[from..].find("==").map(|i| i + from) {
        let after = open + 2;
        let Some(close) = s[after..].find("==").map(|i| i + after) else { break };
        if close > after && !s[after..close].starts_with(' ') && !s[after..close].ends_with(' ') {
            out.push(open..close + 2);
        }
        from = close + 2;
    }
    out
}

/// The note to set. The text is what the editor holds, changes and all.
pub struct Note<'a> {
    pub text: &'a str,
    pub title: &'a str,
    pub path: &'a Path,
    pub vault: Option<&'a Path>,
}

/// The app's part of the look: faces by family, in the order they are
/// tried, the system's faces for what those lack, and the accent's shades.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub words: Vec<String>,
    pub code: Vec<String>,
    pub fallbacks: Vec<String>,
    pub accent: String,
    pub link: String,
}

static CARRIED: OnceLock<&'static [&'static [u8]]> = OnceLock::new();

/// The faces the app carries. Called once at start; they are read when
/// the first note is set.
pub fn carry(faces: &'static [&'static [u8]]) {
    let _ = CARRIED.set(faces);
}

fn carried() -> &'static [Font] {
    static FONTS: OnceLock<Vec<Font>> = OnceLock::new();
    FONTS.get_or_init(|| CARRIED.get().copied().unwrap_or(&[]).iter().copied().chain([MATH_FACE]).flat_map(|b| Font::iter(Bytes::new(b))).collect())
}

/// The machine's fonts. Listing them takes a moment and most notes never
/// ask, since the carried faces cover them.
fn machine() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

/// Every face of a family on the machine.
fn machine_faces(family: &str) -> Vec<Font> {
    static READ: LazyLock<Mutex<HashMap<PathBuf, Bytes>>> = LazyLock::new(Default::default);
    let mut out = Vec::new();
    for face in machine().faces().filter(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(family))) {
        let fontdb::Source::File(path) = &face.source else { continue };
        let mut read = READ.lock().unwrap_or_else(|e| e.into_inner());
        if !read.contains_key(path) {
            let Ok(bytes) = std::fs::read(path) else { continue };
            read.insert(path.clone(), Bytes::new(bytes));
        }
        out.extend(read.get(path).and_then(|b| Font::new(b.clone(), face.index)));
    }
    out
}

/// The fonts the note needs, and the families to try in order: the
/// chosen ones, then a system's fallback only if the note holds a
/// character the chosen faces lack, since one such file can run to
/// hundreds of megabytes.
fn gather(text: &str, look: &Look) -> (Vec<Font>, Vec<String>) {
    let same = |f: &Font, family: &str| f.info().family.eq_ignore_ascii_case(family);
    let mut fonts = carried().to_vec();
    for family in look.words.iter().chain(&look.code) {
        if !fonts.iter().any(|f| same(f, family)) {
            fonts.extend(machine_faces(family));
        }
    }
    let covered = |fonts: &[Font], families: &[String], c: char| fonts.iter().any(|f| families.iter().any(|n| same(f, n)) && f.info().coverage.contains(c as u32));
    let mut chain: Vec<String> = Vec::new();
    let all: Vec<String> = look.words.iter().chain(&look.code).cloned().collect();
    let mut lacking: HashSet<char> = text.chars().filter(|c| !c.is_ascii() && !c.is_whitespace() && !c.is_control() && !matches!(*c, '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')).filter(|c| !covered(&fonts, &all, *c)).collect();
    for family in &look.fallbacks {
        if lacking.is_empty() {
            break;
        }
        let faces = machine_faces(family);
        if faces.is_empty() {
            continue;
        }
        lacking.retain(|c| !faces.iter().any(|f| f.info().coverage.contains(*c as u32)));
        fonts.extend(faces);
        chain.push(family.clone());
    }
    (fonts, chain)
}

/// A place Typst works in: the template's files, the note's pictures and
/// the fonts, and nothing of the disk.
struct Place {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    main: Source,
    sources: HashMap<FileId, Source>,
    files: HashMap<FileId, Bytes>,
}

fn file_id(path: &str) -> Option<FileId> {
    Some(RootedPath::new(VirtualRoot::Project, VirtualPath::new(path).ok()?).intern())
}

fn library() -> &'static LazyHash<Library> {
    static LIBRARY: LazyLock<LazyHash<Library>> = LazyLock::new(|| LazyHash::new(Library::builder().build()));
    &LIBRARY
}

impl World for Place {
    fn library(&self) -> &LazyHash<Library> {
        library()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            return Ok(self.main.clone());
        }
        self.sources.get(&id).cloned().ok_or_else(|| FileError::NotFound(id.vpath().get_without_slash().into()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(bytes) = self.files.get(&id) {
            return Ok(bytes.clone());
        }
        self.source(id).map(|s| Bytes::from_string(s.text().to_string()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}

impl Place {
    fn with_main(&mut self, text: String) {
        self.main = Source::new(self.main.id(), text);
    }

    fn take_pictures(&mut self, walked: &Walked) -> Result<(), String> {
        for (_, at, bytes) in &walked.pictures {
            self.files.insert(file_id(at).ok_or("no picture file")?, bytes.clone());
        }
        Ok(())
    }

    fn compile(&self) -> Result<PagedDocument, String> {
        typst::compile::<PagedDocument>(self).output.map_err(|errors| errors.first().map_or_else(|| "Typst refused it".to_string(), |e| e.message.to_string()))
    }
}

/// `s` as a Typst string.
fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A run of words as markup. Every `#` piece ends in `;`, so what comes
/// next can never be read as part of it.
fn words(s: &str) -> String {
    let piece = |s: &str| if s.is_empty() { String::new() } else { format!("#{};", quoted(s)) };
    let mut out = String::new();
    let mut at = 0;
    for r in highlights(s) {
        out += &piece(&s[at..r.start]);
        out += &format!("#mark[{}];", piece(&s[r.start + 2..r.end - 2]));
        at = r.end;
    }
    out + &piece(&s[at..])
}

/// MiTeX writes an escaped sign with a space after it, which in `\text`
/// is a space; in math a space is nothing.
fn tighten(typ: &str) -> String {
    let mut out = String::with_capacity(typ.len());
    let mut chars = typ.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                chars.next();
                out.push(next);
                if matches!(next, '_' | '&' | '%' | '#' | '$') && chars.peek() == Some(&' ') {
                    chars.next();
                }
            }
        }
    }
    out
}

/// A picture's file: beside the note, from the vault's top, or for a bare
/// name anywhere in the vault, the shortest path first, as a wikilink is.
pub fn find_picture(src: &str, note: &Note) -> Option<PathBuf> {
    if src.contains("://") {
        return None;
    }
    let name = decoded(src.split(['#', '?']).next().unwrap_or(src));
    let ext = Path::new(&name).extension()?.to_string_lossy().to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg") {
        return None;
    }
    let here = note.path.parent().map(|p| p.join(&name));
    let top = note.vault.map(|v| v.join(&name));
    if let Some(found) = here.into_iter().chain(top).find(|p| p.is_file()) {
        return Some(found);
    }
    let vault = note.vault?;
    let bare = Path::new(&name).file_name()?.to_os_string();
    let mut found = Vec::new();
    let mut folders = vec![vault.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                folders.push(path);
            } else if entry.file_name() == bare {
                found.push(path);
            }
        }
    }
    found.sort_by_key(|p| p.components().count());
    found.into_iter().next()
}

/// `%20` and its like, in a link's address.
fn decoded(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%').then(|| s.get(i + 1..i + 3)).flatten().and_then(|h| u8::from_str_radix(h, 16).ok());
        if let Some(b) = hex {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// The width a picture's words ask for, as Obsidian writes it: `alt|300`,
/// or `300` alone.
pub fn asked_width(words: &str) -> Option<u32> {
    let last = words.rsplit('|').next()?.trim();
    (!last.is_empty() && last.bytes().all(|b| b.is_ascii_digit())).then(|| last.parse().ok()).flatten().filter(|w| *w > 0)
}

/// An address a PDF's reader can follow: one with a scheme.
fn live_address(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.len() > 1 && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
}

/// The parts Typst refused, found when the whole failed.
#[derive(Default)]
struct Bad {
    formulas: HashSet<String>,
    pictures: HashSet<PathBuf>,
}

enum Frame {
    Root,
    Plain,
    Para,
    Heading(u8),
    Emph,
    Strong,
    Strike,
    Quote,
    List { start: Option<u64>, items: Vec<String>, tasks: bool, loose: bool },
    /// A task's mark, and where its own line ends and any list under it
    /// begins, since only the line is struck when it is done.
    Item { task: Option<bool>, line_end: Option<usize> },
    Table { aligns: Vec<Alignment>, cells: Vec<String>, shares: Option<Vec<f32>>, columns: usize, at: usize, head: usize },
    Cell,
    Link(Option<String>),
    Image(String),
    Code(String),
    Skip,
    /// A block of HTML, gathered whole.
    Html,
    /// A tag among the words, waiting for its closing one.
    Tag(String, Style),
}

/// One writing of a note as markup, and what it used.
#[derive(Default)]
struct Walked {
    body: String,
    formulas: Vec<String>,
    pictures: Vec<(PathBuf, String, Bytes)>,
}

struct Walk<'a> {
    note: &'a Note<'a>,
    breaks: bool,
    bad: &'a Bad,
    /// Each footnote's body by its label, and which have been set, so a
    /// second reference points at the first.
    notes: HashMap<String, String>,
    noted: HashSet<String>,
    out: Walked,
    stack: Vec<(Frame, String)>,
    first_block: bool,
    /// The widths a person set, a table's at a time in the note's order,
    /// and whether this is the words of a block of HTML, which has none.
    shares: std::collections::VecDeque<Option<Vec<f32>>>,
    nested: bool,
}

fn level_of(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

impl<'a> Walk<'a> {
    fn buf(&mut self) -> &mut String {
        &mut self.stack.last_mut().expect("the root stays").1
    }

    /// Whether the words here are kept as they are: a code block's, an
    /// alt text, what is left out.
    fn verbatim(&self) -> bool {
        self.stack.iter().any(|(f, _)| matches!(f, Frame::Code(_) | Frame::Image(_) | Frame::Skip))
    }

    fn formula(&mut self, src: &str, block: bool) -> String {
        if !self.out.formulas.iter().any(|f| f == src) {
            self.out.formulas.push(src.to_string());
        }
        match (!self.bad.formulas.contains(src)).then(|| mitex::convert_math(src, None).ok()).flatten() {
            Some(typ) => format!("#tex({}, {block});", quoted(&tighten(&typ))),
            None => format!("#raw(block: {block}, {});", quoted(src)),
        }
    }

    fn picture(&mut self, src: &str, alt: &str) -> String {
        // `alt|300`, as Obsidian writes a picture's width.
        let asked = asked_width(alt);
        let alt = if asked.is_some() { alt.rsplit_once('|').map_or("", |(words, _)| words) } else { alt };
        let Some(path) = find_picture(src, self.note).filter(|p| !self.bad.pictures.contains(p)) else { return words(alt) };
        let known = self.out.pictures.iter().find(|(p, _, _)| *p == path).map(|(_, v, _)| v.clone());
        let place = match known {
            Some(place) => place,
            None => {
                let Ok(bytes) = std::fs::read(&path) else { return words(alt) };
                let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                let place = format!("/pictures/{}.{ext}", self.out.pictures.len());
                self.out.pictures.push((path, place.clone(), Bytes::new(bytes)));
                place
            }
        };
        let asked = asked.map_or("none".to_string(), |w| w.to_string());
        format!("#pic({}, {}, {asked});\n\n", quoted(&place), quoted(alt))
    }

    fn start(&mut self, tag: &Tag) {
        let frame = match tag {
            Tag::Paragraph => Frame::Para,
            Tag::Heading { level, .. } => Frame::Heading(level_of(*level)),
            Tag::BlockQuote(_) => Frame::Quote,
            Tag::CodeBlock(kind) => Frame::Code(match kind {
                CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                CodeBlockKind::Indented => String::new(),
            }),
            Tag::MetadataBlock(_) => Frame::Skip,
            Tag::HtmlBlock => Frame::Html,
            Tag::List(start) => {
                if let Some((Frame::Item { line_end, .. }, buf)) = self.stack.last_mut() {
                    line_end.get_or_insert(buf.len());
                }
                Frame::List { start: *start, items: Vec::new(), tasks: false, loose: false }
            }
            Tag::Item => Frame::Item { task: None, line_end: None },
            Tag::Table(aligns) => {
                let shares = if self.nested { None } else { self.shares.pop_front().flatten() };
                Frame::Table { aligns: aligns.clone(), cells: Vec::new(), shares: shares.filter(|s| s.len() == aligns.len()), columns: aligns.len(), at: 0, head: 0 }
            }
            Tag::TableHead | Tag::TableRow => return,
            Tag::TableCell => Frame::Cell,
            Tag::Emphasis => Frame::Emph,
            Tag::Strong => Frame::Strong,
            Tag::Strikethrough => Frame::Strike,
            Tag::Link { link_type, dest_url, .. } => Frame::Link((!matches!(link_type, LinkType::WikiLink { .. }) && live_address(dest_url)).then(|| dest_url.to_string())),
            Tag::Image { dest_url, .. } => Frame::Image(dest_url.to_string()),
            _ => Frame::Plain,
        };
        self.stack.push((frame, String::new()));
    }

    /// The tag on top of the stack closed, with the markup it asks for.
    fn close_tag(&mut self) {
        let Some((Frame::Tag(_, style), body)) = self.stack.pop() else { return };
        let markup = match style {
            Style::Strong => format!("#strong[{body}];"),
            Style::Em => format!("#emph[{body}];"),
            Style::Underline => format!("#underline[{body}];"),
            Style::Strike => format!("#strike[{body}];"),
            Style::Mark => format!("#mark[{body}];"),
            Style::Code => format!("#code-words[{body}];"),
            Style::Sub => format!("#sub[{body}];"),
            Style::Sup => format!("#super[{body}];"),
            Style::Small => format!("#text(0.86em)[{body}];"),
            Style::Link(url) if live_address(&url) => format!("#link({})[{body}];", quoted(&url)),
            Style::Link(_) | Style::Plain => body,
            Style::Color { ink, wash } => {
                let inked = match ink {
                    Some(Rgb(r, g, b)) => format!("#tag-ink({r}, {g}, {b})[{body}];"),
                    None => body,
                };
                match wash {
                    Some(Rgb(r, g, b)) => format!("#tag-wash({r}, {g}, {b})[{inked}];"),
                    None => inked,
                }
            }
        };
        self.buf().push_str(&markup);
    }

    /// A tag among the words.
    fn inline_html(&mut self, tag: &str) {
        match html::read_tag(tag) {
            html::Tag::Break => self.buf().push_str("#linebreak();"),
            html::Tag::Image { src, width, alt } => {
                let alt = tag.get(alt).unwrap_or("").to_string();
                let words = match width {
                    Some(w) => format!("{alt}|{w}"),
                    None => alt,
                };
                let markup = self.picture(&html::entities(&src), &words);
                self.buf().push_str(&markup);
            }
            html::Tag::Open { name, style } => self.stack.push((Frame::Tag(name, style), String::new())),
            html::Tag::Close { name } => {
                let open = self.stack.iter().rposition(|(f, _)| matches!(f, Frame::Tag(n, _) if *n == name));
                if let Some(at) = open {
                    while self.stack.len() > at {
                        self.close_tag();
                    }
                }
            }
            html::Tag::Comment | html::Tag::Not => {}
        }
    }

    /// A block of HTML, written by the road every note takes.
    fn html_block(&mut self, source: &str) -> String {
        let reads = html::to_markdown(source);
        if reads.markdown.is_empty() {
            return String::new();
        }
        let (was, first) = (self.nested, self.first_block);
        self.nested = true;
        self.stack.push((Frame::Plain, String::new()));
        for ev in parse(&reads.markdown) {
            self.event(&ev);
        }
        let (_, body) = self.stack.pop().expect("pushed above");
        self.nested = was;
        self.first_block = first;
        match reads.across {
            Across::Start => body,
            Across::Center => format!("#align(center)[{body}];\n\n"),
            Across::End => format!("#align(right)[{body}];\n\n"),
        }
    }

    fn end(&mut self, tag: &TagEnd) {
        // What a block leaves open ends with it.
        while matches!(self.stack.last(), Some((Frame::Tag(..), _))) {
            self.close_tag();
        }
        if matches!(tag, TagEnd::TableHead) {
            if let Some((Frame::Table { head, cells, .. }, _)) = self.stack.last_mut() {
                *head = cells.len();
            }
            return;
        }
        if matches!(tag, TagEnd::TableRow) {
            return;
        }
        let Some((frame, body)) = self.stack.pop() else { return };
        let block = !matches!(frame, Frame::Emph | Frame::Strong | Frame::Strike | Frame::Link(_) | Frame::Image(_) | Frame::Plain | Frame::Skip | Frame::Cell | Frame::Item { .. } | Frame::Tag(..));
        let text = match frame {
            Frame::Root | Frame::Skip | Frame::Tag(..) => String::new(),
            Frame::Html => self.html_block(&body),
            Frame::Plain => body,
            Frame::Para => {
                if let Some((Frame::Item { .. }, _)) = self.stack.last() {
                    if let Some((Frame::List { loose, .. }, _)) = self.stack.iter_mut().rev().nth(1) {
                        *loose = true;
                    }
                }
                format!("{body}\n\n")
            }
            Frame::Heading(level) => {
                let turn = if level == 1 && self.breaks && !self.first_block { "#pagebreak(weak: true);\n" } else { "" };
                format!("{turn}#heading(level: {level})[{body}];\n\n")
            }
            Frame::Emph => format!("#emph[{body}];"),
            Frame::Strong => format!("#strong[{body}];"),
            Frame::Strike => format!("#strike[{body}];"),
            Frame::Quote => format!("#quote(block: true)[{body}];\n\n"),
            Frame::Link(Some(url)) => format!("#link({})[{body}];", quoted(&url)),
            Frame::Link(None) => body,
            Frame::Image(src) => self.picture(&src, &body),
            Frame::Code(lang) => {
                let lang = if lang.is_empty() { String::new() } else { format!("lang: {}, ", quoted(&lang)) };
                format!("#raw(block: true, {lang}{});\n\n", quoted(body.trim_end_matches('\n')))
            }
            Frame::Item { task, line_end } => {
                let split = line_end.unwrap_or(body.len());
                let item = match task {
                    Some(done) => format!("task({done}, [{}], [{}])", &body[..split], &body[split..]),
                    None => format!("[{body}]"),
                };
                if let Some((Frame::List { items, tasks, .. }, _)) = self.stack.last_mut() {
                    *tasks |= task.is_some();
                    items.push(item);
                }
                return;
            }
            Frame::List { start, items, tasks, loose } => {
                let tight = !loose;
                let items: Vec<String> = items.into_iter().map(|i| if tasks && i.starts_with('[') { format!("plain({i})") } else { i }).collect();
                let items = items.join(",\n");
                match (tasks, start) {
                    (true, _) => format!("#tasks({tight},\n{items});\n\n"),
                    (false, Some(n)) => format!("#enum(start: {n}, tight: {tight},\n{items});\n\n"),
                    (false, None) => format!("#list(tight: {tight},\n{items});\n\n"),
                }
            }
            Frame::Cell => {
                if let Some((Frame::Table { cells, columns, at, .. }, _)) = self.stack.last_mut() {
                    cells.push(format!("[{body}]"));
                    *at = (*at + 1) % (*columns).max(1);
                }
                return;
            }
            Frame::Table { aligns, cells, shares, head, .. } => {
                let align: Vec<&str> = aligns
                    .iter()
                    .map(|a| match a {
                        Alignment::Center => "center",
                        Alignment::Right => "right",
                        _ => "left",
                    })
                    .collect();
                // The widths a person set, or each column as its words
                // ask, as the canvas sets them.
                let columns: Vec<String> = match shares {
                    Some(shares) => shares.iter().map(|s| format!("{s}fr")).collect(),
                    None => vec!["auto".to_string(); aligns.len()],
                };
                let head_cells = cells[..head].join(",");
                let rest = cells[head..].join(",");
                format!("#table(columns: ({},), align: ({},), table.header({head_cells}), {rest});\n\n", columns.join(","), align.join(","))
            }
        };
        if block {
            self.first_block = false;
        }
        self.buf().push_str(&text);
    }

    fn event(&mut self, ev: &Event) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(s) => {
                if self.verbatim() {
                    self.buf().push_str(s);
                } else {
                    let markup = words(s);
                    self.buf().push_str(&markup);
                }
            }
            // A block of HTML's lines, kept whole for its end.
            Event::Html(s) if matches!(self.stack.last(), Some((Frame::Html, _))) => self.buf().push_str(s),
            _ if self.verbatim() => {}
            Event::Code(s) => {
                let markup = format!("#raw({});", quoted(s));
                self.buf().push_str(&markup);
            }
            Event::InlineMath(s) => {
                let markup = self.formula(s, false);
                self.buf().push_str(&markup);
            }
            Event::DisplayMath(s) => {
                let markup = self.formula(s, true);
                self.buf().push_str(&markup);
            }
            Event::SoftBreak | Event::HardBreak => self.buf().push_str("#linebreak();"),
            Event::Rule => {
                self.first_block = false;
                self.buf().push_str("#rule;\n\n");
            }
            Event::TaskListMarker(done) => {
                if let Some((Frame::Item { task, .. }, _)) = self.stack.iter_mut().rev().find(|(f, _)| matches!(f, Frame::Item { .. })) {
                    *task = Some(*done);
                }
            }
            Event::FootnoteReference(label) => {
                let label = label.to_string();
                let tag = format!("fn-{:x}", label.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)));
                let markup = if self.noted.insert(label.clone()) {
                    let body = self.notes.get(&label).cloned().unwrap_or_default();
                    format!("#footnote[{body}]<{tag}>")
                } else {
                    format!("#footnote(<{tag}>);")
                };
                self.buf().push_str(&markup);
            }
            Event::InlineHtml(s) => self.inline_html(s),
            _ => {}
        }
    }
}

fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_MATH | Options::ENABLE_WIKILINKS | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS | Options::ENABLE_FOOTNOTES
}

fn parse(text: &str) -> Vec<Event<'_>> {
    Parser::new_ext(text, options()).collect()
}

/// Whether the note opens with a top heading that says its name, so the
/// name is not printed twice.
fn opens_with_name(events: &[Event], title: &str) -> bool {
    let mut it = events.iter().skip_while(|e| !matches!(e, Event::End(TagEnd::MetadataBlock(_))));
    let rest: Vec<&Event> = if events.iter().any(|e| matches!(e, Event::End(TagEnd::MetadataBlock(_)))) {
        it.next();
        it.collect()
    } else {
        events.iter().collect()
    };
    if !matches!(rest.first(), Some(Event::Start(Tag::Heading { level: HeadingLevel::H1, .. }))) {
        return false;
    }
    let words: String = rest[1..]
        .iter()
        .take_while(|e| !matches!(e, Event::End(TagEnd::Heading(_))))
        .filter_map(|e| match e {
            Event::Text(s) | Event::Code(s) => Some(s.as_ref()),
            _ => None,
        })
        .collect();
    words.trim().eq_ignore_ascii_case(title.trim())
}

fn walk(note: &Note, breaks: bool, bad: &Bad) -> Walked {
    let events = parse(note.text);
    // A footnote's body comes at the end of the parser's events and is
    // wanted where the note refers to it.
    let mut notes = HashMap::new();
    let mut body = Vec::with_capacity(events.len());
    let mut used = Walked::default();
    let mut it = events.iter();
    while let Some(ev) = it.next() {
        if let Event::Start(Tag::FootnoteDefinition(label)) = ev {
            // What it uses goes on the note's own list, so a picture here
            // and one elsewhere are not given one number.
            let mut inner = Walk { note, breaks: false, bad, notes: HashMap::new(), noted: HashSet::new(), out: std::mem::take(&mut used), stack: vec![(Frame::Root, String::new())], first_block: false, shares: Default::default(), nested: true };
            for e in it.by_ref().take_while(|e| !matches!(e, Event::End(TagEnd::FootnoteDefinition))) {
                inner.event(e);
            }
            let (_, text) = inner.stack.pop().expect("the root stays");
            notes.insert(label.to_string(), text.trim_end().to_string());
            used = inner.out;
        } else {
            body.push(ev);
        }
    }
    // The widths are in the separator's dashes, which the events do not carry.
    let shares = Parser::new_ext(note.text, options())
        .into_offset_iter()
        .filter_map(|(ev, r)| match ev {
            Event::Start(Tag::Table(_)) => Some(note.text[r].lines().nth(1).and_then(rhythm::shares_of)),
            _ => None,
        })
        .collect();
    let mut w = Walk { note, breaks, bad, notes, noted: HashSet::new(), out: used, stack: vec![(Frame::Root, String::new())], first_block: true, shares, nested: false };
    for ev in body {
        w.event(ev);
    }
    let (_, text) = w.stack.pop().expect("the root stays");
    w.out.body = text;
    w.out
}

/// The language the page's rules go by, from what most of the note is in.
fn language(text: &str) -> &'static str {
    let (mut latin, mut han, mut kana, mut hangul) = (0usize, 0usize, 0usize, 0usize);
    for c in text.chars() {
        match c as u32 {
            0x41..=0x5A | 0x61..=0x7A => latin += 1,
            0x3040..=0x30FF => kana += 1,
            0xAC00..=0xD7A3 => hangul += 1,
            0x3400..=0x9FFF => han += 1,
            _ => {}
        }
    }
    if han + kana + hangul <= latin / 2 {
        "en"
    } else if kana > 0 {
        "ja"
    } else if hangul > han {
        "ko"
    } else {
        "zh"
    }
}

fn typst_list(names: &[String]) -> String {
    let items: Vec<String> = names.iter().map(|n| quoted(n)).collect();
    format!("({},)", items.join(", "))
}

/// What stands before the note: the names the template reads, the page,
/// the template, and the note's name and contents.
fn head(title: &str, lang: &str, look: &Look, chain: &[String], o: &PdfConfig, paper: &str, show_name: bool) -> String {
    let (_, mut w, mut h) = PAPERS.iter().find(|p| p.0 == paper).copied().unwrap_or(PAPERS[1]);
    if o.landscape {
        std::mem::swap(&mut w, &mut h);
    }
    let margin = match o.margins.as_str() {
        "small" => 14.,
        "large" => 34.,
        _ => 24.,
    };
    let leading = match o.spacing.as_str() {
        "tight" => 0.55,
        "loose" => 1.,
        _ => 0.75,
    };
    let (accent, link, chip, wash) = if o.color { (quoted(&look.accent), quoted(&look.link), None, "\"#F7E2B0\"".to_string()) } else { ("\"#77746B\"".into(), "\"#1F1E1D\"".into(), Some("#F0EFED"), "\"#E4E3E0\"".to_string()) };
    let chip = chip.map_or("accent.lighten(90%)".to_string(), |c| format!("rgb({})", quoted(c)));
    let family = |own: &[String]| typst_list(&own.iter().chain(chain).cloned().collect::<Vec<_>>());
    let size = o.size.clamp(PdfConfig::LEAST_SIZE, PdfConfig::MOST_SIZE);
    let heads: String = rhythm::HEADINGS.iter().map(|h| format!("{}, ", h.scale)).collect();
    let mut s = format!(
        "#let base = {size}pt\n#let heads = ({heads})\n#let ink = rgb(\"#1F1E1D\")\n#let muted = rgb(\"#77746B\")\n#let border = rgb(\"#DFDFDC\")\n#let plate = rgb(\"#F4F3F1\")\n#let accent = rgb({accent})\n#let link-ink = rgb({link})\n#let chip = {chip}\n#let wash = rgb({wash})\n#let words = {}\n#let code = {}\n#let leading = {leading}em\n#let gap = {}em\n#let colored = {}\n#let numbered = {}\n#set document(title: {})\n#set page(width: {w}pt, height: {h}pt, margin: {margin}mm)\n",
        family(&look.words),
        family(&look.code),
        leading + 0.4,
        o.color,
        o.numbers,
        quoted(title),
    );
    s += TEMPLATE;
    s += &format!("\n#set text(lang: {})\n", quoted(lang));
    if show_name {
        s += "#title();\n\n";
    }
    if o.contents {
        s += "#context if query(heading).len() > 0 { outline(title: none, depth: 3, indent: 1.2em); v(1em) }\n";
        if o.breaks {
            s += "#pagebreak(weak: true);\n";
        }
    }
    s
}

/// A document laid out: its pages can be drawn and it can be written.
pub struct Laid {
    doc: PagedDocument,
}

/// A page drawn: straight from Typst, top to bottom, four bytes a pixel,
/// red first.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Laid {
    pub fn pages(&self) -> usize {
        self.doc.pages().len()
    }

    /// A page's size in points.
    pub fn size(&self, page: usize) -> Option<(f32, f32)> {
        let frame = &self.doc.pages().get(page)?.frame;
        Some((frame.width().to_pt() as f32, frame.height().to_pt() as f32))
    }

    pub fn draw(&self, page: usize, pixels_per_point: f32) -> Option<Picture> {
        let pixmap = typst_render::render(self.doc.pages().get(page)?, &typst_render::RenderOptions { pixel_per_pt: typst::utils::Scalar::new(pixels_per_point as f64), ..Default::default() });
        Some(Picture { width: pixmap.width(), height: pixmap.height(), rgba: pixmap.data().to_vec() })
    }

    pub fn pdf(&self) -> Result<Vec<u8>, String> {
        typst_pdf::pdf(&self.doc, &typst_pdf::PdfOptions::default()).map_err(|errors| errors.first().map_or_else(|| "the file could not be made".to_string(), |e| e.message.to_string()))
    }
}

/// Typst's cache let go, when the sheet closes: it holds what it laid out.
pub fn forget() {
    comemo::evict(0);
}

fn lay_out_here(note: &Note, look: &Look, o: &PdfConfig, paper: &str) -> Result<Laid, String> {
    let (fonts, chain) = gather(note.text, look);
    let events = parse(note.text);
    let show_name = o.name && !opens_with_name(&events, note.title);
    drop(events);
    let head = head(note.title, language(note.text), look, &chain, o, paper, show_name);
    let main = file_id("/main.typ").ok_or("no main file")?;
    let mut sources = HashMap::new();
    for (path, text) in SCOPE {
        let id = file_id(path).ok_or("no scope file")?;
        sources.insert(id, Source::new(id, text.to_string()));
    }
    let mut files = HashMap::new();
    files.insert(file_id("/onyx.tmTheme").ok_or("no theme file")?, Bytes::new(THEME));
    let mut place = Place { book: LazyHash::new(FontBook::from_fonts(&fonts)), fonts, main: Source::new(main, String::new()), sources, files };

    let mut bad = Bad::default();
    let walked = walk(note, o.breaks, &bad);
    place.take_pictures(&walked)?;
    place.with_main(format!("{head}{}", walked.body));
    let whole = place.compile();
    let doc = match whole {
        Ok(doc) => doc,
        Err(first) => {
            // Each part alone, to find what the whole could not take.
            for formula in &walked.formulas {
                let trial = Walk { note, breaks: false, bad: &bad, notes: HashMap::new(), noted: HashSet::new(), out: Walked::default(), stack: vec![(Frame::Root, String::new())], first_block: false, shares: Default::default(), nested: true }.formula(formula, false);
                place.with_main(format!("{head}{trial}"));
                if place.compile().is_err() {
                    bad.formulas.insert(formula.clone());
                }
            }
            for (path, at, _) in &walked.pictures {
                place.with_main(format!("{head}#pic({}, \"\");", quoted(at)));
                if place.compile().is_err() {
                    bad.pictures.insert(path.clone());
                }
            }
            if bad.formulas.is_empty() && bad.pictures.is_empty() {
                return Err(first);
            }
            // A picture left out moves the numbers of those after it.
            let walked = walk(note, o.breaks, &bad);
            place.take_pictures(&walked)?;
            place.with_main(format!("{head}{}", walked.body));
            place.compile()?
        }
    };
    comemo::evict(8);
    Ok(Laid { doc })
}

/// A note laid out on paper. On a thread of its own, with room for Typst
/// and with any panic of Typst's stopped there. `paper` is `o.paper`, or
/// the region's when that is empty.
pub fn lay_out(note: &Note, look: &Look, o: &PdfConfig, paper: &str) -> Result<Arc<Laid>, String> {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new().name("pdf".into()).stack_size(STACK).spawn_scoped(scope, || lay_out_here(note, look, o, paper)).map_err(|e| e.to_string())?;
        worker.join().map_err(|_| "Typst stopped on this note".to_string())?
    })
    .map(Arc::new)
}

/// The path to write `chosen` at: it ends in `.pdf`, and a name that had
/// to be finished here does not land on a file the save panel never
/// asked about. A name the panel did ask about stands.
pub fn output_path(chosen: &Path) -> PathBuf {
    if chosen.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return chosen.to_path_buf();
    }
    let name = chosen.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    (0..)
        .map(|n| chosen.with_file_name(if n == 0 { format!("{name}.pdf") } else { format!("{name} {n}.pdf") }))
        .find(|p| !p.exists())
        .unwrap_or_else(|| chosen.with_extension("pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_prints_on_its_own_paper() {
        assert_eq!(paper_of_locale("en-US"), "letter");
        assert_eq!(paper_of_locale("zh-Hans-CN"), "a4");
        assert_eq!(paper_of_locale("en_AU"), "a4");
        assert_eq!(paper_of_locale("es-MX"), "letter");
        assert_eq!(paper_of_locale("en"), "a4");
    }

    #[test]
    fn highlights_are_the_editors() {
        assert_eq!(highlights("a ==b== c"), vec![2..7]);
        assert!(highlights("a == b == c").is_empty());
        assert!(highlights("====").is_empty());
    }

    #[test]
    fn an_address_is_live_only_with_a_scheme() {
        assert!(live_address("https://example.com"));
        assert!(live_address("mailto:a@b.c"));
        assert!(!live_address("Other Note.md"));
        assert!(!live_address("#heading"));
        assert!(!live_address("C:/notes/a.md"));
    }

    #[test]
    fn an_address_is_decoded() {
        assert_eq!(decoded("my%20picture.png"), "my picture.png");
        assert_eq!(decoded("100%.png"), "100%.png");
        assert_eq!(decoded("%E7%9F%B3.png"), "石.png");
    }
}
