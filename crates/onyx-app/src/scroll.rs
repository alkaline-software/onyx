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
//! What floats over the page and is no part of what scrolls under it (the
//! bar at a compact window's foot, the buttons at its top) says where it
//! is (`fixed`): a gesture begun on it moves nothing, as one begun on a
//! bar of the system's does not scroll the list behind it.
//!
//! On an iPhone and an iPad a scroller is pulled past its end as a list
//! of the system's is, and springs back (`stretch`): a finger that goes
//! on past the end draws the scroller after it, less the further it goes;
//! let go, it springs back; a fling that reaches the end carries on past
//! it by its speed and comes back. gpui holds a scroller's offset to its
//! ends at each frame, so the scroller itself is moved, by `stretch_of`,
//! where it is laid out.
//!
//! While something stands over the window (a sheet, the search, the
//! viewer, a menu, a fly-out, or a panel of the system's), nothing under
//! it scrolls: it says so as it is drawn (`over`), names its own scrollers
//! with `name_over`, and the wheel goes to those alone, or nowhere.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{point, px, App, Bounds, Global, Pixels, ScrollHandle, ScrollEvent, TouchPhase, Window};

/// What moves a place that scrolls by itself, given a wheel's event.
pub type Wheel = Rc<dyn Fn(&ScrollEvent, &mut Window, &mut App)>;

/// A scroller as the router knows it: a handle it moves itself, or a
/// place that scrolls by its own count, where it was last laid out.
#[derive(Clone)]
enum Scroller {
    Handle(ScrollHandle),
    Own(Rc<Cell<Bounds<Pixels>>>, Wheel),
    /// A place that moves sideways alone (a table wider than its room):
    /// it owns a gesture begun over it that goes more across than down.
    Side(Bounds<Pixels>, Wheel),
}

impl Scroller {
    fn contains(&self, at: &gpui::Point<Pixels>) -> bool {
        match self {
            Scroller::Handle(h) => h.bounds().contains(at),
            Scroller::Own(b, _) => b.get().contains(at),
            Scroller::Side(b, _) => b.contains(at),
        }
    }

    /// Whether it has anything to scroll: a place of its own always may.
    fn scrolls(&self) -> bool {
        match self {
            Scroller::Handle(h) => h.max_offset().y > px(0.),
            Scroller::Own(..) | Scroller::Side(..) => true,
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
    /// What floats over the page's scrollers, where each was last laid out.
    fixed: RefCell<Vec<Rc<Cell<Bounds<Pixels>>>>>,
    /// While set, what is named stands over the window (`lift`).
    lifted: Cell<bool>,
    /// The scroller the gesture under way began in, if it began in one,
    /// and whether it is one of what stands over the window; `Nowhere`
    /// when it began over nothing that scrolls.
    owner: RefCell<Owner>,
    last: Cell<Option<Instant>>,
    /// Whether the last event routed moved nothing, its owner being at
    /// its end that way (`spent`).
    spent: Cell<bool>,
    /// The scroller pulled past its end, and how (`stretch`).
    stretch: RefCell<Option<Stretch>>,
    /// Whether the finger of the gesture under way has lifted: what comes
    /// after is its fling.
    flung: Cell<bool>,
}

/// A scroller pulled past an end: which (its width, its height and its
/// left, which a pull does not change), how far the finger has gone past
/// the end (down at the top above 0, up at the foot below), and, once it
/// is let go or a fling struck the end, how it comes back.
struct Stretch {
    key: (Pixels, Pixels, Pixels),
    pull: f32,
    span: f32,
    back: Option<Back>,
}

#[derive(Clone, Copy)]
enum Back {
    /// Let go at `from` points out: it comes back as a spring does.
    Spring { since: Instant, from: f32 },
    /// A fling struck the end: out to `reach` and back.
    Bounce { since: Instant, reach: f32 },
}

/// How long a spring takes, in seconds, to come most of the way.
const SPRING: f32 = 0.11;

fn key_of(h: &ScrollHandle) -> (Pixels, Pixels, Pixels) {
    let b = h.bounds();
    (b.size.width, b.size.height, b.origin.x)
}

/// How far a scroller shows past its end for a finger `pull` past it:
/// UIKit's curve, less and less the further it goes, never more than
/// the scroller's height.
fn rubber(pull: f32, span: f32) -> f32 {
    let d = span.max(1.);
    let x = pull.abs();
    (1. - 1. / (x * 0.55 / d + 1.)) * d * pull.signum()
}

impl Scrollers {
    /// At the start of a view's draw: the scrollers it names are this
    /// draw's.
    pub fn begin(&self) {
        self.live.borrow_mut().clear();
        self.over.borrow_mut().clear();
        self.zooms.borrow_mut().clear();
        self.fixed.borrow_mut().clear();
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

    /// A place drawn now that moves sideways alone, where it is and what
    /// moves it.
    pub fn name_side(&self, bounds: Bounds<Pixels>, wheel: Wheel) {
        self.push(Scroller::Side(bounds, wheel));
    }

    fn push(&self, s: Scroller) {
        if self.lifted.get() {
            self.over.borrow_mut().push(s);
        } else {
            self.live.borrow_mut().push(s);
        }
    }

    /// A thing drawn now over the page's scrollers that is no part of
    /// them: a gesture begun on it belongs to nothing that scrolls.
    pub fn fixed(&self, bounds: Rc<Cell<Bounds<Pixels>>>) {
        self.fixed.borrow_mut().push(bounds);
    }

    /// Whether a place is on such a thing, with nothing over the window.
    fn on_fixed(&self, at: &gpui::Point<Pixels>) -> bool {
        !self.covered.get() && self.fixed.borrow().iter().any(|b| b.get().contains(at))
    }

    /// Whether a touch at `at` is in the scroller the gesture routed last
    /// belongs to: a touch that lands while a fling still goes stops the
    /// fling when it is, and is a press of its own when it is not.
    pub fn owned_at(&self, at: &gpui::Point<Pixels>) -> bool {
        !self.on_fixed(at) && matches!(&*self.owner.borrow(), Owner::Some(s, _) if s.contains(at))
    }

    /// Whether the gesture routed last began over nothing that scrolls.
    pub fn nowhere(&self) -> bool {
        matches!(&*self.owner.borrow(), Owner::Nowhere)
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

    /// Whether something stands over the window in the frame drawn last.
    pub fn covered(&self) -> bool {
        self.covered.get()
    }

    /// A scroller of what stands over the window.
    pub fn name_over(&self, handle: &ScrollHandle) {
        self.covered.set(true);
        self.over.borrow_mut().push(Scroller::Handle(handle.clone()));
    }

    /// Whether the last event routed asked its owner to move and could
    /// not, the owner standing at its end that way. A finger's fling goes
    /// on by gpui's own count for as long as its speed lasts, whether
    /// anything still moves or not, and a touch in that time is read as
    /// one that stops the fling, never a tap: the window ends a fling
    /// that is spent (`Shell::end_fling`).
    pub fn spent(&self) -> bool {
        self.spent.get()
    }

    /// How far the scroller of `handle` is drawn past its end now: down at
    /// its top above 0, up at its foot below. Nothing off an iPhone and an
    /// iPad, or for any other scroller.
    pub fn stretch_of(&self, handle: &ScrollHandle) -> Pixels {
        let mut stretch = self.stretch.borrow_mut();
        let Some(st) = stretch.as_mut() else { return px(0.) };
        if st.key != key_of(handle) {
            return px(0.);
        }
        let shown = match st.back {
            None => rubber(st.pull, st.span),
            Some(Back::Spring { since, from }) => {
                let t = since.elapsed().as_secs_f32() / SPRING;
                from * (1. + t) * (-t).exp()
            }
            Some(Back::Bounce { since, reach }) => {
                let t = since.elapsed().as_secs_f32() / (SPRING * 0.9);
                reach * t * (1. - t).exp()
            }
        };
        if st.back.is_some() && shown.abs() < 0.3 && match st.back {
            Some(Back::Spring { since, .. } | Back::Bounce { since, .. }) => since.elapsed().as_secs_f32() > SPRING * 2.,
            None => false,
        } {
            *stretch = None;
            return px(0.);
        }
        px(shown)
    }

    /// Whether a scroller is past its end, coming back: the window draws
    /// another frame while it is.
    pub fn stretching(&self) -> bool {
        self.stretch.borrow().as_ref().is_some_and(|s| s.back.is_some())
    }

    /// A gesture's step on a scroller at an end: what of `dy` the end did
    /// not take goes into the stretch, and what a stretch gives back first
    /// comes out of it. Returns what is left to scroll the content by.
    fn pull(&self, h: &ScrollHandle, was: Pixels, dy: Pixels, phase: TouchPhase) -> Pixels {
        if !cfg!(target_os = "ios") {
            return dy;
        }
        let key = key_of(h);
        let span = f32::from(h.bounds().size.height);
        let max = h.max_offset().y;
        let mut stretch = self.stretch.borrow_mut();
        // A new gesture takes hold of a stretch coming back where it is.
        if let Some(st) = stretch.as_mut().filter(|st| st.key == key && st.back.is_some() && phase == TouchPhase::Started) {
            let now = drop_back(st);
            st.pull = unrubber(now, span);
            st.back = None;
        }
        let mut dy = f32::from(dy);
        if self.flung.get() {
            // The fling's own steps: one that reaches past the end bounces
            // there, by how fast it came, and the fling is spent.
            let y = f32::from(was) + dy;
            let past = if y > 0. { y } else if y < -f32::from(max) { y + f32::from(max) } else { 0. };
            if past != 0. && stretch.as_ref().is_none_or(|st| st.back.is_none()) {
                let reach = (dy.abs() * 5.).clamp(10., span * 0.12) * past.signum();
                *stretch = Some(Stretch { key, pull: 0., span, back: Some(Back::Bounce { since: Instant::now(), reach }) });
            }
            return px(dy);
        }
        if let Some(st) = stretch.as_mut().filter(|st| st.key == key && st.back.is_none()) {
            // A pull past an end gives way first to a step back toward it.
            if (st.pull > 0. && dy < 0.) || (st.pull < 0. && dy > 0.) {
                let after = st.pull + dy;
                if after.signum() == st.pull.signum() {
                    st.pull = after;
                    dy = 0.;
                } else {
                    dy = after;
                    st.pull = 0.;
                }
            } else {
                st.pull += dy;
                dy = 0.;
            }
            return px(dy);
        }
        let y = f32::from(was) + dy;
        let past = if y > 0. { y } else if y < -f32::from(max) { y + f32::from(max) } else { 0. };
        if past != 0. {
            *stretch = Some(Stretch { key, pull: past, span, back: None });
            return px(dy - past);
        }
        px(dy)
    }

    /// The finger lifted: a stretch it held springs back.
    fn let_go(&self) {
        if let Some(st) = self.stretch.borrow_mut().as_mut().filter(|st| st.back.is_none()) {
            st.back = Some(Back::Spring { since: Instant::now(), from: rubber(st.pull, st.span) });
            st.pull = 0.;
        }
    }

    /// One wheel event, in the capture phase: what to do with it. `system`
    /// is true while a panel of the system's stands over the window.
    pub fn route(&self, e: &ScrollEvent, system: bool) -> Route {
        self.spent.set(false);
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
        if e.source == gpui::ScrollSource::Touch {
            match e.touch_phase {
                TouchPhase::Started => self.flung.set(false),
                TouchPhase::Ended | TouchPhase::Cancelled if !self.flung.get() => {
                    self.flung.set(true);
                    self.let_go();
                }
                _ => {}
            }
        }
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
            // A gesture that goes more across than down is a sideways
            // place's; one that goes down is never.
            let across = delta.x.abs() > delta.y.abs();
            let under: Vec<&Scroller> = list.iter().rev().filter(|s| s.contains(&e.position) && s.scrolls() && (across || !matches!(s, Scroller::Side(..)))).collect();
            let found = under.iter().find(|s| match s {
                Scroller::Handle(h) => can_move(h, delta.y),
                Scroller::Own(..) => true,
                Scroller::Side(..) => across,
            });
            let found = found.or(under.first()).map(|s| (*s).clone()).filter(|_| !self.on_fixed(&e.position));
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
            Owner::Some(Scroller::Side(_, wheel), _) => Route::Give(wheel),
            Owner::Some(Scroller::Own(bounds, wheel), _) => {
                if bounds.get().contains(&e.position) {
                    Route::Pass
                } else {
                    Route::Give(wheel)
                }
            }
            Owner::Some(Scroller::Handle(owner), _) => {
                let was = owner.offset();
                let dy = if e.source == gpui::ScrollSource::Touch { self.pull(&owner, was.y, delta.y, e.touch_phase) } else { delta.y };
                let x = (was.x + delta.x).clamp(-owner.max_offset().x, px(0.));
                let y = (was.y + dy).clamp(-owner.max_offset().y, px(0.));
                owner.set_offset(point(x, y));
                // A finger drawing the scroller past its end has not spent its gesture.
                let held = !self.flung.get() && self.stretch.borrow().as_ref().is_some_and(|s| s.back.is_none());
                self.spent.set(!held && (delta.x != px(0.) || delta.y != px(0.)) && x == was.x && y == was.y);
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

/// Where a stretch coming back is now.
fn drop_back(st: &Stretch) -> f32 {
    match st.back {
        Some(Back::Spring { since, from }) => {
            let t = since.elapsed().as_secs_f32() / SPRING;
            from * (1. + t) * (-t).exp()
        }
        Some(Back::Bounce { since, reach }) => {
            let t = since.elapsed().as_secs_f32() / (SPRING * 0.9);
            reach * t * (1. - t).exp()
        }
        None => rubber(st.pull, st.span),
    }
}

/// The finger's pull that `rubber` shows as `shown`.
fn unrubber(shown: f32, span: f32) -> f32 {
    let d = span.max(1.);
    let x = (shown.abs() / d).min(0.99);
    (x / (1. - x)) * d / 0.55 * shown.signum()
}
