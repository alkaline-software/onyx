//! Liquid Glass on an iPhone and an iPad: what floats over the page (the
//! bar at its foot, the bar over the keyboard, the round buttons at its
//! top) stands on the system's own glass, which bends and blurs what
//! scrolls under it.
//!
//! gpui draws the whole window into one layer and has no blur of what is
//! behind an element, so the glass is the system's: a view of UIKit's
//! laid over gpui's (`sys::glass`), one for each plate named here. What
//! is under a view of UIKit's is blurred by it, so a plate's icons are
//! the system's to draw too, in the plate, from the same files: a plate
//! holds icons and washes, never words, which only gpui sets in Onyx's
//! faces. Presses stay gpui's: the views take none, and what is pressed
//! is the element that named the plate, where it always was.
//!
//! A plate and its marks name themselves as they are painted, and the
//! frame's plates are told to the system once it is drawn (`flush`). The
//! system's views stand over everything gpui draws, so what gpui draws
//! over a plate says so (`cover`, `cover_by`): a scrim, a sheet, a menu.
//! A plate painted before a cover that lies on it fades as the cover
//! comes and returns as it goes, over the time a menu takes; a plate
//! painted after it (a sheet's own button) is the cover's and stays.
//!
//! Elsewhere there is no glass (`on` is false) and each place draws the
//! plate it always did.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{canvas, prelude::*, px, AnyElement, App, Bounds, Hsla, Pixels, SharedString, Window};
use gpui_component::{Icon, Sizable as _};

/// A plate of glass as the frame names it.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub struct Plate {
    pub key: &'static str,
    pub at: Bounds<Pixels>,
    pub round: f32,
    /// How far it shows, 0 to 1.
    pub show: f32,
    pub marks: Vec<Mark>,
    /// Dark or light whatever the window is, when it stands on a ground
    /// of its own (the viewer's dark one).
    pub dark: Option<bool>,
    /// A finger rests on it: the system's glass swells under a press.
    pub pressed: bool,
    /// Its place among what was painted, for what covers it.
    order: usize,
}

/// A thing on a plate: an icon in an ink, or a wash of a color, or a
/// field or a line of words of the system's own.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub struct Mark {
    pub at: Bounds<Pixels>,
    pub icon: Option<SharedString>,
    pub ink: Hsla,
    pub round: f32,
    pub show: f32,
    pub field: Option<Field>,
    pub label: Option<SharedString>,
    /// The icon a finger is down on: it rises in the glass under the press.
    pub pressed: bool,
}

/// A field of the system's on a plate (`field`): words typed there are
/// the system's to draw, over the glass, with its keyboard, its input
/// methods and its clear button, and come back as `FieldEvent`s.
#[derive(Clone, PartialEq)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub struct Field {
    /// Its name, given back with what it says.
    pub key: &'static str,
    pub placeholder: SharedString,
    /// The words put in it, once for each number: a search opened again
    /// with its last words, a find's phrase.
    pub set: (u64, SharedString),
    /// The keyboard given to it (true) or taken away, once for each number.
    pub focus: (u64, bool),
}

/// What a field of the system's says.
#[derive(Clone, Debug)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub enum FieldEvent {
    /// Its words changed: they are these now.
    Changed(&'static str, String),
    /// Its keyboard's Search key was pressed.
    Return(&'static str),
    /// It took the keyboard (true) or let it go.
    Focus(&'static str, bool),
}

thread_local! {
    static FIELD_EVENTS: RefCell<Option<futures::channel::mpsc::UnboundedSender<FieldEvent>>> = const { RefCell::new(None) };
}

/// Where what the system's fields say goes: one listener, the window's.
pub fn listen_fields() -> futures::channel::mpsc::UnboundedReceiver<FieldEvent> {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    FIELD_EVENTS.with(|e| *e.borrow_mut() = Some(tx));
    rx
}

/// A field of the system's says something (`sys::glass`).
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub fn field_says(ev: FieldEvent) {
    FIELD_EVENTS.with(|e| {
        if let Some(tx) = e.borrow().as_ref() {
            let _ = tx.unbounded_send(ev);
        }
    });
}

/// What stands over the page in a frame: where, how far it has come
/// when it says so itself, and its place among what was painted.
struct Cover {
    key: &'static str,
    at: Bounds<Pixels>,
    strength: Option<f32>,
    order: usize,
}

/// A cover that keeps its own time, across frames: when it came, the
/// plates it lay on when last drawn, and when it went.
struct Timed {
    since: Instant,
    on: Vec<&'static str>,
    gone: Option<Instant>,
}

/// How long a plate takes to go under a cover and to come back.
const COVER: Duration = Duration::from_millis(160);

#[derive(Default)]
struct Frame {
    plates: Vec<Plate>,
    covers: Vec<Cover>,
    /// How far each plate shows, said before it is painted (`show`).
    shows: Vec<(&'static str, f32)>,
    order: usize,
    dark: bool,
    /// The room the marks named now may show in (`within`).
    room: Option<Bounds<Pixels>>,
}

thread_local! {
    static FRAME: RefCell<Frame> = RefCell::new(Frame::default());
    static TIMED: RefCell<HashMap<&'static str, Timed>> = RefCell::new(HashMap::new());
}

/// Whether what floats stands on glass: on an iPhone and an iPad.
pub fn on() -> bool {
    cfg!(target_os = "ios")
}

/// A frame begins: its plates are the ones named from here.
pub fn begin(dark: bool) {
    FRAME.with(|f| *f.borrow_mut() = Frame { dark, ..Default::default() });
}

/// How far a plate shows in this frame, said as the frame is built: an
/// element's own opacity is gpui's to know, so a bar that fades in says
/// how far it has come.
pub fn show(key: &'static str, show: f32) {
    FRAME.with(|f| f.borrow_mut().shows.push((key, show)));
}

/// The plate under what is laid after it in the same element: laid
/// first in a bar or a button, it fills it. Nothing where there is no
/// glass, and the caller draws its own ground.
pub fn plate(key: &'static str, round: f32) -> Option<AnyElement> {
    plate_as(key, round, None)
}

/// A plate that is dark or light whatever the window is.
pub fn plate_as(key: &'static str, round: f32, dark: Option<bool>) -> Option<AnyElement> {
    if !on() {
        return None;
    }
    Some(
        canvas(
            move |at, window, _| {
                FRAME.with(|f| {
                    let mut f = f.borrow_mut();
                    f.order += 1;
                    let order = f.order;
                    f.plates.push(Plate { key, at, round, show: 1., marks: Vec::new(), dark, pressed: false, order });
                    (order, window.insert_hitbox(at, gpui::HitboxBehavior::Normal))
                })
            },
            |_, (order, hitbox), window, _| {
                // Pressed where a finger is down on it, from the landing, as
                // the system's own glass answers: a tap swells it as a hold does.
                // The icon nearest the finger, within a button's reach of it, is the one pressed.
                if crate::widgets::finger_down() && hitbox.is_hovered(window) {
                    let at = window.mouse_position();
                    FRAME.with(|f| {
                        if let Some(p) = f.borrow_mut().plates.iter_mut().find(|p| p.order == order) {
                            p.pressed = true;
                            let far = |m: &Mark| f32::from((m.at.center() - at).x).hypot(f32::from((m.at.center() - at).y));
                            if let Some(m) = p.marks.iter_mut().filter(|m| m.icon.is_some() && m.show > 0.5).min_by(|a, b| far(a).total_cmp(&far(b))).filter(|m| far(m) < 30.) {
                                m.pressed = true;
                            }
                        }
                    });
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element(),
    )
}

fn mark(at: Bounds<Pixels>, icon: Option<SharedString>, ink: Hsla, round: f32) {
    put(Mark { at, icon, ink, round, show: 1., field: None, label: None, pressed: false });
}

fn put(mut m: Mark) {
    let at = m.at;
    FRAME.with(|f| {
        let mut f = f.borrow_mut();
        // A mark leaving its room fades by how much of it is out.
        let show = f.room.map_or(1., |room| {
            let inside = at.right().min(room.right()) - at.left().max(room.left());
            (inside / at.size.width.max(px(1.))).clamp(0., 1.)
        });
        m.show = show;
        if let Some(plate) = f.plates.last_mut() {
            plate.marks.push(m);
        }
    });
}

/// The marks of what is laid between this and `within_end` show only in
/// the room this is laid in: a row of tools that scrolls along its bar
/// under gpui's own clip, which the system's views know nothing of. Laid
/// before the row, in the box that clips it.
pub fn within() -> Option<AnyElement> {
    if !on() {
        return None;
    }
    Some(canvas(|at, _, _| FRAME.with(|f| f.borrow_mut().room = Some(at)), |_, _, _, _| ()).absolute().top_0().left_0().size_full().into_any_element())
}

/// The end of what `within` holds, laid after the row.
pub fn within_end() -> Option<AnyElement> {
    if !on() {
        return None;
    }
    Some(canvas(|_, _, _| FRAME.with(|f| f.borrow_mut().room = None), |_, _, _, _| ()).absolute().size_0().into_any_element())
}

/// An icon on the plate named last: the system's to draw there, gpui's
/// own where there is no glass.
pub fn icon(path: impl Into<SharedString>, size: f32, ink: Hsla) -> AnyElement {
    let path: SharedString = path.into();
    if !on() {
        return Icon::default().path(path).with_size(px(size)).text_color(ink).into_any_element();
    }
    canvas(move |at, _, _| mark(at, Some(path.clone()), ink, 0.), |_, _, _, _| ()).size(px(size)).flex_shrink_0().into_any_element()
}

/// A field of the system's on the plate named last, filling the room it
/// is given; its words in `ink`. Only where there is glass.
pub fn field(f: Field, ink: Hsla) -> AnyElement {
    canvas(move |at, _, _| put(Mark { at, icon: None, ink, round: 0., show: 1., field: Some(f.clone()), label: None, pressed: false }), |_, _, _, _| ()).flex_1().min_w_0().h_full().into_any_element()
}

/// A line of words the system sets on the plate named last, in `ink`,
/// `w` wide, at the right of its room: a count beside a field.
pub fn label(words: impl Into<SharedString>, w: f32, ink: Hsla) -> AnyElement {
    let words: SharedString = words.into();
    canvas(move |at, _, _| put(Mark { at, icon: None, ink, round: 0., show: 1., field: None, label: Some(words.clone()), pressed: false }), |_, _, _, _| ()).w(px(w)).h_full().flex_shrink_0().into_any_element()
}

/// A wash of a color on the plate named last, filling what it is laid
/// in: a button that is on. Nothing where there is no glass.
pub fn wash(ink: Hsla, round: f32) -> Option<AnyElement> {
    if !on() {
        return None;
    }
    Some(canvas(move |at, _, _| mark(at, None, ink, round), |_, _, _, _| ()).absolute().top_0().left_0().size_full().into_any_element())
}

fn covers(key: &'static str, strength: Option<f32>) -> Option<AnyElement> {
    if !on() {
        return None;
    }
    Some(
        canvas(
            move |at, _, _| {
                FRAME.with(|f| {
                    let mut f = f.borrow_mut();
                    f.order += 1;
                    let order = f.order;
                    f.covers.push(Cover { key, at, strength, order });
                })
            },
            |_, _, _, _| (),
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element(),
    )
}

/// What this is laid in stands over the page: the plates painted before
/// it fade where it lies on them, as it comes and as it goes.
pub fn cover(key: &'static str) -> Option<AnyElement> {
    covers(key, None)
}

/// The same for what says itself how far it has come, 0 to 1: a drawer
/// a finger pulls, a viewer that grows.
pub fn cover_by(key: &'static str, strength: f32) -> Option<AnyElement> {
    covers(key, Some(strength))
}

/// The frame's plates told to the system, once the frame is painted.
pub fn flush(window: &mut Window, cx: &App) {
    if !on() {
        return;
    }
    crate::editor::stamp("glass flush");
    let ease = |t: f32| t * t * (3. - 2. * t);
    let mut moving = false;
    let (plates, dark) = FRAME.with(|f| {
        let mut f = f.borrow_mut();
        let (covers, shows) = (std::mem::take(&mut f.covers), std::mem::take(&mut f.shows));
        let mut plates = std::mem::take(&mut f.plates);
        for p in &mut plates {
            p.show = shows.iter().filter(|(key, _)| *key == p.key).map(|(_, s)| *s).product();
        }
        TIMED.with(|timed| {
            let mut timed = timed.borrow_mut();
            for c in &covers {
                let under: Vec<&'static str> = plates.iter().filter(|p| p.order < c.order && c.at.intersects(&p.at)).map(|p| p.key).collect();
                let strength = match c.strength {
                    Some(s) => s,
                    None => {
                        let t = timed.entry(c.key).or_insert_with(|| Timed { since: Instant::now(), on: Vec::new(), gone: None });
                        if t.gone.take().is_some() {
                            t.since = Instant::now();
                        }
                        t.on = under.clone();
                        let come = (t.since.elapsed().as_secs_f32() / COVER.as_secs_f32()).min(1.);
                        moving |= come < 1.;
                        ease(come)
                    }
                };
                for p in plates.iter_mut().filter(|p| under.contains(&p.key)) {
                    p.show *= 1. - strength.clamp(0., 1.);
                }
            }
            // A cover no longer drawn lets its plates back over a moment.
            timed.retain(|key, t| {
                if covers.iter().any(|c| c.key == *key) {
                    return true;
                }
                let left = 1. - (t.gone.get_or_insert_with(Instant::now).elapsed().as_secs_f32() / COVER.as_secs_f32()).min(1.);
                if left <= 0. {
                    return false;
                }
                moving = true;
                for p in plates.iter_mut().filter(|p| t.on.contains(&p.key)) {
                    p.show *= 1. - ease(left);
                }
                true
            });
        });
        (plates, f.dark)
    });
    // A cover still coming or going: another frame, asked here as the
    // frame is over (`request_animation_frame` is for while it is drawn).
    if moving {
        window.refresh();
    }
    crate::sys::glass(window, cx, dark, &plates);
    crate::editor::stamp("glass told");
}
