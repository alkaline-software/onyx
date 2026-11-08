//! A link shown one way or another, as Craft turns a link block into a
//! rich link and back: the link among words, a card of a size (`card.rs`),
//! or for a file a frame shows (`embed.rs`) the whole file. Each is one
//! line as written, so a change is the line written again, one step of
//! undo: `[words](href)`, `<a href data-card>words</a>`, `![words](href)`.
//! A link among other words comes out onto a line of its own as it turns
//! into a card, the words before and after it kept on theirs.
//! `docs/editor.md`, Cards.

use std::ops::Range;

use gpui::*;
use onyx_core::html::{self, Across, CardSize};

use super::{Editor, Mode};
use crate::lines::{Pos, Sel};
use crate::markdown::{LineKind, SpanKind};

/// How a link is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Link,
    Card(CardSize),
    /// The whole file, in a frame.
    Preview,
}

/// A link a menu can show another way: its line, a byte of it in the
/// line, how it shows now, and whether it leads to a file a frame shows.
#[derive(Clone, Copy, Debug)]
pub struct LayoutAt {
    pub line: usize,
    pub at: usize,
    pub now: Layout,
    pub preview: bool,
    /// A large card's place across the page, which its menu changes.
    pub across: Option<Across>,
}

/// The link a line holds at a byte: where it is in the line, where it
/// leads, its words, how it shows, and whether it is all the line holds.
struct Found {
    range: Range<usize>,
    href: String,
    words: String,
    now: Layout,
    alone: bool,
    across: Across,
}

impl Editor {
    fn link_on(&self, line: usize, at: usize) -> Option<Found> {
        if line >= self.doc.line_count() {
            return None;
        }
        let text = self.doc.line(line);
        let whole = {
            let lead = text.len() - text.trim_start().len();
            lead..lead + text.trim().len()
        };
        if let Some(c) = html::card_line(text) {
            return Some(Found { range: whole, href: c.href, words: html::entities(&text[c.words]), now: Layout::Card(c.size), alone: true, across: c.across.unwrap_or(Across::Start) });
        }
        if let Some(on) = self.reader().picture_on(line) {
            let path = self.pictures.borrow_mut().find_path(&on.src)?;
            if !crate::layout::is_framed(&path) {
                return None;
            }
            let words = text.get(on.words.clone()).unwrap_or("");
            let words = if on.html { html::entities(words) } else { words.to_string() };
            return Some(Found { range: whole, href: on.src, words, now: Layout::Preview, alone: true, across: Across::Start });
        }
        if self.analysis.lines[line].kind != LineKind::Text {
            return None;
        }
        let start = self.analysis.line_starts[line];
        let span = self.analysis.line_spans[line].iter().map(|&ix| &self.analysis.spans[ix]).find(|s| matches!(s.kind, SpanKind::Link(_) | SpanKind::WikiLink(_)) && s.range.start <= start + at && start + at <= s.range.end)?;
        let range = span.range.start - start..span.range.end - start;
        let inner = text.get(span.inner().start - start..span.inner().end - start).unwrap_or("");
        let (href, words) = match &span.kind {
            SpanKind::Link(url) => (url.clone(), if text[range.clone()].starts_with('<') { html::entities(inner) } else { inner.to_string() }),
            SpanKind::WikiLink(name) => {
                let shown = inner.split_once('|').map_or(inner, |(_, alias)| alias);
                (name.split(['#', '|']).next().unwrap_or(name).trim().to_string(), shown.trim().to_string())
            }
            _ => return None,
        };
        Some(Found { alone: range == whole, range, href, words, now: Layout::Link, across: Across::Start })
    }

    /// What a menu at a byte of a line can change of the link there.
    pub(crate) fn layout_at(&self, line: usize, at: usize) -> Option<LayoutAt> {
        if self.locked() || self.mode == Mode::Source {
            return None;
        }
        let found = self.link_on(line, at)?;
        let web = onyx_core::link_card::is_web(&found.href);
        let file = (!web).then(|| self.pictures.borrow_mut().find_any(&found.href)).flatten();
        if !web && file.is_none() {
            return None;
        }
        let preview = file.as_deref().is_some_and(crate::layout::is_framed);
        let across = (found.now == Layout::Card(CardSize::Large)).then_some(found.across);
        Some(LayoutAt { line, at, now: found.now, preview, across })
    }

    /// A menu's or a probe's choice by its word (`link`, `small`, `medium`,
    /// `large`, `preview`; a large card's `left`, `center`, `right`) for the
    /// link at a byte of a line.
    pub fn layout_choice(&mut self, line: usize, at: usize, what: &str, cx: &mut Context<Self>) {
        let across = match what {
            "left" => Some(Across::Start),
            "center" => Some(Across::Center),
            "right" => Some(Across::End),
            _ => None,
        };
        if let Some(across) = across {
            if let Some(found) = self.layout_at(line, at).filter(|f| f.across.is_some_and(|a| a != across)) {
                self.write_link(found, Layout::Card(CardSize::Large), Some(across), cx);
            }
            return;
        }
        let to = match what {
            "link" => Layout::Link,
            "preview" => Layout::Preview,
            w => match CardSize::read(w) {
                Some(size) => Layout::Card(size),
                None => return,
            },
        };
        if let Some(found) = self.layout_at(line, at) {
            self.set_layout(found, to, cx);
        }
    }

    /// The link shown as `to`, its line written again; it is selected
    /// whole after, as a piece is.
    pub fn set_layout(&mut self, at: LayoutAt, to: Layout, cx: &mut Context<Self>) {
        if to != at.now {
            self.write_link(at, to, None, cx);
        }
    }

    /// The link written as `to`, a large card placed at `across`, or
    /// where it stood when none is given.
    fn write_link(&mut self, at: LayoutAt, to: Layout, across: Option<Across>, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let Some(found) = self.link_on(at.line, at.at) else { return };
        // A file named by its name alone, with no folder, is a wikilink
        // again, as `![[guide.pdf]]` and `[[Other]]` name one.
        let wiki = !onyx_core::link_card::is_web(&found.href) && !found.href.contains(['/', '\\', ' ']) && !found.href.contains("%20");
        let plain = found.words.is_empty() || found.words == found.href || std::path::Path::new(&found.href).file_stem().is_some_and(|s| s.to_string_lossy() == found.words);
        let written = match to {
            Layout::Link if wiki => if plain { format!("[[{}]]", found.href) } else { format!("[[{}|{}]]", found.href, found.words) },
            Layout::Link => html::link_markup(&found.href, &found.words),
            // Only a large card is narrower than the words, so only it is placed.
            Layout::Card(size) => html::card_markup(&found.href, &found.words, size, if size == CardSize::Large { across.unwrap_or(found.across) } else { Across::Start }),
            Layout::Preview if wiki => format!("![[{}]]", found.href),
            Layout::Preview => html::picture_markup(&found.href, &found.words, None, Across::Start, false),
        };
        let text = self.doc.line(at.line).to_string();
        let (line_text, own) = if found.alone {
            (format!("{}{}{}", &text[..found.range.start], written, &text[found.range.end..]), at.line)
        } else {
            // Out of its words onto a line of its own, between theirs, in
            // the same paragraph: another app reads the words and the link
            // on as they were.
            let before = text[..found.range.start].trim_end();
            let after = text[found.range.end..].trim_start();
            let mut parts = Vec::new();
            if !before.is_empty() {
                parts.push(before.to_string());
            }
            let own = at.line + parts.len();
            parts.push(written);
            if !after.is_empty() {
                parts.push(after.to_string());
            }
            (parts.join("\n"), own)
        };
        self.doc.end_step();
        let (from, to_pos) = (Pos::new(at.line, 0), Pos::new(at.line, text.len()));
        self.doc.replace(from, to_pos, &line_text, self.sel);
        self.editing = None;
        self.sel = Sel::caret(Pos::new(own, 0));
        self.after_edit(cx);
        self.doc.end_step();
        // Selected whole, not opened as words, as the caret an edit leaves
        // on a piece's line would have it.
        self.editing = None;
        if self.object_line(own) {
            self.sel = self.object_sel(own);
        }
        cx.notify();
    }
}
