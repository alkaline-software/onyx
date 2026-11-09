//! A file of a vault's `.assets` folder read for showing. Most kinds come
//! out as markdown, which the note's own reader draws: a note as it is,
//! code fenced in its language, a table of values as a markdown table, a
//! notebook as its cells, a page of HTML, a Word document or a deck as
//! their text, a workbook as a table a sheet. A picture, a PDF and a
//! video or a sound are the window's to draw; a PDF's pages are drawn
//! here, by hayro.
//!
//! The rules follow Emaki's file pane (`panels.rs`, `files.rs`,
//! `office.rs`), and so do its limits: no file is read whole past
//! `TEXT_BYTES`, a table stops at `TABLE_ROWS`, an Office file or a PDF
//! too large is said to be. A LaTeX or Typst file is shown as its code:
//! Emaki's readers for them write Quarto's markdown (fenced divs,
//! cross-references, citations), which this reader does not draw.
//!
//! Nothing here writes, and nothing here runs another program. The
//! readers are other people's (calamine, quick-xml, hayro), and `read`
//! and `pdf_page` run in a task of the window's, where a panic takes the
//! app down: a file that makes one panic is a file not shown.

mod office;

use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

/// What the window shows for a file.
#[derive(Debug, Clone, PartialEq)]
pub enum Preview {
    /// Markdown the note's reader draws: the file itself, or what it becomes as markdown. `cut` when only the start was read.
    Markdown { text: String, cut: bool },
    /// A picture the window draws from its path (png, jpg, jpeg, gif, webp, svg, bmp, tiff, ico).
    Picture,
    /// A PDF with its page count; pages are drawn by `pdf_page`.
    Pdf { pages: usize },
    /// A video (true) or a sound (false), by its extension.
    Media { video: bool },
    /// Not something the window shows, or not readable: a short reason in plain English, one of `REASONS`.
    None(String),
}

/// Which group the sidebar's list puts a file in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Picture,
    Document,
    Media,
    Other,
}

/// Every reason `Preview::None` and `pdf_page` give, so the window can
/// hold each to its words.
pub const REASONS: &[&str] = &[UNREADABLE, NOT_SHOWN, EMPTY, NO_TEXT, TOO_LARGE, LOCKED, PDF_UNREADABLE, NO_PAGE, PAGE_UNDRAWN];

const UNREADABLE: &str = "This file could not be read.";
const NOT_SHOWN: &str = "Onyx cannot show this kind of file.";
const EMPTY: &str = "This file is empty.";
const NO_TEXT: &str = "There is no text in this file to show.";
const TOO_LARGE: &str = "This file is too large to show here.";
const LOCKED: &str = "This file is protected by a password.";
const PDF_UNREADABLE: &str = "This PDF could not be read.";
const NO_PAGE: &str = "This PDF has no such page.";
const PAGE_UNDRAWN: &str = "This page could not be drawn.";

/// The most of a text file that is read.
const TEXT_BYTES: usize = 1_500_000;
/// The most lines of code shown.
const CODE_LINES: usize = 20_000;
/// The most rows of a table, its head among them.
const TABLE_ROWS: usize = 500;
/// The most columns of a table.
const TABLE_COLS: usize = 40;
/// The most rows of a workbook, across its sheets: sheets past it are left out.
const BOOK_ROWS: usize = 2_000;
/// The most bytes of an Office file read; it is read whole, as a zip.
const OFFICE_BYTES: u64 = 40_000_000;
/// The most bytes of a PDF read; it is read whole.
const PDF_BYTES: u64 = 120_000_000;
/// The widest and the tallest a page is drawn, in pixels.
const PAGE_WIDEST: u32 = 4_096;
const PAGE_TALLEST: u32 = 16_384;
/// How far into a file a zero byte says it is not text.
const SNIFF: usize = 8_192;

const PICTURES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "tif", "tiff", "ico"];
const VIDEOS: &[&str] = &["mp4", "m4v", "mov", "webm", "ogv"];
const SOUNDS: &[&str] = &["mp3", "wav", "m4a", "aac", "flac", "ogg", "oga", "opus", "aif", "aiff"];
const MARKDOWN: &[&str] = &["md", "markdown", "qmd", "rmd", "txt"];
const DOCUMENTS: &[&str] = &["docx", "pptx"];
const BOOKS: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "ods"];
/// Text read as text whatever its bytes, as well as markdown and code.
const TEXTS: &[&str] = &["csv", "tsv", "ipynb", "html", "htm"];

fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Which group a file is in, by its name alone. A file of a name not
/// known here is `Other`, though `read` may still find it is text.
pub fn kind(path: &Path) -> Kind {
    let ext = ext_of(path);
    let ext = ext.as_str();
    if PICTURES.contains(&ext) {
        Kind::Picture
    } else if VIDEOS.contains(&ext) || SOUNDS.contains(&ext) {
        Kind::Media
    } else if ext == "pdf" || MARKDOWN.contains(&ext) || DOCUMENTS.contains(&ext) || BOOKS.contains(&ext) || TEXTS.contains(&ext) || language(path).is_some() {
        Kind::Document
    } else {
        Kind::Other
    }
}

/// Reads the file; runs off the window's thread, so it may take a moment but must not read a huge file whole.
pub fn read(path: &Path) -> Preview {
    std::panic::catch_unwind(|| read_file(path)).unwrap_or_else(|_| Preview::None(UNREADABLE.into()))
}

fn none(why: &str) -> Preview {
    Preview::None(why.into())
}

fn read_file(path: &Path) -> Preview {
    let Ok(meta) = std::fs::metadata(path) else { return none(UNREADABLE) };
    if !meta.is_file() {
        return none(NOT_SHOWN);
    }
    let ext = ext_of(path);
    let ext = ext.as_str();
    if PICTURES.contains(&ext) {
        return Preview::Picture;
    }
    if VIDEOS.contains(&ext) || SOUNDS.contains(&ext) {
        return Preview::Media { video: VIDEOS.contains(&ext) };
    }
    if meta.len() == 0 {
        return none(EMPTY);
    }
    if ext == "pdf" {
        return match open_pdf(path) {
            Ok(pdf) if !pdf.pages().is_empty() => Preview::Pdf { pages: pdf.pages().len() },
            Ok(_) => none(PDF_UNREADABLE),
            Err(why) => none(why),
        };
    }
    if DOCUMENTS.contains(&ext) || BOOKS.contains(&ext) {
        return read_office(path, ext, meta.len());
    }
    let mut bytes = Vec::new();
    if std::fs::File::open(path).and_then(|f| f.take(TEXT_BYTES as u64 + 1).read_to_end(&mut bytes)).is_err() {
        return none(UNREADABLE);
    }
    text_preview(path, bytes)
}

/// What a file that begins with `bytes` (at most `TEXT_BYTES` and one
/// more) is shown as, when it is text.
fn text_preview(path: &Path, mut bytes: Vec<u8>) -> Preview {
    let ext = ext_of(path);
    let ext = ext.as_str();
    if bytes.iter().take(SNIFF).any(|b| *b == 0) {
        return none(NOT_SHOWN);
    }
    let cut = bytes.len() > TEXT_BYTES;
    if cut {
        // The start, up to the last whole line.
        bytes.truncate(TEXT_BYTES);
        let end = bytes.iter().rposition(|b| *b == b'\n').map_or(bytes.len(), |i| i + 1);
        bytes.truncate(end);
    }
    let lang = language(path);
    let known = MARKDOWN.contains(&ext) || TEXTS.contains(&ext) || lang.is_some();
    // A file of a name not known here is text only when its bytes are
    // UTF-8, a character cut off at the end aside.
    if !known && std::str::from_utf8(&bytes).is_err_and(|e| e.error_len().is_some()) {
        return none(NOT_SHOWN);
    }
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    match ext {
        "csv" | "tsv" => {
            let (rows, more) = rows_of(text, if ext == "tsv" { '\t' } else { ',' }, TABLE_ROWS);
            match data_table(&rows) {
                Some(table) => Preview::Markdown { text: table, cut: more || cut },
                None => none(NO_TEXT),
            }
        }
        "ipynb" if !cut => match notebook_markdown(text) {
            Some(cells) => Preview::Markdown { text: cells, cut: false },
            None => code(text, Some("json"), cut),
        },
        "html" | "htm" if !cut => {
            let page = crate::html::to_markdown(page_body(text)).markdown;
            if page.trim().is_empty() {
                none(NO_TEXT)
            } else {
                Preview::Markdown { text: page + "\n", cut: false }
            }
        }
        _ if MARKDOWN.contains(&ext) && lang.is_none() => Preview::Markdown { text: text.to_string(), cut },
        "ipynb" => code(text, Some("json"), cut),
        "html" | "htm" => code(text, Some("html"), cut),
        _ => code(text, lang, cut),
    }
}

/// What a page of HTML shows: what its `<body>` holds, or what its
/// `<html>` does, with no `<!DOCTYPE>`. `html::to_markdown` reads a block
/// as a note holds one, and takes a tag it does not know, `<body>` among
/// them, for words in a line.
fn page_body(text: &str) -> &str {
    // Lowercase ASCII keeps every byte where it is.
    let lower = text.to_ascii_lowercase();
    let inside = |tag: &str| -> Option<(usize, usize)> {
        let open = lower.find(&format!("<{tag}")).filter(|&at| lower[at + tag.len() + 1..].starts_with(['>', ' ', '\t', '\n', '\r']))?;
        let start = open + lower[open..].find('>')? + 1;
        let end = lower.rfind(&format!("</{tag}")).filter(|&end| end >= start).unwrap_or(lower.len());
        Some((start, end))
    };
    if let Some((start, end)) = inside("body").or_else(|| inside("html")) {
        return &text[start..end];
    }
    match lower.trim_start().strip_prefix("<!doctype").and_then(|rest| rest.find('>').map(|at| lower.len() - rest.len() + at + 1)) {
        Some(after) => &text[after..],
        None => text,
    }
}

/// Code as a fenced block in its language, its first `CODE_LINES` lines.
fn code(text: &str, lang: Option<&str>, cut: bool) -> Preview {
    let total = text.lines().count();
    let kept: String = text.lines().take(CODE_LINES).collect::<Vec<_>>().join("\n");
    Preview::Markdown { text: fenced(&kept, lang.unwrap_or("")), cut: cut || total > CODE_LINES }
}

/// `body` fenced, with a fence longer than any run of backticks in it.
fn fenced(body: &str, lang: &str) -> String {
    let (mut longest, mut run) = (0, 0);
    for c in body.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}{lang}\n{}\n{fence}\n", body.trim_end_matches('\n'))
}

/// The word a fence names a file's language by, as the highlighter and
/// the code block's mark know it; None for a file that is no code.
fn language(path: &Path) -> Option<&'static str> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    // A file known by its whole name.
    let by_name = match name.as_str() {
        "makefile" | "gnumakefile" => Some("make"),
        "cmakelists.txt" => Some("cmake"),
        "dockerfile" | "containerfile" => Some("dockerfile"),
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".zprofile" => Some("bash"),
        ".gitignore" | ".gitattributes" | ".editorconfig" => Some(""),
        _ => None,
    };
    if by_name.is_some() {
        return by_name;
    }
    Some(match ext_of(path).as_str() {
        "rs" => "rust",
        "py" | "pyw" | "pyi" => "python",
        "r" => "r",
        "jl" => "julia",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "json" | "jsonc" | "geojson" | "json5" => "json",
        "sh" | "bash" | "zsh" | "fish" | "ksh" => "bash",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "ini" | "cfg" | "conf" => "ini",
        "css" => "css",
        "scss" | "sass" | "less" => "scss",
        "sql" => "sql",
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "m" | "mm" => "objc",
        "cs" => "csharp",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "swift" => "swift",
        "php" => "php",
        "rb" => "ruby",
        "lua" => "lua",
        "zig" => "zig",
        "ex" | "exs" => "elixir",
        "hs" => "haskell",
        "pl" | "pm" => "perl",
        "dart" => "dart",
        "proto" => "proto",
        "graphql" | "gql" => "graphql",
        "cmake" => "cmake",
        "mk" => "make",
        "diff" | "patch" => "diff",
        "svelte" => "svelte",
        "astro" => "astro",
        "vue" => "vue",
        "vim" => "vim",
        "xml" | "plist" | "xsl" | "xslt" | "rss" | "atom" => "xml",
        "tex" | "latex" | "ltx" | "sty" | "cls" => "latex",
        "bib" => "bibtex",
        "typ" => "typst",
        "do" | "ado" => "stata",
        "ps1" | "psm1" => "powershell",
        "bat" | "cmd" => "batch",
        "log" => "",
        _ => return None,
    })
}

// ------------------------------------------------------------------ tables

/// A comma- or tab-separated file as rows of cells, for showing: quoted
/// cells may hold the separator, a doubled quote and line breaks. No
/// more than `most` rows are read; the second answer says whether the
/// text went on. (Emaki's `files::table`.)
fn rows_of(text: &str, sep: char, most: usize) -> (Vec<Vec<String>>, bool) {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let (mut row, mut cell, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => quoted = false,
                c => cell.push(c),
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => {
                quoted = true;
                any = true;
            }
            c if c == sep => {
                row.push(std::mem::take(&mut cell));
                any = true;
            }
            '\r' => {}
            '\n' => {
                if any || !cell.is_empty() {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                }
                any = false;
                if rows.len() >= most {
                    return (rows, chars.any(|c| !c.is_whitespace()));
                }
            }
            c => cell.push(c),
        }
    }
    if any || !cell.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    (rows, false)
}

/// Rows of values as a markdown table, the first row its head, at most
/// `TABLE_COLS` wide. A value is its characters: what markdown would read
/// as its own marks (a `*`, a `$` of a price) is escaped. None when no
/// cell holds anything.
fn data_table(rows: &[Vec<String>]) -> Option<String> {
    let escaped: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .take(TABLE_COLS)
                .map(|cell| {
                    let mut out = String::with_capacity(cell.len());
                    for c in cell.chars() {
                        // A bar is escaped by `office::table`, a line break made a space.
                        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '~' | '&' | '#' | '$' | '=') {
                            out.push('\\');
                        }
                        out.push(c);
                    }
                    out
                })
                .collect()
        })
        .collect();
    if escaped.is_empty() {
        return None;
    }
    office::table(&escaped)
}

// ---------------------------------------------------------------- notebooks

/// A Jupyter notebook as markdown, to read: each markdown cell as it
/// is, each code cell fenced in the notebook's language with `In [n]`
/// over it, and what it printed fenced under it. A picture among the
/// outputs is a line saying so. `None` when the text is no notebook.
/// (Emaki's `files::notebook_markdown`.)
fn notebook_markdown(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let cells = v.get("cells")?.as_array()?;
    let lang = v.pointer("/metadata/kernelspec/language").or_else(|| v.pointer("/metadata/language_info/name")).and_then(|l| l.as_str()).unwrap_or("python").to_lowercase();
    // A cell's `source` and an output's `text` are a string or its lines.
    let joined = |v: Option<&serde_json::Value>| match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(lines)) => lines.iter().filter_map(|l| l.as_str()).collect(),
        _ => String::new(),
    };
    let mut out = String::new();
    for cell in cells {
        let source = joined(cell.get("source"));
        match cell.get("cell_type").and_then(|t| t.as_str()) {
            Some("markdown") => {
                out.push_str(source.trim_end());
                out.push_str("\n\n");
            }
            Some("code") => {
                let n = cell.get("execution_count").and_then(|n| n.as_u64()).map(|n| n.to_string()).unwrap_or_else(|| " ".into());
                out.push_str(&format!("**In [{n}]**\n\n"));
                out.push_str(&fenced(&source, &lang));
                out.push('\n');
                for output in cell.get("outputs").and_then(|o| o.as_array()).into_iter().flatten() {
                    let data = output.get("data");
                    let words = match output.get("output_type").and_then(|t| t.as_str()) {
                        Some("stream") => joined(output.get("text")),
                        Some("error") => format!("{}: {}", output.get("ename").and_then(|e| e.as_str()).unwrap_or("error"), output.get("evalue").and_then(|e| e.as_str()).unwrap_or("")),
                        _ => joined(data.and_then(|d| d.get("text/plain"))),
                    };
                    if data.and_then(|d| d.as_object()).is_some_and(|d| d.keys().any(|k| k.starts_with("image/"))) {
                        out.push_str("*A picture. Open the notebook to see it.*\n\n");
                    } else if !words.trim().is_empty() {
                        out.push_str(&fenced(&words, "text"));
                        out.push('\n');
                    }
                }
            }
            _ if !source.trim().is_empty() => {
                out.push_str(&fenced(&source, "text"));
                out.push('\n');
            }
            _ => {}
        }
    }
    Some(out.trim_end().to_string() + "\n")
}

// ------------------------------------------------------------------ Office

fn read_office(path: &Path, ext: &str, size: u64) -> Preview {
    if size > OFFICE_BYTES {
        return none(TOO_LARGE);
    }
    let Ok(bytes) = std::fs::read(path) else { return none(UNREADABLE) };
    let refused = |why: office::Refused| none(if why == office::Refused::Locked { LOCKED } else { UNREADABLE });
    if DOCUMENTS.contains(&ext) {
        return match office::document(bytes, ext) {
            Ok(text) if text.trim().is_empty() => none(NO_TEXT),
            Ok(text) => Preview::Markdown { text: text + "\n", cut: false },
            Err(why) => refused(why),
        };
    }
    let sheets = match office::book(bytes, ext, TABLE_ROWS) {
        Ok(sheets) => sheets,
        Err(why) => return refused(why),
    };
    let (mut parts, mut cut, mut rows) = (Vec::new(), false, 0usize);
    for sheet in &sheets {
        if rows >= BOOK_ROWS {
            cut = true;
            break;
        }
        let room = BOOK_ROWS - rows;
        let kept = &sheet.rows[..sheet.rows.len().min(room)];
        rows += kept.len();
        cut |= sheet.more || kept.len() < sheet.rows.len() || sheet.rows.iter().any(|r| r.len() > TABLE_COLS);
        let head = format!("## {}", sheet.name.replace('\n', " "));
        parts.push(match data_table(kept) {
            Some(table) => format!("{head}\n\n{table}"),
            None => head,
        });
    }
    if sheets.iter().all(|s| s.rows.iter().flatten().all(|c| c.trim().is_empty())) {
        return none(NO_TEXT);
    }
    Preview::Markdown { text: parts.join("\n\n") + "\n", cut }
}

// --------------------------------------------------------------------- PDF

fn open_pdf(path: &Path) -> Result<hayro::hayro_syntax::Pdf, &'static str> {
    use hayro::hayro_syntax::{LoadPdfError, Pdf};
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > PDF_BYTES {
        return Err(TOO_LARGE);
    }
    let bytes = std::fs::read(path).map_err(|_| UNREADABLE)?;
    Pdf::new(bytes).map_err(|e| match e {
        LoadPdfError::Decryption(_) => LOCKED,
        _ => PDF_UNREADABLE,
    })
}

/// One PDF page as RGBA8 pixels, `width` pixels wide (height by the page's aspect): (width, height, rgba).
///
/// The page is drawn on white, so every pixel is opaque. `page` counts
/// from 0; `width` is held to 1 through 4096.
pub fn pdf_page(path: &Path, page: usize, width: u32) -> Result<(u32, u32, Vec<u8>), String> {
    std::panic::catch_unwind(|| draw_page(path, page, width)).unwrap_or(Err(PAGE_UNDRAWN)).map_err(String::from)
}

/// Each page's size in points, as it is drawn (its crop box, turned as
/// it says): what a page at 100% is.
pub fn pdf_sizes(path: &Path) -> Result<Vec<(f32, f32)>, String> {
    std::panic::catch_unwind(|| {
        let pdf = open_pdf(path)?;
        Ok(pdf.pages().iter().map(|p| p.render_dimensions()).collect())
    })
    .unwrap_or(Err(PDF_UNREADABLE))
    .map_err(String::from)
}

fn draw_page(path: &Path, page: usize, width: u32) -> Result<(u32, u32, Vec<u8>), &'static str> {
    let pdf = open_pdf(path)?;
    draw_one(&pdf, &hayro::RenderCache::new(), page, width)
}

/// Several pages of one PDF as `pdf_page` draws each, the file read and
/// its fonts loaded once for all of them: a sidebar's thumbnails. A page
/// that cannot be drawn is `None`.
pub fn pdf_pages(path: &Path, pages: &[usize], width: u32) -> Vec<Option<(u32, u32, Vec<u8>)>> {
    std::panic::catch_unwind(|| {
        let Ok(pdf) = open_pdf(path) else { return vec![None; pages.len()] };
        let cache = hayro::RenderCache::new();
        pages.iter().map(|&p| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| draw_one(&pdf, &cache, p, width))).ok().and_then(Result::ok)).collect()
    })
    .unwrap_or_else(|_| vec![None; pages.len()])
}

fn draw_one<'a>(pdf: &'a hayro::hayro_syntax::Pdf, cache: &hayro::RenderCache<'a>, page: usize, width: u32) -> Result<(u32, u32, Vec<u8>), &'static str> {
    use hayro::vello_cpu::color::palette::css::WHITE;
    let pages = pdf.pages();
    let page = pages.get(page).ok_or(NO_PAGE)?;
    let (w, h) = page.render_dimensions();
    if !(w.is_finite() && h.is_finite() && w > 0. && h > 0.) {
        return Err(PAGE_UNDRAWN);
    }
    let width = width.clamp(1, PAGE_WIDEST);
    let scale = width as f32 / w;
    let height = ((h * scale).round() as u32).clamp(1, PAGE_TALLEST);
    let settings = hayro::RenderSettings { x_scale: scale, y_scale: scale, width: Some(width as u16), height: Some(height as u16), bg_color: WHITE };
    let pixmap = hayro::render(page, cache, &hayro::hayro_interpret::InterpreterSettings::default(), &settings);
    Ok((pixmap.width() as u32, pixmap.height() as u32, pixmap.data_as_u8_slice().to_vec()))
}

/// One line of a PDF's outline (its bookmarks), in the order it reads,
/// a child after its parent.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfMark {
    pub title: String,
    /// How deep it stands, the top being 0.
    pub depth: usize,
    /// The page it leads to, from 0, and how far down it in points, where
    /// the PDF says; none where it leads nowhere in the file.
    pub page: Option<usize>,
    pub down: Option<f32>,
    /// Whether it has lines under it, and whether the PDF has them shown.
    pub kids: bool,
    pub open: bool,
}

/// The most lines an outline is read to: a loop in a broken file ends.
const MARKS_MOST: usize = 5000;

/// A PDF's outline, as its viewer's sidebar lists it; empty when it has
/// none.
pub fn pdf_outline(path: &Path) -> Result<Vec<PdfMark>, String> {
    std::panic::catch_unwind(|| {
        let pdf = open_pdf(path)?;
        Ok(read_outline(&pdf))
    })
    .unwrap_or(Err(PDF_UNREADABLE))
    .map_err(String::from)
}

/// Contents read from a PDF's type, for one with no outline: the lines
/// set larger than its body text, as an eye takes a page's titles. The
/// body is the size most of its letters have; a line a fifth larger or
/// more is a heading, its level by its size among the headings' (three
/// at most, the larger the higher), a heading wrapped onto a second line
/// joined to its first, and a line that comes back at the same height
/// on three pages or more (a running head) kept only where it first is. A size many of the letters have is a second
/// size of text, not a heading's; a sentence is not a heading; past a
/// few lines a page the smallest headings go. Empty for a PDF with no
/// text, as a scan is.
pub fn pdf_contents(pages: &[PdfText]) -> Vec<PdfMark> {
    struct Line {
        page: usize,
        text: String,
        size: i32,
        top: f32,
        bottom: f32,
    }
    // Sizes in half points: what a line's letters have in common.
    let half = |em: f32| (em * 2.).round() as i32;
    let mut letters: HashMap<i32, usize> = HashMap::new();
    let mut lines = Vec::new();
    for (p, page) in pages.iter().enumerate() {
        for r in page.lines() {
            let inked: Vec<&PdfGlyph> = page.glyphs[r.clone()].iter().filter(|g| !g.text.trim().is_empty()).collect();
            if inked.is_empty() {
                continue;
            }
            let mut sizes: HashMap<i32, usize> = HashMap::new();
            for g in &inked {
                let n = g.text.chars().count();
                *letters.entry(half(g.em)).or_default() += n;
                *sizes.entry(half(g.em)).or_default() += n;
            }
            let Some(size) = sizes.into_iter().max_by_key(|&(s, n)| (n, s)).map(|(s, _)| s) else { continue };
            let top = inked.iter().map(|g| g.base - g.em * 0.85).fold(f32::MAX, f32::min);
            let bottom = inked.iter().map(|g| g.base + g.em * 0.25).fold(f32::MIN, f32::max);
            let text = unligate(&page.words(r.start, r.end).split_whitespace().collect::<Vec<_>>().join(" "));
            lines.push(Line { page: p, text, size, top, bottom });
        }
    }
    let all: usize = letters.values().sum();
    let Some(body) = letters.iter().max_by_key(|&(&s, &n)| (n, -s)).map(|(&s, _)| s) else { return Vec::new() };
    // A size a large share of the letters have is a second size of text
    // (a letter's greeting, a table), not a heading's.
    let rare = |size: i32| letters.get(&size).is_some_and(|&n| n * 6 < all);
    // Down each page as it reads, not in the order the file draws.
    lines.sort_by(|a, b| a.page.cmp(&b.page).then(a.top.total_cmp(&b.top)));
    // Larger by a fifth and rare, a few words with a letter among them,
    // not a sentence running on (a comma at its end, a dozen words).
    let heading = |l: &Line| {
        l.size as f32 >= body as f32 * 1.2 && rare(l.size) && l.text.chars().any(char::is_alphabetic) && l.text.chars().count() <= 80 && l.text.split_whitespace().count() <= 12 && !l.text.ends_with([',', ';', '，', '；'])
    };
    let mut heads: Vec<Line> = Vec::new();
    for l in lines.into_iter().filter(heading) {
        if let Some(last) = heads.last_mut() {
            if last.page == l.page && last.size == l.size && l.top > last.top && l.top - last.bottom < l.size as f32 / 2. * 0.8 {
                last.text.push(' ');
                last.text.push_str(&l.text);
                last.bottom = l.bottom;
                continue;
            }
        }
        heads.push(l);
    }
    // A running head: the same words at the same height on three pages
    // or more. Its first stands, as the document's title.
    let mut on: HashMap<(String, i32), std::collections::HashSet<usize>> = HashMap::new();
    for h in &heads {
        on.entry((h.text.clone(), (h.top / 2.).round() as i32)).or_default().insert(h.page);
    }
    let mut seen = std::collections::HashSet::new();
    heads.retain(|h| {
        let key = (h.text.clone(), (h.top / 2.).round() as i32);
        on.get(&key).is_none_or(|p| p.len() < 3) || seen.insert(key)
    });
    heads.dedup_by(|b, a| a.page == b.page && a.text == b.text);
    let mut levels: Vec<i32> = heads.iter().map(|h| h.size).collect();
    levels.sort_unstable_by(|a, b| b.cmp(a));
    levels.dedup();
    // A list a few lines to a page at most: the smallest size goes while
    // there are more, as a table of contents keeps the larger titles.
    let most = (pages.len() * 6).max(12);
    while heads.len() > most && levels.len() > 1 {
        let Some(least) = levels.pop() else { break };
        heads.retain(|h| h.size != least);
    }
    let depth_of = |size: i32| levels.iter().position(|&s| s == size).unwrap_or(0).min(2);
    let mut out: Vec<PdfMark> = heads
        .iter()
        .take(MARKS_MOST)
        .map(|h| PdfMark { title: h.text.clone(), depth: depth_of(h.size), page: Some(h.page), down: Some((h.top - h.size as f32 / 2. * 0.5).max(0.)), kids: false, open: true })
        .collect();
    // A level no line stands under before it, as the third size with no
    // second over it, comes up to stand under the line before it.
    let mut was = None::<usize>;
    for m in &mut out {
        m.depth = match was {
            None => 0,
            Some(d) => m.depth.min(d + 1),
        };
        was = Some(m.depth);
    }
    for i in 0..out.len() {
        out[i].kids = out.get(i + 1).is_some_and(|n| n.depth > out[i].depth);
    }
    out
}

/// Letters a font draws as one (`ﬁ`) written out, as a title reads.
fn unligate(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'ﬀ' => out.push_str("ff"),
            'ﬁ' => out.push_str("fi"),
            'ﬂ' => out.push_str("fl"),
            'ﬃ' => out.push_str("ffi"),
            'ﬄ' => out.push_str("ffl"),
            'ﬅ' | 'ﬆ' => out.push_str("st"),
            c => out.push(c),
        }
    }
    out
}

fn read_outline(pdf: &hayro::hayro_syntax::Pdf) -> Vec<PdfMark> {
    use hayro::hayro_syntax::object::{Dict, ObjectIdentifier};
    let xref = pdf.xref();
    let Some(root) = xref.get::<Dict<'_>>(xref.root_id()) else { return Vec::new() };
    let Some(first) = root.get::<Dict<'_>>(b"Outlines").and_then(|o| o.get::<Dict<'_>>(b"First")) else { return Vec::new() };
    let pages: HashMap<ObjectIdentifier, usize> = pdf.pages().iter().enumerate().filter_map(|(i, p)| p.raw().obj_id().map(|id| (id, i))).collect();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Each sibling in turn, its children before the next: a stack of the
    // next item to read at each depth.
    let mut stack: Vec<(Dict<'_>, usize)> = vec![(first, 0)];
    while let Some((item, depth)) = stack.pop() {
        if out.len() >= MARKS_MOST || item.obj_id().is_some_and(|id| !seen.insert(id)) {
            continue;
        }
        let title = item.get::<hayro::hayro_syntax::object::String<'_>>(b"Title").map(|s| text_string(s.as_bytes())).unwrap_or_default();
        let dest = item.get::<hayro::hayro_syntax::object::Object<'_>>(b"Dest").or_else(|| {
            let action = item.get::<Dict<'_>>(b"A")?;
            (action.get::<hayro::hayro_syntax::object::Name<'_>>(b"S")?.as_str() == "GoTo").then(|| action.get::<hayro::hayro_syntax::object::Object<'_>>(b"D"))?
        });
        let (page, down) = dest.and_then(|d| destination(pdf, &root, &pages, d, 0)).unwrap_or((None, None));
        let child = item.get::<Dict<'_>>(b"First");
        let count = item.get::<i64>(b"Count").unwrap_or(0);
        out.push(PdfMark { title: title.split_whitespace().collect::<Vec<_>>().join(" "), depth, page, down, kids: child.is_some(), open: count > 0 });
        if let Some(next) = item.get::<Dict<'_>>(b"Next") {
            stack.push((next, depth));
        }
        if let Some(child) = child {
            stack.push((child, depth + 1));
        }
    }
    out
}

/// Where a destination leads: an explicit one (`[page /XYZ left top
/// zoom]`, `[page /FitH top]`), or a name the catalog's `Dests` or the
/// name tree's `Dests` holds.
fn destination<'a>(pdf: &'a hayro::hayro_syntax::Pdf, root: &hayro::hayro_syntax::object::Dict<'a>, pages: &HashMap<hayro::hayro_syntax::object::ObjectIdentifier, usize>, dest: hayro::hayro_syntax::object::Object<'a>, hops: u8) -> Option<(Option<usize>, Option<f32>)> {
    use hayro::hayro_syntax::object::{Dict, MaybeRef, Object};
    if hops > 4 {
        return None;
    }
    let named = |key: &[u8]| -> Option<Object<'a>> {
        if let Some(found) = root.get::<Dict<'_>>(b"Dests").and_then(|d| d.get::<Object<'_>>(key)) {
            return Some(found);
        }
        name_tree(root.get::<Dict<'_>>(b"Names")?.get::<Dict<'_>>(b"Dests")?, key, 0)
    };
    match dest {
        Object::Array(a) => {
            let mut items = a.raw_iter();
            let page = match items.next()? {
                MaybeRef::Ref(r) => pages.get(&r.into()).copied(),
                MaybeRef::NotRef(Object::Number(n)) => usize::try_from(n.as_i64()).ok().filter(|n| *n < pdf.pages().len()),
                _ => None,
            };
            let kind = match items.next() {
                Some(MaybeRef::NotRef(Object::Name(n))) => n.as_str().to_string(),
                _ => String::new(),
            };
            let nums: Vec<Option<f32>> = items
                .map(|o| match o {
                    MaybeRef::NotRef(Object::Number(n)) => Some(n.as_f32()),
                    _ => None,
                })
                .collect();
            let top = match kind.as_str() {
                "XYZ" => nums.get(1).copied().flatten(),
                "FitH" | "FitBH" => nums.first().copied().flatten(),
                "FitR" => nums.get(3).copied().flatten(),
                _ => None,
            };
            // How far down the page the top is, as the page is drawn; a
            // page turned leaves it unknown.
            let down = page.zip(top).and_then(|(p, top)| {
                let page = pdf.pages().get(p)?;
                if !matches!(page.rotation(), hayro::hayro_syntax::page::Rotation::None) {
                    return None;
                }
                let crop = page.intersected_crop_box();
                Some(((crop.y1 as f32) - top).clamp(0., (crop.y1 - crop.y0) as f32))
            });
            Some((page, down))
        }
        Object::Dict(d) => destination(pdf, root, pages, d.get::<Object<'_>>(b"D")?, hops + 1),
        Object::Name(n) => destination(pdf, root, pages, named(n.as_ref())?, hops + 1),
        Object::String(s) => destination(pdf, root, pages, named(s.as_bytes())?, hops + 1),
        _ => None,
    }
}

/// A key in a name tree: its leaves' `Names`, else the kid whose `Limits`
/// hold it.
fn name_tree<'a>(node: hayro::hayro_syntax::object::Dict<'a>, key: &[u8], depth: u8) -> Option<hayro::hayro_syntax::object::Object<'a>> {
    use hayro::hayro_syntax::object::{Array, Dict, Object, String};
    if depth > 32 {
        return None;
    }
    if let Some(names) = node.get::<Array<'_>>(b"Names") {
        let mut it = names.iter::<Object<'_>>();
        while let (Some(k), Some(v)) = (it.next(), it.next()) {
            if let Object::String(k) = k {
                if k.as_bytes() == key {
                    return Some(v);
                }
            }
        }
        return None;
    }
    for kid in node.get::<Array<'_>>(b"Kids")?.iter::<Dict<'_>>() {
        let inside = kid.get::<Array<'_>>(b"Limits").map(|l| l.iter::<String<'_>>().map(|s| s.as_bytes().to_vec()).collect::<Vec<_>>()).is_none_or(|l| l.len() < 2 || (l[0].as_slice() <= key && key <= l[1].as_slice()));
        if inside {
            if let Some(found) = name_tree(kid, key, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

/// A PDF's text string: UTF-16 after its mark, UTF-8 after its own, else
/// PDFDocEncoding, read here as Latin-1 (they differ in a few marks).
fn text_string(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    bytes.iter().map(|&b| b as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn file(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn markdown(p: Preview) -> (String, bool) {
        match p {
            Preview::Markdown { text, cut } => (text, cut),
            other => panic!("not markdown: {other:?}"),
        }
    }

    fn zipped(dir: &Path, name: &str, parts: &[(&str, &str)]) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (part, text) in parts {
            zip.start_file(*part, opts).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn a_file_is_grouped_by_its_name() {
        assert_eq!(kind(Path::new("a/Cat.PNG")), Kind::Picture);
        assert_eq!(kind(Path::new("a.svg")), Kind::Picture);
        assert_eq!(kind(Path::new("talk.mov")), Kind::Media);
        assert_eq!(kind(Path::new("song.flac")), Kind::Media);
        for doc in ["paper.pdf", "notes.md", "data.csv", "run.ipynb", "deck.pptx", "book.xlsx", "main.rs", "Makefile", "page.html"] {
            assert_eq!(kind(Path::new(doc)), Kind::Document, "{doc}");
        }
        assert_eq!(kind(Path::new("archive.zip")), Kind::Other);
        assert_eq!(kind(Path::new("noext")), Kind::Other);
    }

    #[test]
    fn pictures_media_and_nothing_are_said_by_name() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(&file(dir.path(), "a.jpg", b"not really")), Preview::Picture);
        assert_eq!(read(&file(dir.path(), "a.m4a", b"x")), Preview::Media { video: false });
        assert_eq!(read(&file(dir.path(), "a.webm", b"x")), Preview::Media { video: true });
        assert_eq!(read(&file(dir.path(), "blank.md", b"")), Preview::None(EMPTY.into()));
        assert_eq!(read(&file(dir.path(), "a.bin", &[1, 2, 0, 3])), Preview::None(NOT_SHOWN.into()));
        assert_eq!(read(&file(dir.path(), "latin.dat", b"caf\xe9 au lait")), Preview::None(NOT_SHOWN.into()));
        assert_eq!(read(&dir.path().join("gone.md")), Preview::None(UNREADABLE.into()));
        assert!(REASONS.iter().all(|r| r.ends_with('.')));
    }

    #[test]
    fn a_note_is_its_own_text() {
        let dir = tempfile::tempdir().unwrap();
        let text = "# Title\n\nSome *words*.\n";
        assert_eq!(markdown(read(&file(dir.path(), "a.qmd", text.as_bytes()))), (text.to_string(), false));
        assert_eq!(markdown(read(&file(dir.path(), "a.txt", b"plain\nlines\n"))).0, "plain\nlines\n");
    }

    #[test]
    fn a_big_file_is_read_only_to_its_limit() {
        let dir = tempfile::tempdir().unwrap();
        let line = "word ".repeat(19) + "\n";
        let big = line.repeat(TEXT_BYTES / line.len() + 50);
        let (text, cut) = markdown(read(&file(dir.path(), "big.md", big.as_bytes())));
        assert!(cut);
        assert!(text.len() <= TEXT_BYTES && text.ends_with('\n'));
        let lines = "x\n".repeat(CODE_LINES + 5);
        let (text, cut) = markdown(read(&file(dir.path(), "long.py", lines.as_bytes())));
        assert!(cut);
        assert_eq!(text.lines().filter(|l| *l == "x").count(), CODE_LINES);
    }

    #[test]
    fn code_is_fenced_in_its_language_past_its_own_backticks() {
        let dir = tempfile::tempdir().unwrap();
        let (text, cut) = markdown(read(&file(dir.path(), "main.rs", b"fn main() {}\n")));
        assert_eq!((text.as_str(), cut), ("```rust\nfn main() {}\n```\n", false));
        let (text, _) = markdown(read(&file(dir.path(), "doc.py", b"s = \"\"\"\n````\n\"\"\"\n")));
        assert!(text.starts_with("`````python\n") && text.ends_with("\n`````\n"), "{text}");
        // Text of a name not known here is fenced with no language.
        let (text, _) = markdown(read(&file(dir.path(), "NOTES", "naïve\n".as_bytes())));
        assert_eq!(text, "```\nnaïve\n```\n");
        let (text, _) = markdown(read(&file(dir.path(), "paper.tex", b"\\section{A}\n")));
        assert!(text.starts_with("```latex\n"));
    }

    #[test]
    fn a_table_of_values_is_a_markdown_table() {
        let dir = tempfile::tempdir().unwrap();
        let csv = "name,price,note\nTea,$3,\"a, b | c\"\n\"multi\nline\",*4*,\n";
        let (text, cut) = markdown(read(&file(dir.path(), "a.csv", csv.as_bytes())));
        assert!(!cut);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "| name | price | note |");
        assert_eq!(lines[1], "| --- | --- | --- |");
        assert_eq!(lines[2], "| Tea | \\$3 | a, b \\| c |");
        assert_eq!(lines[3], "| multi line | \\*4\\* |  |");
        let (text, _) = markdown(read(&file(dir.path(), "a.tsv", b"a\tb\n1\t2\n")));
        assert!(text.starts_with("| a | b |"));
        let many: String = (0..TABLE_ROWS + 10).map(|i| format!("{i},x\n")).collect();
        let (text, cut) = markdown(read(&file(dir.path(), "many.csv", many.as_bytes())));
        assert!(cut);
        assert_eq!(text.lines().count(), TABLE_ROWS + 1);
    }

    #[test]
    fn a_notebook_is_its_cells() {
        let dir = tempfile::tempdir().unwrap();
        let nb = r##"{"metadata":{"kernelspec":{"language":"python"}},"cells":[
            {"cell_type":"markdown","source":["# Fit\n","A model."]},
            {"cell_type":"code","execution_count":3,"source":"print(1)","outputs":[{"output_type":"stream","text":["1\n"]}]}
        ]}"##;
        let (text, cut) = markdown(read(&file(dir.path(), "a.ipynb", nb.as_bytes())));
        assert!(!cut);
        assert_eq!(text, "# Fit\nA model.\n\n**In [3]**\n\n```python\nprint(1)\n```\n\n```text\n1\n```\n");
        // JSON that is no notebook is shown as JSON.
        let (text, _) = markdown(read(&file(dir.path(), "b.ipynb", b"[1, 2]")));
        assert!(text.starts_with("```json\n"));
    }

    #[test]
    fn a_page_of_html_is_its_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let page = "<!DOCTYPE html>\n<html><head><title>T</title><style>p{}</style></head>\n<body><h1>Hello</h1>\n<p>One <b>two</b></p><ul><li>a</li></ul></body></html>\n";
        let (text, _) = markdown(read(&file(dir.path(), "a.html", page.as_bytes())));
        assert!(text.contains("# Hello") && text.contains("One <b>two</b>") && text.contains("- a"), "{text}");
        assert!(!text.contains("p{}") && !text.contains("DOCTYPE"), "{text}");
        let (text, _) = markdown(read(&file(dir.path(), "b.htm", b"<HTML><Body class=\"x\">\n<h2>Two</h2><p>Words</p></BODY></HTML>")));
        assert_eq!(text, "## Two\n\nWords\n");
        let (text, _) = markdown(read(&file(dir.path(), "c.html", b"<!doctype html>\n<p>Just <i>this</i></p>")));
        assert_eq!(text, "Just <i>this</i>\n");
        assert_eq!(read(&file(dir.path(), "d.html", b"<script>x()</script>")), Preview::None(NO_TEXT.into()));
    }

    #[test]
    fn a_word_document_is_its_text() {
        let dir = tempfile::tempdir().unwrap();
        let body = r#"<?xml version="1.0"?><w:document xmlns:w="w"><w:body>
            <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Plan</w:t></w:r></w:p>
            <w:p><w:r><w:t xml:space="preserve">Do the </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>first</w:t></w:r><w:r><w:t xml:space="preserve"> thing &amp; more.</w:t></w:r></w:p>
            <w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>
            <w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
            </w:body></w:document>"#;
        let path = zipped(dir.path(), "a.docx", &[("word/document.xml", body)]);
        let (text, cut) = markdown(read(&path));
        assert!(!cut);
        assert_eq!(text, "# Plan\n\nDo the **first** thing & more.\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n");
        // A zip that is no document, and a file that is no zip.
        let path = zipped(dir.path(), "b.docx", &[("other.xml", "<x/>")]);
        assert_eq!(read(&path), Preview::None(UNREADABLE.into()));
        assert_eq!(read(&file(dir.path(), "c.docx", b"hello")), Preview::None(UNREADABLE.into()));
    }

    #[test]
    fn a_deck_is_a_section_a_slide() {
        let dir = tempfile::tempdir().unwrap();
        let slide = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
            <p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Welcome</a:t></a:r></a:p></p:txBody></p:sp>
            <p:sp><p:nvSpPr><p:nvPr><p:ph idx="1"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>One</a:t></a:r></a:p><a:p><a:r><a:t>Two</a:t></a:r></a:p></p:txBody></p:sp>
            </p:spTree></p:cSld></p:sld>"#;
        let path = zipped(dir.path(), "a.pptx", &[("ppt/presentation.xml", "<p:presentation xmlns:p=\"p\"/>"), ("ppt/slides/slide1.xml", slide)]);
        let (text, _) = markdown(read(&path));
        assert_eq!(text, "## 1. Welcome\n\n- One\n- Two\n");
    }

    #[test]
    fn a_workbook_is_a_table_a_sheet() {
        let dir = tempfile::tempdir().unwrap();
        let path = zipped(
            dir.path(),
            "a.xlsx",
            &[
                ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#),
                ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
                ("xl/workbook.xml", r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Costs" sheetId="1" r:id="rId1"/></sheets></workbook>"#),
                ("xl/_rels/workbook.xml.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#),
                ("xl/worksheets/sheet1.xml", r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Item</t></is></c><c r="B1" t="inlineStr"><is><t>Cost</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Pen</t></is></c><c r="B2"><v>2.5</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Ink</t></is></c><c r="B3"><v>4</v></c></row></sheetData></worksheet>"#),
            ],
        );
        let (text, cut) = markdown(read(&path));
        assert!(!cut);
        assert_eq!(text, "## Costs\n\n| Item | Cost |\n| --- | --- |\n| Pen | 2.5 |\n| Ink | 4 |\n");
        assert_eq!(read(&file(dir.path(), "b.xlsx", b"hello")), Preview::None(UNREADABLE.into()));
    }

    /// A PDF of one page, 200 by 100 points, black in its lower left
    /// 50 by 50, with its cross-reference table where it says.
    fn one_page_pdf() -> Vec<u8> {
        let content = "0 0 0 rg 0 0 50 50 re f";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ];
        pdf_of(&objects)
    }

    /// A PDF of these objects, the first its catalog, numbered from 1.
    fn pdf_of(objects: &[String]) -> Vec<u8> {
        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
        }
        let xref = out.len();
        out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
        for at in offsets {
            out.push_str(&format!("{at:010} 00000 n \n"));
        }
        out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
        out.into_bytes()
    }

    #[test]
    fn a_pdf_is_counted_and_its_pages_drawn() {
        let dir = tempfile::tempdir().unwrap();
        let path = file(dir.path(), "a.pdf", &one_page_pdf());
        assert_eq!(read(&path), Preview::Pdf { pages: 1 });
        let (w, h, rgba) = pdf_page(&path, 0, 400).unwrap();
        assert_eq!((w, h, rgba.len()), (400, 200, 400 * 200 * 4));
        let at = |x: u32, y: u32| &rgba[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize];
        // The lower left is black, the upper right white, and all opaque.
        assert_eq!(at(10, 190), &[0, 0, 0, 255]);
        assert_eq!(at(390, 10), &[255, 255, 255, 255]);
        assert!(pdf_page(&path, 1, 400).is_err());
        let sizes = pdf_sizes(&path).unwrap();
        assert_eq!(sizes.len(), 1);
        assert!((sizes[0].0 / sizes[0].1 - 2.).abs() < 0.01, "{sizes:?}");
        assert_eq!(read(&file(dir.path(), "b.pdf", b"%PDF-1.4 nothing")), Preview::None(PDF_UNREADABLE.into()));
        assert!(pdf_sizes(&dir.path().join("b.pdf")).is_err());
        // Several pages at once, a missing one as none.
        let drawn = pdf_pages(&path, &[0, 3], 100);
        assert_eq!(drawn.len(), 2);
        assert_eq!(drawn[0].as_ref().map(|d| (d.0, d.1)), Some((100, 50)));
        assert!(drawn[1].is_none());
        assert_eq!(pdf_outline(&path).unwrap(), Vec::new());
    }

    #[test]
    fn a_pdf_outline_is_read_with_where_it_leads() {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R /Names << /Dests << /Names [(two) [4 0 R /Fit]] >> >> >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_string(),
            "<< /Type /Outlines /First 6 0 R /Last 8 0 R /Count 3 >>".to_string(),
            "<< /Title (Intro) /Parent 5 0 R /Next 8 0 R /First 7 0 R /Last 7 0 R /Count -1 /Dest [3 0 R /XYZ 0 80 0] >>".to_string(),
            "<< /Title (Part) /Parent 6 0 R /Dest (two) >>".to_string(),
            // \"Über\" in UTF-16, by a GoTo action; its Next leads back to
            // the first, a loop a broken file can have.
            "<< /Title <FEFF00DC006200650072> /Parent 5 0 R /Next 6 0 R /A << /S /GoTo /D [4 0 R /FitH 30] >> >>".to_string(),
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = file(dir.path(), "o.pdf", &pdf_of(&objects));
        let marks = pdf_outline(&path).unwrap();
        let got: Vec<(&str, usize, Option<usize>, Option<f32>, bool, bool)> = marks.iter().map(|m| (m.title.as_str(), m.depth, m.page, m.down, m.kids, m.open)).collect();
        assert_eq!(got, vec![("Intro", 0, Some(0), Some(20.), true, false), ("Part", 1, Some(1), None, false, false), ("Über", 0, Some(1), Some(70.), false, false)]);
    }

    /// Contents from the type: the title and the sections larger than
    /// the body, a wrapped title joined, a running head left out.
    #[test]
    fn contents_from_type() {
        fn line(glyphs: &mut Vec<PdfGlyph>, text: &str, em: f32, base: f32) {
            let mut x = 50.;
            for (i, c) in text.chars().enumerate() {
                let w = em * 0.5;
                glyphs.push(PdfGlyph { x0: x, x1: x + w, base, em, text: c.to_string(), gap: if i == 0 { 2 } else if c == ' ' { 1 } else { 0 } });
                x += w;
            }
        }
        let page = |head: Option<(&str, f32)>, extra: &[(&str, f32, f32)]| {
            let mut g = Vec::new();
            line(&mut g, "Itinerary", 14., 30.);
            if let Some((t, em)) = head {
                line(&mut g, t, em, 80.);
            }
            for &(t, em, base) in extra {
                line(&mut g, t, em, base);
            }
            for i in 0..12 {
                line(&mut g, "the words of the body of the page run on here", 10., 300. + i as f32 * 14.);
            }
            PdfText { size: (600., 800.), glyphs: g }
        };
        let pages = vec![
            page(Some(("Trip to", 28.)), &[("Southbank", 28., 112.), ("Hotel", 18., 200.)]),
            page(Some(("Flights", 18.)), &[("Seats", 14.5, 240.)]),
            page(None, &[("42", 18., 260.)]),
        ];
        let marks = pdf_contents(&pages);
        let got: Vec<(&str, usize, Option<usize>, bool)> = marks.iter().map(|m| (m.title.as_str(), m.depth, m.page, m.kids)).collect();
        assert_eq!(got, vec![("Itinerary", 0, Some(0), false), ("Trip to Southbank", 0, Some(0), true), ("Hotel", 1, Some(0), false), ("Flights", 1, Some(1), true), ("Seats", 2, Some(1), false)]);
        assert!(pdf_contents(&[PdfText::default()]).is_empty());
    }
}

/// A glyph of a PDF page's text where the page draws it, in points from
/// the page's top left: the line it stands on and how far along it
/// reaches. A page shown is a picture; this is what a find marks on it.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfGlyph {
    pub x0: f32,
    pub x1: f32,
    pub base: f32,
    pub em: f32,
    pub text: String,
    /// What stands between it and the glyph before: nothing (0), a
    /// space (1), or a line break (2).
    pub gap: u8,
}

/// A PDF page's text: its size in points and its glyphs in the order
/// the file draws them.
#[derive(Clone, Debug, Default)]
pub struct PdfText {
    pub size: (f32, f32),
    pub glyphs: Vec<PdfGlyph>,
}

impl PdfText {
    /// Where a run of glyphs stands on the page, a box to a line:
    /// (x, top, width, height) in points.
    pub fn boxes(&self, from: usize, to: usize) -> Vec<(f32, f32, f32, f32)> {
        let mut out: Vec<(f32, f32, f32, f32)> = Vec::new();
        let mut last_line: Option<usize> = None;
        for (ix, g) in self.glyphs.iter().enumerate().take(to + 1).skip(from) {
            let (top, bottom) = (g.base - g.em * 0.85, g.base + g.em * 0.25);
            let new_line = ix > from && g.gap == 2;
            match out.last_mut().filter(|_| !new_line && last_line.is_some()) {
                Some(b) => {
                    let (x0, x1) = (b.0.min(g.x0), (b.0 + b.2).max(g.x1));
                    let (t, bt) = (b.1.min(top), (b.1 + b.3).max(bottom));
                    *b = (x0, t, x1 - x0, bt - t);
                }
                None => out.push((g.x0, top, g.x1 - g.x0, bottom - top)),
            }
            last_line = Some(ix);
        }
        out
    }

    /// The place between two glyphs nearest a point in points, as a
    /// caret stands in text: 0 before the first glyph, `glyphs.len()`
    /// after the last. The line is the one whose height holds the point,
    /// else the nearest; above every line is the start, below them the end.
    pub fn place_at(&self, x: f32, y: f32) -> usize {
        let lines = self.lines();
        let Some(first) = lines.first() else { return 0 };
        let band = |r: &std::ops::Range<usize>| {
            let (mut top, mut bottom) = (f32::MAX, f32::MIN);
            for g in &self.glyphs[r.clone()] {
                top = top.min(g.base - g.em * 0.85);
                bottom = bottom.max(g.base + g.em * 0.25);
            }
            (top, bottom)
        };
        if y < band(first).0 {
            return 0;
        }
        let line = lines.iter().min_by(|a, b| {
            let d = |r: &std::ops::Range<usize>| {
                let (t, bt) = band(r);
                if y < t {
                    t - y
                } else if y > bt {
                    y - bt
                } else {
                    0.
                }
            };
            d(a).total_cmp(&d(b))
        });
        let Some(line) = line else { return 0 };
        self.glyphs[line.clone()].iter().position(|g| x < (g.x0 + g.x1) / 2.).map_or(line.end, |i| line.start + i)
    }

    /// The glyphs line by line, as runs of indices.
    fn lines(&self) -> Vec<std::ops::Range<usize>> {
        let mut out: Vec<std::ops::Range<usize>> = Vec::new();
        for (ix, g) in self.glyphs.iter().enumerate() {
            match out.last_mut() {
                Some(r) if g.gap != 2 => r.end = ix + 1,
                _ => out.push(ix..ix + 1),
            }
        }
        out
    }

    /// The words of the glyphs from `from` up to `to`, a space and a line
    /// break where the page has them.
    pub fn words(&self, from: usize, to: usize) -> String {
        let mut out = String::new();
        for (ix, g) in self.glyphs.iter().enumerate().take(to).skip(from) {
            if ix > from {
                match g.gap {
                    1 => out.push(' '),
                    2 => out.push('\n'),
                    _ => {}
                }
            }
            out.push_str(&g.text);
        }
        out
    }

    /// The word around a glyph: from the space or line break before it
    /// to the one after, as a double click takes it.
    pub fn word_around(&self, at: usize) -> (usize, usize) {
        let n = self.glyphs.len();
        if n == 0 {
            return (0, 0);
        }
        let at = at.min(n - 1);
        let blank = |g: &PdfGlyph| g.text.chars().all(char::is_whitespace);
        let mut from = at;
        while from > 0 && self.glyphs[from].gap == 0 && !blank(&self.glyphs[from - 1]) {
            from -= 1;
        }
        let mut to = at + 1;
        while to < n && self.glyphs[to].gap == 0 && !blank(&self.glyphs[to]) {
            to += 1;
        }
        (from, to)
    }
}

/// Every page's text, by the same reading of the page that draws it.
/// A page that cannot be read has none; text set in a font that says
/// nothing of its characters is not found.
pub fn pdf_text(path: &Path) -> Result<Vec<PdfText>, String> {
    std::panic::catch_unwind(|| read_text(path)).unwrap_or(Err(PDF_UNREADABLE)).map_err(String::from)
}

fn read_text(path: &Path) -> Result<Vec<PdfText>, &'static str> {
    use hayro::hayro_interpret::{interpret_page, Context, InterpreterCache, InterpreterSettings, TransformExt as _};
    use hayro::vello_cpu::kurbo::Rect;
    let pdf = open_pdf(path)?;
    let cache = InterpreterCache::new();
    let mut out = Vec::new();
    for page in pdf.pages().iter() {
        let (w, h) = page.render_dimensions();
        let mut device = TextDevice { out: Vec::new() };
        let mut context = Context::new(page.initial_transform(true).to_kurbo(), Rect::new(0., 0., w as f64, h as f64), &cache, page.xref(), InterpreterSettings::default());
        interpret_page(page, &mut context, &mut device);
        let mut glyphs: Vec<PdfGlyph> = Vec::with_capacity(device.out.len());
        for mut g in device.out {
            // A new line where the baseline moves or the text goes back;
            // a space where there is room for one.
            g.gap = match glyphs.last() {
                None => 2,
                Some(was) if (g.base - was.base).abs() > was.em.max(g.em) * 0.5 || g.x0 < was.x0 - was.em => 2,
                Some(was) if g.x0 - was.x1 > g.em * 0.18 => 1,
                Some(_) => 0,
            };
            glyphs.push(g);
        }
        out.push(PdfText { size: (w, h), glyphs });
    }
    Ok(out)
}

/// Collects a page's glyphs as the interpreter draws them.
struct TextDevice {
    out: Vec<PdfGlyph>,
}

impl<'a> hayro::hayro_interpret::Device<'a> for TextDevice {
    fn set_soft_mask(&mut self, _: Option<hayro::hayro_interpret::SoftMask<'a>>) {}
    fn set_blend_mode(&mut self, _: hayro::hayro_interpret::BlendMode) {}
    fn draw_path(&mut self, _: &hayro::vello_cpu::kurbo::BezPath, _: hayro::vello_cpu::kurbo::Affine, _: &hayro::hayro_interpret::Paint<'a>, _: &hayro::hayro_interpret::PathDrawMode) {}
    fn push_clip_path(&mut self, _: &hayro::hayro_interpret::ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<hayro::hayro_interpret::SoftMask<'a>>, _: hayro::hayro_interpret::BlendMode) {}
    fn draw_glyph(&mut self, glyph: &hayro::hayro_interpret::font::Glyph<'a>, transform: hayro::vello_cpu::kurbo::Affine, glyph_transform: hayro::vello_cpu::kurbo::Affine, _: &hayro::hayro_interpret::Paint<'a>, _: &hayro::hayro_interpret::GlyphDrawMode) {
        use hayro::hayro_interpret::font::Glyph;
        use hayro::hayro_interpret::hayro_cmap::BfString;
        use hayro::vello_cpu::kurbo::Point;
        let Some(unicode) = glyph.as_unicode() else { return };
        let text = match unicode {
            BfString::Char(c) => c.to_string(),
            BfString::String(s) => s,
        };
        // A glyph's own room is a thousand to the em.
        let place = transform * glyph_transform;
        let advance = match glyph {
            Glyph::Outline(o) => o.advance_width().filter(|a| *a > 0.).unwrap_or(500.) as f64,
            _ => 500.,
        };
        let (from, to, up) = (place * Point::new(0., 0.), place * Point::new(advance, 0.), place * Point::new(0., 1000.));
        let em = (up.y - from.y).abs().max((up.x - from.x).abs()) as f32;
        if em < 0.5 || !from.x.is_finite() || !from.y.is_finite() {
            return;
        }
        let (x0, x1) = (from.x.min(to.x) as f32, from.x.max(to.x) as f32);
        self.out.push(PdfGlyph { x0, x1: x1.max(x0 + em * 0.2), base: from.y as f32, em, text, gap: 0 });
    }
    fn draw_image(&mut self, _: hayro::hayro_interpret::Image<'a, '_>, _: hayro::vello_cpu::kurbo::Affine) {}
    fn pop_clip_path(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// Every place the words are on the pages, whatever the case: the page,
/// and its glyphs from and to. A match does not run from one page onto
/// the next.
pub fn pdf_find(pages: &[PdfText], words: &str) -> Vec<(usize, usize, usize)> {
    let needle = words.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (page, t) in pages.iter().enumerate() {
        let mut text = String::new();
        // Where each glyph begins in the text searched.
        let mut starts = Vec::with_capacity(t.glyphs.len());
        for (ix, g) in t.glyphs.iter().enumerate() {
            if ix > 0 && g.gap != 0 {
                text.push(' ');
            }
            starts.push(text.len());
            text.push_str(&g.text.to_lowercase());
        }
        let glyph_of = |byte: usize| starts.partition_point(|s| *s <= byte).saturating_sub(1);
        out.extend(text.match_indices(&needle).map(|(at, hit)| (page, glyph_of(at), glyph_of(at + hit.len() - 1))));
    }
    out
}
