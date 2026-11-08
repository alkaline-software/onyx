//! A file a note embeds, in a frame of its own, as Obsidian frames a PDF:
//! a bar along its top and the file under it, scrolling in the frame
//! while the note stands still. A PDF shows every page, one under the
//! next, as wide as the frame, its words selectable and copied as words;
//! the frame is as tall as its first page there, so one page shows whole.
//! A note, a document, code, a table of values: read as the file's tab
//! reads it (`preview::read`), drawn by an editor of its own as it reads,
//! the frame as tall as the words up to `WORDS_MOST`.
//!
//! A PDF's frame has a sidebar, as pdf.js's has, shut until its button
//! on the bar opens it: the pages small, drawn a few at a time as they
//! come into sight, the one showing marked and kept in sight; or the
//! PDF's outline, read when the sidebar first opens, its lines folded
//! as the file has them. A press on either goes there. ⌘ and the wheel,
//! or a pinch, zoom about the pointer; a right click gives Copy.
//!
//! Every file a note embeds, a picture as a PDF, has one bar, which
//! comes over its top as the pointer comes onto it and goes as it
//! leaves, fading both ways, so at rest the file stands alone. The bar
//! holds the file's icon and name, which a click renames in place, what
//! its kind has, the pencil and a menu with Delete last; a right click
//! on it opens that menu, and a press on its empty part selects the
//! whole in the note. A picture's view is its bar alone: the note paints
//! the picture under it.
//!
//! A PDF is sized as a picture is: by its line's width, its own width
//! (100%) when none is asked, a drag of its edge (`editor/picture.rs`)
//! and its place across the page, with the same buttons for them. A
//! press in it selects its words and the whole of it in the note.
//!
//! The note's editor makes a frame for each line that embeds such a file
//! and lays it out over the line's row (`editor/pages.rs`), so it moves
//! with the note. A press in a frame stays in it and never moves the
//! note's caret; the pencil on the bar does. `docs/editor.md`, Pictures.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, ActiveTheme as _, Icon, Sizable as _};
use onyx_core::preview::{self, PdfMark, PdfText};

use crate::editor::{Editor, EditorEvent, FrameMenu, Look, MenuAt, Mode};
use crate::i18n::t;
use crate::layout::Pictures;
use crate::widgets::{swallow_click, tip, FOLD_ANIM};

actions!(embed, [Leave, Still, LineUp, LineDown, PageUpIn, PageDownIn, ToStart, ToEnd]);

/// The key context of a frame the keyboard is in.
pub(crate) const EMBED_CONTEXT: &str = "Embed";

/// The bar along a frame's top, and a picture's.
pub(crate) const HEAD_H: f32 = 36.;
/// The narrowest a PDF's frame goes.
pub(crate) const CONTROL_LEAST: f32 = 160.;
/// The corners of every piece a note shows whole, a picture and a frame
/// alike: the bar over its top follows them, inside a frame's border by
/// its width, and the ring around it stands out from them by its own.
pub(crate) const CONTROL_ROUND: f32 = 10.;
/// How long the bar takes to come and to go.
const BAR_FADE: f32 = 0.15;
/// The tallest and the shortest a document's words stand in a frame.
pub(crate) const WORDS_MOST: f32 = 560.;
pub(crate) const WORDS_LEAST: f32 = 72.;
/// The room around a document's words.
const WORDS_PAD_X: f32 = 18.;
const WORDS_PAD_Y: f32 = 12.;
/// Less under the words: a note's last block has room under it already.
const WORDS_FOOT: f32 = 2.;
/// Between one page and the next.
const GAP: f32 = 8.;
/// How many pages stay drawn around those showing.
const KEEP: usize = 10;
/// The widest a page is drawn, in pixels.
const WIDEST: f32 = 4096.;
/// The zooms the buttons step through, as points to pixels.
const ZOOMS: [f32; 15] = [0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2., 2.5, 3., 4.];
const BUTTON: f32 = 24.;
/// The sidebar's width, as wide as a third of the frame at most, and a
/// small page's in it.
const SIDE_W: f32 = 168.;
const THUMB_W: f32 = 108.;
/// Above each small page, and its number under it.
const THUMB_GAP: f32 = 12.;
const THUMB_LABEL: f32 = 18.;
/// How many small pages are drawn in one reading of the file, and how
/// many stay drawn around those showing.
const THUMB_BATCH: usize = 6;
const THUMB_KEEP: usize = 30;
/// The bar over the sidebar that turns it from pages to the outline.
const SIDE_HEAD: f32 = 32.;
/// A line of the outline.
const MARK_H: f32 = 26.;

/// What the sidebar lists.
#[derive(Clone, Copy, PartialEq)]
enum List {
    Pages,
    Outline,
}

/// A place in a PDF's words: a page, and the place between two of its
/// glyphs (`PdfText::place_at`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Spot {
    page: usize,
    at: usize,
}

enum Body {
    /// A picture: the bar alone, the note paints the picture under it.
    Picture,
    /// A PDF: its pages come from the note's store (`Pictures::pdfs`).
    Pdf,
    /// Still being read off the window's thread.
    Reading,
    /// Read as markdown, in an editor of its own.
    Words(Entity<Editor>),
    /// Nothing to show, and why.
    Not(SharedString),
}

/// What a card's bar knows of its card (`card.rs`): which card it is, by
/// its link and which of that link's, whether it leads to the web, and a
/// large card's place across the page.
pub(crate) struct CardBar {
    pub key: (String, usize),
    pub web: bool,
    pub across: Option<onyx_core::html::Across>,
}

/// A page drawn: how wide, in pixels, and its pixels.
struct Leaf {
    width: u32,
    image: Arc<RenderImage>,
}

pub(crate) struct Embed {
    path: PathBuf,
    /// Which of the note's frames of this file it is, the first being 0.
    nth: usize,
    note: WeakEntity<Editor>,
    store: Rc<RefCell<Pictures>>,
    body: Body,
    focus: FocusHandle,
    /// The frame's size, which the note's layout gives it.
    pub(crate) size: (f32, f32),
    /// A PDF's scroller; a document scrolls in its editor's own.
    scroll: ScrollHandle,
    /// Points to pixels; none while the pages fit the frame's width.
    zoom: Option<f32>,
    leaves: HashMap<usize, Leaf>,
    /// The page being drawn and how wide; one at a time.
    drawing: Option<(usize, u32)>,
    /// A selection of a PDF's words, from where it began to where it is,
    /// and whether a press is making it.
    sel: Option<(Spot, Spot)>,
    selecting: bool,
    page_field: Option<Entity<InputState>>,
    /// The page the field says, which follows the scroll.
    shown_page: usize,
    /// A place to bring into view once the frame has its size: a page,
    /// and how far down it, in points.
    want: Option<(usize, f32)>,
    /// The width a document's words were last laid out at.
    measured: Option<f32>,
    /// The scale the pages were last drawn at: a change the zoom did not
    /// make (the sidebar, the frame's width) keeps the same place in view.
    last_k: Option<f32>,
    /// The sidebar: whether it is open, what it lists, when it last
    /// opened or shut (for its slide), and its scroller.
    side_open: bool,
    side_list: List,
    side_at: Option<Instant>,
    side_scroll: ScrollHandle,
    /// The small pages, drawn a few at a time, one reading at a time.
    thumbs: HashMap<usize, Leaf>,
    thumbing: bool,
    /// The page the sidebar last brought into its view.
    followed: usize,
    /// The outline's lines opened or folded here, by their place in it.
    folds: HashMap<usize, bool>,
    /// The file's name as a field, while it is being renamed.
    renaming: Option<Entity<InputState>>,
    /// Whether the pointer is on the frame, and whether the bar is wanted
    /// and since when, for its fade.
    hovered: bool,
    bar_on: bool,
    bar_since: Option<Instant>,
    /// The bar of a large card: no file under it, the card is.
    pub(crate) card: Option<CardBar>,
    _subs: Vec<Subscription>,
}

impl Embed {
    pub(crate) fn new(path: PathBuf, nth: usize, note: WeakEntity<Editor>, store: Rc<RefCell<Pictures>>, look: Look, cx: &mut Context<Self>) -> Self {
        let body = if onyx_core::vault::is_pdf(&path) {
            Body::Pdf
        } else if crate::layout::is_framed(&path) {
            Body::Reading
        } else {
            Body::Picture
        };
        let mut me = Self { path, nth, note, store, body, focus: cx.focus_handle(), size: (0., 0.), scroll: ScrollHandle::new(), zoom: None, leaves: HashMap::new(), drawing: None, sel: None, selecting: false, page_field: None, shown_page: usize::MAX, want: None, measured: None, last_k: None, side_open: false, side_list: List::Pages, side_at: None, side_scroll: ScrollHandle::new(), thumbs: HashMap::new(), thumbing: false, followed: usize::MAX, folds: HashMap::new(), renaming: None, hovered: false, bar_on: false, bar_since: None, card: None, _subs: Vec::new() };
        if matches!(me.body, Body::Reading) {
            me.read_words(look, cx);
        }
        me
    }

    /// The bar alone, over a large card: a picture's bar, whose card is
    /// drawn under it.
    pub(crate) fn card_bar(path: PathBuf, card: CardBar, note: WeakEntity<Editor>, store: Rc<RefCell<Pictures>>, look: Look, cx: &mut Context<Self>) -> Self {
        let mut me = Self::new(PathBuf::new(), 0, note, store, look, cx);
        me.path = path;
        me.card = Some(card);
        me
    }

    /// A document read off the window's thread, then given to an editor
    /// that shows it as it reads, its links followed from where it is.
    fn read_words(&mut self, look: Look, cx: &mut Context<Self>) {
        let path = self.path.clone();
        let vault = self.store.borrow().vault.clone();
        let read = cx.background_spawn({
            let path = path.clone();
            async move { preview::read(&path) }
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |me, cx| {
                me.body = match read {
                    preview::Preview::Markdown { text, cut } => {
                        let text = if cut { format!("{text}\n\n*{}*\n", t("Only the start of the file is shown.")) } else { text };
                        let editor = cx.new(Editor::new);
                        editor.update(cx, |e, cx| {
                            e.set_framed(cx);
                            e.set_look(look, cx);
                            e.set_mode(Mode::Reading, cx);
                            e.set_place(path.clone(), vault, cx);
                            e.set_text(&text, cx);
                        });
                        let from = path.clone();
                        me._subs.push(cx.subscribe(&editor, move |me, editor, ev: &EditorEvent, cx| me.pass_on(ev, &from, &editor, cx)));
                        Body::Words(editor)
                    }
                    preview::Preview::None(why) => Body::Not(t(&why).to_string().into()),
                    _ => Body::Not(t("Onyx cannot show this kind of file.").to_string().into()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// What the document's editor asks of the window, asked by the note's
    /// editor: a link by its path from the document, not from the note;
    /// the words' own menu as a frame's, on the document's selection.
    fn pass_on(&mut self, ev: &EditorEvent, from: &Path, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let ev = match ev {
            EditorEvent::OpenLink(url) if !url.contains("://") && !url.starts_with("mailto:") => {
                let vault = self.store.borrow().vault.clone();
                match onyx_core::vault::find_file(vault.as_deref(), from, url) {
                    Some(found) => EditorEvent::OpenLink(found.to_string_lossy().to_string()),
                    None => return,
                }
            }
            EditorEvent::Menu(at, what) if what.link.is_none() && what.file.is_none() => {
                let e = editor.read(cx);
                EditorEvent::Menu(*at, MenuAt { frame: Some(FrameMenu { focus: e.focus_handle(cx), selection: e.has_selection() }), ..Default::default() })
            }
            EditorEvent::OpenLink(url) => EditorEvent::OpenLink(url.clone()),
            EditorEvent::OpenNote(name) => EditorEvent::OpenNote(name.clone()),
            EditorEvent::Menu(at, menu) => EditorEvent::Menu(*at, menu.clone()),
            _ => return,
        };
        if let Some(note) = self.note.upgrade() {
            cx.defer(move |cx| note.update(cx, |_, cx| cx.emit(ev)));
        }
    }

    pub(crate) fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        if let Body::Words(editor) = &self.body {
            editor.update(cx, |e, cx| e.set_look(look, cx));
            self.measured = None;
        }
    }

    /// How tall a document's words stand at the frame's width, laid out
    /// again only when the width changed: what the frame's height goes by.
    /// None for a PDF, whose first page gives it, and while still read.
    pub(crate) fn measure(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<f32> {
        match &self.body {
            Body::Words(editor) => {
                let w = (self.size.0 - 2. - WORDS_PAD_X * 2.).max(40.);
                if self.measured != Some(w) {
                    editor.update(cx, |e, cx| e.relayout(w, window, cx));
                    self.measured = Some(w);
                }
                Some(editor.read(cx).content_height() + WORDS_PAD_Y + WORDS_FOOT)
            }
            Body::Not(_) => Some(WORDS_LEAST),
            _ => None,
        }
    }

    /// The pages' pixels let go, as the frame goes.
    pub(crate) fn let_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (_, leaf) in self.leaves.drain().chain(self.thumbs.drain()) {
            cx.drop_image(leaf.image, Some(window));
        }
        if let Body::Words(editor) = &self.body {
            editor.update(cx, |e, cx| e.let_pictures_go(window, cx));
        }
    }

    /// The bar wanted or not, the pointer near the piece or not (`near`,
    /// from the note, which hears the pointer on a picture), and how far
    /// it shows at this draw, 0 to 1.
    pub(crate) fn settle_bar(&mut self, near: bool, window: &mut Window) -> f32 {
        let want = near || self.hovered || self.renaming.is_some();
        if want != self.bar_on {
            // A fade turned back midway goes back from where it was.
            let shown = self.bar_alpha();
            self.bar_on = want;
            let back = if want { shown } else { 1. - shown };
            self.bar_since = Some(Instant::now() - std::time::Duration::from_secs_f32(back * BAR_FADE));
        }
        let alpha = self.bar_alpha();
        if alpha > 0. && alpha < 1. {
            window.request_animation_frame();
        }
        alpha
    }

    /// How far the bar shows, 0 to 1.
    fn bar_alpha(&self) -> f32 {
        let t = self.bar_since.map_or(1., |at| (at.elapsed().as_secs_f32() / BAR_FADE).min(1.));
        if self.bar_on { t } else { 1. - t }
    }

    /// How wide the frame stands: the row's width, which the line says.
    pub(crate) fn frame_w(&self) -> f32 {
        self.size.0
    }

    /// The room the pages or the words have: the frame's less its bar,
    /// its edge and the sidebar as far as it is out.
    fn body_size(&self) -> (f32, f32) {
        ((self.frame_w() - 2. - self.side_w()).max(10.), (self.size.1 - 2.).max(10.))
    }

    /// How wide the sidebar stands at this draw, as it slides.
    fn side_w(&self) -> f32 {
        let full = SIDE_W.min((self.size.0 / 3.).round());
        let t = self.side_at.map_or(1., |at| ease_out_quint()((at.elapsed().as_secs_f32() / FOLD_ANIM.as_secs_f32()).min(1.)));
        (full * if self.side_open { t } else { 1. - t }).round()
    }

    fn sliding(&self) -> bool {
        self.side_at.is_some_and(|at| at.elapsed() < FOLD_ANIM)
    }

    fn toggle_side(&mut self, cx: &mut Context<Self>) {
        self.side_open = !self.side_open;
        self.side_at = Some(Instant::now());
        self.followed = usize::MAX;
        if self.side_open {
            self.ask_outline(cx);
        }
        cx.notify();
    }

    /// The PDF's outline read once for the note, off the window's thread.
    fn ask_outline(&mut self, cx: &mut Context<Self>) {
        {
            let mut store = self.store.borrow_mut();
            let Some(f) = store.pdfs.get_mut(&self.path).filter(|f| !f.outline_asked) else { return };
            f.outline_asked = true;
        }
        let (path, store) = (self.path.clone(), self.store.clone());
        let job = cx.background_spawn({
            let path = path.clone();
            async move { preview::pdf_outline(&path).unwrap_or_default() }
        });
        cx.spawn(async move |this, cx| {
            let marks = job.await;
            if let Some(f) = store.borrow_mut().pdfs.get_mut(&path) {
                f.outline = Some(Arc::new(marks));
            }
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    fn outline(&self) -> Option<Arc<Vec<PdfMark>>> {
        self.store.borrow().pdfs.get(&self.path).and_then(|f| f.outline.clone())
    }

    fn sizes(&self) -> Vec<(f32, f32)> {
        self.store.borrow().pdfs.get(&self.path).map(|f| f.sizes.clone()).unwrap_or_default()
    }

    fn text(&self) -> Option<Arc<Vec<PdfText>>> {
        self.store.borrow().pdfs.get(&self.path).and_then(|f| f.text.clone())
    }

    /// Points to pixels, as the zoom is or as the widest page fits.
    fn scale(&self, sizes: &[(f32, f32)]) -> f32 {
        let widest = sizes.iter().map(|s| s.0).fold(1., f32::max);
        self.zoom.unwrap_or(self.body_size().0 / widest).max(0.05)
    }

    /// Where each page stands in what scrolls, at `k`: its left, top,
    /// width and height; and how wide and tall the whole is.
    fn places(&self, sizes: &[(f32, f32)], k: f32) -> (Vec<(f32, f32, f32, f32)>, f32, f32) {
        let (bw, _) = self.body_size();
        let widest = sizes.iter().map(|s| s.0).fold(1., f32::max);
        let cw = (widest * k).max(bw).round();
        let mut y = 0.;
        let mut out = Vec::with_capacity(sizes.len());
        for (w, h) in sizes {
            let (pw, ph) = ((w * k).round(), (h * k).round());
            out.push((((cw - pw) / 2.).round(), y, pw, ph));
            y += ph + GAP;
        }
        (out, cw, (y - GAP).max(0.))
    }

    /// The pages any of whose height shows, from the scroll.
    fn showing(&self, places: &[(f32, f32, f32, f32)]) -> std::ops::Range<usize> {
        let (_, bh) = self.body_size();
        let top = -f32::from(self.scroll.offset().y);
        let first = places.partition_point(|p| p.1 + p.3 < top);
        let last = places.partition_point(|p| p.1 < top + bh);
        first..last.max(first + 1).min(places.len())
    }

    /// The zoom set, the place under `at` (a point of the window), or at
    /// the middle of the frame, kept there.
    fn set_zoom(&mut self, zoom: Option<f32>, at: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let sizes = self.sizes();
        if sizes.is_empty() {
            return;
        }
        let was = self.scale(&sizes);
        self.zoom = zoom;
        let now = self.scale(&sizes);
        let (bw, bh) = self.body_size();
        let (ax, ay) = at.map_or((bw / 2., bh / 2.), |at| {
            let o = self.scroll.bounds().origin;
            (f32::from(at.x - o.x).clamp(0., bw), f32::from(at.y - o.y).clamp(0., bh))
        });
        let o = self.scroll.offset();
        let (cx0, cy0) = (-f32::from(o.x) + ax, -f32::from(o.y) + ay);
        let r = now / was;
        let (_, cw, ch) = self.places(&sizes, now);
        let x = (cx0 * r - ax).clamp(0., (cw - bw).max(0.));
        let y = (cy0 * r - ay).clamp(0., (ch - bh).max(0.));
        self.scroll.set_offset(point(px(-x), px(-y)));
        self.last_k = Some(now);
        cx.notify();
    }

    /// The zoom times `by`, about a point of the window: ⌘ and the wheel,
    /// or a pinch.
    fn zoom_by(&mut self, by: f32, at: Point<Pixels>, cx: &mut Context<Self>) {
        let sizes = self.sizes();
        if sizes.is_empty() || !by.is_finite() {
            return;
        }
        let now = self.scale(&sizes);
        let to = (now * by).clamp(ZOOMS[0], ZOOMS[ZOOMS.len() - 1]);
        if (to - now).abs() > 1e-4 {
            self.set_zoom(Some(to), Some(at), cx);
        }
    }

    fn wheel(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.modifiers.platform {
            cx.stop_propagation();
            let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
            self.zoom_by((dy * 0.005).exp(), ev.position, cx);
        }
    }

    fn pinch(&mut self, ev: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.zoom_by(1. + ev.delta, ev.position, cx);
    }

    fn step_zoom(&mut self, up: bool, cx: &mut Context<Self>) {
        let sizes = self.sizes();
        if sizes.is_empty() {
            return;
        }
        let now = self.scale(&sizes);
        let next = if up { ZOOMS.iter().find(|z| **z > now * 1.01) } else { ZOOMS.iter().rev().find(|z| **z < now * 0.99) };
        if let Some(z) = next {
            self.set_zoom(Some(*z), None, cx);
        }
    }

    /// The PDF scrolled to a place on a page, a little under the frame's
    /// top: the find's place.
    pub(crate) fn show(&mut self, page: usize, down: f32, cx: &mut Context<Self>) {
        self.want = Some((page, down));
        cx.notify();
    }

    /// The PDF scrolled to `down` points down a page; a place the find
    /// found stands a little under the frame's top.
    fn go_to(&mut self, page: usize, down: f32, found: bool) {
        let sizes = self.sizes();
        if sizes.is_empty() || self.size.1 <= 0. {
            return;
        }
        let k = self.scale(&sizes);
        let (places, _, ch) = self.places(&sizes, k);
        let Some(p) = places.get(page.min(places.len() - 1)) else { return };
        let (_, bh) = self.body_size();
        // A place found stands a little under the frame's top, which is
        // in sight even when the frame is taller than the window.
        let lead = if found && down > 0. { (bh / 4.).min(96.) } else { 0. };
        let y = (p.1 + down * k - lead).clamp(0., (ch - bh).max(0.));
        self.scroll.set_offset(point(self.scroll.offset().x, px(-y)));
    }

    /// Pages that show, or nearly, drawn at the window's pixels, one at a
    /// time, those showing first; pages far from them let go.
    fn draw_pages(&mut self, places: &[(f32, f32, f32, f32)], showing: std::ops::Range<usize>, scale: f32, window: &mut Window, cx: &mut Context<Self>) {
        let near = showing.start.saturating_sub(1)..(showing.end + 1).min(places.len());
        let (lo, hi) = (showing.start.saturating_sub(KEEP / 2), showing.end + KEEP / 2);
        let far: Vec<usize> = self.leaves.keys().copied().filter(|p| *p < lo || *p >= hi).collect();
        for page in far {
            if let Some(leaf) = self.leaves.remove(&page) {
                cx.drop_image(leaf.image, Some(window));
            }
        }
        if self.drawing.is_some() {
            return;
        }
        let wanted = |page: usize| ((places[page].2 * scale).min(WIDEST).round()) as u32;
        let next = showing.clone().chain(near).find(|&p| self.leaves.get(&p).is_none_or(|l| (l.width as f32) < wanted(p) as f32 * 0.87));
        let Some(page) = next else { return };
        let width = wanted(page).max(16);
        self.drawing = Some((page, width));
        let path = self.path.clone();
        let job = cx.background_spawn(async move { preview::pdf_page(&path, page, width).ok().and_then(|(w, h, rgba)| crate::layout::bgra(w, h, rgba)) });
        cx.spawn_in(window, async move |this, cx| {
            let image = job.await;
            let _ = this.update_in(cx, |me, window, cx| {
                me.drawing = None;
                // A page that cannot be drawn is not asked for again.
                let leaf = match image {
                    Some(image) => Leaf { width, image },
                    None => Leaf { width: u32::MAX, image: Arc::new(RenderImage::new(Vec::new())) },
                };
                if let Some(old) = me.leaves.insert(page, leaf) {
                    cx.drop_image(old.image, Some(window));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Where each small page stands in the sidebar: its left, top, width
    /// and height; and how tall the list is.
    fn thumb_places(sizes: &[(f32, f32)]) -> (Vec<(f32, f32, f32, f32)>, f32) {
        let widest = sizes.iter().map(|s| s.0).fold(1., f32::max);
        let k = THUMB_W / widest;
        let mut y = THUMB_GAP;
        let mut out = Vec::with_capacity(sizes.len());
        for (w, h) in sizes {
            let (pw, ph) = ((w * k).round().max(4.), (h * k).round().max(4.));
            out.push((((SIDE_W - pw) / 2.).round(), y, pw, ph));
            y += ph + THUMB_LABEL + THUMB_GAP;
        }
        (out, y)
    }

    /// The small pages in the sidebar's view, drawn a batch at a time in
    /// one reading of the file; those far from the view let go.
    fn draw_thumbs(&mut self, showing: std::ops::Range<usize>, count: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (lo, hi) = (showing.start.saturating_sub(THUMB_KEEP / 2), showing.end + THUMB_KEEP / 2);
        let far: Vec<usize> = self.thumbs.keys().copied().filter(|p| *p < lo || *p >= hi).collect();
        for page in far {
            if let Some(leaf) = self.thumbs.remove(&page) {
                cx.drop_image(leaf.image, Some(window));
            }
        }
        if self.thumbing {
            return;
        }
        let near = showing.end..(showing.end + 2).min(count);
        let batch: Vec<usize> = showing.chain(near).filter(|p| !self.thumbs.contains_key(p)).take(THUMB_BATCH).collect();
        if batch.is_empty() {
            return;
        }
        self.thumbing = true;
        let width = (THUMB_W * window.scale_factor()).round() as u32;
        let path = self.path.clone();
        let pages = batch.clone();
        let job = cx.background_spawn(async move { preview::pdf_pages(&path, &pages, width).into_iter().map(|d| d.and_then(|(w, h, rgba)| crate::layout::bgra(w, h, rgba))).collect::<Vec<_>>() });
        cx.spawn_in(window, async move |this, cx| {
            let drawn = job.await;
            let _ = this.update_in(cx, |me, window, cx| {
                me.thumbing = false;
                for (page, image) in batch.into_iter().zip(drawn) {
                    // A page that cannot be drawn is not asked for again.
                    let leaf = match image {
                        Some(image) => Leaf { width, image },
                        None => Leaf { width: u32::MAX, image: Arc::new(RenderImage::new(Vec::new())) },
                    };
                    if let Some(old) = me.thumbs.insert(page, leaf) {
                        cx.drop_image(old.image, Some(window));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The sidebar: its switch between pages and the outline when the
    /// PDF has one, and the list; `page` is the one the bar says.
    fn render_side(&mut self, sizes: &[(f32, f32)], page: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let outline = self.outline().filter(|o| !o.is_empty());
        if outline.is_none() {
            self.side_list = List::Pages;
        }
        let (_, bh) = self.body_size();
        let head = if outline.is_some() { SIDE_HEAD } else { 0. };
        let view_h = (bh - head).max(10.);
        let top = -f32::from(self.side_scroll.offset().y);
        let scroll = self.side_scroll.clone();
        if let Some(router) = cx.try_global::<crate::scroll::Router>() {
            router.0.name(&scroll);
        }
        let list = match (self.side_list, &outline) {
            (List::Outline, Some(marks)) => self.render_outline(marks.clone(), top, view_h, cx),
            _ => {
                let (places, tall) = Self::thumb_places(sizes);
                // The page the bar says kept in the list's view as it changes.
                if self.followed != page && !self.sliding() {
                    self.followed = page;
                    if let Some(p) = places.get(page) {
                        let (a, b) = (p.1 - THUMB_GAP, p.1 + p.3 + THUMB_LABEL + THUMB_GAP);
                        let to = if a < top { Some(a) } else if b > top + view_h { Some(b - view_h) } else { None };
                        if let Some(to) = to {
                            self.side_scroll.set_offset(point(px(0.), px(-to.clamp(0., (tall - view_h).max(0.)))));
                        }
                    }
                }
                let top = -f32::from(self.side_scroll.offset().y);
                let first = places.partition_point(|p| p.1 + p.3 + THUMB_LABEL < top);
                let last = places.partition_point(|p| p.1 < top + view_h).max(first + 1).min(places.len());
                self.draw_thumbs(first..last, places.len(), window, cx);
                let mut column = div().relative().w(px(SIDE_W)).h(px(tall));
                for i in first.saturating_sub(1)..(last + 1).min(places.len()) {
                    let p = places[i];
                    let on = i == page;
                    let image = self.thumbs.get(&i).filter(|l| l.width != u32::MAX).map(|l| l.image.clone());
                    let ring = if on { theme.primary } else { theme.border };
                    column = column.child(
                        div()
                            .id(("embed-thumb", i))
                            .absolute()
                            .left(px(0.))
                            .top(px(p.1 - THUMB_GAP / 2.))
                            .w(px(SIDE_W))
                            .h(px(p.3 + THUMB_LABEL + THUMB_GAP))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |me, _, window, cx| {
                                swallow_click(window, cx);
                                window.focus(&me.focus, cx);
                                me.go_to(i, 0., false);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(p.0 - 1.))
                                    .top(px(THUMB_GAP / 2. - 1.))
                                    .w(px(p.2 + 2.))
                                    .h(px(p.3 + 2.))
                                    .rounded(px(3.))
                                    .border(px(if on { 2. } else { 1. }))
                                    .border_color(ring)
                                    .bg(gpui::white())
                                    .overflow_hidden()
                                    .when_some(image, |d, image| d.child(img(image).size_full())),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(px(0.))
                                    .top(px(THUMB_GAP / 2. + p.3 + 3.))
                                    .w(px(SIDE_W))
                                    .flex()
                                    .justify_center()
                                    .text_size(px(11.))
                                    .text_color(if on { theme.foreground } else { theme.muted_foreground })
                                    .child((i + 1).to_string()),
                            ),
                    );
                }
                column.into_any_element()
            }
        };
        let switch = outline.as_ref().map(|_| {
            let chosen = self.side_list;
            let pick = |id: &'static str, icon: &'static str, label: &'static str, list: List, cx: &mut Context<Self>| {
                self.button(id, icon, label, cx, move |me, _, _, cx| {
                    if me.side_list != list {
                        me.side_list = list;
                        me.followed = usize::MAX;
                        me.side_scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    }
                })
                .when(chosen == list, |d| d.bg(theme.muted))
            };
            h_flex()
                .h(px(SIDE_HEAD))
                .flex_shrink_0()
                .px(px(8.))
                .gap(px(2.))
                .items_center()
                .child(pick("embed-side-pages", "icons/layout-dashboard.svg", t("Thumbnails"), List::Pages, cx))
                .child(pick("embed-side-outline", "icons/list-bullets.svg", t("Outline"), List::Outline, cx))
        });
        div()
            .w(px(self.side_w()))
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.background)
            .on_mouse_down(MouseButton::Left, cx.listener(|me, _, window, cx| window.focus(&me.focus, cx)))
            .child(
                div()
                    .w(px(SIDE_W))
                    .h_full()
                    .flex()
                    .flex_col()
                    .children(switch)
                    .child(div().relative().flex_1().min_h_0().child(div().id("embed-side").size_full().overflow_y_scroll().track_scroll(&scroll).child(list)).vertical_scrollbar(&scroll)),
            )
            .into_any_element()
    }

    /// The outline's lines in view, a line folded hiding those under it.
    fn render_outline(&mut self, marks: Arc<Vec<PdfMark>>, top: f32, view_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        // A line alone at the top (the document's title) stands open,
        // whatever the file says, or the outline would be one line.
        let lone = marks.iter().filter(|m| m.depth == 0).count() == 1;
        let open_of = |i: usize, m: &PdfMark| self.folds.get(&i).copied().unwrap_or(m.open || (lone && m.depth == 0));
        let mut shown: Vec<usize> = Vec::new();
        let mut hide_below: Option<usize> = None;
        for (i, m) in marks.iter().enumerate() {
            if hide_below.is_some_and(|d| m.depth > d) {
                continue;
            }
            hide_below = None;
            shown.push(i);
            if m.kids && !open_of(i, m) {
                hide_below = Some(m.depth);
            }
        }
        let first = ((top / MARK_H).floor().max(0.) as usize).saturating_sub(2);
        let last = (((top + view_h) / MARK_H).ceil() as usize + 2).min(shown.len());
        let mut column = div().relative().w(px(SIDE_W)).h(px(shown.len() as f32 * MARK_H + 8.));
        for (row, &i) in shown.iter().enumerate().take(last).skip(first) {
            let m = &marks[i];
            let open = open_of(i, m);
            let to = m.page.map(|p| (p, m.down.unwrap_or(0.)));
            let hover = theme.muted;
            let chevron = div()
                .id(("embed-fold", i))
                .size(px(16.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .when(m.kids, |d| {
                    d.cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |me, _, window, cx| {
                            swallow_click(window, cx);
                            me.folds.insert(i, !open);
                            cx.notify();
                        }))
                        .child(Icon::default().path(if open { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" }).with_size(px(12.)).text_color(theme.muted_foreground))
                });
            column = column.child(
                h_flex()
                    .id(("embed-mark", i))
                    .absolute()
                    .left(px(4.))
                    .top(px(4. + row as f32 * MARK_H))
                    .w(px(SIDE_W - 8.))
                    .h(px(MARK_H))
                    .pl(px(2. + m.depth.min(4) as f32 * 10.))
                    .pr(px(6.))
                    .gap(px(2.))
                    .items_center()
                    .rounded(px(5.))
                    .text_size(px(12.))
                    .text_color(if to.is_some() { theme.foreground } else { theme.muted_foreground })
                    .when(to.is_some(), |d| d.cursor_pointer().hover(move |s| s.bg(hover)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |me, _, window, cx| {
                        swallow_click(window, cx);
                        window.focus(&me.focus, cx);
                        if let Some((page, down)) = to {
                            me.go_to(page, down, false);
                            cx.notify();
                        }
                    }))
                    .child(chevron)
                    .child(div().min_w_0().flex_1().truncate().child(m.title.clone()))
                    .when(m.title.chars().count() > 18, |d| {
                        let title: SharedString = m.title.clone().into();
                        d.managed_tooltip(move |window, cx| tip(title.clone(), window, cx))
                    }),
            );
        }
        column.into_any_element()
    }

    /// The place in the words under a point of the window.
    fn spot_at(&self, at: Point<Pixels>) -> Option<Spot> {
        let text = self.text()?;
        let sizes = self.sizes();
        let k = self.scale(&sizes);
        let (places, _, _) = self.places(&sizes, k);
        let origin = self.scroll.bounds().origin + self.scroll.offset();
        let (x, y) = (f32::from(at.x - origin.x), f32::from(at.y - origin.y));
        let page = places.partition_point(|p| p.1 + p.3 + GAP <= y).min(places.len().checked_sub(1)?);
        let p = places[page];
        let t = text.get(page)?;
        Some(Spot { page, at: t.place_at((x - p.0) / k, (y - p.1) / k) })
    }

    fn press(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        cx.stop_propagation();
        let Some(spot) = self.spot_at(ev.position) else {
            self.sel = None;
            cx.notify();
            return;
        };
        self.sel = Some(match ev.click_count {
            1 if ev.modifiers.shift => (self.sel.map_or(spot, |s| s.0), spot),
            1 => (spot, spot),
            _ => {
                let text = self.text();
                let (from, to) = text.as_ref().and_then(|t| t.get(spot.page)).map_or((spot.at, spot.at), |t| t.word_around(spot.at));
                (Spot { page: spot.page, at: from }, Spot { page: spot.page, at: to })
            }
        });
        self.selecting = ev.click_count == 1;
        cx.notify();
    }

    /// A right click on the pages: the keyboard to the frame, and its
    /// menu, Copy while words are selected.
    fn right_press(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        cx.stop_propagation();
        let menu = MenuAt { frame: Some(FrameMenu { focus: self.focus.clone(), selection: self.selected().is_some() }), ..Default::default() };
        let at = ev.position;
        if let Some(note) = self.note.upgrade() {
            cx.defer(move |cx| note.update(cx, |_, cx| cx.emit(EditorEvent::Menu(at, menu))));
        }
    }

    fn drag(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selecting || ev.pressed_button != Some(MouseButton::Left) {
            self.selecting = false;
            return;
        }
        if let (Some(spot), Some(sel)) = (self.spot_at(ev.position), self.sel.as_mut()) {
            if sel.1 != spot {
                sel.1 = spot;
                cx.notify();
            }
        }
    }

    /// The selection's words, a page's after the page before's.
    fn selected(&self) -> Option<String> {
        let (a, b) = self.sel?;
        let (a, b) = (a.min(b), a.max(b));
        if a == b {
            return None;
        }
        let text = self.text()?;
        let mut out = Vec::new();
        for page in a.page..=b.page {
            let t = text.get(page)?;
            let from = if page == a.page { a.at } else { 0 };
            let to = if page == b.page { b.at } else { t.glyphs.len() };
            out.push(t.words(from, to));
        }
        Some(out.join("\n"))
    }

    fn copy(&mut self, _: &gpui_component::input::Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(words) = self.selected() {
            crate::widgets::to_clipboard(words, cx);
        }
    }

    fn select_all(&mut self, _: &gpui_component::input::SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.text() else { return };
        let Some(last) = text.len().checked_sub(1) else { return };
        self.sel = Some((Spot { page: 0, at: 0 }, Spot { page: last, at: text[last].glyphs.len() }));
        cx.notify();
    }

    /// The PDF scrolled by the keys: a line, a frame, or to an end.
    fn scroll_by(&mut self, by: f32, cx: &mut Context<Self>) {
        let scroll = match &self.body {
            Body::Pdf => self.scroll.clone(),
            Body::Words(editor) => editor.read(cx).scroll.clone(),
            _ => return,
        };
        let o = scroll.offset();
        let y = (f32::from(o.y) - by).clamp(-f32::from(scroll.max_offset().y), 0.);
        scroll.set_offset(point(o.x, px(y)));
        cx.notify();
    }

    fn leave(&mut self, _: &Leave, window: &mut Window, cx: &mut Context<Self>) {
        self.sel = None;
        if let Some(note) = self.note.upgrade() {
            let focus = note.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    /// The field that says the page and takes one, made once.
    fn page_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(f) = &self.page_field {
            return f.clone();
        }
        let field = cx.new(|cx| {
            let mut f = InputState::new(window, cx);
            f.set_text_align(TextAlign::Center, cx);
            f
        });
        self._subs.push(cx.subscribe_in(&field, window, |me, field, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { .. } => {
                let typed = field.read(cx).value().trim().parse::<usize>().ok();
                if let Some(n) = typed.filter(|n| *n >= 1) {
                    let n = (n - 1).min(me.sizes().len().saturating_sub(1));
                    me.go_to(n, 0., false);
                    me.shown_page = usize::MAX;
                }
                window.focus(&me.focus, cx);
                cx.notify();
            }
            InputEvent::Blur => {
                me.shown_page = usize::MAX;
                cx.notify();
            }
            _ => {}
        }));
        self.page_field = Some(field.clone());
        field
    }

    /// The line of the note this frame or bar stands on.
    fn line(&self, cx: &App) -> Option<usize> {
        let note = self.note.upgrade()?;
        match &self.card {
            Some(card) => note.read(cx).line_of_card(&card.key),
            None => note.read(cx).line_of_frame(&(self.path.clone(), self.nth)),
        }
    }

    /// What the bar's pencil does: the caret in the line that embeds the
    /// file, which shows under it as written, so it can be changed there.
    fn edit_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(line), Some(note)) = (self.line(cx), self.note.upgrade()) else { return };
        note.update(cx, |e, cx| e.edit_object(line, window, cx));
    }

    /// A press in the frame: the whole selected in the note, the keyboard
    /// left where it is.
    fn mark_whole(&mut self, cx: &mut Context<Self>) {
        let (Some(line), Some(note)) = (self.line(cx), self.note.upgrade()) else { return };
        cx.defer(move |cx| note.update(cx, |e, cx| e.mark_object(line, cx)));
    }

    /// A press on the bar's empty part: the whole selected in the note.
    fn select_whole(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(line), Some(note)) = (self.line(cx), self.note.upgrade()) else { return };
        note.update(cx, |e, cx| e.select_object(line, window, cx));
    }

    /// The bar's menu: a picture's place and size, the file's own
    /// choices, and Delete last, as a right click on the piece gives it.
    fn more(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let line = self.line(cx);
        let Some(note) = self.note.upgrade() else { return };
        let menu = match line {
            Some(line) => note.read(cx).piece_menu(line, self.file()),
            None => MenuAt { file: self.file(), ..Default::default() },
        };
        cx.defer(move |cx| note.update(cx, |_, cx| cx.emit(EditorEvent::Menu(at, menu))));
    }

    /// The file under the bar: none for a card of a page of the web.
    fn file(&self) -> Option<PathBuf> {
        (!self.card.as_ref().is_some_and(|c| c.web)).then(|| self.path.clone())
    }

    /// A choice of a picture's bar, for the picture on its line; a card's
    /// place, for the card.
    fn picture_do(&mut self, what: &'static str, cx: &mut Context<Self>) {
        let (Some(line), Some(note)) = (self.line(cx), self.note.upgrade()) else { return };
        if self.card.is_some() {
            note.update(cx, |e, cx| e.layout_choice(line, 0, what, cx));
        } else {
            note.update(cx, |e, cx| e.picture_choice(line, what, cx));
        }
    }

    /// The name as a field, its words selected, the keyboard in it.
    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stem = self.path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let field = cx.new(|cx| InputState::new(window, cx));
        field.update(cx, |f, cx| {
            f.set_value(stem, window, cx);
            f.select_all(window, cx);
        });
        self._subs.push(cx.subscribe_in(&field, window, |me, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { .. } | InputEvent::Blur => me.finish_rename(true, window, cx),
            _ => {}
        }));
        self.renaming = Some(field.clone());
        // The field takes the keyboard once it is drawn: a focus given to
        // what is not drawn yet is lost at once, and the name kept.
        cx.on_next_frame(window, move |_, window, cx| window.focus(&field.read(cx).focus_handle(cx), cx));
        cx.notify();
    }

    /// The name taken, or let go: the file renamed by the window, which
    /// follows every link to it (`Shell::rename_entry`).
    fn finish_rename(&mut self, keep: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.renaming.take() else { return };
        let name = field.read(cx).value().trim().to_string();
        let stem = self.path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if keep && !name.is_empty() && name != stem {
            let ev = EditorEvent::Rename(self.path.clone(), name);
            if let Some(note) = self.note.upgrade() {
                cx.defer(move |cx| note.update(cx, |_, cx| cx.emit(ev)));
            }
        }
        if field.read(cx).focus_handle(cx).is_focused(window) {
            if let Some(note) = self.note.upgrade() {
                let focus = note.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
        }
        cx.notify();
    }

    fn render_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let sizes = self.sizes();
        if sizes.is_empty() {
            return self.render_not(t("This PDF could not be read.").to_string().into(), cx);
        }
        let sliding = self.sliding();
        if sliding {
            window.request_animation_frame();
        }
        let k = self.scale(&sizes);
        let (places, cw, ch) = self.places(&sizes, k);
        // A scale the zoom did not set (the sidebar sliding, the frame
        // grown) keeps the same place at the top.
        if let Some(was) = self.last_k.filter(|was| (was - k).abs() > 1e-4) {
            let (bw, bh) = self.body_size();
            let o = self.scroll.offset();
            let r = k / was;
            let x = (-f32::from(o.x) * r).clamp(0., (cw - bw).max(0.));
            let y = (-f32::from(o.y) * r).clamp(0., (ch - bh).max(0.));
            self.scroll.set_offset(point(px(-x), px(-y)));
        }
        self.last_k = Some(k);
        if let Some((page, down)) = self.want.take() {
            self.go_to(page, down, true);
        }
        let showing = self.showing(&places);
        // While the sidebar slides the pages are only scaled; they are
        // drawn again at their size once it stops.
        if !sliding || self.leaves.is_empty() {
            self.draw_pages(&places, showing.clone(), k * window.scale_factor(), window, cx);
        }
        // The page the field says: the one a third of the way down.
        let (_, bh) = self.body_size();
        let mark = -f32::from(self.scroll.offset().y) + bh / 3.;
        let page = places.partition_point(|p| p.1 + p.3 + GAP <= mark).min(places.len() - 1);
        if page != self.shown_page {
            let field = self.page_field(window, cx);
            if !field.read(cx).focus_handle(cx).is_focused(window) {
                field.update(cx, |f, cx| f.set_value((page + 1).to_string(), window, cx));
                self.shown_page = page;
            }
        }
        // What the canvas paints: the pages near the view, the selection
        // and the find's places over them.
        let text = self.text();
        let near = showing.start.saturating_sub(1)..(showing.end + 1).min(places.len());
        let leaves: Vec<(usize, Option<Arc<RenderImage>>)> = near.clone().map(|p| (p, self.leaves.get(&p).filter(|l| l.width != u32::MAX).map(|l| l.image.clone()))).collect();
        let mut washes: Vec<(f32, f32, f32, f32, Hsla)> = Vec::new();
        let boxes = |page: usize, from: usize, to: usize, ink: Hsla, out: &mut Vec<(f32, f32, f32, f32, Hsla)>| {
            let (Some(t), Some(p)) = (text.as_ref().and_then(|t| t.get(page)), places.get(page)) else { return };
            if to <= from {
                return;
            }
            for b in t.boxes(from, to - 1) {
                out.push((p.0 + b.0 * k, p.1 + b.1 * k, b.2 * k, b.3 * k, ink));
            }
        };
        if let Some(note) = self.note.upgrade() {
            for (page, from, to, on) in note.read(cx).marks_of(&self.path, self.nth) {
                if near.contains(&page) {
                    boxes(page, from, to + 1, if on { theme.selection } else { theme.selection.opacity(0.45) }, &mut washes);
                }
            }
        }
        if let Some((a, b)) = self.sel {
            let (a, b) = (a.min(b), a.max(b));
            for page in a.page.max(near.start)..=b.page.min(near.end.saturating_sub(1)) {
                let len = text.as_ref().and_then(|t| t.get(page)).map_or(0, |t| t.glyphs.len());
                boxes(page, if page == a.page { a.at } else { 0 }, if page == b.page { b.at } else { len }, theme.selection, &mut washes);
            }
        }
        let paper = gpui::white();
        let edge = theme.border;
        let paint_places = places.clone();
        let canvas = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let o = bounds.origin;
                for (page, image) in &leaves {
                    let p = paint_places[*page];
                    let b = Bounds::new(o + point(px(p.0), px(p.1)), size(px(p.2), px(p.3)));
                    window.paint_quad(quad(b, px(2.), paper, px(1.), edge, BorderStyle::default()));
                    if let Some(image) = image {
                        let _ = window.paint_image(b, b, Corners::all(px(2.)), image.clone(), 0, false);
                    }
                }
                for (x, y, w, h, ink) in &washes {
                    window.paint_quad(fill(Bounds::new(o + point(px(*x), px(*y)), size(px(*w), px(*h))), *ink));
                }
            },
        )
        .size_full();
        let scroll = self.scroll.clone();
        if let Some(router) = cx.try_global::<crate::scroll::Router>() {
            router.0.zooms(&scroll);
        }
        let side = (self.side_open || sliding).then(|| self.render_side(&sizes, page, window, cx));
        let worded = text.as_ref().is_some_and(|t| t.iter().any(|p| !p.glyphs.is_empty()));
        let pages = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme.muted)
            .child(
                div()
                    .id("embed-pages")
                    .size_full()
                    .overflow_scroll()
                    .track_scroll(&scroll)
                    .child(
                        div()
                            .w(px(cw))
                            .h(px(ch))
                            .when(worded, |d| d.cursor(CursorStyle::IBeam))
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::press))
                            .on_mouse_down(MouseButton::Right, cx.listener(Self::right_press))
                            .on_mouse_move(cx.listener(Self::drag))
                            .on_scroll_wheel(cx.listener(Self::wheel))
                            .on_pinch(cx.listener(Self::pinch))
                            .child(canvas),
                    ),
            )
            .vertical_scrollbar(&scroll);
        h_flex().size_full().children(side).child(pages).into_any_element()
    }

    fn render_words(&mut self, editor: Entity<Editor>, cx: &mut Context<Self>) -> AnyElement {
        let scroll = editor.read(cx).scroll.clone();
        if let Some(router) = cx.try_global::<crate::scroll::Router>() {
            router.0.name(&scroll);
        }
        div()
            .relative()
            .size_full()
            .child(div().id("embed-words").size_full().overflow_y_scroll().track_scroll(&scroll).px(px(WORDS_PAD_X)).pt(px(WORDS_PAD_Y)).pb(px(WORDS_FOOT)).child(editor))
            .vertical_scrollbar(&scroll)
            .into_any_element()
    }

    fn render_not(&self, why: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        div().size_full().flex().items_center().justify_center().px(px(16.)).text_size(px(12.5)).text_color(theme.muted_foreground).child(why).into_any_element()
    }

    fn button(&self, id: &'static str, icon: &'static str, label: &'static str, cx: &mut Context<Self>, press: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static) -> Stateful<Div> {
        self.toggle(id, icon, label, false, true, cx, press)
    }

    /// A button of the bar that may be the one chosen, or not to be had.
    #[allow(clippy::too_many_arguments)]
    fn toggle(&self, id: &'static str, icon: &'static str, label: &'static str, on: bool, able: bool, cx: &mut Context<Self>, press: impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static) -> Stateful<Div> {
        let theme = cx.theme();
        let hover = theme.muted;
        let ink = if on {
            theme.primary
        } else if able {
            theme.muted_foreground
        } else {
            theme.muted_foreground.opacity(0.4)
        };
        div()
            .id(id)
            .size(px(BUTTON))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .when(on, |d| d.bg(hover))
            .managed_tooltip(move |window, cx| tip(label, window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(able, |d| {
                d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |me, ev, window, cx| {
                    swallow_click(window, cx);
                    press(me, ev, window, cx);
                }))
            })
            .child(Icon::default().path(icon).with_size(px(14.)).text_color(ink))
    }

    /// A share on the bar, a PDF's zoom or a picture's size: a press sets
    /// it back as `back` says.
    fn share(&self, id: &'static str, words: String, accent: bool, label: &'static str, cx: &mut Context<Self>, back: impl Fn(&mut Self, &mut Context<Self>) + 'static) -> Stateful<Div> {
        let theme = cx.theme();
        let hover = theme.muted;
        div()
            .id(id)
            .min_w(px(42.))
            .h(px(BUTTON))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .text_size(px(12.))
            .text_color(if accent { theme.primary } else { theme.muted_foreground })
            .managed_tooltip(move |window, cx| tip(label, window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |me, _, window, cx| {
                swallow_click(window, cx);
                back(me, cx);
            }))
            .child(words)
    }

    /// The bar every file has, a picture's as a frame's: the file's icon
    /// and name, which a click renames, then what its kind has, the pencil
    /// and the menu. A bar is as wide as its file, so what it holds goes
    /// by its room: the menu always, then a PDF's page, View, the size,
    /// the pencil, the place, the sidebar's button, the icon, and the name
    /// while a few letters of it fit.
    fn render_head(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use onyx_core::html::Across;
        let theme = cx.theme().clone();
        let web = self.card.as_ref().is_some_and(|c| c.web);
        let name = if web { onyx_core::link_card::host(&self.path.to_string_lossy()) } else { self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() };
        let icon: SharedString = if web { "icons/globe.svg".into() } else { crate::file_icons::path(&name, false, false, theme.mode.is_dark()) };
        let changes = self.note.upgrade().is_some_and(|n| !n.read(cx).locked());
        // A card's bar stands over its card as a picture's over its picture,
        // but it views nothing and its name is a page's when it is the web's.
        let card_across = self.card.as_ref().and_then(|c| c.across);
        let picture = matches!(self.body, Body::Picture) && self.card.is_none();
        let pdf = matches!(self.body, Body::Pdf) && !self.sizes().is_empty();
        let facts = self.line(cx).and_then(|l| self.note.upgrade().and_then(|n| n.read(cx).picture_facts(l)));
        // The room left for the rest: the bar's less its edges and the menu.
        let mut room = self.size.0 - 2. - 20. - BUTTON;
        let mut take = |need: f32| {
            let fits = room >= need;
            if fits {
                room -= need;
            }
            fits
        };
        let page = pdf && take(84.);
        let view = picture && take(BUTTON + 2.);
        let size = changes && facts.is_some() && take(48.);
        let pencil = changes && take(BUTTON + 11.);
        let place = changes && (facts.is_some() || card_across.is_some()) && take(BUTTON * 3. + 17.);
        let side = pdf && take(BUTTON + 7.);
        let iconed = take(22.);
        let named = room >= 40.;
        let rule = || div().w(px(1.)).h(px(14.)).mx(px(4.)).bg(theme.border);
        let mut right = h_flex().gap(px(2.)).flex_shrink_0().items_center();
        if page {
            let field = self.page_field(window, cx);
            let count = self.sizes().len();
            right = right
                .child(div().w(px(40.)).h(px(22.)).px(px(3.)).flex().items_center().rounded(px(5.)).bg(theme.background).border_1().border_color(theme.input).text_size(px(12.)).child(Input::new(&field).appearance(false).bordered(false).px(px(0.))))
                .child(div().pl(px(5.)).pr(px(2.)).text_size(px(12.)).text_color(theme.muted_foreground).child(format!("/ {count}")));
            if place || size || pencil {
                right = right.child(rule());
            }
        }
        if let Some(f) = facts {
            if place {
                let able = !f.full;
                right = right
                    .child(self.toggle("embed-left", "icons/align-left.svg", t("Align Left"), able && f.across == Across::Start, able, cx, |me, _, _, cx| me.picture_do("left", cx)))
                    .child(self.toggle("embed-center", "icons/align-center.svg", t("Align Center"), able && f.across == Across::Center, able, cx, |me, _, _, cx| me.picture_do("center", cx)))
                    .child(self.toggle("embed-right", "icons/align-right.svg", t("Align Right"), able && f.across == Across::End, able, cx, |me, _, _, cx| me.picture_do("right", cx)))
                    .child(rule());
            }
            if size {
                // A PDF zoomed in its frame says the zoom; else, as a
                // picture, its width against its own. A press: 100%.
                let share = match self.zoom.filter(|_| pdf) {
                    Some(_) => (self.scale(&self.sizes()) * 100.).round() as u32,
                    None => f.share,
                };
                right = right.child(self.share("embed-size", format!("{share}%"), share == 100, t("Original Size"), cx, |me, cx| {
                    if me.zoom.is_some() {
                        me.set_zoom(None, None, cx);
                    }
                    me.picture_do("auto", cx);
                }));
            }
        }
        if let (Some(across), true) = (card_across, place) {
            right = right
                .child(self.toggle("embed-left", "icons/align-left.svg", t("Align Left"), across == Across::Start, true, cx, |me, _, _, cx| me.picture_do("left", cx)))
                .child(self.toggle("embed-center", "icons/align-center.svg", t("Align Center"), across == Across::Center, true, cx, |me, _, _, cx| me.picture_do("center", cx)))
                .child(self.toggle("embed-right", "icons/align-right.svg", t("Align Right"), across == Across::End, true, cx, |me, _, _, cx| me.picture_do("right", cx)))
                .child(rule());
        }
        if view {
            right = right.child(self.button("embed-view", "icons/maximize.svg", t("View"), cx, |me, _, _, cx| me.picture_do("view", cx)));
        }
        if pencil {
            if size || view {
                right = right.child(rule());
            }
            right = right.child(self.button("embed-edit", "icons/pencil-simple.svg", t("Edit Link"), cx, |me, _, window, cx| me.edit_line(window, cx)));
        }
        right = right.child(self.button("embed-more", "icons/ellipsis.svg", t("More"), cx, |me, ev, _, cx| me.more(ev.position(), cx)));
        let side = side.then(|| self.button("embed-side", "icons/panel-left.svg", t("Toggle Sidebar"), cx, |me, _, _, cx| me.toggle_side(cx)).ml(px(-4.)).when(self.side_open, |d| d.bg(theme.muted)));
        // The name, or the field it is renamed in.
        let label = match self.renaming.clone() {
            Some(field) => {
                let ext = self.path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                h_flex()
                    .id("embed-rename")
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_action(cx.listener(|me, _: &Leave, window, cx| me.finish_rename(false, window, cx)))
                    .child(div().flex_1().min_w_0().h(px(22.)).px(px(4.)).flex().items_center().rounded(px(5.)).bg(theme.background).border_1().border_color(theme.ring).text_size(px(12.5)).child(Input::new(&field).appearance(false).bordered(false).px(px(0.))))
                    .child(div().flex_shrink_0().pl(px(3.)).text_size(px(12.5)).text_color(theme.muted_foreground).child(ext))
                    .into_any_element()
            }
            None if named => {
                let hover = theme.muted;
                div()
                    .id("embed-name")
                    .min_w_0()
                    .px(px(4.))
                    .mx(px(-4.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(theme.muted_foreground)
                    .when(changes && !web, |d| {
                        d.cursor(CursorStyle::IBeam)
                            .hover(move |s| s.bg(hover))
                            .managed_tooltip(|window, cx| tip(t("Rename"), window, cx))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|me, _, window, cx| {
                                swallow_click(window, cx);
                                me.start_rename(window, cx);
                            }))
                    })
                    .child(name)
                    .into_any_element()
            }
            None => div().into_any_element(),
        };
        // Over a picture the bar is its top; in a frame, inside its border.
        let round = if picture { CONTROL_ROUND } else { CONTROL_ROUND - 1. };
        h_flex()
            .id("embed-head")
            .occlude()
            .w_full()
            .h(px(HEAD_H))
            .flex_shrink_0()
            .px(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded_t(px(round))
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.background.opacity(0.94))
            // The bar's empty part: the whole, selected in the note; a right
            // click, its menu.
            .on_mouse_down(MouseButton::Left, cx.listener(|me, _, window, cx| {
                cx.stop_propagation();
                me.select_whole(window, cx);
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(|me, ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                me.select_whole(window, cx);
                me.more(ev.position, cx);
            }))
            .child(h_flex().flex_1().min_w_0().gap(px(7.)).items_center().children(side).when(iconed, |d| d.child(Icon::default().path(icon).with_size(px(14.)).flex_shrink_0())).child(label))
            .child(right)
            .into_any_element()
    }
}

impl Focusable for Embed {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Embed {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let alpha = self.bar_alpha();
        let head = (alpha > 0.).then(|| self.render_head(window, cx));
        let hover = cx.listener(|me: &mut Self, on: &bool, _, cx| {
            me.hovered = *on;
            cx.notify();
        });
        // A picture's view is its bar: the note paints the picture under.
        if matches!(self.body, Body::Picture) {
            return div().id("embed").key_context(EMBED_CONTEXT).track_focus(&self.focus).size_full().opacity(alpha).on_hover(hover).on_action(cx.listener(Self::leave)).children(head).into_any_element();
        }
        let body = match &self.body {
            Body::Picture => div().into_any_element(),
            Body::Pdf => self.render_pdf(window, cx),
            Body::Words(editor) => {
                let editor = editor.clone();
                self.render_words(editor, cx)
            }
            Body::Reading => div().size_full().into_any_element(),
            Body::Not(why) => {
                let why = why.clone();
                self.render_not(why, cx)
            }
        };
        div()
            .id("embed")
            .key_context(EMBED_CONTEXT)
            .track_focus(&self.focus)
            .occlude()
            .on_hover(hover)
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(CONTROL_ROUND))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            // A press anywhere in it selects the whole in the note too, the
            // keyboard staying here for its words.
            .capture_any_mouse_down(cx.listener(|me, _, _, cx| me.mark_whole(cx)))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::leave))
            .on_action(cx.listener(|_, _: &Still, _, _| {}))
            .on_action(cx.listener(|me, _: &LineUp, _, cx| me.scroll_by(-48., cx)))
            .on_action(cx.listener(|me, _: &LineDown, _, cx| me.scroll_by(48., cx)))
            .on_action(cx.listener(|me, _: &PageUpIn, _, cx| me.scroll_by(-(me.body_size().1 - 40.), cx)))
            .on_action(cx.listener(|me, _: &PageDownIn, _, cx| me.scroll_by(me.body_size().1 - 40., cx)))
            .on_action(cx.listener(|me, _: &ToStart, _, cx| me.scroll_by(-1e7, cx)))
            .on_action(cx.listener(|me, _: &ToEnd, _, cx| me.scroll_by(1e7, cx)))
            .on_action(cx.listener(|me, _: &crate::shell::ZoomIn, _, cx| me.step_zoom(true, cx)))
            .on_action(cx.listener(|me, _: &crate::shell::ZoomOut, _, cx| me.step_zoom(false, cx)))
            .on_action(cx.listener(|me, _: &crate::shell::ZoomReset, _, cx| me.set_zoom(None, None, cx)))
            .child(div().size_full().child(body))
            .children(head.map(|head| div().absolute().top_0().left_0().w_full().opacity(alpha).child(head)))
            .child(corners(theme.background, CONTROL_ROUND - 1.))
            .into_any_element()
    }
}

/// What stands outside a frame's round corners, in the sheet's color: a
/// view clips its children to a square, so a page that reaches the frame's
/// edge would show past its corners. It stands inside the border, round
/// as the border's inside is (`round`), and the border is drawn after.
/// A card's picture is the same (`card.rs`).
pub(crate) fn corners(sheet: Hsla, round: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, _| {
            let r = px(round);
            let (x0, y0, x1, y1) = (b.left(), b.top(), b.right(), b.bottom());
            // Each corner, and its two neighbors along the edges; the arc
            // between them is a cubic's quarter circle.
            const K: f32 = 0.552_284_8;
            for (c, a, e) in [
                (point(x0, y0), point(x0 + r, y0), point(x0, y0 + r)),
                (point(x1, y0), point(x1 - r, y0), point(x1, y0 + r)),
                (point(x1, y1), point(x1, y1 - r), point(x1 - r, y1)),
                (point(x0, y1), point(x0, y1 - r), point(x0 + r, y1)),
            ] {
                let mut path = PathBuilder::fill();
                path.move_to(c);
                path.line_to(a);
                path.cubic_bezier_to(e, a + (c - a) * K, e + (c - e) * K);
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, sheet);
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The keys a frame takes while the keyboard is in it. A frame stands
/// in the note's editor, so a key the editor binds would reach the note
/// through it: every key that changes the note is taken here and does
/// nothing, and those that move scroll the frame.
pub(crate) fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(EMBED_CONTEXT);
    let mut keys = vec![
        KeyBinding::new("secondary-c", gpui_component::input::Copy, c),
        KeyBinding::new("secondary-a", gpui_component::input::SelectAll, c),
        KeyBinding::new("escape", Leave, c),
        KeyBinding::new("up", LineUp, c),
        KeyBinding::new("down", LineDown, c),
        KeyBinding::new("pageup", PageUpIn, c),
        KeyBinding::new("pagedown", PageDownIn, c),
        KeyBinding::new("space", PageDownIn, c),
        KeyBinding::new("shift-space", PageUpIn, c),
        KeyBinding::new("secondary-up", ToStart, c),
        KeyBinding::new("secondary-down", ToEnd, c),
        KeyBinding::new("home", ToStart, c),
        KeyBinding::new("end", ToEnd, c),
    ];
    for key in [
        "enter", "shift-enter", "tab", "shift-tab", "backspace", "shift-backspace", "delete", "alt-backspace", "secondary-backspace", "left", "right", "shift-left", "shift-right", "shift-up", "shift-down", "alt-left", "alt-right", "alt-shift-left", "alt-shift-right", "secondary-left", "secondary-right", "secondary-shift-left", "secondary-shift-right", "shift-home", "shift-end", "secondary-shift-up", "secondary-shift-down", "secondary-b", "secondary-i", "secondary-enter", "secondary-x", "secondary-v", "secondary-z", "secondary-shift-z",
    ] {
        keys.push(KeyBinding::new(key, Still, c));
    }
    keys
}
