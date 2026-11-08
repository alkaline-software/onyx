//! A note as a Word document: a `.docx`, the zip of XML any word
//! processor opens (Word, Pages, Google Docs, LibreOffice).
//!
//! The note is read by `reading::read`, as the web page's writer reads
//! it, and set with the export sheet's options: the paper, its turn and
//! margins, the faces, the size, the line spacing, the name on top, page
//! numbers, the contents, color, and a new page for each top heading.
//!
//! - Every look is a style of the document's own (`Normal`, `Heading1`
//!   to `Heading6`, `Title`, `Quote`, `Code`, `CodeChar`, `Hyperlink`),
//!   so a person can change one in Word and every paragraph follows.
//! - A picture goes in whole, as large as it is or as it asks, never past
//!   the words' width or most of a page. Word reads PNG, JPEG, GIF and
//!   BMP; a WebP is written as a PNG. An SVG, which Word reads only with
//!   a PNG beside it, and a picture at an address are their words.
//! - The contents are a real contents field, its entries written in, so
//!   they show before Word is asked to update it; they leave out page
//!   numbers, which only Word can know.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

use crate::config::PdfConfig;
use crate::html::Width;
use crate::pdf::{Look, Note};
use crate::reading::{self, Align, Block, Doc, Inline, Mark, Pic};
use crate::rhythm;

/// The path to write `chosen` at, ending in `.docx`.
pub fn output_path(chosen: &Path) -> PathBuf {
    crate::pdf::finished(chosen, "docx")
}

const INK: &str = "1F1E1D";
const MUTED: &str = "77746B";
const BORDER: &str = "DFDFDC";
const PLATE: &str = "F4F3F1";

/// Points as twentieths of a point, Word's measure of length.
fn tw(pt: f64) -> i64 {
    (pt * 20.).round() as i64
}

/// Points as English Metric Units, DrawingML's measure.
fn emu(pt: f64) -> i64 {
    (pt * 12700.).round() as i64
}

/// `s` as XML text, with what XML cannot hold left out.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || matches!(c as u32, 0xFFFE | 0xFFFF) => {}
            c => out.push(c),
        }
    }
    out
}

/// A color without its `#`, upper case, for Word.
fn hex(color: &str) -> String {
    color.trim_start_matches('#').to_ascii_uppercase()
}

/// A run's look.
#[derive(Clone, Default)]
struct Run {
    style: Option<&'static str>,
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
    color: Option<String>,
    shade: Option<String>,
    sub: bool,
    sup: bool,
    small: bool,
    /// Inside a hyperlink already: another cannot open in it.
    linked: bool,
}

impl Run {
    fn rpr(&self, size: f64) -> String {
        let mut r = String::new();
        if let Some(s) = self.style {
            r += &format!("<w:rStyle w:val=\"{s}\"/>");
        }
        if self.bold {
            r += "<w:b/><w:bCs/>";
        }
        if self.italic {
            r += "<w:i/><w:iCs/>";
        }
        if self.strike {
            r += "<w:strike/>";
        }
        if let Some(c) = &self.color {
            r += &format!("<w:color w:val=\"{c}\"/>");
        }
        if self.small {
            let half = (size * 0.86 * 2.).round() as i64;
            r += &format!("<w:sz w:val=\"{half}\"/><w:szCs w:val=\"{half}\"/>");
        }
        if self.underline {
            r += "<w:u w:val=\"single\"/>";
        }
        if let Some(fill) = &self.shade {
            r += &format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{fill}\"/>");
        }
        if self.sub {
            r += "<w:vertAlign w:val=\"subscript\"/>";
        } else if self.sup {
            r += "<w:vertAlign w:val=\"superscript\"/>";
        }
        if r.is_empty() { r } else { format!("<w:rPr>{r}</w:rPr>") }
    }
}

/// Where a paragraph stands: what it is in and how far in it starts.
#[derive(Clone, Default)]
struct Place {
    style: Option<&'static str>,
    /// How many quotes it is in.
    quote: u8,
    /// Points in from the words' left edge, for what stands in a list.
    indent: f64,
    align: Option<Align>,
    /// Its list's number and level, for an item's first paragraph.
    number: Option<(usize, usize)>,
    /// A task's line, done or not, which stands in by its box.
    hanging: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Document,
    Notes,
}

struct Writer<'a> {
    doc: &'a Doc,
    o: &'a PdfConfig,
    size: f64,
    accent: String,
    chip: String,
    wash: String,
    /// The words' width and the page's height in points.
    room: f64,
    page_height: f64,
    part: Part,
    /// Each part's relationships: id, type, target, and whether it is
    /// outside the file.
    rels: [Vec<(String, &'static str, String, bool)>; 2],
    media: Vec<(String, Vec<u8>)>,
    /// A picture's file in `word/media`, by its place in `Doc::pictures`,
    /// with its size in pixels; none when Word cannot show it.
    files: HashMap<usize, Option<(String, (u32, u32))>>,
    drawings: usize,
    bookmarks: usize,
    /// Ordered lists, each with its own numbering from its start.
    ordered: Vec<u64>,
    first_block: bool,
    headings: Vec<(u8, String)>,
    /// The next PDF page follows its part's line on the same page.
    leaf_first: bool,
}

/// Bookmark ids apart from the headings': the places the way back from a
/// file set after the note returns to, the files' starts, the top.
const FROM_ID: usize = 100_000;
const PART_ID: usize = 200_000;
const TOP_ID: usize = 300_000;

const LIST_STEP: f64 = 18.;

impl Writer<'_> {
    fn rel(&mut self, kind: &'static str, target: String, external: bool) -> String {
        let list = &mut self.rels[self.part as usize];
        if let Some((id, ..)) = list.iter().find(|(_, k, t, e)| *k == kind && *t == target && *e == external) {
            return id.clone();
        }
        let id = format!("rId{}", list.len() + 10);
        list.push((id.clone(), kind, target, external));
        id
    }

    fn text_run(&self, text: &str, look: &Run) -> String {
        if text.is_empty() {
            return String::new();
        }
        let mut out = format!("<w:r>{}", look.rpr(self.size));
        // A tab is its own element in a run.
        for (i, piece) in text.split('\t').enumerate() {
            if i > 0 {
                out += "<w:tab/>";
            }
            if !piece.is_empty() {
                out += &format!("<w:t xml:space=\"preserve\">{}</w:t>", esc(piece));
            }
        }
        out + "</w:r>"
    }

    fn words(&mut self, words: &[Inline], look: &Run) -> String {
        let mut out = String::new();
        for w in words {
            match w {
                Inline::Text(t) => out += &self.text_run(t, look),
                Inline::Code(t) => out += &self.text_run(t, &Run { style: Some("CodeChar"), shade: Some(self.chip.clone()), ..look.clone() }),
                Inline::Math(m, _) => out += &self.text_run(m, &Run { style: Some("CodeChar"), ..look.clone() }),
                Inline::Break => out += "<w:r><w:br/></w:r>",
                Inline::Marked(mark, inner) => {
                    let mut l = look.clone();
                    match mark {
                        Mark::Strong => l.bold = true,
                        Mark::Em => l.italic = true,
                        Mark::Strike => l.strike = true,
                        Mark::Underline => l.underline = true,
                        Mark::Highlight => l.shade = Some(self.wash.clone()),
                        Mark::Code => {
                            l.style = Some("CodeChar");
                            l.shade = Some(self.chip.clone());
                        }
                        Mark::Sub => l.sub = true,
                        Mark::Sup => l.sup = true,
                        Mark::Small => l.small = true,
                        Mark::Ink(r, g, b) => {
                            if self.o.color {
                                l.color = Some(format!("{r:02X}{g:02X}{b:02X}"));
                            }
                        }
                        // Half of the color over white, as the canvas draws it.
                        Mark::Wash(r, g, b) => {
                            if self.o.color {
                                let half = |v: u8| ((v as u16 + 255) / 2) as u8;
                                l.shade = Some(format!("{:02X}{:02X}{:02X}", half(*r), half(*g), half(*b)));
                            }
                        }
                    }
                    out += &self.words(inner, &l);
                }
                Inline::Link(Some(url), inner) if !look.linked => {
                    let id = self.rel("http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink", url.clone(), true);
                    let inside = self.words(inner, &Run { style: Some("Hyperlink"), linked: true, ..look.clone() });
                    out += &format!("<w:hyperlink r:id=\"{id}\">{inside}</w:hyperlink>");
                }
                Inline::Link(_, inner) => out += &self.words(inner, look),
                // Words that lead to a file set after the note; the first
                // in the note's words is where its way back returns.
                Inline::Jump(n, first, inner) if !look.linked => {
                    let inside = self.words(inner, &Run { style: Some("Hyperlink"), linked: true, ..look.clone() });
                    let link = format!("<w:hyperlink w:anchor=\"_OnyxPart{n}\" w:history=\"1\">{inside}</w:hyperlink>");
                    out += &if *first { format!("<w:bookmarkStart w:id=\"{id}\" w:name=\"_OnyxFrom{n}\"/>{link}<w:bookmarkEnd w:id=\"{id}\"/>", id = FROM_ID + n) } else { link };
                }
                Inline::Jump(_, _, inner) => out += &self.words(inner, look),
                // A card among other words: its name, leading there.
                Inline::Card(c) => out += &self.words(&[Inline::Jump(c.n, c.first, vec![Inline::Text(c.title.clone())])], look),
                Inline::Picture(p) => out += &self.picture(p, self.room),
                Inline::Note(n, first) => {
                    if *first && self.part == Part::Document {
                        out += &format!("<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteReference w:id=\"{n}\"/></w:r>");
                    } else {
                        // Word refers to a footnote once; a second reference
                        // is its number, as the PDF points at the first.
                        out += &self.text_run(&n.to_string(), &Run { style: Some("FootnoteReference"), ..look.clone() });
                    }
                }
            }
        }
        out
    }

    /// A picture's file in `word/media` and its size, written once.
    fn file_of(&mut self, n: usize) -> Option<(String, (u32, u32))> {
        if let Some(known) = self.files.get(&n) {
            return known.clone();
        }
        let pic = &self.doc.pictures[n];
        let made = match (pic.ext.as_str(), pic.size) {
            ("png" | "jpeg" | "gif" | "bmp", Some(size)) => Some((pic.ext.clone(), pic.bytes.clone(), size)),
            // Word does not read WebP: it goes in as a PNG.
            ("webp", Some(size)) => image::load_from_memory(&pic.bytes).ok().and_then(|img| {
                let mut png = Vec::new();
                img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
                Some(("png".to_string(), png, size))
            }),
            _ => None,
        };
        let known = made.map(|(ext, bytes, size)| {
            let name = format!("image{}.{ext}", self.media.len() + 1);
            self.media.push((name.clone(), bytes));
            (name, size)
        });
        self.files.insert(n, known.clone());
        known
    }

    /// A picture as an inline drawing, at most `room` points wide; its
    /// words when Word cannot show it.
    fn picture(&mut self, p: &Pic, room: f64) -> String {
        self.picture_in(p, room, 0.8 * self.page_height)
    }

    /// A picture at most `room` points wide and `tallest` tall.
    fn picture_in(&mut self, p: &Pic, room: f64, tallest: f64) -> String {
        let alt = Run::default();
        let Some((name, (pw, ph))) = p.found.and_then(|n| self.file_of(n)) else { return self.text_run(&p.alt, &alt) };
        if pw == 0 || ph == 0 {
            return self.text_run(&p.alt, &alt);
        }
        // A pixel is three quarters of a point, as on the web.
        let natural = pw as f64 * 0.75;
        let wanted = match p.width {
            Some(Width::Px(w)) => w as f64 * 0.75,
            Some(Width::Share(n)) => room * n as f64 / 100.,
            None => natural,
        };
        let ratio = ph as f64 / pw as f64;
        let w = wanted.min(room).min(tallest / ratio).max(1.);
        let h = w * ratio;
        let id = self.rel("http://schemas.openxmlformats.org/officeDocument/2006/relationships/image", format!("media/{name}"), false);
        self.drawings += 1;
        let n = self.drawings;
        let (cx, cy) = (emu(w), emu(h));
        let descr = esc(&p.alt);
        format!(
            "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:effectExtent l=\"0\" t=\"0\" r=\"0\" b=\"0\"/><wp:docPr id=\"{n}\" name=\"Picture {n}\" descr=\"{descr}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"{name}\" descr=\"{descr}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
        )
    }

    /// A paragraph's properties, in the order Word's schema asks. `rule`
    /// draws a line under it.
    fn ppr(&self, at: &Place, before_break: bool, rule: bool) -> String {
        let mut p = String::new();
        if let Some(s) = at.style {
            p += &format!("<w:pStyle w:val=\"{s}\"/>");
        }
        if before_break {
            p += "<w:pageBreakBefore/>";
        }
        if let Some((num, level)) = at.number {
            p += &format!("<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num}\"/></w:numPr>");
        }
        if at.quote > 0 || rule {
            p += "<w:pBdr>";
            if at.quote > 0 {
                p += &format!("<w:left w:val=\"single\" w:sz=\"16\" w:space=\"10\" w:color=\"{}\"/>", self.accent);
            }
            if rule {
                p += &format!("<w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"1\" w:color=\"{BORDER}\"/>");
            }
            p += "</w:pBdr>";
        }
        let left = at.indent + at.quote as f64 * 14.;
        if at.number.is_none() && (left > 0. || at.hanging > 0.) {
            p += &format!("<w:ind w:left=\"{}\" w:hanging=\"{}\"/>", tw(left + at.hanging), tw(at.hanging));
        } else if at.number.is_some() && at.quote > 0 {
            p += &format!("<w:ind w:left=\"{}\" w:hanging=\"{}\"/>", tw(left), tw(LIST_STEP * 0.75));
        }
        match at.align {
            Some(Align::Center) => p += "<w:jc w:val=\"center\"/>",
            Some(Align::Right) => p += "<w:jc w:val=\"right\"/>",
            _ => {}
        }
        if p.is_empty() { p } else { format!("<w:pPr>{p}</w:pPr>") }
    }

    fn para(&self, at: &Place, runs: &str) -> String {
        format!("<w:p>{}{runs}</w:p>\n", self.ppr(at, false, false))
    }

    /// The words left for blocks standing at `at`.
    fn room_at(&self, at: &Place) -> f64 {
        (self.room - at.indent - at.quote as f64 * 14. - at.hanging).max(36.)
    }

    fn blocks(&mut self, blocks: &[Block], at: &Place) -> String {
        blocks.iter().map(|b| self.block(b, at)).collect()
    }

    fn block(&mut self, b: &Block, at: &Place) -> String {
        let first = std::mem::replace(&mut self.first_block, false);
        let plain = Run::default();
        match b {
            Block::Para(words) => {
                let runs = self.words(words, &plain);
                self.para(at, &runs)
            }
            Block::Heading(level, words) => {
                let runs = self.words(words, &plain);
                let style = ["Heading1", "Heading2", "Heading3", "Heading4", "Heading5", "Heading6"][(*level as usize).clamp(1, 6) - 1];
                let turn = *level == 1 && self.o.breaks && !first && self.part == Part::Document;
                let placed = Place { style: Some(style), ..at.clone() };
                if self.part == Part::Notes {
                    return format!("<w:p>{}{runs}</w:p>\n", self.ppr(&placed, false, false));
                }
                self.bookmarks += 1;
                let n = self.bookmarks;
                self.headings.push((*level, reading::plain_text(words)));
                format!("<w:p>{}<w:bookmarkStart w:id=\"{n}\" w:name=\"_Toc{n}\"/>{runs}<w:bookmarkEnd w:id=\"{n}\"/></w:p>\n", self.ppr(&placed, turn, false))
            }
            Block::Quote(inner) => {
                let inside = Place { quote: at.quote + 1, number: None, ..at.clone() };
                self.blocks(inner, &inside)
            }
            Block::Code { text, .. } => {
                let look = Run::default();
                let mut runs = String::new();
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        runs += "<w:r><w:br/></w:r>";
                    }
                    runs += &self.text_run(line, &look);
                }
                let placed = Place { style: Some("Code"), number: None, ..at.clone() };
                self.para(&placed, &runs)
            }
            Block::Math(m) => {
                let runs = self.text_run(m, &Run { style: Some("CodeChar"), ..Default::default() });
                self.para(&Place { align: Some(Align::Center), number: None, ..at.clone() }, &runs)
            }
            Block::List { start, items } => self.list(*start, items, at),
            Block::Table { aligns, head, rows } => self.table(aligns, head, rows, at),
            Block::Rule => format!("<w:p>{}</w:p>\n", self.ppr(&Place { number: None, ..at.clone() }, false, true)),
            Block::Picture(p) => {
                let room = self.room_at(at);
                let runs = self.picture(p, room);
                self.para(&Place { number: None, ..at.clone() }, &runs)
            }
            Block::Card(c) => self.card(c, at),
            // A PDF's page, on a page of its own as large as the words'
            // room holds it, under its part's line when it is the first.
            Block::Leaf(p) => {
                let turn = !std::mem::replace(&mut self.leaf_first, false);
                let runs = self.picture_in(p, self.room, self.page_height - 3. * self.size);
                format!("<w:p><w:pPr>{}<w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/><w:jc w:val=\"center\"/></w:pPr>{runs}</w:p>\n", if turn { "<w:pageBreakBefore/>" } else { "" })
            }
            Block::Placed(align, inner) => {
                self.first_block = first;
                self.blocks(inner, &Place { align: Some(*align), ..at.clone() })
            }
        }
    }

    fn list(&mut self, start: Option<u64>, items: &[reading::Item], at: &Place) -> String {
        let depth = (at.indent / LIST_STEP).round() as usize;
        // Bullets share a numbering; each ordered list counts from its own start.
        let num = match start {
            Some(n) => {
                self.ordered.push(n);
                self.ordered.len() + 1
            }
            None => 1,
        };
        let mut out = String::new();
        for item in items {
            let inside = Place { indent: at.indent + LIST_STEP, number: None, align: None, ..at.clone() };
            let mut rest = &item.blocks[..];
            let line = match rest.first() {
                Some(Block::Para(words)) => {
                    rest = &rest[1..];
                    words.clone()
                }
                _ => Vec::new(),
            };
            match item.task {
                Some(done) => {
                    let mark = self.text_run(if done { "☑ " } else { "☐ " }, &Run { color: done.then(|| self.accent.clone()), ..Default::default() });
                    let look = if done { Run { strike: true, color: Some(MUTED.to_string()), ..Default::default() } } else { Run::default() };
                    let runs = self.words(&line, &look);
                    let placed = Place { indent: at.indent, hanging: 14., number: None, ..at.clone() };
                    out += &self.para(&placed, &format!("{mark}{runs}"));
                }
                None => {
                    let runs = self.words(&line, &Run::default());
                    let placed = Place { number: Some((num, depth.min(8))), ..at.clone() };
                    out += &self.para(&placed, &runs);
                }
            }
            self.first_block = false;
            out += &self.blocks(rest, &inside);
        }
        out
    }

    /// A file set after the note, as the editor's medium card: a table of
    /// one row, its name (leading there), a few of its words and its kind
    /// and size, and its first page's top beside them. Word has no round
    /// corners for a table, so its corners are square.
    fn card(&mut self, c: &reading::Card, at: &Place) -> String {
        let room = self.room_at(at);
        let tall = self.size * 7.4;
        let side = if c.picture.is_some() { (self.size * 11.1).min(room * 0.4) } else { 0. };
        let left = room - side;
        let quiet = Run { color: Some(MUTED.to_string()), small: true, ..Default::default() };
        let gap = "<w:pPr><w:spacing w:before=\"0\" w:after=\"60\"/></w:pPr>";
        let name = self.text_run(&c.title, &Run { bold: true, linked: true, ..Default::default() });
        let link = format!("<w:hyperlink w:anchor=\"_OnyxPart{}\" w:history=\"1\">{name}</w:hyperlink>", c.n);
        let link = if c.first { format!("<w:bookmarkStart w:id=\"{id}\" w:name=\"_OnyxFrom{n}\"/>{link}<w:bookmarkEnd w:id=\"{id}\"/>", id = FROM_ID + c.n, n = c.n) } else { link };
        let mut text = format!("<w:p>{gap}{link}</w:p>");
        if let Some(w) = &c.words {
            let short: String = w.chars().take(160).collect();
            text += &format!("<w:p>{gap}{}</w:p>", self.text_run(&short, &quiet));
        }
        text += &format!("<w:p>{gap}{}</w:p>", self.text_run(&c.meta, &quiet));
        let line = |side: &str| format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{BORDER}\"/>");
        let mut out = format!(
            "<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblInd w:w=\"{}\" w:type=\"dxa\"/><w:tblBorders>{}{}{}{}{}</w:tblBorders><w:tblLayout w:type=\"fixed\"/><w:tblCellMar><w:top w:w=\"{pad}\" w:type=\"dxa\"/><w:left w:w=\"{pad}\" w:type=\"dxa\"/><w:bottom w:w=\"{pad}\" w:type=\"dxa\"/><w:right w:w=\"{pad}\" w:type=\"dxa\"/></w:tblCellMar><w:tblLook w:val=\"0000\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"{}\"/>",
            tw(room),
            tw(at.indent + at.quote as f64 * 14.),
            line("top"),
            line("left"),
            line("bottom"),
            line("right"),
            line("insideV"),
            tw(left),
            pad = tw(self.size),
        );
        if side > 0. {
            out += &format!("<w:gridCol w:w=\"{}\"/>", tw(side));
        }
        out += &format!("</w:tblGrid><w:tr><w:trPr><w:cantSplit/><w:trHeight w:val=\"{}\" w:hRule=\"exact\"/></w:trPr><w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/></w:tcPr>{text}</w:tc>", tw(tall), tw(left));
        if let Some(p) = c.picture.as_ref().filter(|_| side > 0.) {
            let runs = self.picture_in(p, side, tall);
            out += &format!("<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/><w:tcMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"0\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"0\" w:type=\"dxa\"/></w:tcMar></w:tcPr><w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>{runs}</w:p></w:tc>", tw(side));
        }
        // Word needs a paragraph between a table and what follows.
        out + "</w:tr></w:tbl>\n<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"120\" w:lineRule=\"exact\"/></w:pPr></w:p>\n"
    }

    fn table(&mut self, aligns: &[Align], head: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], at: &Place) -> String {
        let columns = aligns.len().max(head.len()).max(1);
        let room = self.room_at(at);
        let each = tw(room / columns as f64);
        let mut out = format!(
            "<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblInd w:w=\"{}\" w:type=\"dxa\"/><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{BORDER}\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{BORDER}\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{BORDER}\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{BORDER}\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{BORDER}\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"{BORDER}\"/></w:tblBorders><w:tblLayout w:type=\"autofit\"/><w:tblCellMar><w:top w:w=\"{pad_y}\" w:type=\"dxa\"/><w:left w:w=\"{pad_x}\" w:type=\"dxa\"/><w:bottom w:w=\"{pad_y}\" w:type=\"dxa\"/><w:right w:w=\"{pad_x}\" w:type=\"dxa\"/></w:tblCellMar><w:tblLook w:val=\"0000\"/></w:tblPr><w:tblGrid>",
            each * columns as i64,
            tw(at.indent + at.quote as f64 * 14.),
            pad_y = tw(self.size * 0.35),
            pad_x = tw(self.size * 0.6),
        );
        for _ in 0..columns {
            out += &format!("<w:gridCol w:w=\"{each}\"/>");
        }
        out += "</w:tblGrid>";
        let row_of = |w: &mut Self, cells: &[Vec<Inline>], header: bool| {
            let mut r = String::from("<w:tr>");
            r += if header { "<w:trPr><w:cantSplit/><w:tblHeader/></w:trPr>" } else { "<w:trPr><w:cantSplit/></w:trPr>" };
            for i in 0..columns {
                let words = cells.get(i).map(Vec::as_slice).unwrap_or(&[]);
                let look = Run { bold: header, ..Default::default() };
                let runs = w.words(words, &look);
                let jc = match aligns.get(i) {
                    Some(Align::Center) => "<w:jc w:val=\"center\"/>",
                    Some(Align::Right) => "<w:jc w:val=\"right\"/>",
                    _ => "",
                };
                let shade = if header { format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{PLATE}\"/>") } else { String::new() };
                r += &format!("<w:tc><w:tcPr><w:tcW w:w=\"{each}\" w:type=\"dxa\"/>{shade}</w:tcPr><w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/>{jc}</w:pPr>{runs}</w:p></w:tc>");
            }
            r + "</w:tr>"
        };
        out += &row_of(self, head, true);
        for row in rows {
            out += &row_of(self, row, false);
        }
        // Word needs a paragraph between a table and what follows.
        out + "</w:tbl>\n<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"120\" w:lineRule=\"exact\"/></w:pPr></w:p>\n"
    }
}

fn rels_xml(rels: &[(String, &str, String, bool)], fixed: &str) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    out += fixed;
    for (id, kind, target, external) in rels {
        let mode = if *external { " TargetMode=\"External\"" } else { "" };
        out += &format!("<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{}\"{mode}/>", esc(target));
    }
    out + "</Relationships>"
}

const NAMESPACES: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"";

/// A family list's first name and, when it has one, the face behind it
/// for Chinese, Japanese and Korean.
fn faces(list: &[String]) -> (String, String) {
    let first = list.first().cloned().unwrap_or_else(|| "Inter".to_string());
    let east = list.get(1).cloned().unwrap_or_else(|| first.clone());
    (esc(&first), esc(&east))
}

fn styles_xml(look: &Look, o: &PdfConfig, size: f64, accent: &str, link: &str) -> String {
    let (words, words_east) = faces(&look.words);
    let (code, code_east) = faces(&look.code);
    let half = |pt: f64| (pt * 2.).round() as i64;
    // Typst's leading as Word's line: the face's own line, about 1.2 of
    // its size, and the leading over it.
    let leading: f64 = match o.spacing.as_str() {
        "tight" => 0.55,
        "loose" => 1.,
        _ => 0.75,
    };
    let line = ((1. + leading) / 1.2 * 240.).round() as i64;
    let after = tw(0.5 * size);
    let underline = if o.color { "" } else { "<w:u w:val=\"single\"/>" };
    let mut s = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles {NAMESPACES}>\
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{words}\" w:hAnsi=\"{words}\" w:eastAsia=\"{words_east}\" w:cs=\"{words}\"/><w:color w:val=\"{INK}\"/><w:sz w:val=\"{sz}\"/><w:szCs w:val=\"{sz}\"/><w:lang w:val=\"en-US\" w:eastAsia=\"zh-CN\"/></w:rPr></w:rPrDefault>\
<w:pPrDefault><w:pPr><w:spacing w:before=\"0\" w:after=\"{after}\" w:line=\"{line}\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>\
<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/><w:uiPriority w:val=\"1\"/><w:semiHidden/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:pBdr><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"6\" w:color=\"{BORDER}\"/></w:pBdr><w:spacing w:after=\"{title_after}\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val=\"{title_sz}\"/><w:szCs w:val=\"{title_sz}\"/></w:rPr></w:style>",
        sz = half(size),
        title_sz = half(size * rhythm::TITLE as f64),
        title_after = tw(size * 1.5),
    );
    for (i, h) in rhythm::HEADINGS.iter().enumerate() {
        let pt = size * h.scale as f64;
        s += &format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{n}\"><w:name w:val=\"heading {n}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"{before}\" w:after=\"{after}\" w:line=\"{line}\" w:lineRule=\"auto\"/><w:outlineLvl w:val=\"{i}\"/></w:pPr><w:rPr><w:b/><w:bCs/><w:sz w:val=\"{sz}\"/><w:szCs w:val=\"{sz}\"/></w:rPr></w:style>",
            n = i + 1,
            before = tw(pt * 1.2),
            after = tw(pt * 0.45),
            line = (h.line as f64 * 240.).round() as i64,
            sz = half(pt),
        );
    }
    s += &format!(
        "<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Code\"><w:name w:val=\"Code\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepLines/><w:pBdr><w:top w:val=\"single\" w:sz=\"4\" w:space=\"8\" w:color=\"{PLATE}\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"8\" w:color=\"{PLATE}\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"8\" w:color=\"{PLATE}\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"8\" w:color=\"{PLATE}\"/></w:pBdr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{PLATE}\"/><w:spacing w:before=\"{code_gap}\" w:after=\"{code_gap}\" w:line=\"276\" w:lineRule=\"auto\"/><w:ind w:left=\"{code_in}\" w:right=\"{code_in}\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"{code}\" w:hAnsi=\"{code}\" w:eastAsia=\"{code_east}\" w:cs=\"{code}\"/><w:sz w:val=\"{code_sz}\"/><w:szCs w:val=\"{code_sz}\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"CodeChar\"><w:name w:val=\"Code Char\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:rPr><w:rFonts w:ascii=\"{code}\" w:hAnsi=\"{code}\" w:eastAsia=\"{code_east}\" w:cs=\"{code}\"/><w:sz w:val=\"{code_sz}\"/><w:szCs w:val=\"{code_sz}\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:uiPriority w:val=\"99\"/><w:rPr><w:color w:val=\"{link}\"/>{underline}</w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"FootnoteText\"><w:name w:val=\"footnote text\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"60\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:sz w:val=\"{note_sz}\"/><w:szCs w:val=\"{note_sz}\"/></w:rPr></w:style>\
<w:style w:type=\"character\" w:styleId=\"FootnoteReference\"><w:name w:val=\"footnote reference\"/><w:basedOn w:val=\"DefaultParagraphFont\"/><w:rPr><w:color w:val=\"{accent}\"/><w:vertAlign w:val=\"superscript\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"TOC1\"><w:name w:val=\"toc 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"60\"/></w:pPr><w:rPr><w:b/><w:bCs/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"TOC2\"><w:name w:val=\"toc 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"60\"/><w:ind w:left=\"{toc2}\"/></w:pPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"TOC3\"><w:name w:val=\"toc 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"60\"/><w:ind w:left=\"{toc3}\"/></w:pPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Footer\"><w:name w:val=\"footer\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:spacing w:after=\"0\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr><w:color w:val=\"{MUTED}\"/><w:sz w:val=\"{foot_sz}\"/><w:szCs w:val=\"{foot_sz}\"/></w:rPr></w:style>\
</w:styles>",
        code_sz = half(size * 0.86),
        note_sz = half(size * 0.86),
        foot_sz = half(size * 0.8),
        code_gap = tw(size * 0.9),
        code_in = tw(size * 0.6),
        toc2 = tw(size * 1.2),
        toc3 = tw(size * 2.4),
    );
    s
}

fn numbering_xml(ordered: &[u64]) -> String {
    let mut s = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering {NAMESPACES}>");
    let level = |i: usize, fmt: &str, text: &str| {
        let left = tw(LIST_STEP * (i + 1) as f64);
        format!("<w:lvl w:ilvl=\"{i}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{left}\" w:hanging=\"{}\"/></w:pPr></w:lvl>", tw(LIST_STEP * 0.75))
    };
    s += "<w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"hybridMultilevel\"/>";
    for i in 0..9 {
        s += &level(i, "bullet", ["•", "◦", "▪"][i % 3]);
    }
    s += "</w:abstractNum>";
    // Each ordered list is a numbering of its own, its first level
    // counting from the list's start, which every reader honors.
    for (i, start) in ordered.iter().enumerate() {
        s += &format!("<w:abstractNum w:abstractNumId=\"{}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>", i + 1);
        for l in 0..9 {
            let fmt = ["decimal", "lowerLetter", "lowerRoman"][l % 3];
            let from = if l == 0 { *start } else { 1 };
            s += &level(l, fmt, &format!("%{}.", l + 1)).replacen("<w:start w:val=\"1\"/>", &format!("<w:start w:val=\"{from}\"/>"), 1);
        }
        s += "</w:abstractNum>";
    }
    s += "<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>";
    for i in 0..ordered.len() {
        s += &format!("<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"{}\"/></w:num>", i + 2, i + 1);
    }
    s + "</w:numbering>"
}

/// The note as a Word document.
pub fn write(note: &Note, look: &Look, o: &PdfConfig) -> Result<Vec<u8>, String> {
    let (page_w, page_h) = reading::paper(o);
    let margin = reading::margin(o);
    let attached = if o.attached { crate::attach::gather(note) } else { Vec::new() };
    // A PDF's page drawn at twice the words' width in points.
    let doc = reading::read(note, o.name, &attached, ((page_w - 2. * margin) * 2.).round().clamp(600., 2400.) as u32);
    let size = o.size.clamp(PdfConfig::LEAST_SIZE, PdfConfig::MOST_SIZE) as f64;
    let (accent, link, chip, wash) = if o.color { (hex(&look.accent), hex(&look.link), reading::tint(&look.accent, 0.1), "F7E2B0".to_string()) } else { (MUTED.to_string(), INK.to_string(), "F0EFED".to_string(), "E4E3E0".to_string()) };
    let mut w = Writer {
        doc: &doc,
        o,
        size,
        accent: accent.clone(),
        chip,
        wash,
        room: page_w - 2. * margin,
        page_height: page_h - 2. * margin,
        part: Part::Document,
        rels: [Vec::new(), Vec::new()],
        media: Vec::new(),
        files: HashMap::new(),
        drawings: 0,
        bookmarks: 0,
        ordered: Vec::new(),
        first_block: true,
        headings: Vec::new(),
        leaf_first: false,
    };

    let mut body = String::new();
    if let Some(title) = &doc.title {
        body += &format!("<w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr>{}</w:p>\n", w.text_run(title, &Run::default()));
    }
    let mut main = w.blocks(&doc.blocks, &Place::default());
    // The files set after the note, each from a new page under a quiet
    // line: the way back at the left, the file's name at the right.
    if doc.parts.iter().any(|p| !p.back) {
        body += &format!("<w:bookmarkStart w:id=\"{TOP_ID}\" w:name=\"_OnyxTop\"/><w:bookmarkEnd w:id=\"{TOP_ID}\"/>");
    }
    for (n, part) in doc.parts.iter().enumerate() {
        // Quiet, and not underlined as a reader's own link style would.
        let half = (size * 0.86 * 2.).round() as i64;
        let quiet = |t: &str, tab: &str| format!("<w:r><w:rPr><w:color w:val=\"{MUTED}\"/><w:sz w:val=\"{half}\"/><w:szCs w:val=\"{half}\"/><w:u w:val=\"none\"/></w:rPr>{tab}<w:t xml:space=\"preserve\">{}</w:t></w:r>", esc(t));
        let back = if part.back { format!("_OnyxFrom{n}") } else { "_OnyxTop".to_string() };
        let up = quiet(&format!("↑ {}", note.title), "");
        let name = quiet(&part.name, "<w:tab/>");
        main += &format!(
            "<w:p><w:pPr><w:pageBreakBefore/><w:tabs><w:tab w:val=\"right\" w:pos=\"{}\"/></w:tabs><w:spacing w:after=\"{}\"/></w:pPr><w:bookmarkStart w:id=\"{id}\" w:name=\"_OnyxPart{n}\"/><w:hyperlink w:anchor=\"{back}\" w:history=\"1\">{up}</w:hyperlink>{name}<w:bookmarkEnd w:id=\"{id}\"/></w:p>\n",
            tw(w.room),
            tw(size * 1.2),
            id = PART_ID + n
        );
        if let Some(title) = &part.title {
            main += &format!("<w:p><w:pPr><w:pStyle w:val=\"Title\"/></w:pPr>{}</w:p>\n", w.text_run(title, &Run::default()));
        }
        w.leaf_first = true;
        w.first_block = true;
        main += &w.blocks(&part.blocks, &Place::default());
    }

    // The contents, three levels deep as the PDF's are, written in so
    // they show before Word updates the field.
    if o.contents && !w.headings.is_empty() {
        let top = w.headings.iter().map(|(l, _)| *l).filter(|l| *l <= 3).min().unwrap_or(1);
        let entries: Vec<(usize, u8, String)> = w.headings.iter().enumerate().filter(|(_, (l, _))| *l <= 3).map(|(i, (l, t))| (i + 1, l - top + 1, t.clone())).collect();
        for (k, (n, level, words)) in entries.iter().enumerate() {
            let begin = if k == 0 { "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> TOC \\o \"1-3\" \\h \\z \\u \\n </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>" } else { "" };
            let run = w.text_run(words, &Run::default());
            body += &format!("<w:p><w:pPr><w:pStyle w:val=\"TOC{level}\"/></w:pPr>{begin}<w:hyperlink w:anchor=\"_Toc{n}\" w:history=\"1\">{run}</w:hyperlink></w:p>\n");
        }
        body += "<w:p><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>\n";
        if o.breaks {
            body += "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\n";
        }
    }
    body += &main;

    // Footnotes, each the blocks of its body, the first with its number.
    w.part = Part::Notes;
    let mut notes = String::from("<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>");
    for (i, blocks) in doc.notes.iter().enumerate() {
        let n = i + 1;
        let mut inner = w.blocks(blocks, &Place { style: Some("FootnoteText"), ..Default::default() });
        let mark = "<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> </w:t></w:r>";
        match inner.find("</w:pPr>") {
            Some(at) if inner.starts_with("<w:p>") => inner.insert_str(at + "</w:pPr>".len(), mark),
            _ => inner = format!("<w:p><w:pPr><w:pStyle w:val=\"FootnoteText\"/></w:pPr>{mark}</w:p>{inner}"),
        }
        notes += &format!("<w:footnote w:id=\"{n}\">{inner}</w:footnote>");
    }

    let (tw_w, tw_h, tw_m) = (tw(page_w), tw(page_h), tw(margin));
    let orient = if o.landscape { " w:orient=\"landscape\"" } else { "" };
    let footer = if o.numbers { "<w:footerReference w:type=\"default\" r:id=\"rId3\"/>" } else { "" };
    let section = format!("<w:sectPr>{footer}<w:pgSz w:w=\"{tw_w}\" w:h=\"{tw_h}\"{orient}/><w:pgMar w:top=\"{tw_m}\" w:right=\"{tw_m}\" w:bottom=\"{tw_m}\" w:left=\"{tw_m}\" w:header=\"{h}\" w:footer=\"{h}\" w:gutter=\"0\"/></w:sectPr>", h = tw(margin / 2.));
    let document = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document {NAMESPACES}><w:body>\n{body}{section}</w:body></w:document>");
    let footnotes = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:footnotes {NAMESPACES}>{notes}</w:footnotes>");
    let footer_xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:ftr {NAMESPACES}><w:p><w:pPr><w:pStyle w:val=\"Footer\"/></w:pPr><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p></w:ftr>");

    let rel = |id: &str, kind: &str, target: &str| format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{target}\"/>");
    let mut fixed = rel("rId1", "styles", "styles.xml") + &rel("rId2", "numbering", "numbering.xml") + &rel("rId4", "settings", "settings.xml") + &rel("rId5", "footnotes", "footnotes.xml");
    if o.numbers {
        fixed += &rel("rId3", "footer", "footer1.xml");
    }
    let document_rels = rels_xml(&w.rels[0], &fixed);
    let notes_rels = rels_xml(&w.rels[1], "");
    let settings = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings {NAMESPACES}><w:defaultTabStop w:val=\"720\"/><w:characterSpacingControl w:val=\"doNotCompress\"/><w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr><w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat></w:settings>");
    let footer_type = if o.numbers { "<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>" } else { "" };
    let types = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/><Default Extension=\"gif\" ContentType=\"image/gif\"/><Default Extension=\"bmp\" ContentType=\"image/bmp\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/><Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/>{footer_type}<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>"
    );
    let package_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>";
    let core = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title></cp:coreProperties>", esc(note.title));
    let app = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Onyx</Application></Properties>";

    let mut parts: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), types.into_bytes()),
        ("_rels/.rels".into(), package_rels.as_bytes().to_vec()),
        ("docProps/core.xml".into(), core.into_bytes()),
        ("docProps/app.xml".into(), app.as_bytes().to_vec()),
        ("word/document.xml".into(), document.into_bytes()),
        ("word/_rels/document.xml.rels".into(), document_rels.into_bytes()),
        ("word/styles.xml".into(), styles_xml(look, o, size, &accent, &link).into_bytes()),
        ("word/numbering.xml".into(), numbering_xml(&w.ordered).into_bytes()),
        ("word/settings.xml".into(), settings.into_bytes()),
        ("word/footnotes.xml".into(), footnotes.into_bytes()),
        ("word/_rels/footnotes.xml.rels".into(), notes_rels.into_bytes()),
    ];
    if o.numbers {
        parts.push(("word/footer1.xml".into(), footer_xml.into_bytes()));
    }
    for (name, bytes) in std::mem::take(&mut w.media) {
        parts.push((format!("word/media/{name}"), bytes));
    }
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in parts {
        // A picture is packed already; deflating it again buys nothing.
        let packed = name.starts_with("word/media/");
        let options = SimpleFileOptions::default().compression_method(if packed { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated });
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

