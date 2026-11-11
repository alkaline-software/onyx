//! The window's shared drawing: one way to draw each thing, used by every
//! view and never drawn again by hand. A button is `choice_pill`; on or
//! off is `Motion::switch`; a choice among a few is `Motion::segmented`; a
//! key is drawn on caps by `key_text`; a hover's label is `tip`; a page's
//! content comes in by `page_in`; something over the window comes in by
//! `fade_in`; something small that is there or is not plays its change by
//! `Motion::presence`.
//!
//! The timings are the studio's (the constitution's `docs/design.md`,
//! Motion, Timing), and every motion eases with `ease_out_quint`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, ActiveTheme as _, Icon, Sizable as _};

/// A menu or a list coming in, and going out.
pub const MENU_IN: Duration = Duration::from_millis(140);
pub const MENU_OUT: Duration = Duration::from_millis(110);
/// A fold, a row that comes or goes, a switch, something small that is
/// there or is not.
pub const FOLD_ANIM: Duration = Duration::from_millis(200);
/// A choice's plate sliding, a sidebar, a tab.
pub const SLIDE: Duration = Duration::from_millis(220);
/// A page's content fading and rising in.
pub const PAGE_IN: Duration = Duration::from_millis(240);

/// What a button is, which is how its pill is drawn.
#[derive(Clone, Copy, PartialEq)]
pub enum Press {
    /// One of several, or the way out: an outline. The default.
    Quiet,
    /// The one thing a page waits on: filled with the accent.
    Primary,
    /// What cannot be taken back: the danger's ink and outline.
    #[allow(dead_code)] // a deletion goes to the trash; nothing in Onyx cannot be taken back yet
    Danger,
}

/// The window's button: a pill with an icon before its word. Every
/// button is one; the toolkit's `Button` is only for a button that is an
/// icon alone. `off` is one that cannot be pressed now: paler, and the
/// caller gives it no click (`.when(!off, ..)`).
pub fn choice_pill(id: impl Into<ElementId>, icon: &'static str, label: impl Into<SharedString>, kind: Press, off: bool, theme: &gpui_component::Theme) -> Stateful<Div> {
    let ink = match kind {
        Press::Danger => theme.danger,
        Press::Primary => theme.primary_foreground,
        Press::Quiet => theme.foreground,
    };
    let (ground, edge, hover) = match kind {
        Press::Danger => (None, theme.danger.opacity(0.35), theme.danger.opacity(0.1)),
        Press::Primary => (Some(theme.primary), theme.primary, theme.primary.opacity(0.85)),
        // An edge of the ink, not the border's color: on a card the
        // border is the card's own shade and the pill had no outline.
        Press::Quiet => (None, theme.foreground.opacity(0.16), theme.foreground.opacity(0.06)),
    };
    h_flex()
        .id(id)
        .h(px(28.))
        .px(px(12.))
        .gap(px(6.))
        .flex_shrink_0()
        .items_center()
        .rounded_full()
        .text_size(px(12.5))
        .text_color(ink)
        .border_1()
        .border_color(edge)
        .when_some(ground, |d, bg| d.bg(bg))
        .child(Icon::default().path(icon).with_size(px(14.)).text_color(ink))
        .child(label.into())
        .map(|d| if off { d.opacity(0.4) } else { d.cursor_pointer().hover(move |s| s.bg(hover)) })
}

/// `choice_pill` in the size a strip or a foot line takes.
pub fn small_pill(id: impl Into<ElementId>, icon: &'static str, label: impl Into<SharedString>, kind: Press, off: bool, theme: &gpui_component::Theme) -> Stateful<Div> {
    choice_pill(id, icon, label, kind, off, theme).h(px(24.)).px(px(10.)).gap(px(5.)).text_size(px(12.))
}

/// The app's name, as a sidebar's wordmark or About's title has it: in
/// its own face, the regular drawn twice half a pixel apart for a weight
/// between regular and bold, which Optima does not have.
pub fn wordmark(name: &'static str, size: f32, color: Hsla, cx: &App) -> Div {
    let family = crate::fonts::wordmark_family(cx);
    let word = |dx: f32| div().absolute().left(px(dx)).top_0().child(name);
    div()
        .relative()
        .font_family(family)
        .text_size(px(size))
        .line_height(px(size * 1.25))
        .text_color(color)
        // The first copy takes the room; the second stands over it.
        .child(div().child(name))
        .child(word(0.5))
}

/// The dot on the gear while a newer version waits, at its top right, so
/// it is seen with the settings shut: `moving` from `Motion::presence`.
pub fn waiting_dot(on: bool, moving: Option<(bool, u32)>, color: Hsla) -> Option<AnyElement> {
    (on || moving.is_some()).then(|| {
        let dot = div().absolute().top(px(3.)).right(px(3.)).size(px(7.)).rounded_full().bg(color);
        match moving {
            Some((coming, n)) => dot.with_animation(ElementId::NamedInteger("gear-dot".into(), n as u64), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element(),
            None => dot.into_any_element(),
        }
    })
}

/// The icon a key's sign is drawn with: none for a character that is not
/// one. Off a Mac ⌘ and ⌥ are the words Ctrl and Alt on their caps.
fn key_sign(c: char) -> Option<&'static str> {
    Some(match c {
        '⌘' if cfg!(target_os = "macos") => "icons/key-command.svg",
        '⌥' if cfg!(target_os = "macos") => "icons/key-option.svg",
        '⇧' => "icons/key-shift.svg",
        '⌃' => "icons/chevron-up.svg",
        '↩' => "icons/key-return.svg",
        '⇥' => "icons/key-tab.svg",
        '↑' => "icons/arrow-up.svg",
        '↓' => "icons/arrow-down.svg",
        _ => return None,
    })
}

/// The signs a key is written with.
const KEY_SIGNS: [char; 8] = ['⌘', '⌥', '⇧', '⌃', '↩', '⇥', '↑', '↓'];

/// A word a sign can stand before as one key: "⇧Tab".
const KEY_WORDS: [&str; 5] = ["Tab", "Esc", "Enter", "Space", "Return"];

/// A piece of a line that names keys: words, or one shortcut's keys.
enum KeyPiece {
    Words(String),
    Keys(Vec<String>),
}

/// A line cut into its words and its shortcuts. A shortcut is a run of
/// signs with the one key after them (a letter, a digit, a mark, or one
/// of `KEY_WORDS`). Brackets that hold only shortcuts are dropped: the
/// caps set them apart already.
fn key_pieces(text: &str) -> Vec<KeyPiece> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<KeyPiece> = Vec::new();
    let mut words = String::new();
    let mut i = 0;
    while i < chars.len() {
        if !KEY_SIGNS.contains(&chars[i]) {
            words.push(chars[i]);
            i += 1;
            continue;
        }
        let mut keys: Vec<String> = Vec::new();
        while i < chars.len() && KEY_SIGNS.contains(&chars[i]) {
            keys.push(chars[i].to_string());
            i += 1;
        }
        let rest: String = chars[i..].iter().collect();
        if let Some(w) = KEY_WORDS.iter().find(|w| rest.starts_with(**w)) {
            keys.push(w.to_string());
            i += w.chars().count();
        } else if let Some(c) = chars.get(i).filter(|c| !c.is_whitespace() && !matches!(c, ')' | '(')) {
            let alone = chars.get(i + 1).is_none_or(|n| !n.is_alphanumeric());
            if alone && (c.is_alphanumeric() || matches!(c, ',' | '.' | '/' | '[' | ']' | '-' | '=')) {
                keys.push(c.to_uppercase().to_string());
                i += 1;
            }
        }
        if !words.is_empty() {
            out.push(KeyPiece::Words(std::mem::take(&mut words)));
        }
        out.push(KeyPiece::Keys(keys));
    }
    if !words.is_empty() {
        out.push(KeyPiece::Words(words));
    }
    // "(⌘K)": the brackets go.
    let mut at = 0;
    while at + 2 < out.len() + 1 {
        let opens = matches!(&out[at], KeyPiece::Words(w) if w.ends_with('('));
        if opens && matches!(out.get(at + 1), Some(KeyPiece::Keys(_))) && matches!(out.get(at + 2), Some(KeyPiece::Words(w)) if w.starts_with(')')) {
            if let KeyPiece::Words(w) = &mut out[at] {
                w.pop();
            }
            if let KeyPiece::Words(w) = &mut out[at + 2] {
                w.remove(0);
            }
        }
        at += 1;
    }
    out.retain(|p| !matches!(p, KeyPiece::Words(w) if w.trim().is_empty() && w.is_empty()));
    out
}

const KEY_CAP: f32 = 17.;

/// One key on its cap: a small plate, every one the same height and no
/// narrower than that, the key in its middle. A sign is an icon and a
/// letter a letter, both of one size; on equal plates they read as one
/// size, which side by side on a line of words they never did.
fn key_cap(key: &str) -> Div {
    let sign = key.chars().next().filter(|_| key.chars().count() == 1).and_then(key_sign);
    let word = match key {
        "⌘" => "Ctrl",
        "⌥" => "Alt",
        other => other,
    };
    div()
        .flex_shrink_0()
        .h(px(KEY_CAP))
        .min_w(px(KEY_CAP))
        .px(px(3.5))
        .rounded(px(4.5))
        .bg(hsla(0., 0., 0.5, 0.17))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(10.5))
        .line_height(px(KEY_CAP))
        .font_weight(FontWeight::MEDIUM)
        .map(|d| match sign {
            Some(path) => d.child(Icon::default().path(path).with_size(px(10.5))),
            None => d.child(word.to_string()),
        })
}

/// Words that name keys, written as the Mac has them ("Search (⌘K)"),
/// with each shortcut drawn as its keys on caps. Every place the window
/// shows a key goes through this (or `kbd_hint`, or `tip`); a string with
/// a sign in it is never handed to the window as it is.
pub fn key_text(text: &str) -> Div {
    let mut row = h_flex().flex_shrink_0().items_center();
    for piece in key_pieces(text) {
        row = match piece {
            KeyPiece::Words(w) => row.child(div().flex_shrink_0().child(w.replace(' ', "\u{a0}"))),
            KeyPiece::Keys(keys) => row.child(h_flex().flex_shrink_0().gap(px(2.)).mx(px(1.)).children(keys.iter().map(|k| key_cap(k)))),
        };
    }
    row
}

/// A shortcut beside what it does, in the quiet ink.
pub fn kbd_hint(text: &str, theme: &gpui_component::Theme) -> Div {
    key_text(text).text_color(theme.muted_foreground)
}

/// One key on its cap by its name: `key_text` reads a name as a key only
/// after a sign.
pub fn kbd_key(key: &str, theme: &gpui_component::Theme) -> Div {
    div().flex_shrink_0().text_color(theme.muted_foreground).child(key_cap(key))
}

/// A hover's label, with the keys it names on caps: what every
/// `managed_tooltip` in the window builds. The toolkit's managed tooltip
/// comes in, and slides to the next element on a row; gpui's plain
/// `.tooltip` jumps in and out, and a row with both read as two kinds.
pub fn tip(text: impl Into<SharedString>, window: &mut Window, cx: &mut App) -> AnyView {
    let text: SharedString = text.into();
    gpui_component::tooltip::Tooltip::element(move |_, _| key_text(&text)).build(window, cx)
}

/// A page's content, fading and rising into place when it arrives,
/// keyed on the page so it plays once each time the page is shown.
pub fn page_in(id: impl Into<SharedString>, el: Div) -> AnyElement {
    el.with_animation(ElementId::Name(id.into()), Animation::new(PAGE_IN).with_easing(ease_out_quint()), |d, t| d.opacity(t).pt(px(8. * (1. - t)))).into_any_element()
}

/// Something that lies over the whole window with no way out of its own
/// (a question, a lightbox), fading in once each time it appears. The id
/// is the element's own: forgotten once it is no longer drawn.
#[allow(dead_code)] // for a product's sheet; the sample has none
pub fn fade_in(id: &'static str, el: impl IntoElement) -> impl IntoElement {
    div().absolute().inset_0().child(el).with_animation(id, Animation::new(MENU_IN).with_easing(ease_out_quint()), |d, t| d.opacity(t))
}

/// The shadow under a floating card: a wide soft drop in the ink's own
/// hue and a hairline of contact, so the card lifts off the page rather
/// than sitting in a gray halo. Nothing casts a shadow at rest.
pub fn float_shadow(theme: &gpui_component::Theme) -> Vec<BoxShadow> {
    let dark = theme.mode.is_dark();
    let ink = if dark { black() } else { theme.foreground };
    vec![
        BoxShadow { color: ink.opacity(if dark { 0.45 } else { 0.08 }), offset: point(px(0.), px(12.)), blur_radius: px(32.), spread_radius: px(-8.), inset: false },
        BoxShadow { color: ink.opacity(if dark { 0.3 } else { 0.05 }), offset: point(px(0.), px(1.)), blur_radius: px(3.), spread_radius: px(0.), inset: false },
    ]
}

/// Text to the clipboard. A probe says it instead: the clipboard is the
/// person's, and a copy under a script stays in its window.
pub fn to_clipboard(text: impl Into<String>, cx: &mut App) {
    let text = text.into();
    if std::env::var_os("ONYX_GO").is_some() {
        eprintln!("copy {text:?}");
    } else {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
}

/// A click the window takes for itself. The toolkit's text selection
/// begins a drag at every left press and ends it at the release; a
/// handler that only stops the release leaves every later move extending
/// a selection nobody is making.
pub fn swallow_click(window: &mut Window, cx: &mut App) {
    gpui_base::TextSelection::end(window, cx);
    cx.stop_propagation();
}

/// What moves in a view: the presences it has drawn and its choices'
/// plates. A view keeps one and draws its switches, choices and small
/// things through it, so a change is played once and the first sight
/// plays nothing.
#[derive(Default)]
pub struct Motion {
    /// Each thing that is there or is not: whether it is, when that
    /// changed, and how many times it has.
    presences: RefCell<HashMap<String, (bool, Instant, u32)>>,
    /// Each segmented control: the choice it came from, the one it is on,
    /// and when it changed.
    choices: RefCell<HashMap<&'static str, (&'static str, &'static str, Option<Instant>)>>,
    /// Where each segment sat at the last draw, relative to its track.
    segments: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
}

impl Motion {
    /// How something that is there or is not stands at this draw, asked
    /// at every draw whether it is there or not, so a change is seen as
    /// one: whether it is coming, and the change's number, while it moves;
    /// nothing at rest. What was never drawn before is not played. One
    /// going is drawn until its time is up, and once more then as nothing.
    pub fn presence<V: 'static>(&self, id: &str, on: bool, cx: &mut Context<V>) -> Option<(bool, u32)> {
        let mut all = self.presences.borrow_mut();
        let now = Instant::now();
        let p = all.entry(id.to_string()).or_insert_with(|| (on, now.checked_sub(FOLD_ANIM * 2).unwrap_or(now), 0));
        if p.0 != on {
            *p = (on, now, p.2 + 1);
            if !on {
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(FOLD_ANIM + Duration::from_millis(20)).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        }
        (p.2 > 0 && p.1.elapsed() < FOLD_ANIM).then_some((on, p.2))
    }

    /// How far a presence's change has run, eased as it is drawn, and 1
    /// once it is over or was never asked.
    pub fn progress(&self, id: &str) -> f32 {
        self.presences.borrow().get(id).map(|p| ease_out_quint()((p.1.elapsed().as_secs_f32() / FOLD_ANIM.as_secs_f32()).min(1.))).unwrap_or(1.)
    }

    /// A switch, for what is on or off: the track fills with the accent
    /// and the knob slides to its end, both over a moment. A press
    /// anywhere on it turns it.
    #[allow(clippy::type_complexity)]
    pub fn switch<V: 'static>(&self, id: &'static str, on: bool, turn: Rc<dyn Fn(&mut V, bool, &mut Window, &mut Context<V>)>, cx: &mut Context<V>) -> AnyElement {
        const W: f32 = 36.;
        const H: f32 = 20.;
        const KNOB: f32 = 16.;
        const INSET: f32 = 2.;
        let travel = W - KNOB - INSET * 2.;
        let theme = cx.theme().clone();
        let moving = self.presence(&format!("switch-{id}"), on, cx);
        let at = move |t: f32| px(INSET + travel * t);
        let fill = div().absolute().inset_0().rounded_full().bg(theme.primary);
        let knob = div().absolute().top(px(INSET)).size(px(KNOB)).rounded_full().bg(white()).shadow_sm();
        let (fill, knob) = match moving {
            Some((coming, n)) => (
                fill.with_animation(ElementId::Name(format!("switch-{id}-fill-{n}").into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element(),
                knob.with_animation(ElementId::Name(format!("switch-{id}-knob-{n}").into()), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.left(at(if coming { t } else { 1. - t }))).into_any_element(),
            ),
            None => (fill.opacity(if on { 1. } else { 0. }).into_any_element(), knob.left(at(if on { 1. } else { 0. })).into_any_element()),
        };
        div()
            .id(SharedString::from(format!("switch-{id}")))
            .relative()
            .flex_shrink_0()
            .w(px(W))
            .h(px(H))
            .rounded_full()
            .bg(theme.switch)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| turn(this, !on, window, cx)))
            .child(fill)
            .child(knob)
            .into_any_element()
    }

    /// A choice among a few: a muted track, the chosen one on a raised
    /// plate. The plate is one element under the row, placed from where
    /// each segment sat at the last draw, and slides from the old choice
    /// to the new over a moment, keyed on the new choice so a click plays
    /// it once. On the first draw, before any segment is measured, the
    /// chosen one paints its own plate, so nothing flashes. `fill` shares
    /// the width it is given among the choices.
    #[allow(clippy::type_complexity)]
    pub fn segmented<V: 'static>(&self, control: &'static str, options: Vec<(&'static str, &'static str, SharedString)>, current: &'static str, fill: bool, on: Rc<dyn Fn(&mut V, &'static str, &mut Window, &mut Context<V>)>, cx: &mut Context<V>) -> AnyElement {
        let (pad, seg_h, seg_px, seg_text, round) = (px(3.), px(22.), px(11.), px(12.5), px(6.));
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let track_bg = if dark { theme.tab_bar_segmented } else { theme.muted };
        let plate_bg = if dark { theme.secondary_active } else { theme.popover };
        let (from, to) = {
            let mut st = self.choices.borrow_mut();
            let e = st.entry(control).or_insert((current, current, None));
            if e.1 != current {
                e.0 = e.1;
                e.1 = current;
                e.2 = Some(Instant::now());
            }
            // The slide belongs to the change. A control drawn anew later
            // (its window opened again) is a new element, whose animation
            // would start over and slide the plate once more from a
            // choice made long before.
            let sliding = e.2.is_some_and(|at| at.elapsed() < SLIDE + Duration::from_millis(80));
            (if sliding { e.0 } else { e.1 }, e.1)
        };
        let (b_from, b_to) = {
            let b = self.segments.borrow();
            (b.get(&format!("{control}-{from}")).copied(), b.get(&format!("{control}-{to}")).copied())
        };
        let plate = match (b_from, b_to) {
            (Some(a), Some(b)) => Some(div().absolute().rounded(round).bg(plate_bg).shadow_sm().with_animation(ElementId::Name(format!("{control}-plate-{to}").into()), Animation::new(SLIDE).with_easing(ease_out_quint()), move |d, t| {
                let x = a.origin.x + (b.origin.x - a.origin.x) * t;
                let w = a.size.width + (b.size.width - a.size.width) * t;
                d.left(x).top(b.origin.y).w(w).h(b.size.height)
            })),
            _ => None,
        };
        let measured = plate.is_some();
        let ids: Vec<String> = options.iter().map(|(k, _, _)| format!("{control}-{k}")).collect();
        let segments = self.segments.clone();
        let entity = cx.entity().downgrade();
        let row = h_flex()
            .gap(px(2.))
            .when(fill, |d| d.flex_1().min_w_0())
            // Where each segment landed, relative to the track. A change
            // is noted, and the next draw places the plate from it.
            .on_children_prepainted(move |bounds, _, cx| {
                let Some(first) = bounds.first() else { return };
                let mut changed = false;
                let mut map = segments.borrow_mut();
                for (id, b) in ids.iter().zip(bounds.iter()) {
                    let rel = Bounds { origin: point(b.origin.x - first.origin.x + pad, pad), size: b.size };
                    if map.get(id) != Some(&rel) {
                        map.insert(id.clone(), rel);
                        changed = true;
                    }
                }
                drop(map);
                if changed {
                    let _ = entity.update(cx, |_, cx| cx.notify());
                }
            })
            .children(options.into_iter().map(|(key, icon, label)| {
                let active = key == current;
                let on = on.clone();
                let lit = theme.foreground;
                h_flex()
                    .id(SharedString::from(format!("{control}-{key}")))
                    .h(seg_h)
                    .px(seg_px)
                    .gap(px(5.))
                    .items_center()
                    .rounded(round)
                    .cursor_pointer()
                    .text_size(seg_text)
                    .when(fill, |d| d.flex_1().justify_center())
                    .when(active && !measured, |d| d.bg(plate_bg).shadow_sm())
                    // A hover that changes the text's color is set in every
                    // state, never under a `when`: gpui hears the pointer
                    // leave only while a hover style is set, and a segment
                    // came back to rest lit.
                    .text_color(if active { lit } else { theme.muted_foreground })
                    .hover(move |s| s.text_color(lit))
                    .when(active, |d| d.font_weight(FontWeight::MEDIUM))
                    .on_click(cx.listener(move |this, _, window, cx| on(this, key, window, cx)))
                    .when(!icon.is_empty(), |d| d.child(Icon::default().path(icon).with_size(seg_text + px(1.5))))
                    .child(label)
            }));
        h_flex()
            .id(SharedString::from(format!("{control}-track")))
            .relative()
            .p(pad)
            .rounded(round + px(2.))
            .bg(track_bg)
            .map(|d| if fill { d.w_full() } else { d.flex_shrink_0() })
            .children(plate)
            .child(row)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's glob brings its own `test` attribute.
    use super::{key_pieces, KeyPiece};

    fn shown(text: &str) -> Vec<String> {
        key_pieces(text)
            .into_iter()
            .map(|p| match p {
                KeyPiece::Words(w) => w,
                KeyPiece::Keys(k) => format!("[{}]", k.join(" ")),
            })
            .collect()
    }

    #[test]
    fn a_shortcut_is_its_keys_and_its_brackets_go() {
        assert_eq!(shown("Search (⌘K)"), ["Search ", "[⌘ K]"]);
        assert_eq!(shown("⇧Tab"), ["[⇧ Tab]"]);
        assert_eq!(shown("Send ⌘↩"), ["Send ", "[⌘ ↩]"]);
        assert_eq!(shown("No keys here"), ["No keys here"]);
    }
}

/// A drawing's answer to the pointer, as emaki's handscroll has it: how
/// far it is drawn to one side and how high, each from -1 to 1 with rest
/// at 0, how near the pointer is, from 0 away to 1 over it, how fast
/// each is moving, and where the pointer asks them to be. All follow on
/// a spring (`step`), so the drawing runs a little past and settles.
#[derive(Default)]
pub struct Fig {
    wind: f32,
    wind_v: f32,
    lift: f32,
    lift_v: f32,
    near: f32,
    near_v: f32,
    to: (f32, f32, f32),
    at: Option<Instant>,
    /// Where the drawing was last laid out, for the pointer's place on it.
    pub bounds: Bounds<Pixels>,
}

impl Fig {
    /// The pointer's place across and down the drawing, or none when it
    /// has left: the drawing is asked there, or back to rest.
    pub fn ask(&mut self, at: Option<Point<Pixels>>) {
        self.to = match at {
            Some(at) if self.bounds.size.width > px(0.) => (((at.x - self.bounds.left()) / self.bounds.size.width * 2. - 1.).clamp(-1., 1.), ((at.y - self.bounds.top()) / self.bounds.size.height * 2. - 1.).clamp(-1., 1.), 1.),
            _ => (0., 0., 0.),
        };
    }

    /// A frame's worth of the spring. Whether it is still moving.
    pub fn step(&mut self) -> bool {
        let now = Instant::now();
        let dt = self.at.map(|at| now.duration_since(at).as_secs_f32()).unwrap_or(0.).min(1. / 30.);
        self.at = Some(now);
        // Stiff enough to keep up with the pointer, loose enough to run
        // a little past where it stops.
        let (stiff, damp) = (90., 13.);
        for _ in 0..2 {
            let h = dt / 2.;
            self.wind_v += (stiff * (self.to.0 - self.wind) - damp * self.wind_v) * h;
            self.wind += self.wind_v * h;
            self.lift_v += (stiff * (self.to.1 - self.lift) - damp * self.lift_v) * h;
            self.lift += self.lift_v * h;
            self.near_v += (stiff * (self.to.2 - self.near) - damp * self.near_v) * h;
            self.near += self.near_v * h;
        }
        let still = |at: f32, to: f32, v: f32| (at - to).abs() < 0.002 && v.abs() < 0.01;
        if still(self.wind, self.to.0, self.wind_v) && still(self.lift, self.to.1, self.lift_v) && still(self.near, self.to.2, self.near_v) {
            (self.wind, self.lift, self.near, self.wind_v, self.lift_v, self.near_v) = (self.to.0, self.to.1, self.to.2, 0., 0., 0.);
            self.at = None;
            return false;
        }
        true
    }

    /// Across and down, each from -1 to 1, and how near, from 0 to 1.
    pub fn at(&self) -> (f32, f32, f32) {
        (self.wind, self.lift, self.near)
    }
}
