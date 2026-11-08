//! The HTML a note may hold, read one way for the canvas and for paper.
//!
//! Markdown lets HTML stand in a note: a tag among the words (`<br>`,
//! `<u>`, `<span style="color: red">`) and a whole block (`<table>`,
//! `<details>`, `<p align="center">`). `read_tag` says what one tag
//! means. `to_markdown` turns a block into the markdown that reads the
//! same, tags among the words kept as they are, so the editor and
//! `pdf.rs` draw it by the road every note takes and cannot differ.
//!
//! Nothing here runs anything: a script, a style sheet or a frame is
//! left out, and an unknown tag stands for its words.

use std::ops::Range;

/// A color as a note names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// What a tag among the words does to the words it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Style {
    Strong,
    Em,
    Underline,
    Strike,
    Mark,
    Code,
    Sub,
    Sup,
    Small,
    Link(String),
    /// An ink, a wash, or both.
    Color { ink: Option<Rgb>, wash: Option<Rgb> },
    /// No look of its own: the words stand as they are.
    Plain,
}

/// One tag, as written between `<` and `>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    Comment,
    Break,
    /// A picture: its source, its width, and where its alt text stands in
    /// the tag.
    Image { src: String, width: Option<Width>, alt: Range<usize> },
    Open { name: String, style: Style },
    Close { name: String },
    /// Not a tag: `<` and words.
    Not,
}

/// A tag's name, lowercased, and where its attributes begin.
pub(crate) fn name_of(s: &str) -> (String, usize) {
    let end = s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(s.len());
    (s[..end].to_ascii_lowercase(), end)
}

/// A tag's attributes: each name, lowercased, and its value's bytes in
/// `s`. A name with no value has an empty range at its end.
pub(crate) fn attrs(s: &str, from: usize) -> Vec<(String, Range<usize>)> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = from;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        if i == start {
            break;
        }
        let name = s[start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') {
            out.push((name, i..i));
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        match bytes.get(i) {
            Some(&q @ (b'"' | b'\'')) => {
                let at = i + 1;
                let end = s[at..].find(q as char).map_or(s.len(), |e| at + e);
                out.push((name, at..end));
                i = (end + 1).min(s.len());
            }
            _ => {
                let at = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                    i += 1;
                }
                out.push((name, at..i));
            }
        }
    }
    out
}

pub(crate) fn attr<'a>(s: &'a str, list: &[(String, Range<usize>)], name: &str) -> Option<&'a str> {
    list.iter().find(|(n, _)| n == name).map(|(_, r)| &s[r.clone()])
}

/// One declaration of a `style` attribute.
fn declared<'a>(style: &'a str, name: &str) -> Option<&'a str> {
    style.split(';').find_map(|d| {
        let (k, v) = d.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// A color as CSS writes it.
pub fn color(css: &str) -> Option<Rgb> {
    let css = css.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
    if let Some(hex) = css.strip_prefix('#') {
        if !hex.is_ascii() {
            return None;
        }
        let pair = |a: usize| u8::from_str_radix(hex.get(a..a + 2)?, 16).ok();
        let one = |a: usize| u8::from_str_radix(hex.get(a..a + 1)?, 16).ok().map(|v| v * 17);
        return match hex.len() {
            3 | 4 => Some(Rgb(one(0)?, one(1)?, one(2)?)),
            6 | 8 => Some(Rgb(pair(0)?, pair(2)?, pair(4)?)),
            _ => None,
        };
    }
    if let Some(rest) = css.strip_prefix("rgb(").or_else(|| css.strip_prefix("rgba(")) {
        let parts: Vec<u8> = rest.trim_end_matches(')').split(|c: char| c == ',' || c == '/' || c.is_whitespace()).filter(|p| !p.is_empty()).take(3).filter_map(|p| p.parse::<f32>().ok()).map(|v| v.clamp(0., 255.) as u8).collect();
        return (parts.len() == 3).then(|| Rgb(parts[0], parts[1], parts[2]));
    }
    const NAMES: &[(&str, u32)] = &[
        ("black", 0x000000), ("silver", 0xc0c0c0), ("gray", 0x808080), ("grey", 0x808080), ("white", 0xffffff), ("maroon", 0x800000), ("red", 0xff0000), ("purple", 0x800080), ("fuchsia", 0xff00ff), ("magenta", 0xff00ff), ("green", 0x008000), ("lime", 0x00ff00), ("olive", 0x808000), ("yellow", 0xffff00), ("navy", 0x000080), ("blue", 0x0000ff), ("teal", 0x008080), ("aqua", 0x00ffff), ("cyan", 0x00ffff), ("orange", 0xffa500), ("pink", 0xffc0cb), ("brown", 0xa52a2a), ("gold", 0xffd700), ("indigo", 0x4b0082), ("violet", 0xee82ee), ("crimson", 0xdc143c), ("coral", 0xff7f50), ("salmon", 0xfa8072), ("tomato", 0xff6347), ("orangered", 0xff4500), ("skyblue", 0x87ceeb), ("steelblue", 0x4682b4), ("royalblue", 0x4169e1), ("seagreen", 0x2e8b57), ("forestgreen", 0x228b22), ("lightgray", 0xd3d3d3), ("lightgrey", 0xd3d3d3), ("darkgray", 0xa9a9a9), ("darkgrey", 0xa9a9a9), ("lightblue", 0xadd8e6), ("lightgreen", 0x90ee90), ("lightyellow", 0xffffe0),
    ];
    NAMES.iter().find(|(n, _)| *n == css).map(|(_, v)| Rgb((v >> 16) as u8, (v >> 8) as u8, *v as u8))
}

fn width_of(value: &str) -> Option<Width> {
    let value = value.trim();
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    let n: u32 = digits.parse().ok().filter(|w| *w > 0)?;
    if value[digits.len()..].trim_start().starts_with('%') {
        return Some(Width::Share(n.min(100)));
    }
    Some(Width::Px(n))
}

/// How wide a picture asks to be: in pixels (`width="300"`), or as a
/// share of the words' width (`width="100%"`), as the Full size writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Px(u32),
    /// Percent, 1 to 100.
    Share(u32),
}

impl Width {
    /// Its width in a room of `room` pixels.
    pub fn in_room(self, room: f32) -> f32 {
        match self {
            Width::Px(w) => w as f32,
            Width::Share(n) => room * n as f32 / 100.,
        }
    }

    /// As an attribute's value: `300`, or `100%`.
    pub fn written(self) -> String {
        match self {
            Width::Px(w) => w.to_string(),
            Width::Share(n) => format!("{n}%"),
        }
    }
}

/// What one tag means, written whole with its `<` and `>`.
pub fn read_tag(tag: &str) -> Tag {
    if tag.starts_with("<!--") {
        return Tag::Comment;
    }
    let Some(inner) = tag.strip_prefix('<') else { return Tag::Not };
    if inner.starts_with('!') || inner.starts_with('?') {
        return Tag::Comment;
    }
    if let Some(close) = inner.strip_prefix('/') {
        let (name, _) = name_of(close);
        return if name.is_empty() { Tag::Not } else { Tag::Close { name } };
    }
    let (name, end) = name_of(inner);
    if name.is_empty() || !inner[..end].starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Tag::Not;
    }
    let list = attrs(inner, end);
    let at = |name: &str| attr(inner, &list, name);
    let style = match name.as_str() {
        "br" | "wbr" => return Tag::Break,
        // A PDF's first page stands as a picture does; its words are its title.
        "img" | "embed" => {
            let words = if name == "img" { "alt" } else { "title" };
            let alt = list.iter().find(|(n, _)| n == words).map_or(0..0, |(_, r)| r.start + 1..r.end + 1);
            let width = at("width").and_then(width_of).or_else(|| at("style").and_then(|s| declared(s, "width")).and_then(width_of));
            return Tag::Image { src: at("src").unwrap_or("").to_string(), width, alt };
        }
        "b" | "strong" => Style::Strong,
        "i" | "em" | "cite" | "dfn" | "var" => Style::Em,
        "u" | "ins" => Style::Underline,
        "s" | "del" | "strike" => Style::Strike,
        "mark" => Style::Mark,
        "code" | "kbd" | "samp" | "tt" => Style::Code,
        "sub" => Style::Sub,
        "sup" => Style::Sup,
        "small" => Style::Small,
        "a" => at("href").map_or(Style::Plain, |h| Style::Link(entities(h))),
        "font" => colored(at("color").and_then(color), None),
        _ => match at("style") {
            Some(css) => colored(declared(css, "color").and_then(color), declared(css, "background-color").or_else(|| declared(css, "background")).and_then(color)),
            None => Style::Plain,
        },
    };
    Tag::Open { name, style }
}

fn colored(ink: Option<Rgb>, wash: Option<Rgb>) -> Style {
    if ink.is_some() || wash.is_some() {
        Style::Color { ink, wash }
    } else {
        Style::Plain
    }
}

/// A text with HTML's entities written out.
pub fn entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let known = rest.find(';').filter(|e| *e <= 10).and_then(|e| entity(&rest[1..e]).map(|c| (c, e)));
        match known {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "middot" => '·',
        "bull" => '•',
        "times" => '×',
        "rarr" => '→',
        "larr" => '←',
        _ => return None,
    })
}

// -- a block ----------------------------------------------------------------

/// How a block stands across the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Across {
    #[default]
    Start,
    Center,
    End,
}

/// A block of HTML as markdown, and how it stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Block {
    pub markdown: String,
    pub across: Across,
}

#[derive(Debug)]
enum Node {
    Text(String),
    Element { name: String, raw: String, kids: Vec<Node> },
}

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];
const DROPPED: &[&str] = &["script", "style", "head", "title", "iframe", "object", "embed", "video", "audio", "svg", "canvas", "noscript", "template", "form", "select", "textarea", "button"];
const BLOCKS: &[&str] = &["address", "article", "aside", "blockquote", "center", "dd", "details", "div", "dl", "dt", "figcaption", "figure", "footer", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "li", "main", "nav", "ol", "p", "pre", "section", "summary", "table", "ul"];

/// The tags an opening tag closes where one was left open: `<li>` after
/// `<li>`, as a page writes them.
fn closes(open: &str, coming: &str) -> bool {
    match open {
        "li" => coming == "li",
        "dt" | "dd" => matches!(coming, "dt" | "dd"),
        "td" | "th" => matches!(coming, "td" | "th" | "tr" | "tbody" | "tfoot"),
        "tr" => matches!(coming, "tr" | "tbody" | "tfoot"),
        "thead" | "tbody" => matches!(coming, "tbody" | "tfoot"),
        "p" => BLOCKS.contains(&coming),
        _ => false,
    }
}

/// The tree of a block, as forgiving as a browser: a tag left open ends
/// with what holds it, and a closing tag with no opening one is passed by.
fn tree(src: &str) -> Vec<Node> {
    let mut stack: Vec<(String, String, Vec<Node>)> = vec![(String::new(), String::new(), Vec::new())];
    fn fold(stack: &mut Vec<(String, String, Vec<Node>)>) {
        if let Some((name, raw, kids)) = stack.pop() {
            if let Some(top) = stack.last_mut() {
                top.2.push(Node::Element { name, raw, kids });
            }
        }
    }
    let mut at = 0;
    while at < src.len() {
        let rest = &src[at..];
        let Some(lt) = rest.find('<') else {
            if let Some(top) = stack.last_mut() {
                top.2.push(Node::Text(rest.to_string()));
            }
            break;
        };
        if lt > 0 {
            if let Some(top) = stack.last_mut() {
                top.2.push(Node::Text(rest[..lt].to_string()));
            }
            at += lt;
            continue;
        }
        let end = if rest.starts_with("<!--") { rest.find("-->").map(|e| e + 3) } else { tag_end(rest) };
        let Some(end) = end else {
            if let Some(top) = stack.last_mut() {
                top.2.push(Node::Text("<".to_string()));
            }
            at += 1;
            continue;
        };
        let raw = &rest[..end];
        at += end;
        match read_tag(raw) {
            Tag::Comment => {}
            Tag::Not => {
                if let Some(top) = stack.last_mut() {
                    top.2.push(Node::Text(raw.to_string()));
                }
            }
            Tag::Close { name } => {
                if let Some(depth) = stack.iter().rposition(|(n, _, _)| *n == name) {
                    if depth > 0 {
                        while stack.len() > depth {
                            fold(&mut stack);
                        }
                    }
                }
            }
            Tag::Break => {
                if let Some(top) = stack.last_mut() {
                    top.2.push(Node::Element { name: "br".to_string(), raw: raw.to_string(), kids: Vec::new() });
                }
            }
            Tag::Image { .. } => {
                if let Some(top) = stack.last_mut() {
                    top.2.push(Node::Element { name: "img".to_string(), raw: raw.to_string(), kids: Vec::new() });
                }
            }
            Tag::Open { name, .. } => {
                while stack.len() > 1 && stack.last().is_some_and(|(open, _, _)| closes(open, &name)) {
                    fold(&mut stack);
                }
                if VOID.contains(&name.as_str()) || raw.ends_with("/>") {
                    if let Some(top) = stack.last_mut() {
                        top.2.push(Node::Element { name, raw: raw.to_string(), kids: Vec::new() });
                    }
                } else {
                    stack.push((name, raw.to_string(), Vec::new()));
                }
            }
        }
    }
    while stack.len() > 1 {
        fold(&mut stack);
    }
    stack.pop().map(|(_, _, kids)| kids).unwrap_or_default()
}

/// Where a tag at the start of `s` ends, past a `>` inside a quoted value.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i + 1),
            _ => {}
        }
    }
    None
}

fn is_block(node: &Node) -> bool {
    matches!(node, Node::Element { name, .. } if BLOCKS.contains(&name.as_str()))
}

/// How a tag asks its block to stand.
fn across_of(raw: &str, name: &str) -> Option<Across> {
    if name == "center" {
        return Some(Across::Center);
    }
    let inner = raw.strip_prefix('<')?;
    let (_, end) = name_of(inner);
    let list = attrs(inner, end);
    let value = attr(inner, &list, "align").or_else(|| attr(inner, &list, "style").and_then(|s| declared(s, "text-align")))?;
    match value.trim().to_ascii_lowercase().as_str() {
        "center" | "middle" => Some(Across::Center),
        "right" | "end" => Some(Across::End),
        "left" | "start" => Some(Across::Start),
        _ => None,
    }
}

/// A text as markdown that reads as those words and nothing else.
fn escaped(text: &str, out: &mut String) {
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '$' | '~' | '|' | '=') {
            out.push('\\');
        }
        out.push(c);
    }
}

/// A line that is one picture and nothing else, written as HTML: an
/// `<img>` or an `<embed>`, alone or in a `<p>`, `<div>` or `<center>`
/// that places it across the page. What `picture_markup` writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PictureLine {
    pub src: String,
    /// Its words (`alt`, or an embed's `title`), in the line's bytes.
    pub alt: Range<usize>,
    pub width: Option<Width>,
    /// Where its wrapper places it; none with no wrapper.
    pub across: Option<Across>,
    /// An `<embed>`, a PDF's first page, not an `<img>`.
    pub embed: bool,
}

/// The picture a line holds as HTML, when it holds that and nothing else.
pub fn picture_line(line: &str) -> Option<PictureLine> {
    let lead = line.len() - line.trim_start().len();
    let words = line.trim();
    let tag_end = |s: &str| s.find('>').map(|e| e + 1);
    let first = tag_end(words)?;
    let mut across = None;
    let mut tag = 0..first;
    if let Tag::Open { name, .. } = read_tag(&words[..first]) {
        if !matches!(name.as_str(), "p" | "div" | "center") {
            return None;
        }
        across = across_of(&words[..first], &name).or(Some(Across::Start));
        let inner = first + words[first..].len() - words[first..].trim_start().len();
        let end = inner + tag_end(&words[inner..])?;
        tag = inner..end;
        let after = words[end..].trim();
        match read_tag(after) {
            Tag::Close { name: closed } if closed == name && tag_end(after) == Some(after.len()) => {}
            _ => return None,
        }
    } else if first != words.len() {
        return None;
    }
    let raw = &words[tag.clone()];
    let embed = name_of(&raw[1..]).0 == "embed";
    let Tag::Image { src, width, alt } = read_tag(raw) else { return None };
    let at = lead + tag.start;
    Some(PictureLine { src: entities(&src), alt: at + alt.start..at + alt.end, width, across, embed })
}

/// A picture as a note writes it, from its source, its words, its width
/// and where it stands. With neither a width nor a place it is markdown,
/// `![words](src)`; with either it is HTML any app reads: an `<img>` with
/// its `width`, or for a PDF an `<embed>`, in a `<p align>` when it is
/// centered or to the right.
pub fn picture_markup(src: &str, words: &str, width: Option<Width>, across: Across, embed: bool) -> String {
    if width.is_none() && across == Across::Start {
        let mut alt = String::new();
        for c in words.chars() {
            if matches!(c, '[' | ']' | '\\') {
                alt.push('\\');
            }
            alt.push(c);
        }
        return if src.contains([' ', '(', ')', '<', '>']) { format!("![{alt}](<{src}>)") } else { format!("![{alt}]({src})") };
    }
    let quoted = |s: &str| s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;");
    let src = quoted(&src.replace(' ', "%20"));
    let wide = width.map(|w| format!(" width=\"{}\"", w.written())).unwrap_or_default();
    let tag = if embed { format!("<embed src=\"{src}\" title=\"{}\"{wide}>", quoted(words)) } else { format!("<img src=\"{src}\" alt=\"{}\"{wide}>", quoted(words)) };
    match across {
        Across::Start => tag,
        Across::Center => format!("<p align=\"center\">{tag}</p>"),
        Across::End => format!("<p align=\"right\">{tag}</p>"),
    }
}

/// How large a link stands as a card on a line of its own, as Craft's
/// link and file blocks have it (`data-card` on an `<a>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CardSize {
    /// One row: its icon, its name and where it is.
    Small,
    /// Its name, a few words of it and where it is, its picture beside.
    Medium,
    /// Its picture over its name and a few words of it.
    Large,
}

impl CardSize {
    pub fn written(self) -> &'static str {
        match self {
            CardSize::Small => "small",
            CardSize::Medium => "medium",
            CardSize::Large => "large",
        }
    }

    pub fn read(s: &str) -> Option<CardSize> {
        match s.trim().to_ascii_lowercase().as_str() {
            "small" => Some(CardSize::Small),
            "medium" => Some(CardSize::Medium),
            "large" => Some(CardSize::Large),
            _ => None,
        }
    }
}

/// A line that is one link shown as a card and nothing else: what
/// `card_markup` writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLine {
    /// Where it leads, its entities written out.
    pub href: String,
    /// Its words, in the line's bytes, as written.
    pub words: Range<usize>,
    pub size: CardSize,
    /// Where its wrapper places it; none with no wrapper.
    pub across: Option<Across>,
}

/// The card a line holds, `<a href data-card>words</a>` alone on it, its
/// words plain, or in a `<p>`, `<div>` or `<center>` that places it.
pub fn card_line(line: &str) -> Option<CardLine> {
    let lead = line.len() - line.trim_start().len();
    let s = line.trim();
    let first = s.find('>')? + 1;
    if let Tag::Open { name, .. } = read_tag(&s[..first]) {
        if matches!(name.as_str(), "p" | "div" | "center") {
            let across = across_of(&s[..first], &name).or(Some(Across::Start));
            let close = s.rfind("</")?;
            if !matches!(read_tag(&s[close..]), Tag::Close { name: closed } if closed == name) || close + s[close..].find('>')? + 1 != s.len() || close < first {
                return None;
            }
            let inner = &s[first..close];
            let pad = inner.len() - inner.trim_start().len();
            let mut c = card_line(inner.trim())?;
            let at = lead + first + pad;
            c.words = c.words.start + at..c.words.end + at;
            c.across = across;
            return Some(c);
        }
    }
    let open_end = first;
    let inner = s.get(1..open_end - 1).filter(|_| s.starts_with('<'))?;
    let (name, end) = name_of(inner);
    if name != "a" {
        return None;
    }
    let list = attrs(inner, end);
    let size = CardSize::read(attr(inner, &list, "data-card")?)?;
    let href = entities(attr(inner, &list, "href")?);
    if href.trim().is_empty() {
        return None;
    }
    let rest = &s[open_end..];
    let close = rest.rfind("</")?;
    let tail = &rest[close..];
    if !matches!(read_tag(tail), Tag::Close { name } if name == "a") || tail.find('>')? + 1 != tail.len() || rest[..close].contains('<') {
        return None;
    }
    let at = lead + open_end;
    Some(CardLine { href, words: at..at + close, size, across: None })
}

/// A link as a card of a size: `<a href data-card>`, which any app reads
/// as the link it is, in a `<p align>` when it is centered or to the
/// right.
pub fn card_markup(href: &str, words: &str, size: CardSize, across: Across) -> String {
    let quoted = |s: &str| s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;");
    let words = words.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let tag = format!("<a href=\"{}\" data-card=\"{}\">{words}</a>", quoted(&href.replace(' ', "%20")), size.written());
    match across {
        Across::Start => tag,
        Across::Center => format!("<p align=\"center\">{tag}</p>"),
        Across::End => format!("<p align=\"right\">{tag}</p>"),
    }
}

/// Where a note is read in parts by a markdown parser, so a line that is
/// a picture or a card alone, written as HTML, ends its block of HTML.
/// Markdown's rule runs a block that opens with a tag alone on its line
/// to the next blank line, so a heading right under a picture would be
/// read as HTML; a note need not keep a blank line under one. A part
/// starts at a line's start, so the parts' events read as one note's.
pub fn parts(text: &str, options: pulldown_cmark::Options) -> Vec<Range<usize>> {
    use pulldown_cmark::{Event, Parser, Tag as Md};
    let lone = |l: &str| !l.starts_with([' ', '\t']) && (picture_line(l).is_some() || card_line(l).is_some());
    // Most notes have none: a look at the lines first, not a reading.
    let mut lines = text.lines().peekable();
    let mut any = false;
    while let Some(l) = lines.next() {
        if lone(l) && lines.peek().is_some_and(|n| !n.trim().is_empty()) {
            any = true;
            break;
        }
    }
    if !any {
        return vec![0..text.len()];
    }
    let mut out = Vec::new();
    let mut from = 0;
    loop {
        let part = &text[from..];
        let cut = Parser::new_ext(part, options).into_offset_iter().find_map(|(ev, r)| {
            if !matches!(ev, Event::Start(Md::HtmlBlock)) || (r.start > 0 && !part[..r.start].ends_with('\n')) {
                return None;
            }
            let block = &part[r.clone()];
            let end = block.find('\n')?;
            (lone(block[..end].trim_end_matches('\r')) && !block[end + 1..].trim().is_empty()).then_some(r.start + end + 1)
        });
        match cut {
            Some(c) => {
                out.push(from..from + c);
                from += c;
            }
            None => {
                out.push(from..text.len());
                return out;
            }
        }
    }
}

/// A link as markdown, `[words](href)`.
pub fn link_markup(href: &str, words: &str) -> String {
    let mut text = String::new();
    for c in words.chars() {
        if matches!(c, '[' | ']' | '\\') {
            text.push('\\');
        }
        text.push(c);
    }
    if href.contains([' ', '(', ')', '<', '>']) {
        format!("[{text}](<{href}>)")
    } else {
        format!("[{text}]({href})")
    }
}

/// A picture as markdown, its width after its alt text, as Obsidian
/// writes one.
fn picture(raw: &str) -> String {
    let Tag::Image { src, width, alt } = read_tag(raw) else { return String::new() };
    let mut words = String::new();
    if let Some(alt) = raw.get(alt) {
        escaped(&entities(alt), &mut words);
    }
    if let Some(w) = width {
        words.push_str(&format!("|{}", w.written()));
    }
    let src = entities(&src);
    if src.contains([' ', '(', ')']) {
        format!("![{words}](<{src}>)")
    } else {
        format!("![{words}]({src})")
    }
}

/// The words of nodes on one line, tags among the words kept as written.
fn inline(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(text) => {
                let mut space = false;
                let text = entities(text);
                let mut piece = String::new();
                for c in text.chars() {
                    if c.is_ascii_whitespace() {
                        space = true;
                    } else {
                        if space && !(piece.is_empty() && (out.is_empty() || out.ends_with(' ') || out.ends_with('\n'))) {
                            piece.push(' ');
                        }
                        space = false;
                        piece.push(c);
                    }
                }
                if space && !piece.is_empty() {
                    piece.push(' ');
                } else if space && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                escaped(&piece, out);
            }
            Node::Element { name, raw, kids } => match name.as_str() {
                "br" => out.push_str("<br>"),
                "img" => out.push_str(&picture(raw)),
                n if DROPPED.contains(&n) => {}
                _ => {
                    let keep = matches!(read_tag(raw), Tag::Open { style, .. } if style != Style::Plain);
                    if keep {
                        out.push_str(&raw.replace('\n', " "));
                    }
                    inline(kids, out);
                    if keep {
                        out.push_str(&format!("</{name}>"));
                    }
                }
            },
        }
    }
}

fn plain_text(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(&entities(t)),
            Node::Element { name, kids, .. } if name == "br" && kids.is_empty() => out.push('\n'),
            Node::Element { kids, .. } => plain_text(kids, out),
        }
    }
}

struct Writer {
    across: Option<Across>,
}

impl Writer {
    /// A run of nodes as blocks: words between blocks are a paragraph.
    fn blocks(&mut self, nodes: &[Node], out: &mut Vec<String>) {
        let mut words: Vec<&[Node]> = Vec::new();
        let mut from = 0;
        for (i, node) in nodes.iter().enumerate() {
            if is_block(node) {
                if from < i {
                    words.push(&nodes[from..i]);
                }
                for run in words.drain(..) {
                    paragraph(run, out);
                }
                self.block(node, out);
                from = i + 1;
            }
        }
        if from < nodes.len() {
            paragraph(&nodes[from..], out);
        }
    }

    fn block(&mut self, node: &Node, out: &mut Vec<String>) {
        let Node::Element { name, raw, kids } = node else { return };
        if self.across.is_none() {
            self.across = across_of(raw, name);
        }
        match name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse::<usize>().unwrap_or(1);
                let mut words = String::new();
                inline(kids, &mut words);
                let words = words.trim();
                if !words.is_empty() {
                    out.push(format!("{} {words}", "#".repeat(level)));
                }
            }
            "hr" => out.push("---".to_string()),
            "ul" | "ol" => {
                let mut lines = Vec::new();
                list(name == "ol", kids, 0, &mut lines);
                if !lines.is_empty() {
                    out.push(lines.join("\n"));
                }
            }
            "blockquote" => {
                let mut inner = Vec::new();
                self.blocks(kids, &mut inner);
                let text = inner.join("\n\n");
                if !text.is_empty() {
                    out.push(text.lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n"));
                }
            }
            "pre" => {
                let mut code = String::new();
                plain_text(kids, &mut code);
                let lang = kids.iter().find_map(|k| match k {
                    Node::Element { name, raw, .. } if name == "code" => raw.split("language-").nth(1).map(|r| r.chars().take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '+' | '#')).collect::<String>()),
                    _ => None,
                });
                let fence = "`".repeat(code.lines().map(|l| l.chars().take_while(|c| *c == '`').count()).max().unwrap_or(0).max(2) + 1);
                out.push(format!("{fence}{}\n{}\n{fence}", lang.unwrap_or_default(), code.trim_matches('\n')));
            }
            "table" => {
                if let Some(table) = table(kids) {
                    out.push(table);
                }
            }
            "details" => {
                for kid in kids {
                    if let Node::Element { name, kids: inner, .. } = kid {
                        if name == "summary" {
                            let mut words = String::new();
                            inline(inner, &mut words);
                            out.push(format!("**▾ {}**", words.trim()));
                        }
                    }
                }
                let rest: Vec<&Node> = kids.iter().filter(|k| !matches!(k, Node::Element { name, .. } if name == "summary")).collect();
                for node in rest {
                    self.blocks(std::slice::from_ref(node), out);
                }
            }
            "summary" => {
                let mut words = String::new();
                inline(kids, &mut words);
                out.push(format!("**▾ {}**", words.trim()));
            }
            n if DROPPED.contains(&n) => {}
            _ => self.blocks(kids, out),
        }
    }
}

fn paragraph(nodes: &[Node], out: &mut Vec<String>) {
    let mut words = String::new();
    inline(nodes, &mut words);
    let words = words.trim();
    if words.is_empty() || words.split("<br>").all(|w| w.trim().is_empty()) {
        return;
    }
    // Lines of the paragraph; `<br>` is kept, so the words stay one block.
    out.push(words.to_string());
}

fn list(ordered: bool, nodes: &[Node], depth: usize, lines: &mut Vec<String>) {
    let mut n = 0;
    for node in nodes {
        let Node::Element { name, kids, .. } = node else { continue };
        if name != "li" {
            continue;
        }
        n += 1;
        let marker = if ordered { format!("{n}. ") } else { "- ".to_string() };
        let pad = " ".repeat(depth);
        let mut words = String::new();
        let lower: Vec<&Node> = kids.iter().filter(|k| matches!(k, Node::Element { name, .. } if name == "ul" || name == "ol")).collect();
        let upper: Vec<&Node> = kids.iter().filter(|k| !matches!(k, Node::Element { name, .. } if name == "ul" || name == "ol")).collect();
        for k in upper {
            inline(std::slice::from_ref(k), &mut words);
        }
        lines.push(format!("{pad}{marker}{}", words.trim()));
        for k in lower {
            if let Node::Element { name, kids, .. } = k {
                list(name == "ol", kids, depth + marker.len(), lines);
            }
        }
    }
}

/// The rows of a table as a pipe table, the first row its head. Each
/// column aligned as its first cell asks.
fn table(nodes: &[Node]) -> Option<String> {
    fn rows<'a>(nodes: &'a [Node], out: &mut Vec<&'a [Node]>) {
        for node in nodes {
            if let Node::Element { name, kids, .. } = node {
                match name.as_str() {
                    "tr" => out.push(kids),
                    "thead" | "tbody" | "tfoot" => rows(kids, out),
                    _ => {}
                }
            }
        }
    }
    let mut found = Vec::new();
    rows(nodes, &mut found);
    let mut lines: Vec<Vec<String>> = Vec::new();
    let mut aligns: Vec<Option<Across>> = Vec::new();
    for row in found {
        let mut cells = Vec::new();
        for cell in row {
            let Node::Element { name, raw, kids } = cell else { continue };
            if name != "td" && name != "th" {
                continue;
            }
            if lines.is_empty() {
                aligns.push(across_of(raw, name));
            }
            let mut words = String::new();
            for kid in kids {
                if is_block(kid) {
                    if let Node::Element { kids: inner, .. } = kid {
                        if !words.is_empty() {
                            words.push_str("<br>");
                        }
                        inline(inner, &mut words);
                    }
                } else {
                    inline(std::slice::from_ref(kid), &mut words);
                }
            }
            cells.push(words.trim().replace('\n', "<br>"));
        }
        if !cells.is_empty() {
            lines.push(cells);
        }
    }
    let columns = lines.iter().map(Vec::len).max()?;
    let mut text = String::new();
    for (ix, line) in lines.iter().enumerate() {
        let mut cells = line.clone();
        cells.resize(columns, String::new());
        text.push_str(&format!("| {} |\n", cells.join(" | ")));
        if ix == 0 {
            let marks: Vec<&str> = (0..columns)
                .map(|c| match aligns.get(c).copied().flatten() {
                    Some(Across::Center) => ":---:",
                    Some(Across::End) => "---:",
                    Some(Across::Start) => ":---",
                    None => "---",
                })
                .collect();
            text.push_str(&format!("| {} |\n", marks.join(" | ")));
        }
    }
    Some(text.trim_end().to_string())
}

/// A block of HTML as the markdown that reads the same.
pub fn to_markdown(src: &str) -> Block {
    let nodes = tree(src);
    let mut writer = Writer { across: None };
    let mut out = Vec::new();
    writer.blocks(&nodes, &mut out);
    Block { markdown: out.join("\n\n"), across: writer.across.unwrap_or_default() }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_picture_line_is_read_as_it_is_written() {
        use super::{picture_line, picture_markup, Across, Width};
        let line = picture_markup("../.assets/a b.png", "A \"cat\"", Some(Width::Px(480)), Across::Center, false);
        assert_eq!(line, "<p align=\"center\"><img src=\"../.assets/a%20b.png\" alt=\"A &quot;cat&quot;\" width=\"480\"></p>");
        let p = picture_line(&line).unwrap();
        assert_eq!((p.src.as_str(), p.width, p.across, p.embed), ("../.assets/a%20b.png", Some(Width::Px(480)), Some(Across::Center), false));
        assert_eq!(&line[p.alt.clone()], "A &quot;cat&quot;");
        let right = picture_markup("x.pdf", "Plan", None, Across::End, true);
        assert_eq!(right, "<p align=\"right\"><embed src=\"x.pdf\" title=\"Plan\"></p>");
        let p = picture_line(&format!("  {right}")).unwrap();
        assert_eq!((p.across, p.embed, p.width), (Some(Across::End), true, None));
        assert_eq!(&format!("  {right}")[p.alt.clone()], "Plan");
        let alone = picture_markup("a.png", "a", Some(Width::Px(200)), Across::Start, false);
        assert_eq!(alone, "<img src=\"a.png\" alt=\"a\" width=\"200\">");
        assert_eq!(picture_line(&alone).unwrap().across, None);
        assert_eq!(picture_markup("a b.png", "a [1]", None, Across::Start, false), "![a \\[1\\]](<a b.png>)");
        // Full: a share of the words' width, as any app reads it.
        let full = picture_markup("a.png", "a", Some(Width::Share(100)), Across::Start, false);
        assert_eq!(full, "<img src=\"a.png\" alt=\"a\" width=\"100%\">");
        assert_eq!(picture_line(&full).unwrap().width, Some(Width::Share(100)));
        assert_eq!(picture_line("<img src=\"a.png\" style=\"width: 50%\">").unwrap().width, Some(Width::Share(50)));
        assert!(picture_line("<p align=\"center\"><img src=\"a.png\"> and words</p>").is_none());
        assert!(picture_line("<img src=\"a.png\"> words").is_none());
        assert!(picture_line("<span><img src=\"a.png\"></span>").is_none());
    }

    #[test]
    fn a_lone_picture_ends_its_block_of_html() {
        use pulldown_cmark::{Event, Options, Parser, Tag};
        let text = "<img src=\"a.png\">\n## Head\n<a href=\"x.pdf\" data-card=\"small\">x</a>\n# Two\n\n<div>\nkept\n</div>\n";
        let parts = super::parts(text, Options::empty());
        assert_eq!(parts.len(), 2);
        let heads = parts.iter().flat_map(|r| Parser::new_ext(&text[r.clone()], Options::empty())).filter(|e| matches!(e, Event::Start(Tag::Heading { .. }))).count();
        assert_eq!(heads, 2);
        assert_eq!(super::parts("<div>\nwords\n</div>", Options::empty()), vec![0..18]);
    }

    #[test]
    fn a_card_line_is_read_as_it_is_written() {
        use super::{card_line, card_markup, link_markup, Across, CardSize};
        let line = card_markup("https://craft.do/a b?x=1&y=2", "Craft & co", CardSize::Medium, Across::Start);
        assert_eq!(line, "<a href=\"https://craft.do/a%20b?x=1&amp;y=2\" data-card=\"medium\">Craft &amp; co</a>");
        let c = card_line(&format!("  {line}")).unwrap();
        assert_eq!((c.href.as_str(), c.size), ("https://craft.do/a%20b?x=1&y=2", CardSize::Medium));
        assert_eq!(&format!("  {line}")[c.words.clone()], "Craft &amp; co");
        assert_eq!(card_line("<a data-card=\"small\" href=\"x.pdf\">x.pdf</a>").unwrap().size, CardSize::Small);
        assert!(card_line("<a href=\"x.pdf\">x.pdf</a>").is_none());
        assert!(card_line("<a href=\"x.pdf\" data-card=\"huge\">x</a>").is_none());
        assert!(card_line("<a href=\"x.pdf\" data-card=\"small\">x</a> and words").is_none());
        assert!(card_line("<a href=\"x.pdf\" data-card=\"small\"><b>x</b></a>").is_none());
        assert_eq!(link_markup("a b.pdf", "Plan [1]"), "[Plan \\[1\\]](<a b.pdf>)");
        let placed = card_markup("x.pdf", "Plan", CardSize::Large, Across::Center);
        assert_eq!(placed, "<p align=\"center\"><a href=\"x.pdf\" data-card=\"large\">Plan</a></p>");
        let c = card_line(&placed).unwrap();
        assert_eq!((c.size, c.across, &placed[c.words.clone()]), (CardSize::Large, Some(Across::Center), "Plan"));
        assert!(card_line("<p><a href=\"x.pdf\" data-card=\"large\">Plan</a> more</p>").is_none());
    }

    use super::*;

    #[test]
    fn a_tag_says_what_it_does() {
        assert_eq!(read_tag("<br>"), Tag::Break);
        assert_eq!(read_tag("<BR />"), Tag::Break);
        assert_eq!(read_tag("<!-- note -->"), Tag::Comment);
        assert_eq!(read_tag("</u>"), Tag::Close { name: "u".into() });
        assert_eq!(read_tag("<u>"), Tag::Open { name: "u".into(), style: Style::Underline });
        assert_eq!(read_tag(r#"<a href="https://a.b/c?d=1&amp;e=2">"#), Tag::Open { name: "a".into(), style: Style::Link("https://a.b/c?d=1&e=2".into()) });
        assert_eq!(read_tag(r#"<span style="color: #f00; background-color: yellow">"#), Tag::Open { name: "span".into(), style: Style::Color { ink: Some(Rgb(255, 0, 0)), wash: Some(Rgb(255, 255, 0)) } });
        assert_eq!(read_tag(r#"<font color=red>"#), Tag::Open { name: "font".into(), style: Style::Color { ink: Some(Rgb(255, 0, 0)), wash: None } });
        assert_eq!(read_tag("<custom-thing>"), Tag::Open { name: "custom-thing".into(), style: Style::Plain });
        let tag = r#"<img src="a b.png" alt="A cat" width="300">"#;
        let Tag::Image { src, width, alt } = read_tag(tag) else { panic!() };
        assert_eq!((src.as_str(), width, &tag[alt]), ("a b.png", Some(Width::Px(300)), "A cat"));
        assert_eq!(read_tag("< x"), Tag::Not);
    }

    #[test]
    fn a_color_reads_as_css_writes_it() {
        assert_eq!(color("#abc"), Some(Rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(color("#a1b2c3"), Some(Rgb(0xa1, 0xb2, 0xc3)));
        assert_eq!(color("rgb(1, 2, 3)"), Some(Rgb(1, 2, 3)));
        assert_eq!(color("rgba(1 2 3 / 50%)"), Some(Rgb(1, 2, 3)));
        assert_eq!(color("Crimson"), Some(Rgb(0xdc, 0x14, 0x3c)));
        assert_eq!(color("var(--x)"), None);
        assert_eq!(color("#中"), None);
    }

    #[test]
    fn entities_are_written_out() {
        assert_eq!(entities("a &amp; b &lt;c&gt; &#65;&#x42; &nope; & d"), "a & b <c> AB &nope; & d");
    }

    #[test]
    fn a_block_becomes_the_markdown_that_reads_the_same() {
        assert_eq!(to_markdown("<h2>Title <b>x</b></h2>\n<p>One *two*</p>").markdown, "## Title <b>x</b>\n\nOne \\*two\\*");
        assert_eq!(to_markdown("<ul>\n<li>a<li>b\n<ul><li>c</li></ul></li></ul>").markdown, "- a\n- b\n  - c");
        assert_eq!(to_markdown("<ol><li>a</li><li>b</li></ol>").markdown, "1. a\n2. b");
        assert_eq!(to_markdown("<blockquote><p>a</p><p>b</p></blockquote>").markdown, "> a\n>\n> b");
        assert_eq!(to_markdown("<pre><code class=\"language-rust\">let a = 1;\n</code></pre>").markdown, "```rust\nlet a = 1;\n```");
        assert_eq!(to_markdown("<hr>").markdown, "---");
        assert_eq!(to_markdown("<!-- only a note -->").markdown, "");
        assert_eq!(to_markdown("</div>").markdown, "");
    }

    #[test]
    fn a_table_becomes_a_pipe_table() {
        let t = "<table><thead><tr><th align=\"center\">A</th><th>B</th></tr></thead><tbody><tr><td>1<br>2</td><td>x|y</td></tr><tr><td>only</td></tr></tbody></table>";
        assert_eq!(to_markdown(t).markdown, "| A | B |\n| :---: | --- |\n| 1<br>2 | x\\|y |\n| only |  |");
    }

    #[test]
    fn a_block_knows_how_it_stands() {
        let b = to_markdown(r#"<p align="center"><img src="a.png" width="200" alt="x"></p>"#);
        assert_eq!(b.markdown, "![x|200](a.png)");
        assert_eq!(b.across, Across::Center);
        assert_eq!(to_markdown("<center>words</center>").across, Across::Center);
        assert_eq!(to_markdown(r#"<div style="text-align: right">words</div>"#).across, Across::End);
    }

    #[test]
    fn what_could_run_is_left_out() {
        let b = to_markdown("<script>alert(1)</script><style>p{}</style><iframe src=x></iframe><p onclick=\"x()\">words</p>");
        assert_eq!(b.markdown, "words");
    }

    #[test]
    fn details_show_their_summary_and_their_words() {
        assert_eq!(to_markdown("<details><summary>More</summary>Inside</details>").markdown, "**▾ More**\n\nInside");
    }

    #[test]
    fn broken_html_loses_nothing_and_never_panics() {
        assert_eq!(to_markdown("<p>one<p>two").markdown, "one\n\ntwo");
        assert_eq!(to_markdown("<b>open").markdown, "<b>open</b>");
        assert_eq!(to_markdown("a < b").markdown, "a \\< b");
        for s in ["<", "<a", "<a href=\"", "<!--", "</", "<>", "<é>", "<a é=é>", "&", "&#xffffffff;", "<td>", "</td>"] {
            let _ = to_markdown(s);
            let _ = read_tag(s);
        }
    }
}
