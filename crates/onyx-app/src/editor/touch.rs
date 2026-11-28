//! A finger in the words on an iPhone and an iPad, as the system's own
//! text views have it: a finger held on the words picks up the caret, the
//! loupe over it, and lets it go where it lifts (in a note that reads
//! only, it takes the word and runs by words); the selection's two ends
//! have handles a finger drags, the loupe over the end it moves; and a
//! double or triple tap, a hold let go, a handle let go or a tap on the
//! caret brings the system's edit menu over the selection (`sys::edit_menu`), whose choices come back as actions
//! (`Shell`'s listener: Cut, Copy, Paste, `SelectWord`, `SelectEverything`).
//! A hold on a link, a picture, a box or a table's handle is not the
//! words' and keeps the app's own menu.

use gpui::*;

use super::{CaretAt, Editor, EditorEvent, Grain, Hit};
use crate::lines::{Pos, Sel};

/// What a finger has hold of.
#[derive(Clone, Copy, Debug)]
pub(super) enum Grab {
    /// A held finger: the caret under it, or, where the words take no
    /// change, the selection by words from the word it landed on.
    Hold { word: Option<(Pos, Pos)> },
    /// An end of the selection, the finger this far under the middle of
    /// its line (the end goes by what is under the handle, not the tip).
    Start { dy: f32 },
    End { dy: f32 },
}

/// How far from a handle's knob a finger takes it.
const REACH: f32 = 24.;
/// A knob's size, as the system draws a selection's handles.
const KNOB: f32 = 10.;

impl Editor {
    /// The selection's two handles in the window: each end's
    /// bar, the start's knob above it and the end's below.
    pub(super) fn handle_bars(&self) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
        if !cfg!(target_os = "ios") || !self.focused || self.sel.is_caret() {
            return None;
        }
        let (a, b) = self.sel.range();
        let bar = |c: CaretAt| Bounds::new(point(self.origin.x + px(c.x - 1.), self.origin.y + px(c.bar_y)), size(px(2.), px(c.bar_h)));
        Some((bar(self.caret_at(a)?), bar(self.caret_at(b)?)))
    }

    /// The edit menu's place in the window: over the selection, or the caret.
    fn menu_rect(&self) -> Option<Bounds<Pixels>> {
        let (a, b) = self.sel.range();
        let (ca, cb) = (self.caret_at(a)?, self.caret_at(b)?);
        let (x0, x1) = (ca.x.min(cb.x), ca.x.max(cb.x).max(ca.x.min(cb.x) + 2.));
        let (top, foot) = (ca.bar_y, cb.bar_y + cb.bar_h);
        Some(Bounds::new(point(self.origin.x + px(x0), self.origin.y + px(top)), size(px(x1 - x0), px(foot - top))))
    }

    /// The choices the menu offers here.
    fn menu_choices(&self, cx: &App) -> Vec<crate::sys::EditChoice> {
        use crate::sys::EditChoice::*;
        let pastes = !self.locked() && cx.read_from_clipboard().and_then(|c| c.text()).is_some_and(|t| !t.is_empty());
        let mut choices = Vec::new();
        if self.sel.is_caret() {
            if !self.doc.end().eq(&Pos::default()) {
                choices.extend([Select, SelectAll]);
            }
        } else {
            if !self.locked() {
                choices.push(Cut);
            }
            choices.push(Copy);
        }
        if pastes {
            choices.push(Paste);
        }
        if !self.sel.is_caret() && self.sel.range() != (Pos::default(), self.doc.end()) {
            choices.push(SelectAll);
        }
        choices
    }

    /// At each frame: the menu asked for is shown, and the menu showing
    /// follows what it stands over or goes when that changed.
    pub(super) fn settle_menu(&mut self, window: &Window, cx: &App) {
        if !cfg!(target_os = "ios") {
            return;
        }
        if self.grab.is_some() || !self.focused {
            if self.menu_for.take().is_some() {
                crate::sys::edit_menu_hide();
            }
            if !self.focused {
                self.menu_wanted = false;
            }
            return;
        }
        if std::mem::take(&mut self.menu_wanted) {
            let choices = self.menu_choices(cx);
            if let (Some(at), false) = (self.menu_rect(), choices.is_empty()) {
                if super::tracing() {
                    super::stamp(&format!("edit menu at {at:?} for {:?}", self.sel));
                }
                crate::sys::edit_menu(window, at, choices);
                self.menu_for = Some(self.sel);
            }
            return;
        }
        if let Some(sel) = self.menu_for {
            if sel != self.sel {
                crate::sys::edit_menu_hide();
                self.menu_for = None;
            } else if let Some(at) = self.menu_rect() {
                crate::sys::edit_menu_follow(at);
            }
        }
    }

    /// A tap that placed the selection: a double or a triple one asks for
    /// the menu; one on the caret where it stood shows the menu or puts it
    /// away; any other puts it away.
    pub(super) fn tapped(&mut self, taps: usize, before: Sel, was_focused: bool) {
        if !cfg!(target_os = "ios") {
            return;
        }
        if super::tracing() {
            super::stamp(&format!("tapped {taps} {:?} -> {:?} focused {was_focused}", before, self.sel));
        }
        if taps > 1 {
            self.menu_wanted = true;
        } else if was_focused && before.is_caret() && self.sel == before {
            if crate::sys::edit_menu_shown() {
                crate::sys::edit_menu_hide();
                self.menu_for = None;
            } else {
                self.menu_wanted = true;
            }
        } else if self.menu_for.take().is_some() {
            crate::sys::edit_menu_hide();
        }
    }

    /// The caret's bar at `p`, in the window.
    fn caret_in_window(&self, p: Pos) -> Option<Bounds<Pixels>> {
        let c = self.caret_at(p)?;
        Some(Bounds::new(point(self.origin.x + px(c.x), self.origin.y + px(c.bar_y)), size(px(2.), px(c.bar_h))))
    }

    /// A finger held at (`x`, `y`) from the element's top left: the words'
    /// when it is on words and nothing else, which it then takes.
    fn hold_begins(&mut self, x: f32, y: f32, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.grab.is_some() || self.control_at(x, y).is_some() || self.picture_at(x, y).is_some() {
            return false;
        }
        if matches!(self.hit(x, y), Some(Hit::Link(_)) | Some(Hit::Task(_))) {
            return false;
        }
        let p = if self.under_words(y) { Some(self.doc.end()) } else { self.pos_at(x, y, &self.cache.borrow()) };
        let Some(p) = p else { return false };
        cx.emit(EditorEvent::Pressed);
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        self.menu_wanted = false;
        self.pressed_link = None;
        self.preferred_x = None;
        if self.locked() {
            let (a, b) = self.grain_range(p, Grain::Word);
            self.sel = Sel { anchor: a, head: b };
            self.grab = Some(Grab::Hold { word: Some((a, b)) });
        } else {
            self.sel = Sel::caret(p);
            self.grab = Some(Grab::Hold { word: None });
            window.request_virtual_keyboard();
        }
        crate::sys::loupe(window, self.caret_in_window(self.sel.head).map(|c| (at, c)));
        self.kick_blink(cx);
        cx.notify();
        true
    }

    /// A finger on a handle, as it lands: taken when it is.
    fn drag_begins(&mut self, x: f32, y: f32, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.grab.is_some() || !self.focus.is_focused(window) {
            return false;
        }
        let near = |cx_: f32, cy: f32| (x - cx_).hypot(y - cy);
        // A caret is not taken at its landing: a second tap of a double
        // one lands where the first put it. A held finger moves it.
        if self.sel.is_caret() {
            return false;
        }
        let (a, b) = self.sel.range();
        let (Some(ca), Some(cb)) = (self.caret_at(a), self.caret_at(b)) else { return false };
        // The knob or the bar under it, whichever end is nearer.
        let ds = near(ca.x, (ca.bar_y - KNOB / 2.).max(y.min(ca.bar_y + ca.bar_h)));
        let de = near(cb.x, (cb.bar_y + cb.bar_h + KNOB / 2.).min(y.max(cb.bar_y)));
        if ds.min(de) > REACH {
            return false;
        }
        let (grab, end) = if ds < de { (Grab::Start { dy: ca.bar_y + ca.bar_h / 2. - y }, a) } else { (Grab::End { dy: cb.bar_y + cb.bar_h / 2. - y }, b) };
        self.grab = Some(grab);
        crate::sys::loupe(window, self.caret_in_window(end).map(|c| (at, c)));
        cx.notify();
        true
    }

    /// The finger that has hold moved to (`x`, `y`).
    fn grab_moves(&mut self, x: f32, y: f32, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(grab) = self.grab else { return };
        let dy = match grab {
            Grab::Start { dy } | Grab::End { dy } => dy,
            Grab::Hold { .. } => 0.,
        };
        let Some(p) = self.pos_at(x, (y + dy).max(0.), &self.cache.borrow()) else { return };
        let sel = match grab {
            Grab::Hold { word: None } => Sel::caret(p),
            Grab::Hold { word: Some((a, b)) } => {
                let (pa, pb) = self.grain_range(p, Grain::Word);
                let (anchor, head) = if pa < a { (b, pa) } else { (a, pb.max(b)) };
                Sel { anchor, head: self.held_to(anchor, head) }
            }
            // An end does not pass the other: the selection keeps a letter.
            Grab::Start { .. } => {
                let end = self.sel.end();
                if p >= end {
                    return;
                }
                Sel { anchor: end, head: self.held_to(end, p) }
            }
            Grab::End { .. } => {
                let start = self.sel.start();
                if p <= start {
                    return;
                }
                Sel { anchor: start, head: self.held_to(start, p) }
            }
        };
        crate::sys::loupe(window, self.caret_in_window(sel.head).map(|c| (at, c)));
        if sel != self.sel {
            self.sel = sel;
            self.want_scroll = true;
            self.kick_blink(cx);
            cx.notify();
        }
    }

    /// The finger that had hold lifted: the menu over what it left.
    fn grab_ends(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.grab.take().is_none() {
            return;
        }
        crate::sys::loupe(window, None);
        self.menu_wanted = true;
        cx.notify();
    }
}

/// The finger's listeners for an editor painted at `bounds`: a hold in the
/// words (`hitbox`) and a drag of a handle or the caret (anywhere in
/// `wide`, a handle's knob standing a little past the words).
pub(super) fn listen(editor: Entity<Editor>, bounds: Bounds<Pixels>, hitbox: Hitbox, wide: Hitbox, window: &mut Window) {
    let local = move |p: Point<Pixels>| (f32::from(p.x - bounds.origin.x), f32::from(p.y - bounds.origin.y));
    let ed = editor.clone();
    window.on_mouse_event(move |ev: &LongPressEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        match ev.phase {
            TouchPhase::Started => {
                if window.default_prevented() || !hitbox.is_hovered(window) {
                    return;
                }
                let (x, y) = local(ev.start_position);
                if ed.update(cx, |e, cx| e.hold_begins(x, y, ev.start_position, window, cx)) {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }
            TouchPhase::Moved => {
                let (x, y) = local(ev.position);
                ed.update(cx, |e, cx| e.grab_moves(x, y, ev.position, window, cx));
            }
            TouchPhase::Ended | TouchPhase::Cancelled => ed.update(cx, |e, cx| e.grab_ends(window, cx)),
        }
    });
    let ed = editor;
    window.on_mouse_event(move |ev: &TouchDragEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        match ev.phase {
            TouchPhase::Started => {
                if window.default_prevented() || !wide.is_hovered(window) {
                    return;
                }
                let (x, y) = local(ev.start_position);
                if ed.update(cx, |e, cx| e.drag_begins(x, y, ev.start_position, window, cx)) {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }
            TouchPhase::Moved => {
                let (x, y) = local(ev.position);
                ed.update(cx, |e, cx| e.grab_moves(x, y, ev.position, window, cx));
            }
            TouchPhase::Ended | TouchPhase::Cancelled => ed.update(cx, |e, cx| e.grab_ends(window, cx)),
        }
    });
}

/// The selection's handles, as the system draws them: a bar at each end
/// as tall as the caret, a round knob over the start and under the end.
pub(super) fn paint_handles(bars: (Bounds<Pixels>, Bounds<Pixels>), ink: Hsla, window: &mut Window) {
    let (a, b) = bars;
    for (bar, top) in [(a, true), (b, false)] {
        window.paint_quad(fill(bar, ink));
        let cy = if top { bar.top() - px(KNOB / 2. - 1.) } else { bar.bottom() + px(KNOB / 2. - 1.) };
        let knob = Bounds::new(point(bar.center().x - px(KNOB / 2.), cy - px(KNOB / 2.)), size(px(KNOB), px(KNOB)));
        window.paint_quad(fill(knob, ink).corner_radii(px(KNOB / 2.)));
    }
}
