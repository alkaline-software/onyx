//! The tabs in the strip: one per open note, as emaki has one per open
//! session. A folder's tabs stand together on one pill, the segmented
//! control's track, with the folder's mark at its left end and the note
//! showing on a raised plate. Every width here is ours, so the plate is
//! placed by sum and never measured, and a change in how many tabs there
//! are is a move, not a jump: a new tab grows in from nothing while the
//! others give way, and a closed one's room is taken up the same way.
//!
//! A tab is as wide as its name needs, no wider; when the row has no
//! room for all of them, the widest give way first. Whatever changes the
//! row's room, each width and the row's place head for their new values
//! from where they stand, so the tabs glide however far or often it
//! changes. A tab is dragged to
//! another place on its pill and a pill along the row, by its handle,
//! and ⌘ held shows the number that goes to each tab.
//!
//! The tabs themselves (which notes are open, which shows) are the
//! shell's (`Shell::tabs`); this is their drawing and their motion.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use crate::i18n::t;
use crate::shell::Shell;
use crate::widgets::{swallow_click, tip};

/// A tab's width is its name's and no more: the title in the tab's type
/// plus `TAB_ROOM`, the edges and the close button, never under
/// `TAB_LEAST`, a comfortable tab for a short name, nor over `TAB_MAX`.
/// Once the row has no room for every tab at its own width, the widest
/// give way first, down to `TAB_MIN` each; past that the row scrolls.
pub const TAB_MAX: f32 = 200.;
pub const TAB_MIN: f32 = 88.;
pub const TAB_LEAST: f32 = 140.;
/// The tab's edges, the gap, the close button, the outline's two sides
/// and a little over, since a name wider than its room by a hair is cut
/// to an ellipsis.
const TAB_ROOM: f32 = 10. + 6. + 18. + 5. + 2. + 2.;
const TAB_TEXT: f32 = 12.5;
pub const TAB_GAP: f32 = 2.;
pub const TAB_ANIM: Duration = Duration::from_millis(220);
/// The room kept clear between the tabs and whatever stands at either
/// end of the strip.
pub const STRIP_GAP: f32 = 14.;
/// A pill holds one folder's tabs: a tab's height on it, the track's
/// padding around them, the folder's mark at its left end, and the gap
/// between two pills.
pub const TAB_H: f32 = 24.;
pub const PILL_PAD: f32 = 3.;
const PILL_HANDLE: f32 = 20.;
const PILL_GAP: f32 = 6.;
/// Where a pill's first tab starts.
const PILL_LEAD: f32 = PILL_PAD + PILL_HANDLE + TAB_GAP;
/// The square after the last tab, with a plus on it, which shows the
/// vault's page and leaves every tab open: a tab's height on a track
/// of its own, and the gap before it.
const PLUS_W: f32 = TAB_H + 2. * PILL_PAD;
const PLUS_ROOM: f32 = PLUS_W + PILL_GAP;
/// How long ⌘ (Ctrl elsewhere) is held, alone, before every tab shows
/// the number that goes to it.
const HINT_HOLD: Duration = Duration::from_millis(500);

/// What a pill is wide besides its tabs.
fn pill_chrome(tabs: usize) -> f32 {
    PILL_LEAD + PILL_PAD + TAB_GAP * (tabs.max(1) as f32 - 1.)
}

/// A length of the row on its way from one value to another: a tab's
/// width, how far in the pills start, how far the row is scrolled.
#[derive(Clone, Copy)]
struct Width {
    from: f32,
    to: f32,
    at: Instant,
}

impl Width {
    fn at_rest(value: f32, at: Instant) -> Self {
        Self { from: value, to: value, at }
    }

    fn now(&self) -> f32 {
        let t = (self.at.elapsed().as_secs_f32() / TAB_ANIM.as_secs_f32()).min(1.);
        self.from + (self.to - self.from) * ease_out_quint()(t)
    }

    fn done(&self) -> bool {
        self.at.elapsed() >= TAB_ANIM
    }

    /// Set off for `to` from where it stands, unless it is going there.
    /// Asked again at every frame of a window being dragged wider, it
    /// follows with no jump and catches up when the drag stops.
    fn head(&mut self, to: f32, at: Instant) {
        if (self.to - to).abs() > 0.5 {
            *self = Self { from: self.now(), to, at };
        }
    }
}

/// A tab or a pill on its way to the place a drag gave it: how far from
/// that place it started, and when. It is drawn that far off and comes in.
#[derive(Clone, Copy)]
struct Shift {
    from: f32,
    at: Instant,
}

impl Shift {
    fn done(&self) -> bool {
        self.at.elapsed() >= TAB_ANIM
    }

    fn now(&self) -> f32 {
        let t = (self.at.elapsed().as_secs_f32() / TAB_ANIM.as_secs_f32()).min(1.);
        self.from * (1. - ease_out_quint()(t))
    }
}

/// The plate under the showing tab: the tab it left, the one it is
/// under, and when that changed. Each change has a number, so its move
/// is played once.
#[derive(Default)]
struct Plate {
    from: Option<PathBuf>,
    to: Option<PathBuf>,
    at: Option<Instant>,
    serial: u32,
}

/// A tab being dragged along its pill, by its path.
#[derive(Clone)]
struct DragTab(PathBuf);

/// A pill being dragged along the row, by its folder.
#[derive(Clone)]
struct DragPill(PathBuf);

/// What follows the pointer while a tab or a pill is dragged: its name
/// on a plate.
struct Ghost(SharedString);

impl Render for Ghost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div().h(px(30.)).max_w(px(TAB_MAX)).px(px(10.)).flex().items_center().rounded(px(8.)).bg(theme.muted).border_1().border_color(theme.border).text_size(px(TAB_TEXT)).text_color(theme.foreground).truncate().opacity(0.9).child(self.0.clone())
    }
}

/// What the row is told of each tab.
pub struct Item {
    pub path: PathBuf,
    pub title: String,
}

#[derive(Default)]
pub struct Tabs {
    widths: HashMap<PathBuf, Width>,
    /// Each title's width in the tab's type, measured once.
    fit: HashMap<String, f32>,
    plate: Plate,
    /// When each folder's pill came, for its track to be let out.
    born: HashMap<PathBuf, Instant>,
    /// The row's width, given at each draw.
    row_w: f32,
    row_seen: f32,
    /// How far in from the row's left the pills start: half of what they
    /// leave over, so they stand in the middle, none once they fill it.
    lead: Option<Width>,
    /// The row's scroll on its way to the showing tab.
    glide: Option<Width>,
    /// How many tabs the widths were last set for.
    n: usize,
    /// The showing tab is brought into the row at the next sync.
    reveal: bool,
    pub scroll: ScrollHandle,
    groups: Vec<(PathBuf, Vec<PathBuf>)>,
    /// The tab or the pill under the pointer in a drag, drawn faint.
    drag_tab: Option<PathBuf>,
    drag_pill: Option<PathBuf>,
    /// A tab or a pill a drag moved, on its way to its new place.
    shift: HashMap<PathBuf, Shift>,
    pill_shift: HashMap<PathBuf, Shift>,
    /// The pill being moved, painted over the ones it passes.
    pill_top: Option<PathBuf>,
    /// The numbers ⌘ and a digit go to the tabs with are up.
    pub hints: bool,
    /// The press of ⌘ alone being waited out, and how many there were.
    hint_press: Option<u32>,
    hint_presses: u32,
}

impl Tabs {
    /// The pills: one folder's tabs together, the pills in the order
    /// their first tabs had and each pill's tabs in theirs.
    pub fn group(tabs: &[PathBuf]) -> Vec<(PathBuf, Vec<PathBuf>)> {
        let mut groups: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
        for path in tabs {
            let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
            match groups.iter_mut().find(|(f, _)| *f == folder) {
                Some((_, keys)) => keys.push(path.clone()),
                None => groups.push((folder, vec![path.clone()])),
            }
        }
        groups
    }

    /// The order the row has: each pill's tabs, pill by pill.
    pub fn order(groups: &[(PathBuf, Vec<PathBuf>)]) -> Vec<PathBuf> {
        groups.iter().flat_map(|(_, keys)| keys.iter().cloned()).collect()
    }

    /// The width a title asks for: measured in the tab's type at its
    /// heavier weight, the showing tab's, so the name fits in either.
    fn fit(&mut self, title: &str, window: &Window, cx: &App) -> f32 {
        if let Some(w) = self.fit.get(title) {
            return *w;
        }
        let font = Font { family: cx.theme().font_family.clone(), weight: FontWeight::MEDIUM, ..Default::default() };
        let run = TextRun { len: title.len(), font, color: gpui::black(), background_color: None, underline: None, strikethrough: None };
        let text = f32::from(window.text_system().shape_line(SharedString::from(title.to_string()), px(TAB_TEXT), &[run], None).width());
        let w = (text + TAB_ROOM).ceil().clamp(TAB_LEAST, TAB_MAX);
        self.fit.insert(title.to_string(), w);
        w
    }

    /// Each tab's width given the room: its own while they all fit, else
    /// the widest cut to one cap, the cap set so the row is filled, and
    /// no tab under `TAB_MIN`. The narrow keep their width throughout.
    fn share(wanted: &[f32], room: f32) -> Vec<f32> {
        if wanted.iter().sum::<f32>() <= room {
            return wanted.to_vec();
        }
        let mut sorted = wanted.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let (mut left, mut count) = (room, sorted.len());
        let mut cap = TAB_MIN;
        for w in sorted {
            if w * count as f32 <= left {
                left -= w;
                count -= 1;
            } else {
                cap = (left / count as f32).max(TAB_MIN);
                break;
            }
        }
        wanted.iter().map(|w| w.min(cap)).collect()
    }

    /// Give every tab its width and the plate its tab, at every draw.
    /// `row_w` is figured by the caller as the frame is built and never
    /// measured off the last one: a measure taken while a frame is drawn
    /// can ask for no other frame (`docs/window.md`).
    pub fn sync(&mut self, row_w: f32, items: &[Item], shown: Option<&Path>, window: &Window, cx: &App) {
        self.row_w = row_w;
        if !cx.has_active_drag() {
            self.drag_tab = None;
            self.drag_pill = None;
        }
        self.shift.retain(|_, s| !s.done());
        self.pill_shift.retain(|_, s| !s.done());
        if self.pill_shift.is_empty() {
            self.pill_top = None;
        }
        // The key let go while the window was not looking (⌘Tab to
        // another app sends no release here) takes the numbers with it.
        if self.hint_press.is_some() && !(window.is_window_active() && window.modifiers().secondary()) {
            self.hint_press = None;
            self.hints = false;
        }
        let tabs: Vec<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
        self.groups = Self::group(&tabs);
        let keys = Self::order(&self.groups);
        let first = self.widths.is_empty();
        self.widths.retain(|k, _| keys.contains(k));
        let groups = &self.groups;
        self.born.retain(|f, _| groups.iter().any(|(g, _)| g == f));
        let showing = shown.filter(|s| keys.iter().any(|k| k == *s)).map(Path::to_path_buf);
        if self.plate.to != showing {
            self.reveal = true;
            self.plate.from = self.plate.to.take().filter(|_| !first);
            self.plate.to = showing;
            self.plate.at = (!first).then(Instant::now);
            self.plate.serial += 1;
        }
        if keys.is_empty() {
            self.n = 0;
            self.lead = None;
            self.glide = None;
            return;
        }
        let now = Instant::now();
        for (folder, _) in &self.groups {
            if !self.born.contains_key(folder) {
                // One there from the start is given a past it is done with.
                self.born.insert(folder.clone(), if first { now.checked_sub(TAB_ANIM * 2).unwrap_or(now) } else { now });
            }
        }
        // The row less the plus square's room, which is always there.
        let row = (row_w - PLUS_ROOM).max(0.);
        let n = keys.len();
        let chrome: f32 = self.groups.iter().map(|(_, keys)| pill_chrome(keys.len())).sum::<f32>() + PILL_GAP * (self.groups.len() as f32 - 1.).max(0.);
        let wanted: Vec<f32> = keys.iter().map(|k| items.iter().find(|i| i.path == *k).map(|i| self.fit(&i.title, window, cx)).unwrap_or(TAB_MIN)).collect();
        let targets = if row <= 0. { wanted.clone() } else { Self::share(&wanted, row - chrome) };
        let lead = ((row - chrome - targets.iter().sum::<f32>()) / 2.).max(0.);
        match &mut self.lead {
            Some(l) => l.head(lead, now),
            None => self.lead = Some(Width::at_rest(lead, now)),
        }
        // The tab shown is brought into the row when the tabs are wider
        // than it and the tab stands outside: by sum, as every place here is.
        let reveal = (self.reveal || (row - self.row_seen).abs() > 0.5) && row > 0.;
        if reveal {
            self.reveal = false;
            self.row_seen = row;
        }
        if reveal && chrome + targets.iter().sum::<f32>() > row {
            let mut x = 0.;
            let mut span = None;
            let mut at = 0;
            for (_, keys) in &self.groups {
                let mut tab_x = x + PILL_LEAD;
                for (i, k) in keys.iter().enumerate() {
                    if Some(k) == self.plate.to.as_ref() {
                        span = Some((x, tab_x + targets[at + i] + PILL_PAD));
                    }
                    tab_x += targets[at + i] + TAB_GAP;
                }
                x += pill_chrome(keys.len()) + targets[at..at + keys.len()].iter().sum::<f32>() + PILL_GAP;
                at += keys.len();
            }
            if let Some((left, right)) = span {
                let at = self.scroll.offset();
                let seen = -f32::from(at.x);
                let to = if left < seen { left } else if right > seen + row { right - row } else { seen };
                if (to - seen).abs() > 0.5 {
                    self.glide.get_or_insert(Width::at_rest(seen, now)).head(to, now);
                }
            }
        }
        if let Some(g) = self.glide {
            self.scroll.set_offset(point(px(-g.now()), self.scroll.offset().y));
            if g.done() {
                self.glide = None;
            }
        }
        let moved = n != self.n && !first;
        self.n = n;
        for (key, target) in keys.iter().zip(targets) {
            match self.widths.get_mut(key) {
                None => {
                    self.widths.insert(key.clone(), Width { from: if moved { 0. } else { target }, to: target, at: now });
                }
                Some(w) => w.head(target, now),
            }
        }
    }

    /// Whether anything in the row is on its way somewhere.
    pub fn moving(&self) -> bool {
        self.widths.values().chain(&self.lead).chain(&self.glide).any(|w| !w.done()) || self.plate.at.is_some_and(|at| at.elapsed() < TAB_ANIM + Duration::from_millis(80)) || self.born.values().any(|at| at.elapsed() < TAB_ANIM) || self.shift.values().chain(self.pill_shift.values()).any(|s| !s.done())
    }

    /// A tab's settled width.
    fn width_of(&self, key: &Path) -> f32 {
        self.widths.get(key).map(|w| w.to).unwrap_or(TAB_MIN)
    }

    /// Where each pill stands in the row once every width has settled:
    /// its left edge and its width, in the window's coordinates.
    fn pill_spans(&self, row: Bounds<Pixels>) -> Vec<(f32, f32)> {
        let widths: Vec<f32> = self.groups.iter().map(|(_, keys)| pill_chrome(keys.len()) + keys.iter().map(|k| self.width_of(k)).sum::<f32>()).collect();
        // From the row's left, as far in as they settle, moved by how far
        // the row is scrolled.
        let mut x = f32::from(row.left()) + self.lead.map_or(0., |l| l.to) + f32::from(self.scroll.offset().x);
        widths
            .into_iter()
            .map(|w| {
                let at = x;
                x += w + PILL_GAP;
                (at, w)
            })
            .collect()
    }

    /// Where each tab of a pill starts, from the pill's left edge.
    fn tab_lefts(&self, keys: &[PathBuf]) -> Vec<f32> {
        let mut x = PILL_LEAD;
        keys.iter()
            .map(|k| {
                let at = x;
                x += self.width_of(k) + TAB_GAP;
                at
            })
            .collect()
    }

    /// A tab dragged along its pill stands after every other tab of the
    /// pill whose middle the pointer has passed, and goes no further than
    /// the pill's ends; the one tab of a pill takes the pill with it.
    /// Going by the middle keeps two tabs of different widths from
    /// changing places back and forth under a pointer that is still.
    /// Returns the row's new order when the drag changed it.
    fn drag_tab_to(&mut self, key: &Path, x: Pixels, row: Bounds<Pixels>) -> Option<Vec<PathBuf>> {
        let g = self.groups.iter().position(|(_, keys)| keys.iter().any(|k| k == key))?;
        let (folder, keys) = self.groups[g].clone();
        if keys.len() == 1 {
            return self.drag_pill_to(&folder, x, row);
        }
        let (left, _) = *self.pill_spans(row).get(g)?;
        let from = keys.iter().position(|k| k == key)?;
        let lefts = self.tab_lefts(&keys);
        let x = f32::from(x) - left;
        let to = keys.iter().enumerate().filter(|(i, k)| *i != from && lefts[*i] + self.width_of(k) / 2. < x).count();
        let changed = self.drag_tab.as_deref() != Some(key);
        self.drag_tab = Some(key.to_path_buf());
        if to == from {
            return changed.then(|| Self::order(&self.groups));
        }
        let mut moved = keys.clone();
        let tab = moved.remove(from);
        moved.insert(to, tab);
        // Each tab that changed places slides to its new one from where
        // it is now, which is not its old place if it was still on its
        // way there.
        let after = self.tab_lefts(&moved);
        let now = Instant::now();
        for (i, k) in keys.iter().enumerate() {
            let new = moved.iter().position(|t| t == k).unwrap_or(i);
            if (lefts[i] - after[new]).abs() > 0.5 {
                let rest = self.shift.get(k).map(|s| s.now()).unwrap_or(0.);
                self.shift.insert(k.clone(), Shift { from: lefts[i] - after[new] + rest, at: now });
            }
        }
        self.groups[g].1 = moved;
        Some(Self::order(&self.groups))
    }

    /// A pill dragged along the row stands after every pill whose middle
    /// the pointer has passed. Returns the row's new order when the drag
    /// changed it.
    fn drag_pill_to(&mut self, folder: &Path, x: Pixels, row: Bounds<Pixels>) -> Option<Vec<PathBuf>> {
        let from = self.groups.iter().position(|(f, _)| f == folder)?;
        let spans = self.pill_spans(row);
        let to = spans.iter().enumerate().filter(|(i, (left, w))| *i != from && left + w / 2. < f32::from(x)).count();
        let changed = self.drag_pill.as_deref() != Some(folder);
        self.drag_pill = Some(folder.to_path_buf());
        if to == from {
            return changed.then(|| Self::order(&self.groups));
        }
        self.pill_top = Some(folder.to_path_buf());
        let before: HashMap<PathBuf, f32> = self.groups.iter().zip(spans.iter()).map(|((f, _), (left, _))| (f.clone(), *left)).collect();
        let pill = self.groups.remove(from);
        self.groups.insert(to, pill);
        // Every pill that moved slides to where it now stands.
        let now = Instant::now();
        for ((f, _), (left, _)) in self.groups.iter().zip(self.pill_spans(row)) {
            let was = before.get(f).copied().unwrap_or(left);
            if (was - left).abs() > 0.5 {
                let rest = self.pill_shift.get(f).map(|s| s.now()).unwrap_or(0.);
                self.pill_shift.insert(f.clone(), Shift { from: was - left + rest, at: now });
            }
        }
        Some(Self::order(&self.groups))
    }

    /// ⌘ (Ctrl elsewhere) held by itself for `HINT_HOLD` shows each tab's
    /// number, the one ⌘ and a digit goes to, and they go when the key is
    /// let go or another modifier joins it. A quick ⌘C never sees them.
    /// The wait is a timer from the press, answered only if that press is
    /// still the one held.
    pub fn modifiers_changed(&mut self, m: &Modifiers, cx: &mut Context<Shell>) {
        let alone = m.secondary() && m.number_of_modifiers() == 1;
        if alone == self.hint_press.is_some() {
            return;
        }
        if !alone {
            self.hint_press = None;
            if std::mem::take(&mut self.hints) {
                cx.notify();
            }
            return;
        }
        self.hint_presses += 1;
        let press = self.hint_presses;
        self.hint_press = Some(press);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HINT_HOLD).await;
            let _ = this.update(cx, |this, cx| {
                if this.tabs_row.hint_press == Some(press) && !this.tabs_row.groups.is_empty() {
                    this.tabs_row.hints = true;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The row: the pills at their widths, `lead` in from its left, in
    /// the middle while they fit and from the left with the row scrolling
    /// once they do not.
    pub fn render(&self, items: &[Item], shown: Option<&Path>, root: Option<&Path>, cx: &mut Context<Shell>) -> AnyElement {
        let theme = cx.theme().clone();
        let dark = theme.mode.is_dark();
        let track_bg = if dark { theme.tab_bar_segmented } else { theme.muted };
        let plate_bg = if dark { theme.secondary_active } else { theme.popover };
        let settled = Width::at_rest(TAB_MIN, Instant::now());
        let hints = self.hints;
        let mut row = h_flex()
            .id("tabs")
            .flex_1()
            .min_w_0()
            .h_full()
            .mx(px(STRIP_GAP))
            .pl(px(self.lead.map_or(0., |l| l.now())))
            .items_center()
            .gap(px(PILL_GAP))
            .overflow_x_scroll()
            .track_scroll(&self.scroll)
            .on_drag_move::<DragTab>(cx.listener(|this, e: &DragMoveEvent<DragTab>, window, cx| {
                let key = e.drag(cx).0.clone();
                if let Some(order) = this.tabs_row.drag_tab_to(&key, e.event.position.x, e.bounds) {
                    this.reorder_tabs(&order, window, cx);
                }
            }))
            .on_drag_move::<DragPill>(cx.listener(|this, e: &DragMoveEvent<DragPill>, window, cx| {
                let folder = e.drag(cx).0.clone();
                if let Some(order) = this.tabs_row.drag_pill_to(&folder, e.event.position.x, e.bounds) {
                    this.reorder_tabs(&order, window, cx);
                }
            }));
        let live = self.plate.at.is_some_and(|at| at.elapsed() < TAB_ANIM + Duration::from_millis(80));
        let mut ix: usize = 0;
        for (folder, keys) in &self.groups {
            let widths: Vec<Width> = keys.iter().map(|k| self.widths.get(k).copied().unwrap_or(settled)).collect();
            // A tab a drag has just moved is drawn off its place by what
            // is left of the way, and the plate under it goes with it.
            let shifts: Vec<Option<Shift>> = keys.iter().map(|k| self.shift.get(k).copied()).collect();
            // The plate: from the tab left to the tab shown when both are
            // on this pill, else in or out where it is. A move that is
            // over is drawn as it ended, so a pill drawn afresh does not
            // play it.
            let at = |k: &Option<PathBuf>| k.as_ref().and_then(|k| keys.iter().position(|t| t == k));
            let (a, b) = (at(&self.plate.from).filter(|_| live), at(&self.plate.to));
            let plate = (a.is_some() || b.is_some()).then(|| {
                let (ia, ib) = (a.or(b).unwrap_or(0), b.or(a).unwrap_or(0));
                let (o_a, o_b) = (if a.is_some() || !live { 1. } else { 0. }, if b.is_some() { 1. } else { 0. });
                let widths = widths.clone();
                let shifts = shifts.clone();
                div().absolute().top(px(PILL_PAD)).h(px(TAB_H)).rounded(px(7.)).bg(plate_bg).shadow_sm().with_animation(
                    ElementId::Name(format!("tab-plate-{}-{}", folder.display(), self.plate.serial).into()),
                    Animation::new(TAB_ANIM).with_easing(ease_out_quint()),
                    move |d, t| {
                        let place = |i: usize| (PILL_LEAD + widths[..i].iter().map(|w| w.now() + TAB_GAP).sum::<f32>() + shifts[i].map(|s| s.now()).unwrap_or(0.), widths[i].now());
                        let ((xa, wa), (xb, wb)) = (place(ia), place(ib));
                        d.left(px(xa + (xb - xa) * t)).w(px(wa + (wb - wa) * t)).opacity(o_a + (o_b - o_a) * t)
                    },
                )
            });
            // The folder's mark says whose the pill is and moves the pill;
            // its hover names the folder from the vault's root, or the
            // vault itself.
            let name: SharedString = match root {
                Some(root) if folder == root => root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().into(),
                Some(root) if *folder == onyx_core::vault::assets(root) => crate::i18n::t("Assets").into(),
                Some(root) => folder.strip_prefix(root).unwrap_or(folder).to_string_lossy().to_string().into(),
                None => folder.to_string_lossy().to_string().into(),
            };
            let ghost = name.clone();
            let handle = div()
                .id(ElementId::Name(format!("pill-{}", folder.display()).into()))
                .w(px(PILL_HANDLE))
                .h(px(TAB_H))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .cursor_grab()
                .text_color(theme.muted_foreground)
                .hover(|s| s.text_color(theme.foreground))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .on_mouse_down(MouseButton::Right, cx.listener({
                    let folder = folder.clone();
                    move |this, ev: &MouseDownEvent, window, cx| {
                        swallow_click(window, cx);
                        this.show_pill_menu(folder.clone(), ev.position, cx);
                    }
                }))
                .on_drag(DragPill(folder.clone()), move |_, _, _, cx| cx.new(|_| Ghost(ghost.clone())))
                .managed_tooltip(move |window, cx| tip(name.clone(), window, cx))
                .child(Icon::new(IconName::Folder).with_size(px(13.)));
            let born = self.born.get(folder).is_some_and(|at| at.elapsed() < TAB_ANIM);
            let mut pill = h_flex()
                .relative()
                .flex_shrink_0()
                .p(px(PILL_PAD))
                .gap(px(TAB_GAP))
                .rounded(px(9.))
                .bg(track_bg)
                .when(self.drag_pill.as_deref() == Some(folder.as_path()), |d| d.opacity(0.45))
                .when_some(self.pill_shift.get(folder), |d, s| d.left(px(s.now())))
                .children(plate)
                .child(handle);
            for ((key, width), shift) in keys.iter().zip(widths.iter().copied()).zip(shifts.iter().copied()) {
                let item = items.iter().find(|i| i.path == *key);
                let title: SharedString = item.map(|i| i.title.clone()).unwrap_or_default().into();
                let active = shown == Some(key.as_path());
                let dragged = self.drag_tab.as_deref() == Some(key.as_path());
                let (open_key, close_key) = (key.clone(), key.clone());
                let hover_bg = plate_bg.opacity(0.5);
                let close_bg = theme.border;
                let ghost = title.clone();
                let fresh = width.from == 0. && !width.done();
                let tab = h_flex()
                    .id(ElementId::Name(format!("tab-{}", key.display()).into()))
                    .size_full()
                    .pl(px(10.))
                    .pr(px(5.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(7.))
                    // Always there, so nothing moves when it shows: the
                    // outline every tab wears while the numbers are up.
                    .border_1()
                    .border_color(if hints { theme.border } else { gpui::transparent_black() })
                    .cursor_pointer()
                    .overflow_hidden()
                    .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
                    .when(dragged, |d| d.opacity(0.45))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                    .on_mouse_down(MouseButton::Right, cx.listener({
                        let key = key.clone();
                        move |this, ev: &MouseDownEvent, window, cx| {
                            swallow_click(window, cx);
                            this.show_entry_menu(key.clone(), ev.position, cx);
                        }
                    }))
                    .on_drag(DragTab(key.clone()), move |_, _, _, cx| cx.new(|_| Ghost(ghost.clone())))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.show_tab(&open_key, window, cx)
                    }))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(TAB_TEXT)).font_weight(if active { FontWeight::MEDIUM } else { FontWeight::NORMAL }).text_color(if active { theme.foreground } else { theme.muted_foreground }).child(title))
                    // The close button. A note is written on its own
                    // (`Shell::note_changed`), so no dot says it is not.
                    .child(
                        div()
                            .id(ElementId::Name(format!("tab-close-{}", key.display()).into()))
                            .size(px(18.))
                            .flex_shrink_0()
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(move |s| s.bg(close_bg))
                            .managed_tooltip(|window, cx| tip(t("Close Tab (⌘W)"), window, cx))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.close_tab(&close_key, window, cx);
                            }))
                            .child(Icon::new(IconName::Close).xsmall().text_color(theme.muted_foreground)),
                    );
                // The number ⌘ goes to this tab with (`go_tab`): the first
                // nine, in the row's order. The tab clips what it holds,
                // so the plate is beside it in a box of the tab's size.
                let plate = (ix < 9).then_some(ix + 1).filter(|_| hints).map(|n| hint_plate(n, &theme));
                ix += 1;
                // The width is read off the clock the plate reads, so the
                // two never part by a frame; the animation is only there
                // for that reading, and its id does not change with a
                // width, or a press on a moving tab would lose its click.
                let slot = div().relative().h(px(TAB_H)).flex_shrink_0().child(tab).children(plate);
                pill = pill.child(slot.with_animation(ElementId::Name(format!("tab-w-{}", key.display()).into()), Animation::new(TAB_ANIM).with_easing(ease_out_quint()), move |d, t| {
                    let d = d.w(px(width.now())).left(px(shift.map(|s| s.now()).unwrap_or(0.)));
                    if fresh {
                        d.opacity(t)
                    } else {
                        d
                    }
                }));
            }
            // A new pill's track is let out with its tab, in a wrapper
            // that clips it. The pill being moved passes over the ones it
            // changes places with, not under them: painted after the row.
            let pill_w = pill_chrome(keys.len()) + widths.iter().map(|w| w.now()).sum::<f32>();
            let wrap = div().flex_shrink_0().when(born, |d| d.overflow_hidden()).child(pill);
            row = row.child(if born {
                wrap.with_animation(ElementId::Name(format!("pill-born-{}", folder.display()).into()), Animation::new(TAB_ANIM).with_easing(ease_out_quint()), move |d, t| d.w(px(pill_w * t)).opacity(t))
                .into_any_element()
            } else if self.pill_top.as_deref() == Some(folder.as_path()) {
                deferred(wrap).into_any_element()
            } else {
                wrap.into_any_element()
            });
        }
        // The plus after the last tab: a new note, in the folder of the
        // note showing, its name asked first.
        let hover_bg = plate_bg.opacity(0.5);
        row = row.child(
            div()
                .id("tab-plus")
                .size(px(PLUS_W))
                .flex_shrink_0()
                .p(px(PILL_PAD))
                .rounded(px(9.))
                .bg(track_bg)
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .managed_tooltip(|window, cx| tip(t("New Note (⌘N)"), window, cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.new_note(&crate::shell::NewNote, window, cx)
                }))
                .child(
                    div()
                        .size(px(TAB_H))
                        .rounded(px(7.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(move |s| s.bg(hover_bg))
                        .child(Icon::default().path("icons/plus.svg").with_size(px(14.)).text_color(theme.muted_foreground)),
                ),
        );
        row.into_any_element()
    }
}

/// The number ⌘ and a digit goes to a tab with (`go_tab`), on a small
/// plate set half over the tab's lower edge while ⌘ is held.
fn hint_plate(n: usize, theme: &gpui_component::Theme) -> Div {
    div().absolute().left_0().right_0().bottom(px(-8.)).flex().justify_center().child(
        div()
            .h(px(16.))
            .min_w(px(18.))
            .px(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(5.))
            .bg(theme.background)
            .border_1()
            .border_color(theme.border)
            .shadow_sm()
            .text_size(px(10.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.foreground)
            .child(n.to_string())
            .with_animation("tab-hint", Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()), |d, t| d.opacity(t)),
    )
}

#[cfg(test)]
mod tests {
    // Named, not `super::*`: gpui's glob carries a `test` macro of its own.
    use super::{Tabs, Width, TAB_ANIM, TAB_MIN};
    use std::time::Instant;

    /// A new target starts from where the length stands, not from where
    /// it was going, and the same target asked again does not start over.
    #[test]
    fn a_length_heads_off_from_where_it_stands() {
        let now = Instant::now();
        let mut w = Width { from: 100., to: 200., at: now.checked_sub(TAB_ANIM / 4).unwrap() };
        let stood = w.now();
        assert!(stood > 100. && stood < 200.);
        w.head(50., now);
        assert!((w.from - stood).abs() < 5. && w.to == 50. && w.at == now);
        let later = now + TAB_ANIM;
        w.head(50.2, later);
        assert!(w.to == 50. && w.at == now);
    }

    /// The widest tabs give way first, the narrow keep their width, and
    /// none goes under `TAB_MIN`.
    #[test]
    fn tabs_share_the_row_widest_first() {
        assert_eq!(Tabs::share(&[60., 120., 200.], 400.), vec![60., 120., 200.]);
        let cut = Tabs::share(&[60., 120., 200.], 300.);
        assert_eq!(cut[0], 60.);
        assert_eq!(cut[1], 120.);
        assert!((cut[2] - 120.).abs() < 0.01);
        let floor = Tabs::share(&[60., 200., 200.], 100.);
        assert_eq!(floor, vec![60., TAB_MIN, TAB_MIN]);
    }
}
