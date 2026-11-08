//! A note as one web page: an HTML file that holds everything it shows,
//! its pictures written into it, so it opens anywhere as it was made.
//!
//! The note is read by `reading::read`, as the Word writer reads it, and
//! set with the export sheet's options: the faces, the size, the line
//! spacing, the name on top, the contents, and color. Paper, margins and
//! page numbers belong to pages and are not here. A picture at an address
//! stays at its address; one on the disk goes in as a `data:` address.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::PdfConfig;
use crate::html::Width;
use crate::pdf::{Look, Note};
use crate::reading::{self, Align, Block, Doc, Inline, Mark, Pic};
use crate::rhythm;

/// How wide a PDF's page set after the note is drawn, in pixels: the
/// words' width at twice a screen's pixels.
const LEAF_WIDTH: u32 = 1400;

/// The path to write `chosen` at, ending in `.html`.
pub fn output_path(chosen: &Path) -> PathBuf {
    crate::pdf::finished(chosen, "html")
}

/// `s` with what HTML reads as markup written as entities.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// A list of families as CSS writes it. The first is the note's face;
/// a reader without it gets the system's own `like` faces before the
/// faces for Chinese, Japanese and Korean, which carry Latin letters of
/// their own and would take them over.
fn families(names: &[String], like: &[&str], generic: &str) -> String {
    let quoted = |n: &str| format!("\"{}\"", n.replace(['"', '\\'], ""));
    let mut out: Vec<String> = names.iter().take(1).map(|n| quoted(n)).collect();
    out.extend(like.iter().map(|n| n.to_string()));
    out.extend(names.iter().skip(1).map(|n| quoted(n)));
    out.push(generic.to_string());
    out.join(", ")
}

struct Writer<'a> {
    doc: &'a Doc,
    /// Off, a tag's color is left out, as on paper.
    color: bool,
    out: String,
    /// Each heading's anchor, in order, given as the contents are made.
    anchors: Vec<String>,
    heading: usize,
}

impl Writer<'_> {
    fn words(&mut self, words: &[Inline]) {
        for w in words {
            match w {
                Inline::Text(t) => self.out.push_str(&esc(t)),
                Inline::Code(t) => self.out += &format!("<code>{}</code>", esc(t)),
                Inline::Math(m, false) => self.out += &format!("<code class=\"math\">{}</code>", esc(m)),
                Inline::Math(m, true) => self.out += &format!("<code class=\"math display\">{}</code>", esc(m)),
                Inline::Break => self.out.push_str("<br>"),
                Inline::Marked(Mark::Ink(..) | Mark::Wash(..), inner) if !self.color => self.words(inner),
                Inline::Marked(mark, inner) => {
                    let (open, close) = match mark {
                        Mark::Strong => ("<strong>".to_string(), "</strong>"),
                        Mark::Em => ("<em>".to_string(), "</em>"),
                        Mark::Strike => ("<s>".to_string(), "</s>"),
                        Mark::Underline => ("<u>".to_string(), "</u>"),
                        Mark::Highlight => ("<mark>".to_string(), "</mark>"),
                        Mark::Code => ("<code>".to_string(), "</code>"),
                        Mark::Sub => ("<sub>".to_string(), "</sub>"),
                        Mark::Sup => ("<sup>".to_string(), "</sup>"),
                        Mark::Small => ("<small>".to_string(), "</small>"),
                        Mark::Ink(r, g, b) => (format!("<span class=\"ink\" style=\"color: rgb({r}, {g}, {b})\">"), "</span>"),
                        Mark::Wash(r, g, b) => (format!("<span class=\"wash\" style=\"background: rgba({r}, {g}, {b}, 0.5)\">"), "</span>"),
                    };
                    self.out.push_str(&open);
                    self.words(inner);
                    self.out.push_str(close);
                }
                Inline::Link(Some(url), inner) => {
                    self.out += &format!("<a href=\"{}\">", esc(url));
                    self.words(inner);
                    self.out.push_str("</a>");
                }
                Inline::Link(None, inner) => self.words(inner),
                Inline::Jump(n, first, inner) => {
                    let id = if *first { format!(" id=\"from-{n}\"") } else { String::new() };
                    self.out += &format!("<a href=\"#part-{n}\"{id}>");
                    self.words(inner);
                    self.out.push_str("</a>");
                }
                Inline::Picture(p) => self.picture(p),
                // A card among other words: its name, leading there.
                Inline::Card(c) => {
                    let id = if c.first { format!(" id=\"from-{}\"", c.n) } else { String::new() };
                    self.out += &format!("<a href=\"#part-{}\"{id}>{}</a>", c.n, esc(&c.title));
                }
                Inline::Note(n, first) => {
                    let id = if *first { format!(" id=\"ref-{n}\"") } else { String::new() };
                    self.out += &format!("<sup class=\"ref\"{id}><a href=\"#note-{n}\">{n}</a></sup>");
                }
            }
        }
    }

    /// A picture as an `<img>`: its file written in, or its address; with
    /// neither, its words.
    fn picture(&mut self, p: &Pic) {
        let src = match p.found {
            Some(n) => {
                let pic = &self.doc.pictures[n];
                format!("data:{};base64,{}", pic.mime(), reading::base64(&pic.bytes))
            }
            None if p.address => p.src.clone(),
            None => {
                self.out.push_str(&esc(&p.alt));
                return;
            }
        };
        let width = match p.width {
            Some(Width::Px(w)) => format!(" width=\"{w}\""),
            Some(Width::Share(n)) => format!(" style=\"width: {n}%\""),
            None => String::new(),
        };
        self.out += &format!("<img src=\"{}\" alt=\"{}\"{width}>", esc(&src), esc(&p.alt));
    }

    fn blocks(&mut self, blocks: &[Block]) {
        for b in blocks {
            self.block(b);
        }
    }

    fn block(&mut self, b: &Block) {
        match b {
            Block::Para(words) => {
                self.out.push_str("<p>");
                self.words(words);
                self.out.push_str("</p>\n");
            }
            Block::Heading(level, words) => {
                let id = self.anchors.get(self.heading).cloned().unwrap_or_default();
                self.heading += 1;
                self.out += &format!("<h{level} id=\"{}\">", esc(&id));
                self.words(words);
                self.out += &format!("</h{level}>\n");
            }
            Block::Quote(inner) => {
                self.out.push_str("<blockquote>\n");
                self.blocks(inner);
                self.out.push_str("</blockquote>\n");
            }
            Block::Code { lang, text } => {
                let class = if lang.is_empty() { String::new() } else { format!(" class=\"language-{}\"", esc(lang)) };
                self.out += &format!("<pre><code{class}>{}</code></pre>\n", esc(text));
            }
            Block::Math(m) => self.out += &format!("<pre class=\"math\"><code>{}</code></pre>\n", esc(m)),
            Block::List { start, items } => {
                let tasks = items.iter().any(|i| i.task.is_some());
                let (open, close) = match start {
                    Some(n) if *n != 1 => (format!("<ol start=\"{n}\""), "</ol>"),
                    Some(_) => ("<ol".to_string(), "</ol>"),
                    None => ("<ul".to_string(), "</ul>"),
                };
                self.out += &format!("{open}{}>\n", if tasks { " class=\"tasks\"" } else { "" });
                for item in items {
                    match item.task {
                        Some(done) => {
                            // Only the task's own line is struck when it is done.
                            self.out += &format!("<li class=\"task{}\"><input type=\"checkbox\" disabled{}> ", if done { " done" } else { "" }, if done { " checked" } else { "" });
                            let mut rest = &item.blocks[..];
                            if let Some(Block::Para(words)) = rest.first() {
                                self.out.push_str("<span class=\"line\">");
                                self.words(words);
                                self.out.push_str("</span>\n");
                                rest = &rest[1..];
                            }
                            self.blocks(rest);
                        }
                        None => {
                            self.out.push_str(if tasks { "<li class=\"plain\">" } else { "<li>" });
                            // A tight item's words stand in it, as a browser sets them.
                            match &item.blocks[..] {
                                [Block::Para(words), rest @ ..] if rest.iter().all(|b| matches!(b, Block::List { .. })) => {
                                    self.words(words);
                                    self.out.push('\n');
                                    self.blocks(rest);
                                }
                                all => self.blocks(all),
                            }
                        }
                    }
                    self.out.push_str("</li>\n");
                }
                self.out += &format!("{close}\n");
            }
            Block::Table { aligns, head, rows } => {
                let align = |i: usize| match aligns.get(i) {
                    Some(Align::Center) => " style=\"text-align: center\"",
                    Some(Align::Right) => " style=\"text-align: right\"",
                    _ => "",
                };
                self.out.push_str("<table>\n<thead><tr>");
                for (i, cell) in head.iter().enumerate() {
                    self.out += &format!("<th{}>", align(i));
                    self.words(cell);
                    self.out.push_str("</th>");
                }
                self.out.push_str("</tr></thead>\n<tbody>\n");
                for row in rows {
                    self.out.push_str("<tr>");
                    for (i, cell) in row.iter().enumerate() {
                        self.out += &format!("<td{}>", align(i));
                        self.words(cell);
                        self.out.push_str("</td>");
                    }
                    self.out.push_str("</tr>\n");
                }
                self.out.push_str("</tbody>\n</table>\n");
            }
            Block::Rule => self.out.push_str("<hr>\n"),
            Block::Picture(p) => {
                self.out.push_str("<p class=\"picture\">");
                self.picture(p);
                self.out.push_str("</p>\n");
            }
            // A file set after the note, as the editor's medium card.
            Block::Card(c) => {
                let id = if c.first { format!(" id=\"from-{}\"", c.n) } else { String::new() };
                self.out += &format!("<a class=\"card\" href=\"#part-{}\"{id}><span class=\"card-text\"><span class=\"card-title\">{}</span>", c.n, esc(&c.title));
                if let Some(w) = &c.words {
                    self.out += &format!("<span class=\"card-words\">{}</span>", esc(w));
                }
                self.out += &format!("<span class=\"card-meta\">{}</span></span>", esc(&c.meta));
                if let Some(p) = &c.picture {
                    self.out.push_str("<span class=\"card-picture\">");
                    self.picture(p);
                    self.out.push_str("</span>");
                }
                self.out.push_str("</a>\n");
            }
            Block::Leaf(p) => {
                self.out.push_str("<p class=\"leaf\">");
                self.picture(p);
                self.out.push_str("</p>\n");
            }
            Block::Placed(align, inner) => {
                let side = match align {
                    Align::Center => "center",
                    Align::Right => "right",
                    Align::Left => "left",
                };
                self.out += &format!("<div style=\"text-align: {side}\">\n");
                self.blocks(inner);
                self.out.push_str("</div>\n");
            }
        }
    }
}

/// Every heading of the note, in the order the page sets them, with its
/// level and words.
fn headings(blocks: &[Block], out: &mut Vec<(u8, String)>) {
    for b in blocks {
        match b {
            Block::Heading(level, words) => out.push((*level, reading::plain_text(words))),
            Block::Placed(_, inner) | Block::Quote(inner) => headings(inner, out),
            Block::List { items, .. } => items.iter().for_each(|i| headings(&i.blocks, out)),
            _ => {}
        }
    }
}

/// An anchor for each heading: its words, lower case, spaces as dashes,
/// a number after one already taken.
fn anchors(heads: &[(u8, String)]) -> Vec<String> {
    let mut taken: HashMap<String, usize> = HashMap::new();
    heads
        .iter()
        .map(|(_, words)| {
            let mut slug: String = words.trim().to_lowercase().chars().filter_map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { Some(c) } else if c.is_whitespace() { Some('-') } else { None }).collect();
            if slug.is_empty() {
                slug = "section".to_string();
            }
            let n = taken.entry(slug.clone()).or_insert(0);
            *n += 1;
            if *n > 1 { format!("{slug}-{}", *n - 1) } else { slug }
        })
        .collect()
}

/// The note as one web page.
pub fn write(note: &Note, look: &Look, o: &PdfConfig) -> Result<String, String> {
    let attached = if o.attached { crate::attach::gather(note) } else { Vec::new() };
    let doc = reading::read(note, o.name, &attached, LEAF_WIDTH);
    let mut heads = Vec::new();
    headings(&doc.blocks, &mut heads);
    for part in &doc.parts {
        headings(&part.blocks, &mut heads);
    }
    let anchors = anchors(&heads);

    let ink = "#1F1E1D";
    let muted = "#77746B";
    let border = "#DFDFDC";
    let plate = "#F4F3F1";
    let (accent, link, chip, wash) = if o.color { (look.accent.clone(), look.link.clone(), format!("#{}", reading::tint(&look.accent, 0.1)), "#F7E2B0".to_string()) } else { (muted.to_string(), ink.to_string(), "#F0EFED".to_string(), "#E4E3E0".to_string()) };
    let size = o.size.clamp(PdfConfig::LEAST_SIZE, PdfConfig::MOST_SIZE);
    let line = match o.spacing.as_str() {
        "tight" => 1.45,
        "loose" => 1.85,
        _ => 1.6,
    };
    let words = families(&look.words.iter().chain(&look.fallbacks).cloned().collect::<Vec<_>>(), &["-apple-system", "system-ui", "\"Segoe UI\""], "sans-serif");
    let code = families(&look.code.iter().chain(&look.fallbacks).cloned().collect::<Vec<_>>(), &["ui-monospace", "\"SF Mono\"", "Menlo", "Consolas"], "monospace");
    let heading_sizes: String = rhythm::HEADINGS.iter().enumerate().map(|(i, h)| format!("h{} {{ font-size: {}em; }}\n", i + 1, h.scale)).collect();
    let title_scale = rhythm::TITLE;
    let underline = if o.color { "none" } else { "underline" };

    let style = format!(
        r#"
:root {{ color-scheme: light; }}
body {{ margin: 0; background: #FFFFFF; color: {ink}; font-family: {words}; font-size: {size}pt; line-height: {line}; -webkit-text-size-adjust: 100%; }}
article {{ max-width: 44em; margin: 0 auto; padding: 3em 1.5em 4em; overflow-wrap: anywhere; }}
h1, h2, h3, h4, h5, h6 {{ font-weight: 600; line-height: 1.3; margin: 1.5em 0 0.6em; }}
h1 {{ font-weight: 700; }}
{heading_sizes}.title {{ font-size: {title_scale}em; font-weight: 700; line-height: 1.25; margin: 0 0 1em; padding-bottom: 0.35em; border-bottom: 0.5pt solid {border}; }}
p {{ margin: 0 0 0.8em; }}
a {{ color: {link}; text-decoration: {underline}; }}
a:hover {{ text-decoration: underline; }}
code, pre {{ font-family: {code}; font-size: 0.86em; }}
code {{ background: {chip}; border-radius: 3pt; padding: 0.1em 0.25em; }}
pre {{ background: {plate}; border-radius: 5pt; padding: 0.9em 1em; margin: 1.2em 0; white-space: pre-wrap; overflow-wrap: anywhere; line-height: 1.5; tab-size: 4; }}
pre code {{ background: none; padding: 0; font-size: 1em; }}
pre.math {{ text-align: center; background: none; }}
mark {{ background: {wash}; color: inherit; border-radius: 2pt; padding: 0 0.1em; }}
blockquote {{ margin: 1.2em 0; padding: 0.1em 0 0.1em 1em; border-left: 2pt solid {accent}; }}
blockquote > :last-child {{ margin-bottom: 0; }}
ul, ol {{ margin: 0 0 0.8em; padding-left: 1.6em; }}
li > ul, li > ol {{ margin-bottom: 0; }}
ul.tasks {{ list-style: none; padding-left: 0.3em; }}
li.task > ul, li.task > ol {{ margin-left: 1.3em; }}
li.task input {{ margin: 0 0.4em 0 0; accent-color: {accent}; vertical-align: -0.1em; }}
li.task.done > .line {{ color: {muted}; text-decoration: line-through; }}
li.plain {{ list-style: disc; margin-left: 1.3em; }}
table {{ border-collapse: collapse; margin: 1.2em 0; }}
th, td {{ border: 0.5pt solid {border}; padding: 0.5em 0.6em; vertical-align: top; }}
th {{ background: {plate}; font-weight: 600; text-align: left; }}
hr {{ border: 0; border-top: 0.5pt solid {border}; margin: 1.5em 0; }}
img {{ max-width: 100%; height: auto; }}
p.picture {{ margin: 1em 0; }}
sup.ref a {{ text-decoration: none; }}
nav.contents {{ margin: 0 0 2em; }}
nav.contents ul {{ list-style: none; padding-left: 1.2em; margin: 0; }}
nav.contents > ul {{ padding-left: 0; }}
nav.contents > ul > li > a {{ font-weight: 600; }}
nav.contents a {{ color: inherit; }}
section.notes {{ margin-top: 2.5em; font-size: 0.86em; color: {ink}; }}
section.notes hr {{ width: 24%; margin: 0 0 1em; }}
section.notes ol {{ padding-left: 1.4em; }}
a.back {{ text-decoration: none; margin-left: 0.3em; }}
section.part {{ margin-top: 3em; padding-top: 1.2em; border-top: 0.5pt solid {border}; }}
p.part-head {{ display: flex; justify-content: space-between; gap: 1em; font-size: 0.86em; color: {muted}; margin: 0 0 1.4em; }}
p.part-head a {{ color: {muted}; text-decoration: none; }}
p.part-head a:hover {{ text-decoration: underline; }}
a.card {{ display: flex; height: 7.4em; margin: 1.2em 0; border: 0.5pt solid {border}; border-radius: 6pt; overflow: hidden; color: {ink}; text-decoration: none; }}
a.card .card-text {{ flex: 1; min-width: 0; display: flex; flex-direction: column; padding: 1em 1.1em; }}
a.card .card-title {{ font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }}
a.card .card-words {{ margin-top: 0.35em; font-size: 0.86em; color: {muted}; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }}
a.card .card-meta {{ margin-top: auto; font-size: 0.8em; color: {muted}; }}
a.card .card-picture {{ width: 11.1em; flex-shrink: 0; border-left: 0.5pt solid {border}; background: #fff; }}
a.card .card-picture img {{ display: block; width: 100%; height: 100%; object-fit: cover; object-position: top; }}
p.leaf {{ margin: 0 0 1.2em; }}
p.leaf img {{ display: block; width: 100%; border: 0.5pt solid {border}; }}
@media print {{ article {{ max-width: none; padding: 0; }} section.part {{ break-before: page; border-top: 0; margin-top: 0; padding-top: 0; }} p.leaf {{ break-inside: avoid; }} }}
"#
    );

    let mut w = Writer { doc: &doc, color: o.color, out: String::new(), anchors: anchors.clone(), heading: 0 };
    if let Some(title) = &doc.title {
        w.out += &format!("<h1 class=\"title\">{}</h1>\n", esc(title));
    }
    if o.contents && !heads.is_empty() {
        // The contents, three levels deep as the PDF's are.
        w.out.push_str("<nav class=\"contents\">\n");
        // The highest level shown stands at the left, whatever it is.
        let top = heads.iter().map(|(l, _)| *l).filter(|l| *l <= 3).min().unwrap_or(1) as usize;
        let mut depth = 0;
        for ((level, words), id) in heads.iter().zip(&anchors) {
            if *level > 3 {
                continue;
            }
            let level = *level as usize - top + 1;
            while depth < level {
                w.out.push_str("<ul>\n");
                depth += 1;
            }
            while depth > level {
                w.out.push_str("</ul>\n");
                depth -= 1;
            }
            w.out += &format!("<li><a href=\"#{}\">{}</a></li>\n", esc(id), esc(words));
        }
        while depth > 0 {
            w.out.push_str("</ul>\n");
            depth -= 1;
        }
        w.out.push_str("</nav>\n");
    }
    w.blocks(&doc.blocks);
    // The files set after the note, each with its way back.
    for (n, part) in doc.parts.iter().enumerate() {
        let back = if part.back { format!("#from-{n}") } else { "#top".to_string() };
        w.out += &format!("<section class=\"part\" id=\"part-{n}\">\n<p class=\"part-head\"><a href=\"{back}\">↑ {}</a><span>{}</span></p>\n", esc(note.title), esc(&part.name));
        if let Some(title) = &part.title {
            w.out += &format!("<h1 class=\"title\">{}</h1>\n", esc(title));
        }
        w.blocks(&part.blocks);
        w.out.push_str("</section>\n");
    }
    if !doc.notes.is_empty() {
        w.out.push_str("<section class=\"notes\">\n<hr>\n<ol>\n");
        for (i, body) in doc.notes.iter().enumerate() {
            let n = i + 1;
            w.out += &format!("<li id=\"note-{n}\">");
            let start = w.out.len();
            w.blocks(body);
            // The way back sits at the end of the note's last paragraph.
            let back = format!(" <a class=\"back\" href=\"#ref-{n}\">↩</a>");
            if w.out.ends_with("</p>\n") && w.out.len() > start {
                let at = w.out.len() - "</p>\n".len();
                w.out.insert_str(at, &back);
            } else {
                w.out.push_str(&back);
            }
            w.out.push_str("</li>\n");
        }
        w.out.push_str("</ol>\n</section>\n");
    }
    let lang = language(note.text);
    Ok(format!(
        "<!doctype html>\n<html lang=\"{lang}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"Onyx\">\n<title>{}</title>\n<style>{style}</style>\n</head>\n<body>\n<article id=\"top\">\n{}</article>\n</body>\n</html>\n",
        esc(note.title),
        w.out
    ))
}

/// The page's language, from what most of the note is in, as the PDF's.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_are_unique() {
        let heads = vec![(1, "Hello World".to_string()), (2, "Hello world".to_string()), (2, "代码 📝".to_string()), (3, "!!".to_string())];
        assert_eq!(anchors(&heads), vec!["hello-world", "hello-world-1", "代码-", "section"]);
    }
}
