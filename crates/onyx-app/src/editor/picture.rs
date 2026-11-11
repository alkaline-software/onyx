//! A picture on the canvas: one piece of the note, its size and place
//! changed on it, and the viewer it opens in.
//!
//! A bar comes over a picture's top while the pointer is on it, as over
//! a file in a frame (`embed.rs` draws both): its name, which takes a
//! new one, where it stands across the page, its size against its own,
//! View, the pencil and a menu. The bar is a view of its own laid over
//! the row's top; the picture under it is painted here.
//!
//! A picture's line, and a frame's, is one piece to the
//! caret, as a block is in Craft: a press on the picture selects it whole
//! and ringed in the accent, and so do ↑ ↓ ← → arriving on its line and
//! ⌫ from the start of the line under it; ⌫ or ⌦ then takes it out, ⌘C
//! copies it, ↩ opens a line under it. The pencil on the bar puts the
//! caret in the line itself, which then shows under it as written.
//!
//! Its whole edge takes a drag, as a window's does, and so does a PDF's
//! frame's, which is sized as a picture is: a side, the top or the foot,
//! or a corner, the pointer showing which, and the edge drawn in the
//! accent while it is held. It keeps its shape, follows the drag, and the
//! line is written once, at the release, as one step of undo; dragged to
//! the words' width it is Full. A double click on the edge brings it back
//! to 100%. A double click on a picture opens the viewer, and so does a
//! click while the note reads.
//!
//! The line is written as `html::picture_markup` writes it: markdown while
//! the picture is left and at its own size, HTML any app reads once it is
//! placed or sized (`width="100%"` for Full), and markdown again when both
//! are undone.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gpui::*;

use super::{Editor, EditorEvent, Mode};
use crate::layout::Row;
use crate::lines::{Pos, Sel};
use onyx_core::html::{self, Across, Width};

/// How far in and out of a picture's edge a press takes the edge, and how
/// far along it from a corner takes the corner.
const EDGE_IN: f32 = 6.;
const EDGE_OUT: f32 = 6.;
const CORNER: f32 = 16.;
/// The narrowest a drag leaves a picture.
const LEAST_WIDTH: f32 = 48.;
/// The ring around a piece selected, out from its edge.
const RING_OUT: f32 = 3.;
const RING_W: f32 = 2.;

/// A part of a picture's edge: which way each of its sides moves the
/// edge out, -1, 0 or 1 across and down. A corner moves both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Edge {
    pub x: i8,
    pub y: i8,
}

impl Edge {
    /// The pointer a window shows on the same part of its edge.
    pub fn cursor(self) -> CursorStyle {
        match (self.x, self.y) {
            (0, _) => CursorStyle::ResizeUpDown,
            (_, 0) => CursorStyle::ResizeLeftRight,
            (x, y) if x == y => CursorStyle::ResizeUpLeftDownRight,
            _ => CursorStyle::ResizeUpRightDownLeft,
        }
    }
}

/// The picture the pointer is over: its line, and since when.
#[derive(Clone, Copy)]
pub(super) struct PicHover {
    pub line: usize,
    pub since: Instant,
}

/// A drag of a picture's edge.
pub(super) struct PicDrag {
    line: usize,
    edge: Edge,
    x0: f32,
    y0: f32,
    w0: f32,
    /// The picture's height, under its bar: the top and the foot move it.
    h0: f32,
    across: Across,
    room: f32,
    /// The narrowest it goes: a PDF's frame keeps room for its bar.
    least: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub(super) fn holds(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    pub(super) fn grown(&self, by: f32) -> Rect {
        Rect { x: self.x - by, y: self.y - by, w: self.w + by * 2., h: self.h + by * 2. }
    }

    /// The part of the edge a point is on, if it is on the edge.
    fn edge_at(&self, x: f32, y: f32) -> Option<Edge> {
        if !self.grown(EDGE_OUT).holds(x, y) {
            return None;
        }
        // A corner's reach runs along both its sides.
        let reach = CORNER.min(self.w / 3.).min(self.h / 3.);
        let side = |at: f32, from: f32, to: f32, near: f32| -> i8 {
            if at <= from + near {
                -1
            } else if at >= to - near {
                1
            } else {
                0
            }
        };
        let (ex, ey) = (side(x, self.x, self.x + self.w, EDGE_IN), side(y, self.y, self.y + self.h, EDGE_IN));
        let edge = match (ex, ey) {
            (0, 0) => return None,
            (0, ey) => Edge { x: side(x, self.x, self.x + self.w, reach), y: ey },
            (ex, 0) => Edge { x: ex, y: side(y, self.y, self.y + self.h, reach) },
            (ex, ey) => Edge { x: ex, y: ey },
        };
        Some(edge)
    }
}

/// What paint draws of the edge of the picture the pointer is over.
pub(super) struct PicPaint {
    /// The control: the bar and the picture under it.
    pub control: Rect,
    /// The edge is held, or the pointer is on it.
    pub edge: bool,
    pub opacity: f32,
}

/// What a picture's bar shows of it (`embed.rs`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PicFacts {
    pub across: Across,
    pub asked: Option<Width>,
    /// As wide as the words: placed across the page it would not move.
    pub full: bool,
    /// Its width against its own, in whole percent.
    pub share: u32,
}

/// A picture of the note, for the viewer: its file, its size in pixels,
/// and its pixels once the canvas has read them.
pub struct Seen {
    pub path: PathBuf,
    pub size: (u32, u32),
    pub drawn: Option<Arc<RenderImage>>,
}

/// The note's pictures in the viewer: which one opens, and where it
/// stands on the canvas, in the window, so it grows from there.
pub struct Viewing {
    pub pictures: Vec<Seen>,
    pub at: usize,
    pub from: Bounds<Pixels>,
}

impl Editor {
    // -- one piece ----------------------------------------------------------

    /// Whether `line` is drawn as a picture or a file's frame and is one
    /// piece to the caret, unless the pencil opened it.
    pub(super) fn object_line(&self, line: usize) -> bool {
        if self.editing == Some(line) || line >= self.doc.line_count() {
            return false;
        }
        let Some(on) = self.reader().picture_on(line) else { return false };
        let mut store = self.pictures.borrow_mut();
        if on.card.is_some() {
            return onyx_core::link_card::is_web(&on.src) || store.find_any(&on.src).is_some();
        }
        let Some(path) = store.find_path(&on.src) else { return false };
        crate::layout::is_framed(&path) || store.size_of(&path).is_some()
    }

    /// The whole of `line`, as a selection.
    pub(super) fn object_sel(&self, line: usize) -> Sel {
        Sel { anchor: Pos::new(line, 0), head: Pos::new(line, self.doc.line(line).len()) }
    }

    /// The piece selected, when the selection is one and nothing else.
    pub(crate) fn object_selected(&self) -> Option<usize> {
        let (a, b) = self.sel.range();
        (a.line == b.line && a.col == 0 && b.col == self.doc.line(a.line).len() && b.col > 0 && self.object_line(a.line)).then_some(a.line)
    }

    /// The piece on `line` selected, the keyboard in the note.
    pub(crate) fn select_object(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        if line >= self.doc.line_count() {
            return;
        }
        self.editing = None;
        self.sel = self.object_sel(line);
        self.preferred_x = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The piece on `line` selected, the keyboard left where it is: a
    /// press in a frame, which keeps it for the frame's words.
    pub(crate) fn mark_object(&mut self, line: usize, cx: &mut Context<Self>) {
        if line >= self.doc.line_count() || self.object_selected() == Some(line) {
            return;
        }
        self.editing = None;
        self.sel = self.object_sel(line);
        self.preferred_x = None;
        cx.notify();
    }

    /// A caret come to rest on a piece's line selects the piece; one gone
    /// from the line the pencil opened closes it.
    pub(super) fn hold_objects(&mut self) {
        if self.editing.is_some_and(|l| l != self.sel.head.line || l >= self.doc.line_count()) {
            self.editing = None;
        }
        if self.sel.is_caret() && self.object_line(self.sel.head.line) && !self.doc.line(self.sel.head.line).is_empty() {
            self.sel = self.object_sel(self.sel.head.line);
        }
    }

    /// A piece a paste or a file brought in, selected whole as a press
    /// on it would, not opened as words: the caret the paste leaves at the
    /// end of its line would show the line as written under it. Only
    /// typing and the pencil open a piece's line.
    pub(super) fn hold_brought(&mut self) {
        let Some(line) = self.editing.filter(|&l| self.sel.is_caret() && l == self.sel.head.line) else { return };
        self.editing = None;
        if self.object_line(line) {
            self.sel = self.object_sel(line);
        }
    }

    /// Where a selection from `anchor` may end on a piece's line: the
    /// whole of it, so a selection takes a picture whole or not at all.
    pub(super) fn held_by_object(&self, anchor: Pos, p: Pos) -> Pos {
        if !self.object_line(p.line) || anchor.line == p.line {
            return p;
        }
        if p >= anchor { Pos::new(p.line, self.doc.line(p.line).len()) } else { Pos::new(p.line, 0) }
    }

    /// ← or → with a piece selected: the end of the line before it or
    /// the start of the one after.
    pub(super) fn leave_object(&self, back: bool) -> Option<Pos> {
        let line = self.object_selected()?;
        Some(if back {
            match line.checked_sub(1) {
                Some(l) => Pos::new(l, self.doc.line(l).len()),
                None => Pos::new(line, 0),
            }
        } else if line + 1 < self.doc.line_count() {
            Pos::new(line + 1, 0)
        } else {
            Pos::new(line, self.doc.line(line).len())
        })
    }

    /// ↩ with a piece selected: a line under it, the caret on it.
    pub(super) fn object_enter(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(line) = self.object_selected() else { return false };
        let end = Pos::new(line, self.doc.line(line).len());
        self.doc.replace(end, end, "\n", self.sel);
        self.sel = Sel::caret(Pos::new(line + 1, 0));
        self.after_edit(cx);
        self.doc.end_step();
        true
    }

    /// ⌫ with a piece selected takes it out; at the start of the line
    /// under one it selects it, taking that line with it when it is
    /// empty, as Craft's blocks do. True when it was one of these.
    pub(super) fn object_back(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(line) = self.object_selected() {
            self.remove_object(line, cx);
            return true;
        }
        let at = self.sel.head;
        if !self.sel.is_caret() || at.col != 0 {
            return false;
        }
        let Some(above) = at.line.checked_sub(1).filter(|l| self.object_line(*l)) else { return false };
        if self.doc.line(at.line).is_empty() {
            let end = Pos::new(above, self.doc.line(above).len());
            self.doc.replace(end, at, "", self.sel);
            self.sel = self.object_sel(above);
            self.after_edit(cx);
        } else {
            self.sel = self.object_sel(above);
            self.after_move(cx);
        }
        true
    }

    /// ⌦, as `object_back`, from the end of the line over a piece.
    pub(super) fn object_forward(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(line) = self.object_selected() {
            self.remove_object(line, cx);
            return true;
        }
        let at = self.sel.head;
        let len = self.doc.line(at.line).len();
        if !self.sel.is_caret() || at.col != len || at.line + 1 >= self.doc.line_count() || !self.object_line(at.line + 1) {
            return false;
        }
        if len == 0 {
            self.doc.replace(at, Pos::new(at.line + 1, 0), "", self.sel);
            self.sel = self.object_sel(at.line);
            self.after_edit(cx);
        } else {
            self.sel = self.object_sel(at.line + 1);
            self.after_move(cx);
        }
        true
    }

    /// The piece on `line` taken out of the note, its line with it, as
    /// one step of undo; a blank line on each side of it leaves one. The
    /// caret goes to the end of the line before.
    pub fn remove_object(&mut self, line: usize, cx: &mut Context<Self>) {
        if self.locked() || line >= self.doc.line_count() {
            return;
        }
        let n = self.doc.line_count();
        let blank = |l: usize| self.doc.line(l).trim().is_empty();
        let last = if line > 0 && line + 1 < n && blank(line - 1) && blank(line + 1) { line + 1 } else { line };
        let (from, to) = if last + 1 < n {
            (Pos::new(line, 0), Pos::new(last + 1, 0))
        } else if line > 0 {
            (Pos::new(line - 1, self.doc.line(line - 1).len()), Pos::new(last, self.doc.line(last).len()))
        } else {
            (Pos::new(0, 0), Pos::new(last, self.doc.line(last).len()))
        };
        self.doc.end_step();
        self.doc.replace(from, to, "", self.sel);
        let caret = match line.checked_sub(1) {
            Some(l) => Pos::new(l, self.doc.line(l).len()),
            None => Pos::new(0, 0),
        };
        self.editing = None;
        self.sel = Sel::caret(self.doc.clamp(caret));
        self.after_edit(cx);
        self.doc.end_step();
    }

    /// The picture's file drawn on `line`: not a frame's.
    pub(super) fn picture_at_line(&self, line: usize) -> Option<PathBuf> {
        let cache = self.cache.borrow();
        match &*cache.entries.get(line)?.row {
            Row::Picture(p) if !p.embed => Some(p.path.clone()),
            _ => None,
        }
    }

    /// The menu of the piece on `line`, a picture, a frame or a card, as
    /// its bar's ⋯ and a right click on it give it: how a file's link
    /// shows, a picture's place and size and the viewer, the file's own
    /// choices, and Delete last.
    pub(crate) fn piece_menu(&self, line: usize, file: Option<PathBuf>) -> super::MenuAt {
        let changes = !self.locked();
        let picture = changes.then(|| self.picture_facts(line).map(|f| (line, f.across, f.asked))).flatten();
        let layout = self.layout_at(line, 0);
        // A card of a page of the web: the link's own choices.
        let link = match self.cache.borrow().entries.get(line).map(|e| &*e.row) {
            Some(Row::Picture(p)) if p.web => {
                let words = self.reader().picture_on(line).map(|on| html::entities(self.doc.line(line).get(on.words).unwrap_or(""))).unwrap_or_default();
                Some(super::MenuLink { url: Some(p.href.clone()), note: None, words, raw: self.doc.line(line).trim().to_string() })
            }
            _ => None,
        };
        super::MenuAt { file, link, picture, layout, remove: changes.then_some(line), ..Default::default() }
    }

    /// For the bar's pencil: the caret in the piece's line, which then
    /// shows under it as written.
    pub(crate) fn edit_object(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        if line >= self.doc.line_count() {
            return;
        }
        self.editing = Some(line);
        self.sel = Sel::caret(Pos::new(line, self.doc.line(line).len()));
        self.want_scroll = true;
        window.focus(&self.focus, cx);
        self.kick_blink(cx);
        cx.notify();
    }

    /// A press beside a picture, not on it nor on its caption: the line
    /// nearest it that is not a picture's, the one before it for the
    /// picture's upper half, the one after for its lower. None when the
    /// press is not beside a picture.
    pub(super) fn beside_picture(&self, x: f32, y: f32) -> Option<Option<Pos>> {
        let (line, top, h, caption) = {
            let cache = self.cache.borrow();
            if cache.entries.is_empty() {
                return None;
            }
            let line = self.line_at(y, &cache);
            let Row::Picture(p) = &*cache.entries.get(line)?.row else { return None };
            (line, cache.tops[line] + p.pad, p.h, p.source.is_some())
        };
        // Its caption takes a press as words do; the room over and under
        // it, as beside it.
        if (caption && y > top + h) || self.on_picture(line, x, y) {
            return None;
        }
        let picture = |l: usize| self.cache.borrow().entries.get(l).is_some_and(|e| matches!(&*e.row, Row::Picture(_)));
        let before = (0..line).rev().find(|&l| !picture(l)).map(|l| Pos::new(l, self.doc.line(l).len()));
        let after = (line + 1..self.doc.line_count()).find(|&l| !picture(l)).map(|l| Pos::new(l, 0));
        Some(if y < top + h / 2. { before.or(after) } else { after.or(before) })
    }

    /// Where the control on `line` stands, from the element's top left: a
    /// picture, or a frame, which is one piece too.
    pub(super) fn control_rect(&self, line: usize) -> Option<Rect> {
        let cache = self.cache.borrow();
        let Row::Picture(p) = &*cache.entries.get(line)?.row else { return None };
        Some(Rect { x: p.x, y: cache.tops[line] + p.pad, w: p.w, h: p.h })
    }

    /// Whether the piece on `line` is sized as a picture is: a picture, or
    /// a PDF's frame. A document's frame runs the words' width.
    fn sized(&self, line: usize) -> bool {
        let cache = self.cache.borrow();
        matches!(cache.entries.get(line).map(|e| &*e.row), Some(Row::Picture(p)) if p.card.is_none() && (!p.embed || onyx_core::vault::is_pdf(&p.path)))
    }

    /// The piece on `line`, when it takes a drag of its edge.
    fn sized_picture(&self, line: usize) -> Option<Rect> {
        if self.sized(line) { self.control_rect(line) } else { None }
    }

    /// What a picture's bar shows: where it stands, its width asked, and
    /// its size against its own, as the drag has it.
    pub(crate) fn picture_facts(&self, line: usize) -> Option<PicFacts> {
        if !self.sized(line) {
            return None;
        }
        let on = self.reader().picture_on(line)?;
        let cache = self.cache.borrow();
        let Row::Picture(p) = &*cache.entries.get(line)?.row else { return None };
        let share = (p.w / p.natural.0.max(1.) * 100.).round() as u32;
        Some(PicFacts { across: p.across, asked: on.asked, full: p.w >= p.room - 0.5, share })
    }

    /// Whether the pointer is on the piece on `line` or near its edge:
    /// its bar shows.
    pub(crate) fn pointer_on(&self, line: usize) -> bool {
        self.pic_hover.is_some_and(|h| h.line == line)
    }

    /// Whether a picture's edge takes a drag: on a note that can change.
    pub(super) fn pictures_change(&self) -> bool {
        !self.locked() && self.mode != Mode::Reading
    }

    /// The picture or frame under the pointer, kept for its bar and its edge.
    pub(super) fn point_at_picture(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        if self.pic_drag.is_some() {
            return;
        }
        let found = self.picture_line_at(x, y);
        if self.pic_hover.map(|h| h.line) != found {
            self.pic_hover = found.map(|line| PicHover { line, since: Instant::now() });
            cx.notify();
        } else if found.is_some() {
            // The edge lights as the pointer comes onto it.
            cx.notify();
        }
    }

    /// The line of the control drawn under a point, its edge's reach
    /// included. The reach stands out past it, over the lines beside it,
    /// so those are asked too.
    fn picture_line_at(&self, x: f32, y: f32) -> Option<usize> {
        let line = {
            let cache = self.cache.borrow();
            if cache.entries.is_empty() {
                return None;
            }
            self.line_at(y, &cache)
        };
        [Some(line), line.checked_add(1), line.checked_sub(1)].into_iter().flatten().find(|&l| self.control_rect(l).is_some_and(|c| c.grown(EDGE_OUT).holds(x, y)))
    }

    /// Whether a point stands on the control on `line` itself, its edge
    /// included: not beside it.
    pub(super) fn on_picture(&self, line: usize, x: f32, y: f32) -> bool {
        self.picture_line_at(x, y) == Some(line)
    }

    /// The part of the edge under a point, on the picture the pointer is over.
    pub(super) fn picture_edge_at(&self, x: f32, y: f32) -> Option<Edge> {
        if !self.pictures_change() {
            return None;
        }
        let line = self.pic_hover?.line;
        self.sized_picture(line)?.edge_at(x, y)
    }

    /// A press on a picture: on its edge, a drag of it, or back to 100% at
    /// a double click; on the picture itself, the picture selected whole.
    /// False when it was on neither. `origin` is the element's top left in
    /// the window.
    pub(super) fn press_picture(&mut self, x: f32, y: f32, clicks: usize, shift: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.pictures_change() {
            return false;
        }
        if let Some(edge) = self.picture_edge_at(x, y) {
            let Some(line) = self.pic_hover.map(|h| h.line) else { return false };
            let Some(picture) = self.sized_picture(line) else { return false };
            if clicks >= 2 {
                // A double click on the edge: back to its own size.
                self.pic_drag = None;
                self.pic_hold = None;
                if let Some(width) = self.hundred(line) {
                    self.set_picture(line, None, Some(width), cx);
                }
                cx.notify();
                return true;
            }
            let (room, across, embed) = match self.cache.borrow().entries.get(line).map(|e| e.row.clone()) {
                Some(row) => match &*row {
                    Row::Picture(p) => (p.room, p.across, p.embed),
                    _ => return false,
                },
                None => return false,
            };
            let least = if embed { crate::embed::CONTROL_LEAST } else { LEAST_WIDTH };
            self.pic_drag = Some(PicDrag { line, edge, x0: x, y0: y, w0: picture.w, h0: picture.h.max(1.), across, room, least });
            self.pic_hold = Some((line, picture.w));
            cx.notify();
            return true;
        }
        // On the picture: the whole of it, unless ⇧ carries a selection
        // there or a second click opens the viewer.
        if clicks == 1 && !shift {
            if let Some((_, line)) = self.picture_at(x, y).filter(|(_, l)| self.object_line(*l)) {
                self.select_object(line, window, cx);
                return true;
            }
        }
        false
    }

    /// A drag of a picture's edge under way. True when there was one.
    pub(super) fn drag_picture(&mut self, x: f32, y: f32, cx: &mut Context<Self>) -> bool {
        let Some(d) = &self.pic_drag else { return false };
        // A centered picture grows on both sides, so each side moves as far
        // as the pointer; a picture at a side grows away from it. The top
        // and the foot grow it by its shape; a corner goes by whichever
        // of its two sides was carried further.
        let k = if d.across == Across::Center { 2. } else { 1. };
        let across = f32::from(d.edge.x) * (x - d.x0) * k;
        let down = f32::from(d.edge.y) * (y - d.y0) * d.w0 / d.h0;
        let dw = match (d.edge.x, d.edge.y) {
            (0, _) => down,
            (_, 0) => across,
            _ if across.abs() >= down.abs() => across,
            _ => down,
        };
        let w = (d.w0 + dw).clamp(d.least.min(d.w0), d.room).round();
        let line = d.line;
        if self.pic_hold != Some((line, w)) {
            self.pic_hold = Some((line, w));
            cx.notify();
        }
        true
    }

    /// The end of a drag: the width written, once. Dragged to the words'
    /// width, the picture is Full.
    pub(super) fn drop_picture(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(d) = self.pic_drag.take() else { return false };
        let held = self.pic_hold.take();
        if let Some((line, w)) = held.filter(|(_, w)| (*w - d.w0).abs() >= 1.) {
            let width = if w >= d.room - 0.5 {
                Some(Some(Width::Share(100)))
            } else {
                Some(Some(Width::Px(w as u32)))
            };
            if let Some(width) = width {
                self.set_picture(line, None, Some(width), cx);
            }
        }
        cx.notify();
        true
    }

    /// The width that shows the picture on `line` at 100%: none asked
    /// when its own size stands at that already, so a markdown line stays
    /// markdown, else its own width in pixels. As far as the room goes
    /// when it is wider than the room.
    fn hundred(&self, line: usize) -> Option<Option<Width>> {
        let cache = self.cache.borrow();
        let Row::Picture(p) = &*cache.entries.get(line)?.row else { return None };
        // A PDF's own size is its own width, as far as the words go.
        let own = if p.embed { p.natural.0.min(p.room) } else { crate::layout::own_width(p.natural, p.room) };
        Some(if own >= p.natural.0 - 0.5 || p.natural.0 > p.room { None } else { Some(Width::Px(p.natural.0.round() as u32)) })
    }

    /// The picture on `line` placed across the page, sized, or both, the
    /// line written again as `html::picture_markup` writes it: one step
    /// of undo. `width` is `Some(None)` for its own size.
    pub fn set_picture(&mut self, line: usize, across: Option<Across>, width: Option<Option<Width>>, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let Some(on) = self.reader().picture_on(line) else { return };
        let text = self.doc.line(line).to_string();
        let path = self.pictures.borrow_mut().find_path(&on.src);
        let embed = path.as_deref().is_some_and(onyx_core::vault::is_pdf);
        let raw = text.get(on.words.clone()).unwrap_or("");
        // Its words as words: an HTML attribute's entities read, markdown's
        // escapes and Obsidian's `|300` (or `|100%`) taken off.
        let words = if on.html {
            html::entities(raw)
        } else {
            let bare = match raw.rsplit_once('|') {
                Some((w, n)) if !n.trim().trim_end_matches('%').is_empty() && n.trim().trim_end_matches('%').bytes().all(|b| b.is_ascii_digit()) => w,
                _ => raw,
            };
            let mut out = String::new();
            let mut chars = bare.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\\' && chars.peek().is_some_and(|n| n.is_ascii_punctuation()) {
                    continue;
                }
                out.push(c);
            }
            out
        };
        let width = width.unwrap_or(on.asked);
        // Full stands across the whole width, so its place says nothing.
        let across = if width == Some(Width::Share(100)) { Across::Start } else { across.unwrap_or(on.across.unwrap_or(Across::Start)) };
        let written = html::picture_markup(&on.src, &words, width, across, embed);
        let lead = text.len() - text.trim_start().len();
        let end = text.trim_end().len();
        if text[lead..end] == written {
            return;
        }
        // A piece selected stays selected, the whole of its new line.
        let whole = self.object_selected() == Some(line);
        let start = self.doc.offset_of(Pos::new(line, 0));
        self.apply_edits(vec![(start + lead..start + end, written)], cx);
        if whole {
            self.sel = self.object_sel(line);
        }
    }

    /// The viewer, opened on the picture on `line`, with the note's other
    /// pictures a key away. `origin` is the element's top left in the
    /// window.
    pub(super) fn view_picture(&mut self, line: usize, origin: Point<Pixels>, cx: &mut Context<Self>) {
        let mut pictures = Vec::new();
        let mut at = None;
        let mut from = None;
        {
            let cache = self.cache.borrow();
            let store = self.pictures.borrow();
            for (l, entry) in cache.entries.iter().enumerate() {
                let Row::Picture(p) = &*entry.row else { continue };
                // A file in a frame is not a picture.
                if p.embed {
                    continue;
                }
                let drawn = store.drawn.get(&p.path).cloned();
                let size = store.sizes.get(&p.path).copied().flatten().unwrap_or((p.w as u32, p.h as u32));
                let place = Rect { x: p.x, y: cache.tops[l] + p.pad, w: p.w, h: p.h };
                if l == line {
                    at = Some(pictures.len());
                    from = Some(Bounds::new(origin + point(px(place.x), px(place.y)), gpui::size(px(place.w), px(place.h))));
                }
                pictures.push(Seen { path: p.path.clone(), size, drawn });
            }
        }
        let (Some(at), Some(from)) = (at, from) else { return };
        self.pic_hover = None;
        cx.emit(EditorEvent::View(Viewing { pictures, at, from }));
        cx.notify();
    }

    /// For a menu or the bar: the viewer on the picture on `line`, from
    /// where the canvas last stood.
    pub fn view_picture_at(&mut self, line: usize, cx: &mut Context<Self>) {
        let origin = self.origin;
        self.view_picture(line, origin, cx);
    }

    /// What paint draws of the edge of the picture the pointer is over.
    pub(super) fn picture_paint(&self) -> Option<PicPaint> {
        if !self.pictures_change() {
            return None;
        }
        let h = self.pic_hover?;
        let control = self.sized_picture(h.line)?;
        let (x, y) = self.pointer_at;
        let edge = self.pic_drag.is_some() || control.edge_at(x, y).is_some();
        let opacity = (h.since.elapsed().as_secs_f32() / 0.12).min(1.);
        Some(PicPaint { control, edge, opacity })
    }

    /// A choice of a picture's menu or bar, or a probe's: the picture on
    /// `line` placed (`left`, `center`, `right`), sized (`auto`, `full`),
    /// given a width in pixels, or opened in the viewer (`view`).
    pub fn picture_choice(&mut self, line: usize, what: &str, cx: &mut Context<Self>) {
        match what {
            "left" => self.set_picture(line, Some(Across::Start), None, cx),
            "center" => self.set_picture(line, Some(Across::Center), None, cx),
            "right" => self.set_picture(line, Some(Across::End), None, cx),
            "auto" | "natural" => {
                if let Some(width) = self.hundred(line) {
                    self.set_picture(line, None, Some(width), cx);
                }
            }
            "full" => self.set_picture(line, None, Some(Some(Width::Share(100))), cx),
            "view" => self.view_picture_at(line, cx),
            w => {
                if let Ok(w) = w.parse::<u32>() {
                    self.set_picture(line, None, Some(Some(Width::Px(w))), cx);
                }
            }
        }
    }

    /// For a probe: the pointer over the picture on `line`, so its edge
    /// can be taken.
    pub fn picture_hover_probe(&mut self, line: usize, cx: &mut Context<Self>) {
        self.pic_hover = Some(PicHover { line, since: Instant::now() - std::time::Duration::from_secs(1) });
        cx.notify();
    }
}

/// The edge of the picture under the pointer, while the pointer is on it
/// or holds it: the ring a selected piece has, in a lighter accent, so the
/// two are one shape and a selected piece's edge only darkens.
pub(super) fn paint_picture_edge(c: &PicPaint, ox: f32, oy: f32, theme: &gpui_component::Theme, window: &mut Window) {
    if !c.edge {
        return;
    }
    let b = Bounds::new(point(px(ox + c.control.x), px(oy + c.control.y)), gpui::size(px(c.control.w), px(c.control.h)));
    paint_ring(b, theme.primary.opacity(0.55 * c.opacity), window);
}

/// The ring around a piece, out from its edge by its gap and its width,
/// its corners round about the piece's own (`CONTROL_ROUND`).
pub(super) fn paint_ring(control: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let out = px(RING_OUT);
    let b = Bounds::new(point(control.origin.x - out, control.origin.y - out), gpui::size(control.size.width + out * 2., control.size.height + out * 2.));
    window.paint_quad(quad(b, px(crate::embed::CONTROL_ROUND + RING_OUT), super::clear_of(color), px(RING_W), color, BorderStyle::default()));
}
