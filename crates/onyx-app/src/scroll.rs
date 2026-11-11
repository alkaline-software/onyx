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
//! A place that scrolls by a count of its own and not by a handle (the
//! terminal's rows back, the toolkit's code editor) names itself too, by
//! where it is and what moves it (`name_own`): it owns a gesture begun
//! over it as a handle's scroller does. While the pointer stays on it the
//! wheel is its own listener's, as the toolkit gives it; carried off it,
//! the router hands each event to what moves it and nothing else hears it.
//!
//! A gesture begun over nothing that scrolls (the tabs' row, the path bar,
//! a margin) moves no named scroller: carried onto one, it is stopped
//! there. What is not named (a sideways strip) still has the events over
//! it, so a sideways flick there works.
//!
//! While something stands over the window (a sheet, the search, the
//! viewer, a menu, a fly-out, or a panel of the system's), nothing under
//! it scrolls: it says so as it is drawn (`over`), names its own scrollers
//! with `name_over`, and the wheel goes to those alone, or nowhere.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{point, px, App, Bounds, Global, Pixels, ScrollHandle, ScrollWheelEvent, TouchPhase, Window};

/// What moves a place that scrolls by itself, given a wheel's event.
pub type Wheel = Rc<dyn Fn(&ScrollWheelEvent, &mut Window, &mut App)>;

/// A scroller as the router knows it: a handle it moves itself, or a
/// place that scrolls by its own count, where it was last laid out.
#[derive(Clone)]
enum Scroller {
    Handle(ScrollHandle),
    Own(Rc<Cell<Bounds<Pixels>>>, Wheel),
}

impl Scroller {
    fn contains(&self, at: &gpui::Point<Pixels>) -> bool {
        match self {
            Scroller::Handle(h) => h.bounds().contains(at),
            Scroller::Own(b, _) => b.get().contains(at),
        }
    }

    /// Whether it has anything to scroll: a place of its own always may.
    fn scrolls(&self) -> bool {
        match self {
            Scroller::Handle(h) => h.max_offset().y > px(0.),
            Scroller::Own(..) => true,
        }
    }
}

/// What the router did with an event: let it go on to what is under the
/// pointer, stopped it, or gives it to a place that scrolls by itself.
pub enum Route {
    Pass,
    Stop,
    Give(Wheel),
}

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
    live: RefCell<Vec<Scroller>>,
    /// The scrollers of what stands over the window, and whether anything
    /// does.
    over: RefCell<Vec<Scroller>>,
    covered: Cell<bool>,
    /// The scrollers that zoom by ⌘ and the wheel.
    zooms: RefCell<Vec<ScrollHandle>>,
    /// While set, what is named stands over the window (`lift`).
    lifted: Cell<bool>,
    /// The scroller the gesture under way began in, if it began in one,
    /// and whether it is one of what stands over the window; `Nowhere`
    /// when it began over nothing that scrolls.
    owner: RefCell<Owner>,
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
        self.push(Scroller::Handle(handle.clone()));
    }

    /// A place drawn now that scrolls by a count of its own: where it is
    /// (set as it is laid out) and what moves it.
    pub fn name_own(&self, bounds: Rc<Cell<Bounds<Pixels>>>, wheel: Wheel) {
        self.push(Scroller::Own(bounds, wheel));
    }

    fn push(&self, s: Scroller) {
        if self.lifted.get() {
            self.over.borrow_mut().push(s);
        } else {
            self.live.borrow_mut().push(s);
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
        self.over.borrow_mut().push(Scroller::Handle(handle.clone()));
    }

    /// One wheel event, in the capture phase: what to do with it. `system`
    /// is true while a panel of the system's stands over the window.
    pub fn route(&self, e: &ScrollWheelEvent, system: bool) -> Route {
        if system {
            *self.owner.borrow_mut() = Owner::None;
            return Route::Stop;
        }
        let covered = self.covered.get();
        if e.modifiers.platform && !covered && self.zooms.borrow().iter().any(|h| h.bounds().contains(&e.position)) {
            *self.owner.borrow_mut() = Owner::None;
            return Route::Pass;
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
        let list = if covered { self.over.borrow().clone() } else { self.live.borrow().clone() };
        if begins {
            // The top scroller under the pointer that can move the
            // gesture's way, else the top one with something to scroll.
            let under: Vec<&Scroller> = list.iter().rev().filter(|s| s.contains(&e.position) && s.scrolls()).collect();
            let found = under.iter().find(|s| match s {
                Scroller::Handle(h) => can_move(h, delta.y),
                Scroller::Own(..) => true,
            });
            let found = found.or(under.first()).map(|s| (*s).clone());
            *self.owner.borrow_mut() = match found {
                Some(s) => Owner::Some(s, covered),
                None => Owner::Nowhere,
            };
        }
        // A gesture begun under what now stands over the window ends there.
        if covered && matches!(&*self.owner.borrow(), Owner::Some(_, false)) {
            *self.owner.borrow_mut() = Owner::None;
        }
        let owner = self.owner.borrow().clone();
        match owner {
            // Begun over nothing that scrolls: it moves no scroller it is
            // carried onto, and what is not named has it.
            Owner::Nowhere => {
                if list.iter().any(|s| s.contains(&e.position)) {
                    Route::Stop
                } else {
                    Route::Pass
                }
            }
            Owner::None => {
                if covered {
                    Route::Stop
                } else {
                    Route::Pass
                }
            }
            // Its own listener has it while the pointer is on it; carried
            // off, it is handed what moves it, and nothing else hears it.
            Owner::Some(Scroller::Own(bounds, wheel), _) => {
                if bounds.get().contains(&e.position) {
                    Route::Pass
                } else {
                    Route::Give(wheel)
                }
            }
            Owner::Some(Scroller::Handle(owner), _) => {
                let x = (owner.offset().x + delta.x).clamp(-owner.max_offset().x, px(0.));
                let y = (owner.offset().y + delta.y).clamp(-owner.max_offset().y, px(0.));
                owner.set_offset(point(x, y));
                Route::Stop
            }
        }
    }
}

/// Who a gesture belongs to: nobody yet (or it ended under something over
/// the window), nothing that scrolls, or a scroller.
#[derive(Clone, Default)]
enum Owner {
    #[default]
    None,
    Nowhere,
    Some(Scroller, bool),
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
