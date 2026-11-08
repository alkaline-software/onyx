//! A scroll gesture belongs to the scroller it began in, its momentum
//! included, wherever the pointer is carried, and stops at that
//! scroller's edge.
//!
//! macOS goes on sending wheel events after the finger lifts, addressed
//! to wherever the pointer is by then, and gpui hands each to the
//! scroller under it. A momentum event is only `Moved`, so a flick in one
//! list followed by a move onto another scrolled the second. The router
//! runs in the capture phase, before any scroller: the scroller under the
//! pointer when a gesture begins (its `Started` phase, or the first event
//! after a short gap, since a mouse wheel sends no phases) owns it, and
//! every event of the gesture is applied to the owner, held to its ends,
//! and stopped.
//!
//! Every scroller that goes up and down names itself as it is drawn
//! (`name`). One that does not leaks: a flick in it scrolls whatever the
//! pointer reaches next. A sideways scroller is not named.
//!
//! A scroller inside another (a frame in a note, `embed.rs`) owns a
//! gesture begun over it only while it can move the gesture's way: one
//! already at its end hands a new gesture to the scroller around it, as
//! a page in a browser takes the wheel from a frame scrolled to its end.
//! A gesture under way stays where it began. A scroller that moves both
//! ways is moved both ways.
//!
//! A frame's pages zoom by ⌘ and the wheel, as a browser's page does: a
//! scroller that names itself by `zooms` too lets such an event pass to
//! its own listener, and nothing scrolls.
//!
//! While something stands over the window (a sheet, the search, the
//! viewer, a menu, a fly-out, or a panel of the system's), nothing under
//! it scrolls: it says so as it is drawn (`over`), names its own scrollers
//! with `name_over`, and the wheel goes to those alone, or nowhere.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{point, px, Global, Pixels, ScrollHandle, ScrollWheelEvent, TouchPhase};

/// The window's scrollers, for a view the window does not draw itself
/// (a frame in a note) to name its own.
pub(crate) struct Router(pub Rc<Scrollers>);

impl Global for Router {}

/// A wheel event this long after the previous one begins a new gesture.
const GAP: Duration = Duration::from_millis(150);

#[derive(Default)]
pub struct Scrollers {
    /// The scrollers drawn in the last frame, in the order drawn: the last
    /// is on top.
    live: RefCell<Vec<ScrollHandle>>,
    /// The scrollers of what stands over the window, and whether anything
    /// does.
    over: RefCell<Vec<ScrollHandle>>,
    covered: Cell<bool>,
    /// The scrollers that zoom by ⌘ and the wheel.
    zooms: RefCell<Vec<ScrollHandle>>,
    /// While set, what is named stands over the window (`lift`).
    lifted: Cell<bool>,
    /// The scroller the gesture under way began in, if it began in one,
    /// and whether it is one of what stands over the window.
    owner: RefCell<Option<(ScrollHandle, bool)>>,
    last: Cell<Option<Instant>>,
}

impl Scrollers {
    /// At the start of a view's draw: the scrollers it names are this
    /// draw's.
    pub fn begin(&self) {
        self.live.borrow_mut().clear();
        self.over.borrow_mut().clear();
        self.zooms.borrow_mut().clear();
        self.covered.set(false);
        self.lifted.set(false);
    }

    /// A scroller drawn now.
    pub fn name(&self, handle: &ScrollHandle) {
        if self.lifted.get() {
            self.over.borrow_mut().push(handle.clone());
        } else {
            self.live.borrow_mut().push(handle.clone());
        }
    }

    /// A scroller drawn now that zooms by ⌘ and the wheel: such an event
    /// over it goes to its own listener.
    pub fn zooms(&self, handle: &ScrollHandle) {
        self.name(handle);
        self.zooms.borrow_mut().push(handle.clone());
    }

    /// From here until `lift(false)`, what is drawn stands over the
    /// window, and the scrollers it names are its own: a fly-out drawing
    /// a part of the window that scrolls.
    pub fn lift(&self, on: bool) {
        if on {
            self.covered.set(true);
        }
        self.lifted.set(on);
    }

    /// Something stands over the window in this draw: what is under it
    /// does not scroll.
    pub fn over(&self) {
        self.covered.set(true);
    }

    /// A scroller of what stands over the window.
    pub fn name_over(&self, handle: &ScrollHandle) {
        self.covered.set(true);
        self.over.borrow_mut().push(handle.clone());
    }

    /// One wheel event, in the capture phase. True when it was applied
    /// here, or nothing may take it, and it must go no further. `system`
    /// is true while a panel of the system's stands over the window.
    pub fn route(&self, e: &ScrollWheelEvent, system: bool) -> bool {
        if system {
            *self.owner.borrow_mut() = None;
            return true;
        }
        let covered = self.covered.get();
        if e.modifiers.platform && !covered && self.zooms.borrow().iter().any(|h| h.bounds().contains(&e.position)) {
            *self.owner.borrow_mut() = None;
            return false;
        }
        let now = Instant::now();
        let fresh = self.last.get().is_none_or(|at| now.duration_since(at) > GAP);
        self.last.set(Some(now));
        let begins = match e.touch_phase {
            TouchPhase::Started => true,
            TouchPhase::Moved => fresh,
            TouchPhase::Ended | TouchPhase::Cancelled => false,
        };
        let delta = e.delta.pixel_delta(px(20.));
        if begins {
            // The top scroller under the pointer that can move the
            // gesture's way, else the top one with something to scroll.
            let list = if covered { self.over.borrow() } else { self.live.borrow() };
            let under: Vec<&ScrollHandle> = list.iter().rev().filter(|h| h.bounds().contains(&e.position) && h.max_offset().y > px(0.)).collect();
            let found = under.iter().find(|h| can_move(h, delta.y)).or(under.first()).map(|h| (*h).clone());
            drop(list);
            *self.owner.borrow_mut() = found.map(|h| (h, covered));
        }
        // A gesture begun under what now stands over the window ends there.
        if covered && self.owner.borrow().as_ref().is_some_and(|(_, over)| !over) {
            *self.owner.borrow_mut() = None;
        }
        let Some((owner, _)) = self.owner.borrow().clone() else { return covered };
        let x = (owner.offset().x + delta.x).clamp(-owner.max_offset().x, px(0.));
        let y = (owner.offset().y + delta.y).clamp(-owner.max_offset().y, px(0.));
        owner.set_offset(point(x, y));
        true
    }
}

/// Whether a scroller has room to move by `dy`: up (above 0) while it
/// is not at its top, down while it is not at its foot.
fn can_move(h: &ScrollHandle, dy: Pixels) -> bool {
    let y = h.offset().y;
    if dy > px(0.) {
        y < px(0.)
    } else if dy < px(0.) {
        y > -h.max_offset().y
    } else {
        true
    }
}
