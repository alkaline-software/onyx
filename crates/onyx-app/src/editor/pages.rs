//! The frames of the files a note embeds (`embed.rs`), as the note's
//! editor keeps them: one for each line that embeds a PDF, a note or a
//! document, kept by its file and by which of that file's frames it is,
//! so a change to another line keeps its scroll and zoom. A frame is
//! made the first time its row is laid out and let go, its pages with
//! it, when its line goes. Each is laid out over its row as the note is
//! painted, so it moves with the note; those out of sight are not drawn.
//! A document's frame lays its words out as soon as they are read, seen
//! or not, so its height is known before it comes into sight and the
//! note never moves as one scrolls past.
//!
//! The note's find looks in a PDF's words too: each PDF's text is read
//! once, by the same reading of the page that draws it
//! (`onyx_core::preview::pdf_text`); a match scrolls its frame to the
//! place and the frame into view, and every match is washed over its
//! page, the one the bar stands on in the selection's color. A page whose
//! font says nothing of its characters has no words to find.
//! `docs/editor.md`, Pictures.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::*;

use super::Editor;
use crate::card::Card;
use crate::embed::{CardBar, Embed, HEAD_H};
use crate::layout::Row;
use crate::lines::{Pos, Sel};

/// A place the find bar's words are on a PDF's page: the PDF's line and
/// file, the page, and the glyphs from and to.
#[derive(Clone, Debug)]
pub(crate) struct PageHit {
    pub line: usize,
    pub path: PathBuf,
    pub page: usize,
    pub from: usize,
    pub to: usize,
}

impl Editor {
    /// This editor shows a file in a frame: what it embeds stays words.
    pub(crate) fn set_framed(&mut self, cx: &mut Context<Self>) {
        self.pictures.borrow_mut().framed = true;
        cx.notify();
    }

    /// How tall the note stands, as last laid out.
    pub(crate) fn content_height(&self) -> f32 {
        self.cache.borrow().total
    }

    /// Each line that embeds a file in a frame, or a picture under a bar:
    /// the line, the file, and which of that file's it is.
    fn frame_lines(&self) -> Vec<(usize, PathBuf, usize)> {
        let cache = self.cache.borrow();
        let mut seen: HashMap<&std::path::Path, usize> = HashMap::new();
        let mut out = Vec::new();
        for (line, entry) in cache.entries.iter().enumerate() {
            if let Row::Picture(p) = &*entry.row {
                if p.bar {
                    let n = seen.entry(p.path.as_path()).or_default();
                    out.push((line, p.path.clone(), *n));
                    *n += 1;
                }
            }
        }
        out
    }

    /// The frame on a line, by its file and which of its frames it is.
    fn frame_on(&self, line: usize) -> Option<(PathBuf, usize)> {
        self.frame_lines().into_iter().find(|(l, _, _)| *l == line).map(|(_, p, n)| (p, n))
    }

    /// Every frame made and sized, those gone let go, and the ones that
    /// show with where they stand in the window.
    pub(super) fn frames_in_view(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> Vec<(Entity<Embed>, Bounds<Pixels>)> {
        let lines = self.frame_lines();
        let keep: HashSet<(PathBuf, usize)> = lines.iter().map(|(_, p, n)| (p.clone(), *n)).collect();
        let gone: Vec<(PathBuf, usize)> = self.embeds.keys().filter(|k| !keep.contains(*k)).cloned().collect();
        for key in gone {
            if let Some(frame) = self.embeds.remove(&key) {
                frame.update(cx, |m, cx| m.let_go(window, cx));
            }
        }
        let view = window.content_mask().bounds;
        let weak = cx.entity().downgrade();
        let mut out = Vec::new();
        for (line, path, nth) in lines {
            // A picture's view is its bar alone, while it shows: the picture
            // is painted under it.
            let (top, x, w, h, embed) = {
                let cache = self.cache.borrow();
                let Some(Row::Picture(p)) = cache.entries.get(line).map(|e| &*e.row) else { continue };
                (cache.tops[line] + p.pad, p.x, p.w, p.h, p.embed)
            };
            let near = self.pointer_on(line);
            let (store, look) = (self.pictures.clone(), self.look.clone());
            let frame = self.embeds.entry((path.clone(), nth)).or_insert_with(|| cx.new(|cx| Embed::new(path.clone(), nth, weak.clone(), store, look, cx))).clone();
            // A document's words laid out at the frame's width give its
            // height; the note is laid out again when it changes.
            let (words, alpha) = frame.update(cx, |m, cx| {
                m.size = (w, h);
                (m.measure(window, cx), m.settle_bar(near, window))
            });
            if !embed && alpha <= 0. {
                continue;
            }
            let h = if embed { h } else { HEAD_H.min(h) };
            if let Some(tall) = words {
                let mut store = self.pictures.borrow_mut();
                if store.embed_heights.get(&path).is_none_or(|was| (was - tall).abs() > 0.5) {
                    store.embed_heights.insert(path.clone(), tall);
                    cx.notify();
                }
            }
            let b = Bounds::new(point(bounds.origin.x + px(x), bounds.origin.y + px(top)), size(px(w), px(h)));
            if b.intersects(&view) {
                out.push((frame, b));
            }
        }
        out
    }

    /// Each line that shows a link as a card: the line, its link, and
    /// which of that link's cards it is.
    fn card_lines(&self) -> Vec<(usize, String, usize)> {
        let cache = self.cache.borrow();
        let mut seen: HashMap<&str, usize> = HashMap::new();
        let mut out = Vec::new();
        for (line, entry) in cache.entries.iter().enumerate() {
            if let Row::Picture(p) = &*entry.row {
                if p.card.is_some() {
                    let n = seen.entry(p.href.as_str()).or_default();
                    out.push((line, p.href.clone(), *n));
                    *n += 1;
                }
            }
        }
        out
    }

    /// The line a card stands on, by its link and which of its it is.
    pub(crate) fn line_of_card(&self, key: &(String, usize)) -> Option<usize> {
        self.card_lines().into_iter().find(|(_, h, n)| *h == key.0 && *n == key.1).map(|(l, _, _)| l)
    }

    /// Every card made and sized, those gone let go, and the ones that
    /// show with where they stand in the window.
    pub(super) fn cards_in_view(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> Vec<(AnyView, Bounds<Pixels>)> {
        let lines = self.card_lines();
        let keep: HashSet<(String, usize)> = lines.iter().map(|(_, h, n)| (h.clone(), *n)).collect();
        self.cards.retain(|k, _| keep.contains(k));
        self.card_bars.retain(|k, _| keep.contains(k));
        let view = window.content_mask().bounds;
        let weak = cx.entity().downgrade();
        let mut out = Vec::new();
        for (line, href, nth) in lines {
            let (top, x, w, h, kind, web, path, words, across) = {
                let cache = self.cache.borrow();
                let Some(Row::Picture(p)) = cache.entries.get(line).map(|e| &*e.row) else { continue };
                let Some(kind) = p.card else { continue };
                let words = self.reader().picture_on(line).map(|on| onyx_core::html::entities(self.doc.line(line).get(on.words).unwrap_or(""))).unwrap_or_default();
                (cache.tops[line] + p.pad, p.x, p.w, p.h, kind, p.web, p.path.clone(), words, p.across)
            };
            let card = self.cards.entry((href.clone(), nth)).or_insert_with(|| cx.new(|cx| Card::new(href.clone(), path.clone(), web, nth, weak.clone(), cx))).clone();
            card.update(cx, |c, cx| {
                if c.size != (w, h) || c.layout != kind || c.words != words {
                    c.size = (w, h);
                    c.layout = kind;
                    c.words = words;
                    cx.notify();
                }
            });
            let b = Bounds::new(point(bounds.origin.x + px(x), bounds.origin.y + px(top)), size(px(w), px(h)));
            if !b.intersects(&view) {
                continue;
            }
            let hovered = card.read(cx).hovered;
            out.push((card.into(), b));
            // A large card has a bar over its top, inside its border, as a
            // picture has: it comes with the pointer and goes as it leaves.
            if kind != onyx_core::html::CardSize::Large {
                self.card_bars.remove(&(href, nth));
                continue;
            }
            let near = hovered || self.pointer_on(line);
            let (store, look) = (self.pictures.clone(), self.look.clone());
            let key = (href.clone(), nth);
            let bar = self.card_bars.entry(key.clone()).or_insert_with(|| cx.new(|cx| Embed::card_bar(path.clone(), CardBar { key, web, across: None }, weak.clone(), store, look, cx))).clone();
            let alpha = bar.update(cx, |m, cx| {
                m.size = (w - 2., HEAD_H);
                if let Some(c) = m.card.as_mut() {
                    if c.across != Some(across) {
                        c.across = Some(across);
                        cx.notify();
                    }
                }
                m.settle_bar(near, window)
            });
            if alpha > 0. {
                out.push((bar.into(), Bounds::new(b.origin + point(px(1.), px(1.)), size(px(w - 2.), px(HEAD_H.min(h - 2.))))));
            }
        }
        out
    }

    /// The find's places on the frame of `path` that is its `nth`: the
    /// page, the glyphs from and to, and whether the bar stands on it.
    pub(crate) fn marks_of(&self, path: &std::path::Path, nth: usize) -> Vec<(usize, usize, usize, bool)> {
        if self.find_pages.is_empty() {
            return Vec::new();
        }
        let lines = self.frame_lines();
        let Some(&(line, _, _)) = lines.iter().find(|(_, p, n)| p == path && *n == nth) else { return Vec::new() };
        let on = self.find_order.get(self.find_at).filter(|(page, _)| *page).map(|(_, k)| *k);
        self.find_pages.iter().enumerate().filter(|(_, h)| h.line == line).map(|(i, h)| (h.page, h.from, h.to, on == Some(i))).collect()
    }

    /// The line a frame or a picture's bar stands on, by its file and
    /// which of that file's it is.
    pub(crate) fn line_of_frame(&self, key: &(PathBuf, usize)) -> Option<usize> {
        self.frame_lines().into_iter().find(|(_, p, n)| *p == key.0 && *n == key.1).map(|(l, _, _)| l)
    }

    /// Each PDF's text, read once off the window's thread, for the find
    /// and for a selection in its frame.
    pub(super) fn read_pdfs(&mut self, cx: &mut Context<Self>) {
        let mut texts: Vec<PathBuf> = Vec::new();
        {
            let mut store = self.pictures.borrow_mut();
            for (path, f) in store.pdfs.iter_mut() {
                if !f.text_asked && !f.sizes.is_empty() {
                    f.text_asked = true;
                    texts.push(path.clone());
                }
            }
        }
        for path in texts {
            cx.spawn(async move |this, cx| {
                let file = path.clone();
                let text = cx.background_spawn(async move { onyx_core::preview::pdf_text(&file).ok() }).await;
                let _ = this.update(cx, |e, cx| {
                    if let Some(text) = text {
                        if let Some(f) = e.pictures.borrow_mut().pdfs.get_mut(&path) {
                            f.text = Some(Arc::new(text));
                        }
                        // A find already open looks in it too, and goes to
                        // the first place when it had found none.
                        if !e.find_query.is_empty() {
                            let had = e.find_order.len();
                            e.refind();
                            if had == 0 && !e.find_order.is_empty() {
                                e.find_at = 0;
                                e.find_go(cx);
                            }
                            cx.emit(super::EditorEvent::Found);
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Every place the find's words are on the note's PDFs, in the order
    /// of their lines, then their pages.
    pub(super) fn find_on_pages(&self, query: &str) -> Vec<PageHit> {
        let lines: Vec<(usize, PathBuf)> = self.frame_lines().into_iter().filter(|(_, p, _)| onyx_core::vault::is_pdf(p)).map(|(l, p, _)| (l, p)).collect();
        let store = self.pictures.borrow();
        let mut out = Vec::new();
        for (line, path) in lines {
            let Some(text) = store.pdfs.get(&path).and_then(|f| f.text.clone()) else { continue };
            for (page, from, to) in onyx_core::preview::pdf_find(&text, query) {
                out.push(PageHit { line, path: path.clone(), page, from, to });
            }
        }
        out
    }

    /// The find bar on a place in a PDF: its frame scrolled to the place,
    /// the caret on the frame's line, and the frame brought into view.
    pub(super) fn find_go_page(&mut self, hit: &PageHit, cx: &mut Context<Self>) {
        let down = {
            let store = self.pictures.borrow();
            store.pdfs.get(&hit.path).and_then(|f| f.text.as_ref()).and_then(|t| t.get(hit.page)).and_then(|t| t.boxes(hit.from, hit.to).first().map(|b| b.1)).unwrap_or(0.)
        };
        if let Some(frame) = self.frame_on(hit.line).and_then(|k| self.embeds.get(&k).cloned()) {
            frame.update(cx, |m, cx| m.show(hit.page, down, cx));
        }
        let tall = self.cache.borrow().entries.get(hit.line).map_or(0., |e| match &*e.row {
            Row::Picture(p) => p.h,
            _ => e.height,
        });
        let end = self.doc.line(hit.line).len();
        self.sel = Sel::caret(Pos::new(hit.line, end));
        self.hold_objects();
        self.want_spot = Some((hit.line, 0., tall.min(HEAD_H + 320.)));
        self.want_scroll = true;
        cx.notify();
    }
}
