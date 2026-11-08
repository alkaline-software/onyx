//! A note read for the writers that are not Typst, Word's (`docx.rs`) and
//! the web's (`web.rs`): the parser's events made a small tree of blocks
//! and words, once, which each writer then sets in its own way.
//!
//! - The note is read as the editor and the PDF read it: the same
//!   parser options, `==highlight==` by `pdf::highlights`, a line break
//!   as a line break, a YAML header left out.
//! - A picture is found as the PDF finds it (`pdf::find_picture`) and read
//!   whole into the tree with its size in pixels, so a writer needs
//!   nothing more of the disk. Nothing else of the disk is read but the
//!   files set after the note (`attach`), read before, a PDF's pages
//!   drawn here as pictures.
//! - A block of HTML goes through `html::to_markdown` and is read as the
//!   note is, as the PDF does, so it comes out as it shows.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

use crate::attach::{self, Attached};
use crate::html::{self, Across, Rgb, Style, Width};
use crate::pdf::{self, Note};

/// A picture's file, read.
pub(crate) struct Picture {
    pub bytes: Vec<u8>,
    /// Its kind as a file name ends: `png`, `jpeg`, `gif`, `webp`, `svg`.
    pub ext: String,
    /// Its size in pixels, when its header says it.
    pub size: Option<(u32, u32)>,
}

impl Picture {
    pub fn mime(&self) -> &'static str {
        match self.ext.as_str() {
            "png" => "image/png",
            "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "bmp" => "image/bmp",
            _ => "application/octet-stream",
        }
    }
}

/// A picture as the note names it.
#[derive(Debug, Clone)]
pub(crate) struct Pic {
    pub src: String,
    pub alt: String,
    pub width: Option<Width>,
    /// The file it was found as, by its place in `Doc::pictures`.
    pub found: Option<usize>,
    /// An address with a scheme: the web can show it, paper cannot.
    pub address: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Mark {
    Strong,
    Em,
    Strike,
    Underline,
    Highlight,
    Code,
    Sub,
    Sup,
    Small,
    Ink(u8, u8, u8),
    Wash(u8, u8, u8),
}

#[derive(Debug, Clone)]
pub(crate) enum Inline {
    Text(String),
    Code(String),
    /// A formula's LaTeX, and whether it is set apart.
    Math(String, bool),
    Break,
    Marked(Mark, Vec<Inline>),
    /// A link: its address when it leads somewhere off the note's vault.
    Link(Option<String>, Vec<Inline>),
    /// Words that lead to a file set after the note (`Doc::parts`), by
    /// its place there, and whether this is where its way back returns.
    Jump(usize, bool, Vec<Inline>),
    /// A file set after the note, embedded here: its card, which stands
    /// as a block when it is all its paragraph holds, else as its name
    /// leading there.
    Card(Box<Card>),
    Picture(Pic),
    /// A footnote's number, and whether this is its first reference.
    Note(usize, bool),
}

/// A file set after the note where the note embeds it, as the editor's
/// medium card: which file, whether its way back returns here, its name,
/// its kind and size, a few of its words, and its first page's top.
#[derive(Debug, Clone)]
pub(crate) struct Card {
    pub n: usize,
    pub first: bool,
    pub title: String,
    pub meta: String,
    pub words: Option<String>,
    pub picture: Option<Pic>,
}

#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub(crate) enum Block {
    Para(Vec<Inline>),
    Heading(u8, Vec<Inline>),
    Quote(Vec<Block>),
    Code { lang: String, text: String },
    Math(String),
    List { start: Option<u64>, items: Vec<Item> },
    Table { aligns: Vec<Align>, head: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
    Rule,
    /// A picture alone in its paragraph.
    Picture(Pic),
    /// A file set after the note, embedded alone in its paragraph.
    Card(Box<Card>),
    /// What a block of HTML placed across the page.
    Placed(Align, Vec<Block>),
    /// A PDF's page set after the note, as a picture as large as a page's
    /// words can hold it.
    Leaf(Pic),
}

/// A file set after the note: its name, whether the note's words hold
/// the place its way back returns to (else it returns to the top), a
/// note's name when it is to stand on top, and what it reads as.
pub(crate) struct Part {
    pub name: String,
    pub back: bool,
    pub title: Option<String>,
    pub blocks: Vec<Block>,
}

/// A note read: its name when it is to stand on top, its blocks, its
/// footnotes in the order they are first referred to, and its pictures.
pub(crate) struct Doc {
    pub title: Option<String>,
    pub blocks: Vec<Block>,
    pub notes: Vec<Vec<Block>>,
    pub pictures: Vec<Picture>,
    pub parts: Vec<Part>,
}

/// How wide, in pixels, a card's first page is drawn.
const CARD_PICTURE: u32 = 480;

pub(crate) fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_MATH | Options::ENABLE_WIKILINKS | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS | Options::ENABLE_FOOTNOTES
}

/// An address a reader can follow: one with a scheme.
pub(crate) fn live_address(url: &str) -> bool {
    url.split_once(':').is_some_and(|(scheme, _)| scheme.len() > 1 && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
}

/// The page's size in points, turned when it is landscape. An empty
/// paper is A4.
pub(crate) fn paper(o: &crate::config::PdfConfig) -> (f64, f64) {
    let (_, mut w, mut h) = pdf::PAPERS.iter().find(|p| p.0 == o.paper).copied().unwrap_or(pdf::PAPERS[1]);
    if o.landscape {
        std::mem::swap(&mut w, &mut h);
    }
    (w, h)
}

/// The page's margin in points, as the PDF sets it.
pub(crate) fn margin(o: &crate::config::PdfConfig) -> f64 {
    let mm = match o.margins.as_str() {
        "small" => 14.,
        "large" => 34.,
        _ => 24.,
    };
    mm * 72. / 25.4
}

/// A color (`#277580`) mixed with white, `keep` of it kept, as `RRGGBB`:
/// the accent's light shades.
pub(crate) fn tint(color: &str, keep: f32) -> String {
    let h = color.trim_start_matches('#');
    let part = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("77"), 16).unwrap_or(0x77) as f32;
    let mix = |v: f32| (v * keep + 255. * (1. - keep)).round() as u8;
    format!("{:02X}{:02X}{:02X}", mix(part(0)), mix(part(2)), mix(part(4)))
}

/// Bytes as base64, for a picture in a web page.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() { ABC[(n >> shift & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// A picture's words and the width they ask for, as Obsidian writes it:
/// `alt|300`, or `alt|50%`.
fn split_width(words: &str) -> (String, Option<Width>) {
    if let Some((alt, last)) = words.rsplit_once('|').or_else(|| Some(("", words))) {
        let last = last.trim();
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if let Some(n) = last.strip_suffix('%').filter(|n| digits(n)).and_then(|n| n.parse().ok()).filter(|n| (1..=100).contains(n)) {
            return (alt.to_string(), Some(Width::Share(n)));
        }
        if digits(last) {
            if let Some(n) = last.parse().ok().filter(|n| *n > 0) {
                return (alt.to_string(), Some(Width::Px(n)));
            }
        }
    }
    (words.to_string(), None)
}

/// A picture's size in pixels from its file's header; an SVG's from its
/// `width` and `height`, else its `viewBox`.
fn size_of(bytes: &[u8], ext: &str) -> Option<(u32, u32)> {
    if ext == "svg" {
        let text = std::str::from_utf8(bytes).ok()?;
        let tag = &text[text.find("<svg")?..];
        let tag = &tag[..tag.find('>')?];
        let attr = |name: &str| {
            let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
            tag[at..].split('"').next().map(str::to_string)
        };
        let number = |s: String| s.trim_end_matches("px").trim().parse::<f32>().ok().filter(|n| *n > 0.);
        if let (Some(w), Some(h)) = (attr("width").and_then(number), attr("height").and_then(number)) {
            return Some((w.round() as u32, h.round() as u32));
        }
        let view: Vec<f32> = attr("viewBox")?.split([' ', ',']).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
        return (view.len() == 4 && view[2] > 0. && view[3] > 0.).then(|| (view[2].round() as u32, view[3].round() as u32));
    }
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

enum Frame {
    Root,
    Para,
    Heading(u8),
    Quote,
    Code(String),
    Skip,
    Html,
    List { start: Option<u64>, items: Vec<Item> },
    Item { task: Option<bool> },
    Table { aligns: Vec<Align>, head: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>>, in_head: bool, row: Vec<Vec<Inline>> },
    Cell,
    Marked(Mark),
    Link(Option<String>, Option<usize>),
    Image(String),
    /// A tag among the words, waiting for its closing one.
    Tag(String, Style),
}

/// What a frame holds: blocks, the words of the block it is in, and the
/// raw text of a code block, an alt text or a block of HTML.
#[derive(Default)]
struct Held {
    blocks: Vec<Block>,
    words: Vec<Inline>,
    raw: String,
}

struct Reader<'a> {
    note: &'a Note<'a>,
    stack: Vec<(Frame, Held)>,
    pictures: Vec<Picture>,
    found: HashMap<PathBuf, usize>,
    /// Each footnote's number by its label, given at its first reference,
    /// counted from `base`, the footnotes of the parts before.
    numbers: HashMap<String, usize>,
    base: usize,
    /// The files set after the note, whether these are the note's own
    /// words, and the files whose way back returns to them.
    attached: &'a [Attached],
    main: bool,
    labeled: HashSet<usize>,
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

impl<'a> Reader<'a> {
    fn top(&mut self) -> &mut Held {
        &mut self.stack.last_mut().expect("the root stays").1
    }

    fn verbatim(&self) -> bool {
        self.stack.iter().any(|(f, _)| matches!(f, Frame::Code(_) | Frame::Image(_) | Frame::Skip | Frame::Html))
    }

    /// Words go in the frame on top; words standing straight in an item,
    /// as a tight list has them, are made a paragraph when it ends.
    fn push_inline(&mut self, i: Inline) {
        self.top().words.push(i);
    }

    /// Words left loose in a frame that holds blocks, made a paragraph.
    fn settle(held: &mut Held) {
        if !held.words.is_empty() {
            let words = std::mem::take(&mut held.words);
            held.blocks.push(paragraph(words));
        }
    }

    fn push_block(&mut self, b: Block) {
        let held = self.top();
        Self::settle(held);
        held.blocks.push(b);
    }

    /// The file set after the note that a reference names, if any.
    fn target(&self, dest: &str) -> Option<usize> {
        if self.attached.is_empty() {
            return None;
        }
        let path = attach::resolve(self.note, dest)?;
        self.attached.iter().position(|a| a.path == path)
    }

    fn jump(&mut self, n: usize, words: Vec<Inline>) -> Inline {
        let first = self.main && self.labeled.insert(n);
        Inline::Jump(n, first, words)
    }

    /// What an embed stands as among the words: a file set after the note
    /// is its card, leading there; anything else is a picture.
    fn embedded(&mut self, src: &str, words: &str, width: Option<Width>) -> Inline {
        if let Some(n) = self.target(src) {
            let (alt, _) = split_width(words);
            let alt = alt.trim();
            let a = &self.attached[n];
            let title = if alt.is_empty() || crate::vault::decoded(alt) == crate::vault::decoded(src.trim()) { a.name.clone() } else { alt.to_string() };
            let (meta, card_words, name) = (attach::card_meta(&a.path), a.card_words(), a.name.clone());
            let drawn = match &a.body {
                attach::Body::Pdf { bytes, .. } => attach::card_picture(bytes, CARD_PICTURE),
                _ => None,
            };
            let picture = drawn.map(|(png, size)| {
                self.pictures.push(Picture { bytes: png, ext: "png".into(), size: Some(size) });
                Pic { src: name.clone(), alt: name, width: None, found: Some(self.pictures.len() - 1), address: false }
            });
            let first = self.main && self.labeled.insert(n);
            return Inline::Card(Box::new(Card { n, first, title, meta, words: card_words, picture }));
        }
        Inline::Picture(self.picture(src, words, width))
    }

    fn picture(&mut self, src: &str, words: &str, width: Option<Width>) -> Pic {
        let (alt, asked) = split_width(words);
        let width = width.or(asked);
        let address = live_address(src) && !src.starts_with("file:");
        let found = if address { None } else { self.read_picture(src) };
        Pic { src: src.to_string(), alt, width, found, address }
    }

    fn read_picture(&mut self, src: &str) -> Option<usize> {
        let path = pdf::find_picture(src, self.note)?;
        if let Some(n) = self.found.get(&path) {
            return Some(*n);
        }
        let bytes = std::fs::read(&path).ok()?;
        let ext = match path.extension()?.to_string_lossy().to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => "jpeg".to_string(),
            e => e.to_string(),
        };
        let size = size_of(&bytes, &ext);
        self.pictures.push(Picture { bytes, ext, size });
        self.found.insert(path, self.pictures.len() - 1);
        Some(self.pictures.len() - 1)
    }

    fn close_tag(&mut self) {
        let Some((Frame::Tag(_, style), held)) = self.stack.pop() else { return };
        let words = held.words;
        let made = match style {
            Style::Strong => Inline::Marked(Mark::Strong, words),
            Style::Em => Inline::Marked(Mark::Em, words),
            Style::Underline => Inline::Marked(Mark::Underline, words),
            Style::Strike => Inline::Marked(Mark::Strike, words),
            Style::Mark => Inline::Marked(Mark::Highlight, words),
            Style::Code => Inline::Marked(Mark::Code, words),
            Style::Sub => Inline::Marked(Mark::Sub, words),
            Style::Sup => Inline::Marked(Mark::Sup, words),
            Style::Small => Inline::Marked(Mark::Small, words),
            Style::Link(url) => Inline::Link(live_address(&url).then_some(url), words),
            Style::Color { ink, wash } => {
                let inked = match ink {
                    Some(Rgb(r, g, b)) => vec![Inline::Marked(Mark::Ink(r, g, b), words)],
                    None => words,
                };
                match wash {
                    Some(Rgb(r, g, b)) => Inline::Marked(Mark::Wash(r, g, b), inked),
                    None => return self.top().words.extend(inked),
                }
            }
            // A tag with no look of its own: its words as they are.
            Style::Plain => return self.top().words.extend(words),
        };
        self.push_inline(made);
    }

    fn inline_html(&mut self, tag: &str) {
        match html::read_tag(tag) {
            html::Tag::Break => self.push_inline(Inline::Break),
            html::Tag::Image { src, width, alt } => {
                let alt = html::entities(tag.get(alt).unwrap_or(""));
                let made = self.embedded(&html::entities(&src), &alt, width);
                self.push_inline(made);
            }
            html::Tag::Open { name, style } => self.stack.push((Frame::Tag(name, style), Held::default())),
            html::Tag::Close { name } => {
                if let Some(at) = self.stack.iter().rposition(|(f, _)| matches!(f, Frame::Tag(n, _) if *n == name)) {
                    while self.stack.len() > at {
                        self.close_tag();
                    }
                }
            }
            html::Tag::Comment | html::Tag::Not => {}
        }
    }

    fn html_block(&mut self, source: &str) {
        let reads = html::to_markdown(source);
        if reads.markdown.trim().is_empty() {
            return;
        }
        self.stack.push((Frame::Root, Held::default()));
        for ev in Parser::new_ext(&reads.markdown, options()) {
            self.event(ev);
        }
        let (_, mut held) = self.stack.pop().expect("pushed above");
        Self::settle(&mut held);
        match reads.across {
            Across::Start => {
                for b in held.blocks {
                    self.push_block(b);
                }
            }
            Across::Center => self.push_block(Block::Placed(Align::Center, held.blocks)),
            Across::End => self.push_block(Block::Placed(Align::Right, held.blocks)),
        }
    }

    fn start(&mut self, tag: Tag) {
        let frame = match tag {
            Tag::Paragraph => Frame::Para,
            Tag::Heading { level, .. } => Frame::Heading(level_of(level)),
            Tag::BlockQuote(_) => Frame::Quote,
            Tag::CodeBlock(kind) => Frame::Code(match kind {
                CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                CodeBlockKind::Indented => String::new(),
            }),
            Tag::MetadataBlock(_) => Frame::Skip,
            Tag::HtmlBlock => Frame::Html,
            Tag::List(start) => Frame::List { start, items: Vec::new() },
            Tag::Item => Frame::Item { task: None },
            Tag::Table(aligns) => Frame::Table {
                aligns: aligns
                    .iter()
                    .map(|a| match a {
                        Alignment::Center => Align::Center,
                        Alignment::Right => Align::Right,
                        _ => Align::Left,
                    })
                    .collect(),
                head: Vec::new(),
                rows: Vec::new(),
                in_head: false,
                row: Vec::new(),
            },
            Tag::TableHead => {
                if let Some((Frame::Table { in_head, .. }, _)) = self.stack.last_mut() {
                    *in_head = true;
                }
                return;
            }
            Tag::TableRow => return,
            Tag::TableCell => Frame::Cell,
            Tag::Emphasis => Frame::Marked(Mark::Em),
            Tag::Strong => Frame::Marked(Mark::Strong),
            Tag::Strikethrough => Frame::Marked(Mark::Strike),
            Tag::Link { link_type, dest_url, .. } => {
                let url = (!matches!(link_type, LinkType::WikiLink { .. }) && live_address(&dest_url)).then(|| dest_url.to_string());
                let to = if url.is_none() { self.target(&dest_url) } else { None };
                Frame::Link(url, to)
            }
            Tag::Image { dest_url, .. } => Frame::Image(dest_url.to_string()),
            // A footnote's body is read apart (`read`).
            _ => Frame::Skip,
        };
        self.stack.push((frame, Held::default()));
    }

    fn end(&mut self, tag: TagEnd) {
        while matches!(self.stack.last(), Some((Frame::Tag(..), _))) {
            self.close_tag();
        }
        match tag {
            TagEnd::TableHead => {
                if let Some((Frame::Table { head, row, in_head, .. }, _)) = self.stack.last_mut() {
                    *head = std::mem::take(row);
                    *in_head = false;
                }
                return;
            }
            TagEnd::TableRow => {
                if let Some((Frame::Table { rows, row, .. }, _)) = self.stack.last_mut() {
                    rows.push(std::mem::take(row));
                }
                return;
            }
            _ => {}
        }
        let Some((frame, mut held)) = self.stack.pop() else { return };
        match frame {
            Frame::Root | Frame::Skip | Frame::Tag(..) => {}
            Frame::Html => self.html_block(&held.raw),
            Frame::Para => {
                let words = std::mem::take(&mut held.words);
                self.push_block(paragraph(words));
            }
            Frame::Heading(level) => self.push_block(Block::Heading(level, held.words)),
            Frame::Quote => {
                Self::settle(&mut held);
                self.push_block(Block::Quote(held.blocks));
            }
            Frame::Code(lang) => self.push_block(Block::Code { lang, text: held.raw.trim_end_matches('\n').to_string() }),
            Frame::Marked(mark) => self.push_inline(Inline::Marked(mark, held.words)),
            Frame::Link(url @ Some(_), _) | Frame::Link(url @ None, None) => self.push_inline(Inline::Link(url, held.words)),
            Frame::Link(None, Some(n)) => {
                let made = self.jump(n, held.words);
                self.push_inline(made);
            }
            Frame::Image(src) => {
                let made = self.embedded(&src, &held.raw, None);
                self.push_inline(made);
            }
            Frame::Item { task } => {
                Self::settle(&mut held);
                if let Some((Frame::List { items, .. }, _)) = self.stack.last_mut() {
                    items.push(Item { task, blocks: held.blocks });
                }
            }
            Frame::List { start, items } => self.push_block(Block::List { start, items }),
            Frame::Cell => {
                if let Some((Frame::Table { row, .. }, _)) = self.stack.last_mut() {
                    row.push(held.words);
                }
            }
            Frame::Table { aligns, head, rows, .. } => self.push_block(Block::Table { aligns, head, rows }),
        }
    }

    fn event(&mut self, ev: Event) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(s) if self.verbatim() => self.top().raw.push_str(&s),
            Event::Html(s) if matches!(self.stack.last(), Some((Frame::Html, _))) => self.top().raw.push_str(&s),
            _ if self.verbatim() => {}
            Event::Text(s) => {
                let mut at = 0;
                for r in pdf::highlights(&s) {
                    if r.start > at {
                        self.push_inline(Inline::Text(s[at..r.start].to_string()));
                    }
                    self.push_inline(Inline::Marked(Mark::Highlight, vec![Inline::Text(s[r.start + 2..r.end - 2].to_string())]));
                    at = r.end;
                }
                if at < s.len() {
                    self.push_inline(Inline::Text(s[at..].to_string()));
                }
            }
            Event::Code(s) => self.push_inline(Inline::Code(s.to_string())),
            Event::InlineMath(s) => self.push_inline(Inline::Math(s.to_string(), false)),
            Event::DisplayMath(s) => self.push_inline(Inline::Math(s.to_string(), true)),
            Event::SoftBreak | Event::HardBreak => self.push_inline(Inline::Break),
            Event::Rule => self.push_block(Block::Rule),
            Event::TaskListMarker(done) => {
                if let Some((Frame::Item { task }, _)) = self.stack.iter_mut().rev().find(|(f, _)| matches!(f, Frame::Item { .. })) {
                    *task = Some(done);
                }
            }
            Event::FootnoteReference(label) => {
                let next = self.base + self.numbers.len() + 1;
                let first = !self.numbers.contains_key(label.as_ref());
                let n = *self.numbers.entry(label.to_string()).or_insert(next);
                self.push_inline(Inline::Note(n, first));
            }
            Event::InlineHtml(s) => self.inline_html(&s),
            _ => {}
        }
    }
}

/// A paragraph, or a picture or a formula when that is all it holds.
fn paragraph(words: Vec<Inline>) -> Block {
    let solid: Vec<&Inline> = words.iter().filter(|i| !matches!(i, Inline::Text(t) if t.trim().is_empty()) && !matches!(i, Inline::Break)).collect();
    if solid.len() == 1 {
        match solid[0] {
            Inline::Picture(p) => return Block::Picture(p.clone()),
            Inline::Card(c) => return Block::Card(c.clone()),
            Inline::Math(m, true) => return Block::Math(m.clone()),
            _ => {}
        }
    }
    Block::Para(words)
}

/// Whether the note opens with a top heading that says its name.
fn opens_with_name(blocks: &[Block], title: &str) -> bool {
    let Some(Block::Heading(1, words)) = blocks.first() else { return false };
    plain_text(words).trim().eq_ignore_ascii_case(title.trim())
}

/// Words as plain text, for a heading's anchor and the contents.
pub(crate) fn plain_text(words: &[Inline]) -> String {
    let mut out = String::new();
    for w in words {
        match w {
            Inline::Text(t) | Inline::Code(t) => out.push_str(t),
            Inline::Math(m, _) => out.push_str(m),
            Inline::Break => out.push(' '),
            Inline::Marked(_, inner) | Inline::Link(_, inner) | Inline::Jump(_, _, inner) => out.push_str(&plain_text(inner)),
            Inline::Card(c) => out.push_str(&c.title),
            Inline::Picture(p) => out.push_str(&p.alt),
            Inline::Note(..) => {}
        }
    }
    out
}

/// The note read whole, with the files set after it (`attach`), a PDF's
/// pages drawn `width` pixels wide. `name` puts its name on top unless
/// it opens with a top heading that says the same, and so for a note set
/// after it.
pub(crate) fn read(note: &Note, name: bool, attached: &[Attached], width: u32) -> Doc {
    // A card is read as the file it shows, embedded.
    let text = attach::cards_as_embeds(note.text);
    let note = &Note { text: &text, title: note.title, path: note.path, vault: note.vault };
    let mut reader = Reader { note, stack: vec![(Frame::Root, Held::default())], pictures: Vec::new(), found: HashMap::new(), numbers: HashMap::new(), base: 0, attached, main: true, labeled: HashSet::new() };
    let (blocks, mut notes) = reader.read_text(note.text);
    let title = (name && !opens_with_name(&blocks, note.title)).then(|| note.title.to_string());
    let labeled = std::mem::take(&mut reader.labeled);
    let (mut pictures, mut found) = (std::mem::take(&mut reader.pictures), std::mem::take(&mut reader.found));
    let mut parts = Vec::new();
    for (n, a) in attached.iter().enumerate() {
        let (title, blocks) = match &a.body {
            attach::Body::Pdf { bytes, .. } => {
                let mut leaves = Vec::new();
                for (page, (png, size)) in attach::pages_as_pictures(bytes, width).into_iter().enumerate() {
                    pictures.push(Picture { bytes: png, ext: "png".into(), size: Some(size) });
                    leaves.push(Block::Leaf(Pic { src: a.name.clone(), alt: format!("{} {}", a.name, page + 1), width: None, found: Some(pictures.len() - 1), address: false }));
                }
                (None, leaves)
            }
            attach::Body::Note(text) | attach::Body::File(text) => {
                let part = Note { text, title: &a.title, path: &a.path, vault: note.vault };
                let mut r = Reader { note: &part, stack: vec![(Frame::Root, Held::default())], pictures, found, numbers: HashMap::new(), base: notes.len(), attached, main: false, labeled: HashSet::new() };
                let (blocks, more) = r.read_text(text);
                notes.extend(more);
                (pictures, found) = (r.pictures, r.found);
                let title = (matches!(a.body, attach::Body::Note(_)) && name && !opens_with_name(&blocks, &a.title)).then(|| a.title.clone());
                (title, blocks)
            }
        };
        parts.push(Part { name: a.name.clone(), back: labeled.contains(&n), title, blocks });
    }
    Doc { title, blocks, notes, pictures, parts }
}

impl Reader<'_> {
    /// A text read: its blocks, and its footnotes' bodies in the order of
    /// their numbers.
    fn read_text(&mut self, text: &str) -> (Vec<Block>, Vec<Vec<Block>>) {
        let events: Vec<Event> = crate::html::parts(text, options()).into_iter().flat_map(|r| Parser::new_ext(&text[r], options())).collect();
        // A footnote's body comes where it is defined, most often at the end;
        // it is read apart and kept by its label.
        let mut bodies: HashMap<String, Vec<Block>> = HashMap::new();
        let mut it = events.into_iter();
        while let Some(ev) = it.next() {
            if let Event::Start(Tag::FootnoteDefinition(label)) = ev {
                self.stack.push((Frame::Root, Held::default()));
                for e in it.by_ref().take_while(|e| !matches!(e, Event::End(TagEnd::FootnoteDefinition))) {
                    self.event(e);
                }
                let (_, mut held) = self.stack.pop().expect("pushed above");
                Reader::settle(&mut held);
                bodies.insert(label.to_string(), held.blocks);
            } else {
                self.event(ev);
            }
        }
        // A tag left open at the end closes with it.
        while matches!(self.stack.last(), Some((Frame::Tag(..), _))) {
            self.close_tag();
        }
        let (_, mut held) = self.stack.pop().expect("the root stays");
        Reader::settle(&mut held);
        let mut order: Vec<(usize, String)> = self.numbers.iter().map(|(l, n)| (*n, l.clone())).collect();
        order.sort();
        let notes = order.into_iter().map(|(_, label)| bodies.remove(&label).unwrap_or_default()).collect();
        (held.blocks, notes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_width_is_read_from_the_words() {
        assert_eq!(split_width("cat|300"), ("cat".to_string(), Some(Width::Px(300))));
        assert_eq!(split_width("cat|50%"), ("cat".to_string(), Some(Width::Share(50))));
        assert_eq!(split_width("300"), (String::new(), Some(Width::Px(300))));
        assert_eq!(split_width("a|b"), ("a|b".to_string(), None));
        assert_eq!(split_width("cat"), ("cat".to_string(), None));
    }

    #[test]
    fn a_tint_goes_toward_white() {
        assert_eq!(tint("#000000", 0.1), "E6E6E6");
        assert_eq!(tint("#FFFFFF", 0.5), "FFFFFF");
    }

    #[test]
    fn an_svg_gives_its_size() {
        assert_eq!(size_of(br#"<svg xmlns="x" width="120" height="40px">"#, "svg"), Some((120, 40)));
        assert_eq!(size_of(br#"<svg viewBox="0 0 300 150">"#, "svg"), Some((300, 150)));
    }
}
