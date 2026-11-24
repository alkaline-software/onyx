//! A folder's look: an icon of Tabler's outline set and a color, chosen in
//! the window (`Picker`) and kept in the vault, in `.onyx/folders.json`,
//! so it goes wherever the vault goes. The main window's tree, the menu
//! bar note's and the tabs' pills draw a folder by it; a folder with no
//! look of its own keeps Catppuccin's icon.
//!
//! Tabler's set is carried as one file (`assets/tabler/outline.tsv`, made
//! by `scripts/tabler.py`): a line an icon, its name, category, tags and
//! drawing, served as `tabler/<name>.svg` (`assets.rs`).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::SystemTime;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};
use onyx_core::config::Config;
use serde::{Deserialize, Serialize};

use crate::i18n::t;
use crate::widgets::{small_pill, swallow_click, Press};

// -- Tabler's icons ---------------------------------------------------------

struct Glyph {
    name: &'static str,
    /// The name, the category and the tags, in lower case, for the search.
    words: String,
    body: &'static str,
}

fn glyphs() -> &'static [Glyph] {
    static GLYPHS: OnceLock<Vec<Glyph>> = OnceLock::new();
    GLYPHS.get_or_init(|| {
        include_str!("../assets/tabler/outline.tsv")
            .lines()
            .filter_map(|line| {
                let mut part = line.splitn(4, '\t');
                let (name, category, tags, body) = (part.next()?, part.next()?, part.next()?, part.next()?);
                Some(Glyph { name, words: format!("{name} {} {}", category.to_lowercase(), tags.to_lowercase()), body })
            })
            .collect()
    })
}

fn by_name() -> &'static HashMap<&'static str, usize> {
    static NAMES: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    NAMES.get_or_init(|| glyphs().iter().enumerate().map(|(i, g)| (g.name, i)).collect())
}

/// The asset at `tabler/<name>.svg`: the icon drawn as Tabler draws it,
/// strokes of two in a box of 24, in the ink it is given.
pub fn svg(path: &str) -> Option<Vec<u8>> {
    let name = path.strip_prefix("tabler/")?.strip_suffix(".svg")?;
    let glyph = &glyphs()[*by_name().get(name)?];
    Some(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#, glyph.body).into_bytes())
}

fn glyph_path(name: &str) -> SharedString {
    format!("tabler/{name}.svg").into()
}

/// The icons offered first, after the folder's default and before the
/// rest in Tabler's order: the ones a folder of notes is most often.
/// Tabler's own folder is not among them: the default is a folder, and
/// takes a color as any icon does.
const FIRST: &[&str] = &[
    "notebook", "book", "books", "notes", "pencil", "bookmark", "star", "heart", "bulb", "briefcase", "school", "home", "user", "users", "calendar", "clock", "inbox", "archive", "box", "tag", "flag", "map", "plane", "car", "camera", "photo", "music", "movie", "palette", "brush", "code", "terminal", "database", "server", "flask", "atom", "math", "chart-bar", "coin", "shopping-cart", "gift", "tools", "settings", "lock", "world", "leaf", "plant", "sun", "moon", "cloud", "coffee", "chef-hat", "barbell", "run", "heartbeat", "pill", "paw", "rocket", "trophy", "puzzle", "device-gamepad", "message", "mail", "phone", "news", "file-text", "list-check", "checklist", "target", "compass", "anchor", "building", "building-bank", "scale", "gavel",
];

/// The icons a search finds, the best first: a name that is the words, then
/// names that start with them, hold them, and last a tag or a category
/// that does. Every word must be found.
pub fn search(query: &str) -> Vec<&'static str> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        let first: Vec<&'static str> = FIRST.iter().filter_map(|n| by_name().get(n).map(|i| glyphs()[*i].name)).collect();
        return first.iter().copied().chain(glyphs().iter().map(|g| g.name).filter(|n| !first.contains(n))).collect();
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut hits: Vec<(u8, &'static str)> = glyphs()
        .iter()
        .filter(|g| words.iter().all(|w| g.words.contains(w)))
        .map(|g| (if g.name == q { 0 } else if g.name.starts_with(q.as_str()) { 1 } else if g.name.contains(q.as_str()) { 2 } else { 3 }, g.name))
        .collect();
    hits.sort_by_key(|h| h.0);
    hits.into_iter().map(|h| h.1).collect()
}

// -- the colors -------------------------------------------------------------

/// The colors offered for a folder: each holds 3 to 1 or better on the
/// window's ground in both appearances.
pub const PRESETS: [&str; 10] = ["#E5484D", "#EC6A2C", "#D9A21B", "#46A758", "#12A594", "#0D9DC4", "#3E63DD", "#7C5CDB", "#D6409F", "#8B7E74"];

pub fn parse_hex(s: &str) -> Option<Hsla> {
    let (r, g, b) = rgb_of(s)?;
    Some(Rgba { r, g, b, a: 1. }.into())
}

fn rgb_of(s: &str) -> Option<(f32, f32, f32)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some((((n >> 16) & 0xff) as f32 / 255., ((n >> 8) & 0xff) as f32 / 255., (n & 0xff) as f32 / 255.))
}

fn hex_of(r: f32, g: f32, b: f32) -> String {
    let c = |v: f32| (v.clamp(0., 1.) * 255.).round() as u32;
    format!("#{:02X}{:02X}{:02X}", c(r), c(g), c(b))
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = (h.rem_euclid(1.)) * 6.;
    let (i, f) = (h.floor(), h - h.floor());
    let (p, q, t) = (v * (1. - s), v * (1. - s * f), v * (1. - s * (1. - f)));
    match i as u32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if d == 0. {
        0.
    } else if max == r {
        ((g - b) / d).rem_euclid(6.) / 6.
    } else if max == g {
        ((b - r) / d + 2.) / 6.
    } else {
        ((r - g) / d + 4.) / 6.
    };
    (h, if max == 0. { 0. } else { d / max }, max)
}

// -- what the vault keeps ---------------------------------------------------

/// A folder's icon by Tabler's name and its color as `#rrggbb`; either may
/// be left to the default.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Look {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl Look {
    fn plain(&self) -> bool {
        self.icon.is_none() && self.color.is_none()
    }
}

/// Every vault's looks read so far, by its root: when its file was last
/// read as it stood then, and each folder's by its path from the root.
#[derive(Default)]
pub struct Looks {
    vaults: RefCell<HashMap<PathBuf, (Option<SystemTime>, BTreeMap<String, Look>)>>,
    /// When each vault's file was last looked at on the disk: a block and
    /// a tile ask for their vault's look several times a frame.
    looked: RefCell<HashMap<PathBuf, std::time::Instant>>,
}

/// A vault's file is looked at no more often than this, for another Mac's
/// write through the cloud: this app's own writes are known at once.
const LOOK_AGAIN: std::time::Duration = std::time::Duration::from_millis(500);

impl Global for Looks {}

fn file(root: &Path) -> PathBuf {
    root.join(".onyx").join("folders.json")
}

/// A folder's key in its vault's looks: its path from the root, and `.`
/// for the vault itself, whose look is its mark's (`library::vault_badge`).
fn key(root: &Path, path: &Path) -> Option<String> {
    if path == root {
        return Some(".".into());
    }
    rel(root, path)
}

fn rel(root: &Path, path: &Path) -> Option<String> {
    let r = path.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
    (!r.is_empty()).then_some(r)
}

fn modified(root: &Path) -> Option<SystemTime> {
    std::fs::metadata(file(root)).and_then(|m| m.modified()).ok()
}

/// A vault's looks read, or read again when its file changed since: another
/// Mac may have written it through the cloud.
pub fn ensure(root: &Path, cx: &mut App) {
    let looks = cx.default_global::<Looks>();
    {
        let mut looked = looks.looked.borrow_mut();
        if looked.get(root).is_some_and(|at| at.elapsed() < LOOK_AGAIN) && looks.vaults.borrow().contains_key(root) {
            return;
        }
        looked.insert(root.to_path_buf(), std::time::Instant::now());
    }
    let at = modified(root);
    let mut vaults = looks.vaults.borrow_mut();
    if vaults.get(root).is_some_and(|(seen, _)| *seen == at) {
        return;
    }
    let map = std::fs::read_to_string(file(root)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    vaults.insert(root.to_path_buf(), (at, map));
}

/// A folder's look, from the deepest vault read that holds it.
pub fn look_of(path: &Path, cx: &App) -> Option<Look> {
    let looks = cx.try_global::<Looks>()?;
    let vaults = looks.vaults.borrow();
    let (root, (_, map)) = vaults.iter().filter(|(root, _)| path.starts_with(root)).max_by_key(|(root, _)| root.components().count())?;
    map.get(&key(root, path)?).cloned()
}

/// A vault's own look, its file read first: for the vaults' lists, where
/// no vault need be open.
pub fn vault_look(root: &Path, cx: &mut App) -> Look {
    ensure(root, cx);
    look_of(root, cx).unwrap_or_default()
}

/// A glyph by its Tabler name, for a vault's mark.
pub fn glyph(name: &str) -> SharedString {
    glyph_path(name)
}

/// A vault's looks written: whole, through the vault's own write, and
/// the file and its folder gone when no folder has a look.
fn write(root: &Path, map: &BTreeMap<String, Look>) -> Result<(), String> {
    let path = file(root);
    if map.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir(dir);
        }
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    onyx_core::vault::write(&path, &format!("{text}\n")).map_err(|e| e.to_string())
}

/// Change a vault's looks and write them; every window draws again.
fn change(root: &Path, cx: &mut App, f: impl FnOnce(&mut BTreeMap<String, Look>) -> bool) {
    ensure(root, cx);
    let changed = {
        let looks = cx.default_global::<Looks>();
        let mut vaults = looks.vaults.borrow_mut();
        let Some((seen, map)) = vaults.get_mut(root) else { return };
        if !f(map) {
            return;
        }
        if let Err(e) = write(root, map) {
            eprintln!("onyx: the folders' looks were not kept: {e}");
        }
        *seen = modified(root);
        true
    };
    if changed {
        cx.refresh_windows();
    }
}

/// A folder given a look, or its own taken back when the look is plain.
pub fn set(root: &Path, folder: &Path, look: Look, cx: &mut App) {
    let Some(key) = key(root, folder) else { return };
    change(root, cx, |map| {
        let was = map.get(&key).cloned().unwrap_or_default();
        if was == look {
            return false;
        }
        if look.plain() {
            map.remove(&key);
        } else {
            map.insert(key, look);
        }
        true
    });
}

/// Folders moved or renamed take their looks, and the folders in them
/// theirs, with them.
pub fn moved(moves: &[(PathBuf, PathBuf)], cx: &mut App) {
    let roots: Vec<PathBuf> = cx.try_global::<Looks>().map(|l| l.vaults.borrow().keys().cloned().collect()).unwrap_or_default();
    for root in roots {
        let pairs: Vec<(String, String)> = moves.iter().filter_map(|(from, to)| Some((rel(&root, from)?, rel(&root, to)?))).collect();
        if pairs.is_empty() {
            continue;
        }
        change(&root, cx, |map| {
            let mut changed = false;
            for (from, to) in &pairs {
                let keys: Vec<String> = map.keys().filter(|k| *k == from || k.starts_with(&format!("{from}/"))).cloned().collect();
                for k in keys {
                    if let Some(look) = map.remove(&k) {
                        map.insert(format!("{to}{}", &k[from.len()..]), look);
                        changed = true;
                    }
                }
            }
            changed
        });
    }
}

/// The looks of a folder on its way out of its vault, and of the folders
/// in it: each vault's root, the folder's path from it, and the look.
pub type Taken = Vec<(PathBuf, String, Look)>;

/// A folder gone: its look, and those of the folders in it, are taken out
/// of their vault's file and handed back, for `put_back` when the folder
/// comes back.
pub fn take(path: &Path, cx: &mut App) -> Taken {
    let roots: Vec<PathBuf> = cx.try_global::<Looks>().map(|l| l.vaults.borrow().keys().cloned().collect()).unwrap_or_default();
    let mut taken = Taken::new();
    for root in roots {
        let Some(gone) = rel(&root, path) else { continue };
        change(&root, cx, |map| {
            let keys: Vec<String> = map.keys().filter(|k| **k == gone || k.starts_with(&format!("{gone}/"))).cloned().collect();
            for k in &keys {
                if let Some(look) = map.remove(k) {
                    taken.push((root.clone(), k.clone(), look));
                }
            }
            !keys.is_empty()
        });
    }
    taken
}

/// The looks `take` took, given back to their folders.
pub fn put_back(taken: Taken, cx: &mut App) {
    let mut by_root: HashMap<PathBuf, Vec<(String, Look)>> = HashMap::new();
    for (root, key, look) in taken {
        by_root.entry(root).or_default().push((key, look));
    }
    for (root, looks) in by_root {
        change(&root, cx, |map| {
            map.extend(looks);
            true
        });
    }
}

// -- drawing a folder -------------------------------------------------------

/// A folder's icon in a tree, 16 square: its own icon, or the default,
/// Catppuccin's, in the folder's color (the muted ink for an icon with
/// none), else Catppuccin's in its own.
pub fn icon(path: &Path, name: &str, open: bool, dark: bool, theme: &gpui_component::Theme, cx: &App) -> AnyElement {
    let look = look_of(path, cx).unwrap_or_default();
    drawn(look.icon.as_deref(), look.color.as_deref().and_then(parse_hex), name, open, dark, crate::widgets::hand(16.), theme)
}

/// An icon by its name, or the default for `None`, in `ink`; the default
/// with no ink is Catppuccin's in its own colors, any other with none in
/// the muted ink. Catppuccin's folder is one line, so it takes an ink as
/// a mask, as Tabler's do.
fn drawn(glyph: Option<&str>, ink: Option<Hsla>, name: &str, open: bool, dark: bool, size: f32, theme: &gpui_component::Theme) -> AnyElement {
    match (glyph, ink) {
        (None, None) => img(crate::file_icons::path(name, true, open, dark)).size(px(size)).flex_shrink_0().into_any_element(),
        (None, Some(ink)) => Icon::default().path(crate::file_icons::path(name, true, open, dark)).with_size(px(size)).text_color(ink).flex_shrink_0().into_any_element(),
        (Some(glyph), ink) => Icon::default().path(glyph_path(glyph)).with_size(px(size)).text_color(ink.unwrap_or(theme.muted_foreground)).flex_shrink_0().into_any_element(),
    }
}

/// A folder's mark on its pill of tabs: its own icon, else the pill's
/// folder, in its color, else in the pill's ink.
pub fn mark(folder: &Path, cx: &App) -> Icon {
    match look_of(folder, cx) {
        Some(look) => {
            let icon = match look.icon {
                Some(glyph) => Icon::default().path(glyph_path(&glyph)).with_size(px(crate::widgets::hand(13.))),
                None => Icon::new(IconName::Folder).with_size(px(crate::widgets::hand(13.))),
            };
            match look.color.as_deref().and_then(parse_hex) {
                Some(ink) => icon.text_color(ink),
                None => icon,
            }
        }
        None => Icon::new(IconName::Folder).with_size(px(crate::widgets::hand(13.))),
    }
}

// -- the picker -------------------------------------------------------------

/// The picker's grid: so many icons a row, each in a square of `CELL`.
const COLS: usize = 10;
const CELL: f32 = 36.;
const CELL_GAP: f32 = 3.;
/// The grid's rows: as many as the window has room for, within these.
const GRID_LEAST: f32 = 1.5;
const GRID_MOST: f32 = 5.5;
const SWATCH: f32 = 26.;

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Square,
    Hue,
}

/// The picker is done with.
pub struct Dismissed;

/// A folder's icon and color chosen: the colors as a row of swatches
/// (none, the app's, the person's own, and a plus for a new one, made on
/// a square of saturation and brightness over a bar of hue, or by its
/// hex), then Tabler's icons in a grid a search narrows. A choice is the
/// folder's at once.
pub struct Picker {
    root: PathBuf,
    folder: PathBuf,
    look: Look,
    search: Entity<InputState>,
    hex: Entity<InputState>,
    found: Vec<&'static str>,
    /// The new color being made, as hue, saturation and brightness, while
    /// its panel is open.
    making: Option<(f32, f32, f32)>,
    dragging: Option<Part>,
    square: Rc<Cell<Bounds<Pixels>>>,
    hue: Rc<Cell<Bounds<Pixels>>>,
    /// The grid's scroll, for the window's router to give the wheel to.
    pub scroll: ScrollHandle,
    focus: FocusHandle,
    /// How far down its ground the card stands, and how far down the
    /// window that ground starts: the card keeps inside the window.
    top: f32,
    inset: f32,
    /// The person's own colors, read as the sheet opens and kept as they
    /// are added or forgotten here: the sheet draws at every move of a
    /// color's drag.
    own_colors: Vec<String>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<Dismissed> for Picker {}

impl Picker {
    pub fn new(root: PathBuf, folder: PathBuf, top: f32, inset: f32, window: &mut Window, cx: &mut Context<Self>) -> Self {
        ensure(&root, cx);
        let look = look_of(&folder, cx).unwrap_or_default();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(t("Search icons")));
        let hex = cx.new(|cx| InputState::new(window, cx).placeholder("#3E63DD"));
        let subs = vec![
            cx.subscribe_in(&search, window, |this, input, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    this.found = self::search(&input.read(cx).value());
                    this.scroll.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                }
            }),
            cx.subscribe_in(&hex, window, |this, input, ev: &InputEvent, _, cx| match ev {
                InputEvent::Change => {
                    if let Some((r, g, b)) = rgb_of(&input.read(cx).value()) {
                        this.making = Some(rgb_to_hsv(r, g, b));
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. } => this.add_color(cx),
                _ => {}
            }),
        ];
        let focus = cx.focus_handle();
        window.focus(&search.read(cx).focus_handle(cx), cx);
        Self { root, folder, look, search, hex, found: self::search(""), making: None, dragging: None, square: Rc::default(), hue: Rc::default(), scroll: ScrollHandle::new(), focus, top, inset, own_colors: Config::load().app.folder_colors, _subs: subs }
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        set(&self.root, &self.folder, self.look.clone(), cx);
        cx.notify();
    }

    fn choose_color(&mut self, color: Option<String>, cx: &mut Context<Self>) {
        self.look.color = color;
        self.apply(cx);
    }

    fn choose_icon(&mut self, icon: Option<String>, cx: &mut Context<Self>) {
        self.look.icon = icon;
        self.apply(cx);
    }

    /// The panel for a new color, from the folder's color or the first.
    fn toggle_making(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.making.take().is_none() {
            let from = self.look.color.clone().unwrap_or_else(|| PRESETS[6].to_string());
            let (r, g, b) = rgb_of(&from).unwrap_or((0.24, 0.39, 0.87));
            self.making = Some(rgb_to_hsv(r, g, b));
            self.hex.update(cx, |i, cx| i.set_value(from, window, cx));
        }
        cx.notify();
    }

    fn making_hex(&self) -> Option<String> {
        self.making.map(|(h, s, v)| {
            let (r, g, b) = hsv_to_rgb(h, s, v);
            hex_of(r, g, b)
        })
    }

    /// The new color kept among the person's own, and the folder's.
    fn add_color(&mut self, cx: &mut Context<Self>) {
        let Some(hex) = self.making_hex() else { return };
        let _ = Config::edit(|c| {
            if !c.app.folder_colors.iter().any(|h| h.eq_ignore_ascii_case(&hex)) {
                c.app.folder_colors.push(hex.clone());
            }
        });
        self.own_colors = Config::load().app.folder_colors;
        self.making = None;
        self.choose_color(Some(hex), cx);
    }

    fn forget_color(&mut self, hex: String, cx: &mut Context<Self>) {
        let _ = Config::edit(|c| c.app.folder_colors.retain(|h| !h.eq_ignore_ascii_case(&hex)));
        self.own_colors = Config::load().app.folder_colors;
        cx.notify();
    }

    /// A press or a drag on the square or the bar: the color under it.
    fn pick_at(&mut self, part: Part, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((h, s, v)) = self.making.as_mut() else { return };
        let b = if part == Part::Square { self.square.get() } else { self.hue.get() };
        if b.size.width <= px(0.) {
            return;
        }
        let x = ((at.x - b.left()) / b.size.width).clamp(0., 1.);
        let y = ((at.y - b.top()) / b.size.height).clamp(0., 1.);
        match part {
            Part::Square => {
                *s = x;
                *v = 1. - y;
            }
            Part::Hue => *h = x,
        }
        if let Some(hex) = self.making_hex() {
            self.hex.update(cx, |i, cx| i.set_value(hex, window, cx));
        }
        cx.notify();
    }

    /// For a probe: words in the search, as typed.
    pub fn probe_search(&mut self, words: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |i, cx| i.set_value(words.to_string(), window, cx));
        self.found = search(words);
        cx.notify();
    }

    /// For a probe: the new color's panel, as the plus opens it.
    pub fn probe_making(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_making(window, cx);
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(Dismissed);
    }

    fn render_colors(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let chosen = self.look.color.clone();
        let ring = |on: bool, inner: AnyElement| {
            div().size(px(SWATCH + 6.)).flex().items_center().justify_center().rounded_full().border_2().border_color(if on { theme.foreground.opacity(0.7) } else { transparent_black() }).child(inner)
        };
        let dot = |hex: &str| div().size(px(SWATCH)).rounded_full().bg(parse_hex(hex).unwrap_or(theme.muted)).into_any_element();
        let mut row = h_flex().flex_wrap().gap(px(4.));
        row = row.child(
            div()
                .id("look-color-none")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.choose_color(None, cx);
                }))
                .child(ring(chosen.is_none(), div().size(px(SWATCH)).rounded_full().border_1().border_color(theme.border).flex().items_center().justify_center().child(Icon::default().path(glyph_path("ban")).with_size(px(14.)).text_color(theme.muted_foreground)).into_any_element())),
        );
        for hex in PRESETS {
            let on = chosen.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(hex));
            row = row.child(div().id(SharedString::from(format!("look-color-{hex}"))).cursor_pointer().on_click(cx.listener(move |this, _, window, cx| {
                swallow_click(window, cx);
                this.choose_color(Some(hex.to_string()), cx);
            })).child(ring(on, dot(hex))));
        }
        for hex in self.own_colors.clone() {
            let on = chosen.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&hex));
            let (pick, forget) = (hex.clone(), hex.clone());
            let hover = theme.foreground;
            row = row.child(
                div()
                    .id(SharedString::from(format!("look-own-{hex}")))
                    .group("own-color")
                    .relative()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.choose_color(Some(pick.clone()), cx);
                    }))
                    .child(ring(on, dot(&hex)))
                    // One's own color goes from its corner.
                    .child(
                        div()
                            .id(SharedString::from(format!("look-own-x-{hex}")))
                            .absolute()
                            .top(px(-2.))
                            .right(px(-2.))
                            .size(px(14.))
                            .rounded_full()
                            .bg(theme.popover)
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(0.)
                            .group_hover("own-color", |s| s.opacity(1.))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                cx.stop_propagation();
                                this.forget_color(forget.clone(), cx);
                            }))
                            .child(Icon::default().path(glyph_path("x")).with_size(px(9.)).text_color(hover)),
                    ),
            );
        }
        row.child(
            div()
                .id("look-color-new")
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.toggle_making(window, cx);
                }))
                .child(ring(self.making.is_some(), div().size(px(SWATCH)).rounded_full().border_1().border_dashed().border_color(theme.muted_foreground).flex().items_center().justify_center().child(Icon::default().path(glyph_path("plus")).with_size(px(14.)).text_color(theme.muted_foreground)).into_any_element())),
        )
        .into_any_element()
    }

    /// The panel a new color is made on: the square, the bar, what it
    /// makes, its hex, and Add.
    fn render_making(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (h, s, v) = self.making?;
        let (r, g, b) = hsv_to_rgb(h, s, v);
        let made: Hsla = Rgba { r, g, b, a: 1. }.into();
        let pure: Hsla = hsla(h, 1., 0.5, 1.);
        let (square, hue) = (self.square.clone(), self.hue.clone());
        let field_focus = self.hex.read(cx).focus_handle(cx);
        let square_el = div()
            .id("look-square")
            .w(px(232.))
            .h(px(116.))
            .cursor_crosshair()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.dragging = Some(Part::Square);
                this.pick_at(Part::Square, e.position, window, cx);
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, cx| {
                if e.pressed_button == Some(MouseButton::Left) && this.dragging == Some(Part::Square) {
                    this.pick_at(Part::Square, e.position, window, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = None))
            .child(
                canvas(
                    move |b, _, _| square.set(b),
                    move |b, _, window, _| {
                        let r = Corners::all(px(8.));
                        window.paint_quad(fill(b, pure).corner_radii(r));
                        window.paint_quad(fill(b, linear_gradient(90., linear_color_stop(white(), 0.), linear_color_stop(white().opacity(0.), 1.))).corner_radii(r));
                        window.paint_quad(fill(b, linear_gradient(180., linear_color_stop(black().opacity(0.), 0.), linear_color_stop(black(), 1.))).corner_radii(r));
                        let c = point(b.left() + b.size.width * s, b.top() + b.size.height * (1. - v));
                        let dot = Bounds::new(point(c.x - px(7.), c.y - px(7.)), size(px(14.), px(14.)));
                        window.paint_quad(fill(dot, made).corner_radii(Corners::all(px(7.))).border_widths(px(2.)).border_color(white()));
                    },
                )
                .size_full(),
            );
        let hue_el = div()
            .id("look-hue")
            .w(px(232.))
            .h(px(12.))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.dragging = Some(Part::Hue);
                this.pick_at(Part::Hue, e.position, window, cx);
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, cx| {
                if e.pressed_button == Some(MouseButton::Left) && this.dragging == Some(Part::Hue) {
                    this.pick_at(Part::Hue, e.position, window, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.dragging = None))
            .child(
                canvas(
                    move |b, _, _| hue.set(b),
                    move |b, _, window, _| {
                        let w = b.size.width / 6.;
                        for i in 0..6 {
                            let seg = Bounds::new(point(b.left() + w * i as f32, b.top()), size(w + px(0.5), b.size.height));
                            let (a, z) = (hsla(i as f32 / 6., 1., 0.5, 1.), hsla((i + 1) as f32 / 6., 1., 0.5, 1.));
                            let mut quad = fill(seg, linear_gradient(90., linear_color_stop(a, 0.), linear_color_stop(z, 1.)));
                            if i == 0 {
                                quad = quad.corner_radii(Corners { top_left: px(6.), bottom_left: px(6.), ..Default::default() });
                            } else if i == 5 {
                                quad = quad.corner_radii(Corners { top_right: px(6.), bottom_right: px(6.), ..Default::default() });
                            }
                            window.paint_quad(quad);
                        }
                        let x = b.left() + b.size.width * h;
                        let knob = Bounds::new(point(x - px(3.), b.top() - px(2.)), size(px(6.), b.size.height + px(4.)));
                        window.paint_quad(fill(knob, hsla(h, 1., 0.5, 1.)).corner_radii(Corners::all(px(3.))).border_widths(px(2.)).border_color(white()));
                    },
                )
                .size_full(),
            );
        Some(
            h_flex()
                .gap(px(12.))
                .items_start()
                .child(v_flex().gap(px(10.)).child(square_el).child(hue_el))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(8.))
                        .child(div().h(px(36.)).rounded(px(8.)).bg(made).border_1().border_color(theme.border))
                        .child(div().id("look-hex").track_focus(&field_focus).px(px(10.)).h(px(32.)).flex().items_center().rounded(px(8.)).bg(theme.muted).text_size(px(13.)).font_family(theme.mono_font_family.clone()).child(Input::new(&self.hex).appearance(false)))
                        .child(small_pill("look-add", "icons/plus.svg", t("Add"), Press::Primary, false, theme).on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.add_color(cx);
                        }))),
                )
                .into_any_element(),
        )
    }

    /// The icons, a row of `COLS` at a time, those in view drawn: the
    /// first square is the folder's own default, a vault's its letter.
    fn render_grid(&self, rows_shown: f32, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let ink = self.look.color.as_deref().and_then(parse_hex).unwrap_or(theme.foreground.opacity(0.8));
        let count = self.found.len() + 1;
        let rows = count.div_ceil(COLS);
        let row_h = CELL + CELL_GAP;
        let view = row_h * rows_shown - CELL_GAP;
        let top = -f32::from(self.scroll.offset().y);
        let first = ((top / row_h).floor().max(0.)) as usize;
        let last = (((top + view) / row_h).ceil() as usize + 1).min(rows);
        let cell = |id: ElementId, on: bool, child: AnyElement| {
            let (hover, lit) = (theme.foreground.opacity(0.06), theme.primary.opacity(0.14));
            div().id(id).size(px(CELL)).flex_shrink_0().flex().items_center().justify_center().rounded(px(8.)).cursor_pointer().when(on, |d| d.bg(lit).border_1().border_color(theme.primary.opacity(0.6))).when(!on, |d| d.hover(move |s| s.bg(hover))).child(child)
        };
        let mut lines = Vec::new();
        for r in first..last {
            let mut line = h_flex().h(px(CELL)).gap(px(CELL_GAP)).flex_shrink_0();
            for c in 0..COLS {
                let i = r * COLS + c;
                if i >= count {
                    break;
                }
                if i == 0 {
                    let ink = self.look.color.as_deref().and_then(parse_hex);
                    let mark = if self.folder == self.root {
                        let name = self.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let letter: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                        div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(ink.unwrap_or(theme.muted_foreground)).child(letter).into_any_element()
                    } else {
                        drawn(None, ink, "folder", false, theme.mode.is_dark(), 18., theme)
                    };
                    line = line.child(cell("look-icon-default".into(), self.look.icon.is_none(), mark).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.choose_icon(None, cx);
                    })));
                    continue;
                }
                let name = self.found[i - 1];
                let on = self.look.icon.as_deref() == Some(name);
                let glyph = Icon::default().path(glyph_path(name)).with_size(px(18.)).text_color(if on || self.look.color.is_some() { ink } else { theme.foreground.opacity(0.75) }).into_any_element();
                line = line.child(cell(ElementId::Name(SharedString::from(format!("look-icon-{name}"))), on, glyph).on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.choose_icon(Some(name.to_string()), cx);
                })));
            }
            lines.push(line.into_any_element());
        }
        let above = first as f32 * row_h;
        let below = (rows.saturating_sub(last)) as f32 * row_h;
        div()
            .relative()
            .h(px(view))
            .child(
                div()
                    .id("look-grid")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(v_flex().gap(px(CELL_GAP)).child(div().h(px((above - CELL_GAP).max(0.))).flex_shrink_0()).children(lines).child(div().h(px(below)).flex_shrink_0())),
            )
            .child(crate::shell::edge_fade(&self.scroll, theme.popover, true, px(0.)))
            .child(crate::shell::edge_fade(&self.scroll, theme.popover, false, px(0.)))
            .into_any_element()
    }
}

impl Focusable for Picker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        // The grid takes what the window leaves: the card's other parts
        // are of known heights, the swatches a row of ten.
        let swatches = PRESETS.len() + 2 + self.own_colors.len();
        let swatch_rows = swatches.div_ceil(10) as f32;
        let colors_h = 16. + 8. + swatch_rows * (SWATCH + 6.) + (swatch_rows - 1.) * 4. + if self.making.is_some() { 8. + 138. } else { 0. };
        let fixed = 32. + 20. + 12. * 3. + colors_h + (16. + 8. + 32. + 8.) + 28.;
        let room = f32::from(window.viewport_size().height) - self.inset - self.top - 16.;
        let rows_shown = ((room - fixed) / (CELL + CELL_GAP)).clamp(GRID_LEAST, GRID_MOST);
        let name = self.folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let label = |words: &str| div().text_size(px(11.5)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(words.to_string());
        let search_focus = self.search.read(cx).focus_handle(cx);
        // A vault's sheet shows its mark, and its color is not a folder's.
        let vault = self.folder == self.root;
        let preview = if vault { crate::shell::library::vault_badge(&self.root, &name, 20., &theme, cx).into_any_element() } else { icon(&self.folder, &name, false, theme.mode.is_dark(), &theme, cx) };
        let colors = self.render_colors(&theme, cx);
        let making = self.render_making(&theme, cx);
        let grid = self.render_grid(rows_shown, &theme, cx);
        crate::widgets::fade_in(
            "folder-look",
            div()
                .id("look-scrim")
                .absolute()
                .inset_0()
                .occlude()
                .bg(theme.overlay)
                .children(crate::widgets::reaches(theme.overlay))
                .flex()
                .flex_col()
                .items_center()
                .pt(px(self.top))
                .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)))
                .child(
                    v_flex()
                        .id("look-card")
                        .occlude()
                        .key_context(crate::shell::SHEET_CONTEXT)
                        .track_focus(&self.focus)
                        .on_action(cx.listener(|this, _: &crate::shell::Escape, _, cx| this.dismiss(cx)))
                        .on_click(|_, window, cx| swallow_click(window, cx))
                        .w(px(420.))
                        .max_w(relative(0.94))
                        .p(px(16.))
                        .gap(px(12.))
                        .rounded(px(18.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.border)
                        .shadow(crate::widgets::float_shadow(&theme))
                        .child(h_flex().gap(px(8.)).items_center().child(div().size(px(16.)).child(preview)).child(div().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(name)))
                        .child(v_flex().gap(px(8.)).child(label(if vault { t("Color") } else { t("Folder Color") })).child(colors).children(making))
                        .child(
                            v_flex()
                                .gap(px(8.))
                                .child(label(t("Icon")))
                                .child(div().id("look-search").track_focus(&search_focus).px(px(10.)).h(px(32.)).flex().items_center().rounded(px(8.)).bg(theme.muted).text_size(px(13.)).child(Input::new(&self.search).appearance(false)))
                                .child(grid),
                        )
                        .child(
                            h_flex()
                                .justify_between()
                                .child(small_pill("look-clear", "icons/close.svg", t("Clear"), Press::Quiet, self.look.plain(), &theme).on_click(cx.listener(|this, _, window, cx| {
                                    swallow_click(window, cx);
                                    this.look = Look::default();
                                    this.apply(cx);
                                })))
                                .child(small_pill("look-done", "icons/check.svg", t("Done"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                                    swallow_click(window, cx);
                                    this.dismiss(cx);
                                }))),
                        ),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    // Named, not `super::*`: gpui's glob carries a `test` macro of its own.
    use super::{by_name, glyphs, hex_of, hsv_to_rgb, parse_hex, rgb_of, rgb_to_hsv, search, svg, Look, FIRST, PRESETS};
    use std::collections::BTreeMap;

    #[test]
    fn tabler_icons_are_served_and_found() {
        assert!(glyphs().len() > 4000);
        let folder = String::from_utf8(svg("tabler/folder.svg").unwrap()).unwrap();
        assert!(folder.contains("viewBox=\"0 0 24 24\"") && folder.contains("<path"));
        assert!(svg("tabler/no-such-icon.svg").is_none());
        assert_eq!(search("folder").first(), Some(&"folder"));
        assert!(search("book").contains(&"notebook"));
        assert_eq!(search("").first(), Some(&"notebook"));
        for name in FIRST {
            assert!(by_name().contains_key(name), "{name} is not in Tabler's set");
        }
    }

    #[test]
    fn colors_go_round_through_hsv() {
        for hex in PRESETS {
            let (r, g, b) = rgb_of(hex).unwrap();
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (r2, g2, b2) = hsv_to_rgb(h, s, v);
            assert_eq!(hex_of(r2, g2, b2), hex);
        }
        assert!(parse_hex("#12345").is_none());
    }

    #[test]
    fn a_moved_folder_takes_its_look() {
        let mut map: BTreeMap<String, Look> = BTreeMap::new();
        map.insert("Projects".into(), Look { icon: Some("rocket".into()), color: None });
        map.insert("Projects/Onyx".into(), Look { icon: None, color: Some("#3E63DD".into()) });
        map.insert("Projects2".into(), Look { icon: Some("star".into()), color: None });
        let (from, to) = ("Projects".to_string(), "Work/Projects".to_string());
        let keys: Vec<String> = map.keys().filter(|k| **k == from || k.starts_with(&format!("{from}/"))).cloned().collect();
        for k in keys {
            let look = map.remove(&k).unwrap();
            map.insert(format!("{to}{}", &k[from.len()..]), look);
        }
        assert!(map.contains_key("Work/Projects") && map.contains_key("Work/Projects/Onyx") && map.contains_key("Projects2"));
    }
}
