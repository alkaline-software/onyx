//! The main window: one view (`Shell`) that draws the frame, the sidebar
//! on it, and the content as a card with rounded corners. The sidebar is
//! the vault's tree: its folders and notes, as the disk has them. The
//! card is the note open in the editor (`editor.rs`) under a strip with
//! its name and how it is shown, or, with no vault open, the landing
//! page. It also holds what the settings window draws (`settings.rs`),
//! so a change shows in both windows at once.
//!
//! Everything on the disk goes through `onyx_core::vault`: a note is
//! written whole when it is left (another note opens, the vault closes,
//! the window closes, the app quits) and at ⌘S, never while it is being
//! typed, and the vault's folder is watched so the tree and an untouched
//! note follow what another app does there.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use onyx_core::config::Config;
use onyx_core::{update, vault};

use crate::ambient::Ambient;
use crate::editor::{CellAt, Editor, EditorEvent, Look, MenuAt, MenuIssue, MenuLink, Mode, Scope, TableFacts, Words};
use crate::i18n::{t, tf};
use crate::menu::{Entry, Menus};
use crate::scroll::Scrollers;
use crate::tabs::{Item as TabItem, Tabs};
use crate::ui_state::{Rect, UiState};
use crate::widgets::{choice_pill, float_shadow, kbd_hint, key_text, page_in, small_pill, swallow_click, tip, Fig, Motion, Press, FOLD_ANIM, SLIDE};

actions!(onyx, [ExportPdf, PrintNote, ToggleSidebar, OpenSettings, Escape, NewNote, NewFolder, NewVault, OpenVault, CloseVault, ShowLive, ShowSource, ShowReading, ToggleReading, ShowNarrow, ShowMiddle, ShowWide, ToggleSearch, FindInNote, FindNext, FindPrev, CloseTab, ReopenTab, ZoomIn, ZoomOut, ZoomReset, Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, Tab9]);

/// The key context of the main window.
pub const KEY_CONTEXT: &str = "Shell";
/// The key context of a sheet over the window.
pub(crate) const SHEET_CONTEXT: &str = "Sheet";

mod library;
mod tree;
pub(crate) use tree::{Choice, TreeDrag};

/// The tree's keys, for the window's key map.
pub fn tree_keys() -> Vec<KeyBinding> {
    tree::key_bindings()
}
pub(crate) use library::{Asset, Library, SideView};
/// The key context of the export sheet.
pub const EXPORT_CONTEXT: &str = "ExportSheet";
/// The key context of the search over the window, and of the find bar
/// over the note.
pub const SEARCH_CONTEXT: &str = "SearchPalette";
pub const FIND_CONTEXT: &str = "FindBar";
pub const TITLE_CONTEXT: &str = "NoteTitle";
/// A text field's menu in the settings window.
pub const SETTINGS_EDIT: &str = "settings-edit";

/// How many lines a note's name wraps to before it scrolls in its field.
const TITLE_ROWS: usize = 6;
/// How tall a scroller's edge fade stands, and with it the zone that
/// counts as out of sight: the editor's `SCROLL_MARGIN`, which keeps the
/// caret out of it. The two change together.
const FADE_H: Pixels = px(28.);
/// How far from the end of a scroll its edge's fade is whole.
const FADE_ROOM: f32 = 56.;
/// The window's ground shows this far around the card.
pub const FRAME: Pixels = px(4.);
/// The card's corner: the window's less the frame's band, so the two
/// curves run side by side. Off a Mac the window's corner is small.
pub const CARD_R: Pixels = px(if cfg!(target_os = "macos") { 12. } else { 4. });
/// The card's top strip, where the traffic lights and the strip's buttons
/// stand on one line.
pub const TITLEBAR_H: Pixels = px(48.);
/// The room the traffic lights take at the window's left.
const TRAFFIC_W: Pixels = px(85.);
const STRIP_BTN: Pixels = px(26.);
pub const SIDEBAR_W: Pixels = px(268.);
/// The sidebar is dragged between these, emaki's. The least still has
/// room for the window's buttons and the name beside them.
const SIDEBAR_MIN: Pixels = px(264.);
const SIDEBAR_MAX: Pixels = px(440.);
/// The most a double click on the edge asks for; a drag may go on to
/// `SIDEBAR_MAX`.
const SIDEBAR_FIT_MAX: Pixels = px(320.);
/// Dragged left of this, the sidebar folds away.
const SIDEBAR_FOLD_AT: Pixels = px(170.);
/// How wide the strip at the sidebar's edge that takes the drag is.
const SIDEBAR_GRIP: Pixels = px(8.);
/// The rail: the sidebar folded to a column at the window's left, with
/// the traffic lights stacked at its head on a Mac, then the sidebar's
/// and the search's buttons, then its entries, each a square on one
/// pitch, and the app's icon and the person's picture at its foot.
pub const RAIL_W: Pixels = px(56.);
const RAIL_ENTRY: Pixels = px(36.);
const RAIL_GAP: Pixels = px(4.);
/// The first rail button's top: under the lights' column on a Mac.
const RAIL_TOP: Pixels = px(if cfg!(target_os = "macos") { 92. } else { 52. });
/// Where the notes card flies out from the rail.
const FLY_TOP: Pixels = px(52.);
/// How many lines of a note the search shows under it.
const SEARCH_LINES: usize = 3;
/// An open group: its three squares of `STRIP_SQUARE`, 2 between them.
const GROUP_W: Pixels = px(76.);
/// The path's row under the tabs, and how far its path keeps from the
/// card's sides.
const PATH_BAR_H: Pixels = px(30.);
const PATH_BAR_PAD: f32 = 6.;
/// How far the tabs' row keeps its squares from the card's sides: what
/// they keep from its top and foot, so the room round them is even.
const STRIP_END: Pixels = px(10.);
/// A script's presses and keys land this many milliseconds apart.
const GO_STEP: u64 = 400;
/// A group's square, a tab's plate tall.
const STRIP_SQUARE: Pixels = px(24.);
/// The line round a group and what it keeps from the squares inside, so
/// the group is as tall as the tabs' pill beside it, and as round.
const STRIP_INSET: Pixels = px(2.);
const STRIP_CAP: Pixels = px(30.);
const STRIP_CAP_R: Pixels = px(9.);
/// Between each end of the tabs' row and the row.
const STRIP_END_GAP: Pixels = px(6.);

/// The ends of the tabs' row.
/// A line under the strip: its words, when it came, the file it names
/// when it says something is done, and whether it is news rather than
/// something gone wrong.
#[derive(Clone)]
struct Notice {
    words: String,
    at: Instant,
    done: Option<PathBuf>,
    /// An icon before the words: the line is quiet, not in the danger's ink.
    icon: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StripEnd {
    Modes,
    Widths,
}

/// How wide a note's sheet runs, for every note: the app's own, as the
/// mode is, and only the person changes it.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PageWidth {
    #[default]
    Narrow,
    Middle,
    Wide,
}

impl PageWidth {
    fn key(self) -> &'static str {
        match self {
            PageWidth::Narrow => "narrow",
            PageWidth::Middle => "middle",
            PageWidth::Wide => "wide",
        }
    }

    fn from_key(key: &str) -> PageWidth {
        match key {
            "middle" => PageWidth::Middle,
            "wide" => PageWidth::Wide,
            _ => PageWidth::Narrow,
        }
    }

    /// Its place on the way from Narrow to Wide, which a change eases along.
    fn level(self) -> f32 {
        match self {
            PageWidth::Narrow => 0.,
            PageWidth::Middle => 1.,
            PageWidth::Wide => 2.,
        }
    }
}

/// Narrow's sheet, a comfortable line of reading. Wide runs to the page's
/// edges and Middle halfway there, as far as `MIDDLE_MOST`, past which a
/// big monitor would stretch it toward Wide. A page no wider than Narrow
/// has room for one width, and all three are it.
const NARROW_W: f32 = 700.;
const MIDDLE_MOST: f32 = 960.;
/// The tinted page shows this much round the sheet, and the words stand
/// `SHEET_GUTTER` in from its edge, Middle's and narrower, and Wide's.
const SHEET_MARGIN: Pixels = px(16.);
const SHEET_GUTTER: (f32, f32) = (32., 48.);
/// The least the window is dragged to.
pub const MIN_W: Pixels = px(720.);
/// How often the vault's changes are looked for.
const WATCH_POLL: Duration = Duration::from_millis(300);

/// The vault open: its folder, its tree as last read, the folders folded
/// shut, and the watcher that says when to read it again.
struct Vault {
    root: PathBuf,
    tree: vault::Entry,
    folded: HashSet<PathBuf>,
    _watcher: Option<vault::Watcher>,
    changes: Rc<std::sync::mpsc::Receiver<Vec<PathBuf>>>,
}

/// An open note, a tab: its file, its own editor (the text, the caret,
/// the scroll and the history stay with the tab while another shows),
/// whether the editor has what the disk has not, and the text as last
/// written or read, which a change on the disk is weighed against.
struct Tab {
    path: PathBuf,
    editor: Entity<Editor>,
    /// The file's name, in the field over the words: the tab's own, like
    /// its editor, so a name half typed stays with its note.
    title: Entity<TextareaState>,
    dirty: bool,
    saved: String,
    /// The write the note's first change since the last asked for,
    /// `SAVE_AFTER` on (`note_changed`).
    save_due: Option<Task<()>>,
    _sub: Subscription,
    _title_sub: Subscription,
    /// A file of the vault's assets shown in the tab, read only; the
    /// editor holds it when it reads as markdown. None for a note.
    asset: Option<Asset>,
}

/// Where a new vault goes: a folder named for it under iCloud Drive's
/// `Onyx` folder, synced by the system, or a folder of the person's own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Cloud,
    Folder,
}

/// The new-vault sheet while it is up: the place chosen, and the folder
/// the vault goes in when the place is a folder of the person's own.
struct NewVaultSheet {
    place: Place,
    folder: PathBuf,
}

pub struct Shell {
    pub(crate) cfg: Config,
    pub(crate) motion: Motion,
    pub(crate) scrollers: Rc<Scrollers>,
    pub(crate) menus: Menus,
    focus: FocusHandle,
    ambient: Entity<Ambient>,
    sidebar_open: bool,
    /// The sidebar's coming or going: which, and when it began.
    side_anim: Option<(bool, Instant)>,
    /// How wide the sidebar is, the person's own and kept, and while a
    /// double click on its edge moves it, the width it left and when.
    sidebar_w: Pixels,
    side_w_anim: Option<(Pixels, Instant)>,
    /// The sidebar's edge is held.
    side_drag: bool,
    side_scroll: ScrollHandle,
    page_scroll: ScrollHandle,
    vault: Option<Vault>,
    /// The open notes, in the row's order: a folder's together.
    tabs: Vec<Tab>,
    /// The tab showing, and the one that showed before the vault's page
    /// took its place, for ⌘W on the page.
    shown: Option<PathBuf>,
    last_shown: Option<PathBuf>,
    /// The tabs closed, the last closed last, for ⌘⇧T.
    closed_tabs: Vec<PathBuf>,
    /// The row's drawing and motion.
    pub(crate) tabs_row: Tabs,
    /// A press on the strip that a tab or a button took first, so the
    /// window is not moved by it; and the press that is the window's.
    pub(crate) press_taken: bool,
    win_move: bool,
    /// What every editor is set in, and how every note is shown.
    pub(crate) look: Look,
    mode: Mode,
    /// The page's width, the card's, figured as each frame is built, so
    /// the sheet is never a frame behind a window being resized.
    /// When a script's last press has landed, for its picture.
    go_ends: u64,
    /// A group opened out of its square: which, when, whether it is
    /// opening or closing, and how open it was when that began.
    strip_fly: Option<(StripEnd, Instant, bool, f32)>,
    width: PageWidth,
    /// A change of width under way: the level it began from, and when.
    width_anim: Option<(f32, Instant)>,
    page_w: f32,
    /// The color the window's own ground was last set to, so it is set
    /// once and not at every frame.
    ground: Cell<Option<Hsla>>,
    /// The drawing over the vault page's greeting, as it answers the pointer.
    home_fig: Rc<RefCell<Fig>>,
    /// Whom the vault's page greets: the account's first name.
    user_name: String,
    /// The notes card flown out from the rail: when it was asked, and
    /// whether it is coming or going.
    rail_fly: Option<(Instant, bool)>,
    /// The search over the vault: its field, what it found, and the
    /// search under way, which the next keystroke drops.
    search_open: bool,
    search_input: Entity<InputState>,
    search_scroll: ScrollHandle,
    search_results: Option<onyx_core::search::Results>,
    search_task: Option<Task<()>>,
    /// The find bar over the note, and its field.
    find_open: bool,
    find_input: Entity<InputState>,
    /// The name's field in the settings, made with their window.
    pub(crate) name_input: Option<Entity<InputState>>,
    /// The row a right click was on.
    pressed: Option<PathBuf>,
    /// The tree's rows chosen, cut and dragged over (`shell/tree.rs`), its
    /// keyboard, and which lit rows join the row above and below.
    choice: Choice,
    tree_focus: FocusHandle,
    lit_joins: RefCell<std::collections::HashMap<PathBuf, (bool, bool)>>,
    /// The sidebar card's view, the vault's assets as it lists them, and
    /// an asset shown over the page.
    side_view: SideView,
    /// The last change of view: the view left, and when.
    side_moved: Option<(SideView, Instant)>,
    library: Library,
    /// The text a menu was opened on, and the link, for what is chosen.
    pub(crate) menu_focus: Option<FocusHandle>,
    menu_link: Option<MenuLink>,
    /// The line of the picture a canvas menu was opened on.
    menu_picture: Option<usize>,
    /// The line of the picture or frame a menu's Delete takes out.
    menu_remove: Option<usize>,
    /// The link a menu can show another way.
    menu_layout: Option<crate::editor::LayoutAt>,
    /// A file a bar renamed in place, renamed at the next draw, where
    /// the window is to hand (`Shell::rename_entry`).
    pending_rename: Option<(PathBuf, String)>,
    /// What the canvas's menu was opened on: the mark, the cell, the editor.
    menu_issue: Option<MenuIssue>,
    menu_cell: Option<CellAt>,
    menu_editor: Option<Entity<Editor>>,
    /// The words the person has settled with the check.
    words: Rc<Words>,
    /// A rename under way: the entry, and the field for its name. Or a
    /// note or a folder about to be made: where, and whether a folder;
    /// its name is asked on the same sheet, in the same field.
    rename: Option<PathBuf>,
    making: Option<(PathBuf, bool)>,
    rename_input: Entity<InputState>,
    /// A new vault under way: the sheet, and the field for its name.
    new_vault: Option<NewVaultSheet>,
    vault_input: Entity<InputState>,
    /// Something to say under the strip, and when it was said.
    /// A line under the strip, gone after a few seconds, and the last one,
    /// kept while it goes.
    notice: Option<Notice>,
    last_notice: Option<Notice>,
    pub(crate) export: Option<crate::export::ExportSheet>,
    /// A picture over the window, and the keyboard while it is.
    pub(crate) viewer: Option<crate::viewer::Viewer>,
    pub(crate) viewer_focus: FocusHandle,
    pub(crate) export_focus: FocusHandle,
    /// When the window's bounds were last written.
    bounds_saved: Option<Instant>,
    pub(crate) settings: Option<AnyWindowHandle>,
    pub(crate) settings_section: &'static str,
    /// Text's page: `fonts` or `size`.
    pub(crate) settings_page: &'static str,
    /// The fields an installed font is named in, made with the settings
    /// window: the words', code's and Chinese's.
    pub(crate) font_inputs: Option<[Entity<InputState>; 3]>,
    pub(crate) settings_scroll: ScrollHandle,
    pub(crate) lang_serial: u32,
    pub(crate) update: UpdateRow,
}

/// What the Updates row says and does.
#[derive(Default, Clone)]
pub(crate) struct UpdateRow {
    pub checking: bool,
    pub found: Option<String>,
    /// While a download runs: how far, from 0 to 100.
    pub downloading: Option<u32>,
    pub said: Option<String>,
}

/// Every row of the tree is this tall.
pub(crate) const TREE_ROW_H: f32 = 30.;
/// How long after a note's first unsaved change it is written.
const SAVE_AFTER: Duration = Duration::from_secs(2);
/// The room over the tree's first row, under the card's band.
pub(crate) const LIST_TOP: f32 = 6.;
/// The room between a row's plate and its edge, at its top and foot.
pub(crate) const ROW_BREATH: f32 = 1.5;
/// Rows drawn beyond the list's box at either end, so a scroll of a few
/// rows shows no blank before the next frame.
const TREE_OVER: f32 = TREE_ROW_H * 6.;
/// Before the list's box has been painted, this much of the tree is
/// drawn from its top.
const TREE_FIRST: f32 = 4000.;

/// One piece of the tree as drawn (`Shell::render_tree`).
enum Piece<'a> {
    Row(&'a vault::Entry, usize),
    /// A folder's children on their way in or out: whether in, the
    /// change's number, and how far along, as a share of their height.
    Fold { folder: &'a vault::Entry, inside: Vec<Piece<'a>>, opening: bool, serial: u32, at: f32 },
}

impl Piece<'_> {
    /// The rows in it, folds and all.
    fn rows(&self) -> usize {
        match self {
            Piece::Row(..) => 1,
            Piece::Fold { inside, .. } => inside.iter().map(Piece::rows).sum(),
        }
    }

    /// What it stands for in the list now.
    fn height(&self) -> f32 {
        match self {
            Piece::Row(..) => TREE_ROW_H,
            Piece::Fold { at, .. } => self.rows() as f32 * TREE_ROW_H * at,
        }
    }
}

/// The key a folder's fold has with `Motion::presence`.
fn fold_key(path: &Path) -> String {
    format!("fold-{}", path.display())
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ui = UiState::load();
        // Every note with changes is written as the window is left for
        // another app, which may read it next.
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.save_all(cx);
            }
        })
        .detach();
        // The bounds are written as they change, at most every two seconds.
        cx.observe_window_bounds(window, |this, window, _| {
            if this.bounds_saved.is_none_or(|at| at.elapsed() > Duration::from_secs(2)) {
                this.bounds_saved = Some(Instant::now());
                this.save_ui(window);
            }
        })
        .detach();
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&rename_input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                if this.making.is_some() {
                    this.finish_making(window, cx);
                } else {
                    this.finish_rename(window, cx);
                }
            }
        })
        .detach();
        let vault_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&vault_input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.finish_new_vault(window, cx);
            }
        })
        .detach();
        // The search's field: each keystroke searches, ↩ opens the first
        // note found. The find bar's: each keystroke finds, ↩ goes to
        // the next match and ⇧↩ to the one before.
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder(t("Search your notes")));
        cx.subscribe_in(&search_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => this.run_search(cx),
            InputEvent::PressEnter { .. } => {
                if let Some(first) = this.search_results.as_ref().and_then(|r| r.notes.first()) {
                    let path = first.path.clone();
                    let query = this.search_input.read(cx).value().to_string();
                    this.open_found(path, &query, window, cx);
                }
            }
            _ => {}
        })
        .detach();
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder(t("Find in this note")));
        cx.subscribe_in(&find_input, window, |this, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::Change => this.run_find(cx),
            InputEvent::PressEnter { shift, .. } => this.find_step(if *shift { -1 } else { 1 }, cx),
            _ => {}
        })
        .detach();
        // A quit writes every note with changes first, and waits for
        // what the window remembers to be on the disk.
        cx.on_app_quit(|this, cx| {
            this.save_all(cx);
            UiState::flush();
            async {}
        })
        .detach();
        let mut cfg = Config::load();
        // The faces' keys before v0.5.0's Text page: each goes to the face
        // that took its place.
        match cfg.editor.text_face.as_str() {
            "system" => cfg.editor.text_face = "inter".into(),
            _ => {}
        }
        match cfg.editor.mono_face.as_str() {
            "jetbrains-mono" => cfg.editor.mono_face = "jetbrains".into(),
            "system-mono" => cfg.editor.mono_face = crate::fonts::MONOS[1].key.into(),
            _ => {}
        }
        let mut shell = Self {
            look: Self::look_of(&cfg.editor),
            mode: Mode::from_key(ui.mode.as_deref().unwrap_or("live")),
            width: ui.page_width.as_deref().map(PageWidth::from_key).unwrap_or_default(),
            width_anim: None,
            strip_fly: None,
            go_ends: 0,
            page_w: 0.,
            cfg,
            motion: Motion::default(),
            scrollers: {
                let scrollers = Rc::new(Scrollers::default());
                cx.set_global(crate::scroll::Router(scrollers.clone()));
                scrollers
            },
            menus: Menus::default(),
            focus: cx.focus_handle(),
            ambient: cx.new(|_| Ambient::default()),
            sidebar_open: ui.sidebar_open.unwrap_or(true),
            side_view: SideView::from_key(ui.side_view.as_deref().unwrap_or("notes")),
            side_moved: None,
            choice: Choice::default(),
            tree_focus: cx.focus_handle(),
            lit_joins: RefCell::new(std::collections::HashMap::new()),
            library: Library::default(),
            side_anim: None,
            sidebar_w: ui.sidebar_w.map(px).unwrap_or(SIDEBAR_W).clamp(SIDEBAR_MIN, SIDEBAR_MAX),
            side_w_anim: None,
            side_drag: false,
            side_scroll: ScrollHandle::new(),
            page_scroll: ScrollHandle::new(),
            vault: None,
            tabs: Vec::new(),
            shown: None,
            last_shown: None,
            closed_tabs: Vec::new(),
            tabs_row: Tabs::default(),
            press_taken: false,
            win_move: false,
            ground: Cell::new(None),
            home_fig: Rc::default(),
            user_name: String::new(),
            rail_fly: None,
            search_open: false,
            search_input,
            search_scroll: ScrollHandle::new(),
            search_results: None,
            search_task: None,
            find_open: false,
            find_input,
            name_input: None,
            pressed: None,
            menu_focus: None,
            menu_link: None,
            menu_picture: None,
            menu_remove: None,
            menu_layout: None,
            pending_rename: None,
            menu_issue: None,
            menu_cell: None,
            menu_editor: None,
            words: Rc::new(Words::load()),
            rename: None,
            making: None,
            rename_input,
            new_vault: None,
            vault_input,
            notice: None,
            last_notice: None,
            export: None,
            export_focus: cx.focus_handle(),
            viewer: None,
            viewer_focus: cx.focus_handle(),
            bounds_saved: None,
            settings: None,
            settings_section: crate::settings::section_named(ui.settings_section.as_deref().unwrap_or("appearance")),
            settings_page: "fonts",
            font_inputs: None,
            settings_scroll: ScrollHandle::new(),
            lang_serial: 0,
            update: UpdateRow::default(),
        };
        shell.user_name = Self::name_of(&shell.cfg);
        // A probe's vault stands over the remembered one.
        let probing = std::env::var_os("ONYX_VAULT").is_some();
        let vault = std::env::var_os("ONYX_VAULT").map(PathBuf::from).or_else(|| ui.vault.clone());
        if let Some(root) = vault.filter(|p| p.is_dir()) {
            shell.open_vault(root, window, cx);
            // A probe names a note to open (`ONYX_NOTE`); otherwise the
            // tabs that were open are opened again, and the one that
            // showed shows.
            let probe_note = std::env::var_os("ONYX_NOTE").map(PathBuf::from).filter(|p| p.is_file());
            match probe_note {
                Some(note) => shell.open_note(note, window, cx),
                None if !probing => {
                    for path in ui.tabs.iter().filter(|p| p.is_file()) {
                        shell.open_note(path.clone(), window, cx);
                    }
                    if let Some(note) = ui.note.filter(|p| p.is_file()) {
                        shell.open_note(note, window, cx);
                    }
                }
                None => {}
            }
        }
        if shell.cfg.app.check_updates {
            shell.check_updates_later(cx);
        }
        shell
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// What the window remembers, as it is now.
    fn ui_state(&self, window: &Window) -> UiState {
        let mut ui = UiState::load();
        // `window_bounds()`, never `bounds()` (`ui_state.rs`).
        if let WindowBounds::Windowed(b) = window.window_bounds() {
            ui.window = Some(Rect { x: f32::from(b.origin.x), y: f32::from(b.origin.y), w: f32::from(b.size.width), h: f32::from(b.size.height) });
        }
        ui.sidebar_open = Some(self.sidebar_open);
        ui.side_view = Some(self.side_view.key().to_string());
        ui.sidebar_w = Some(f32::from(self.sidebar_w));
        ui.settings_section = Some(self.settings_section.to_string());
        ui.page_width = Some(self.width.key().to_string());
        if std::env::var_os("ONYX_VAULT").is_none() {
            ui.vault = self.vault.as_ref().map(|v| v.root.clone());
            ui.note = self.shown.clone();
            ui.tabs = self.tabs.iter().map(|t| t.path.clone()).collect();
            ui.folded = self.vault.as_ref().map(|v| v.folded.iter().cloned().collect()).unwrap_or_default();
        }
        ui
    }

    /// What the window remembers, handed to its writer
    /// (`UiState::save_later`): a frame never waits on the disk for a
    /// tab moved or a folder folded.
    pub fn save_ui(&self, window: &Window) {
        self.ui_state(window).save_later();
    }

    /// Written before this returns, for the window's close, which on
    /// Windows and Linux ends the app.
    pub fn save_ui_now(&self, window: &Window) {
        self.ui_state(window).save();
    }

    /// The faces and size the editors are set in, from the settings.
    fn look_of(e: &onyx_core::config::EditorConfig) -> Look {
        let sizes = e.text_sizes();
        let (text_family, mono_family) = (crate::fonts::text_family(&e.text_face, &e.text_custom), crate::fonts::mono_family(&e.mono_face, &e.mono_custom));
        let (text_fallbacks, mono_fallbacks) = (Some(crate::fonts::cjk_fallbacks(&text_family, &e.cjk_face, &e.cjk_custom)), Some(crate::fonts::cjk_fallbacks(&mono_family, &e.cjk_face, &e.cjk_custom)));
        Look { text_family, mono_family, text_px: sizes.note, mono_px: sizes.code, text_fallbacks, mono_fallbacks }
    }

    /// The settings changed: every editor takes the faces and size.
    pub(crate) fn push_look(&mut self, cx: &mut Context<Self>) {
        self.look = Self::look_of(&self.cfg.editor);
        let look = self.look.clone();
        for tab in &self.tabs {
            tab.editor.update(cx, |ed, cx| ed.set_look(look.clone(), cx));
        }
    }

    // -- the vault ------------------------------------------------------------

    pub(crate) fn vault_root(&self) -> Option<&Path> {
        self.vault.as_ref().map(|v| v.root.as_path())
    }

    fn open_vault(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.save_all(cx);
        self.tabs.clear();
        self.shown = None;
        self.closed_tabs.clear();
        let ui = UiState::load();
        let folded: HashSet<PathBuf> = ui.folded.iter().filter(|p| p.starts_with(&root)).cloned().collect();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();
        let watcher = match vault::watch(&root, move |paths| {
            let _ = tx.send(paths);
        }) {
            Ok(w) => Some(w),
            Err(e) => {
                self.say(tf("The vault is not being watched: {why}", &[("why", &e)]), cx);
                None
            }
        };
        let tree = vault::tree(&root);
        self.vault = Some(Vault { root: root.clone(), tree, folded, _watcher: watcher, changes: Rc::new(rx) });
        let mut ui = ui;
        ui.remember_vault(&root);
        ui.save_later();
        self.watch_changes(cx);
        self.library = Library::default();
        self.reread_assets(cx);
        self.save_ui(window);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The vault's changes, looked for every so often; a burst of them is
    /// one reading of the tree.
    fn watch_changes(&mut self, cx: &mut Context<Self>) {
        let Some(rx) = self.vault.as_ref().map(|v| v.changes.clone()) else { return };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH_POLL).await;
                let mut paths: Vec<PathBuf> = Vec::new();
                while let Ok(p) = rx.try_recv() {
                    paths.extend(p);
                }
                let alive = this
                    .update(cx, |s, cx| {
                        let same = s.vault.as_ref().is_some_and(|v| Rc::ptr_eq(&v.changes, &rx));
                        if same && !paths.is_empty() {
                            s.disk_changed(&paths, cx);
                        }
                        same
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// Something in the vault changed: the tree is read again off the
    /// main thread, and an untouched open note takes what the disk has.
    fn disk_changed(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Some(root) = self.vault_root().map(Path::to_path_buf) else { return };
        if Self::assets_changed(&root, paths) {
            self.reread_assets(cx);
        }
        if !paths.iter().any(|p| Self::change_matters(&root, p)) {
            return;
        }
        // The open notes the change touched, read again with the tree.
        let touched: Vec<PathBuf> = self.tabs.iter().map(|t| t.path.clone()).filter(|t| paths.iter().any(|p| p == t)).collect();
        let task = cx.background_executor().spawn(async move {
            let tree = vault::tree(&root);
            let texts: Vec<(PathBuf, Option<String>)> = touched.into_iter().map(|p| (p.clone(), vault::read(&p).ok())).collect();
            (tree, texts)
        });
        cx.spawn(async move |this, cx| {
            let (tree, texts) = task.await;
            let _ = this.update(cx, |s, cx| {
                if let Some(v) = s.vault.as_mut() {
                    v.tree = tree;
                }
                for (path, text) in texts {
                    match text {
                        Some(text) => s.disk_text(&path, text, cx),
                        // A note gone from the disk is closed.
                        None if !path.exists() => s.drop_tab(&path, cx),
                        None => {}
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// What the disk has for an open note, read after a change there. Its
    /// own write, come back, is nothing. A note with no changes takes it
    /// whole. A note with changes is never replaced: what the disk has is
    /// joined with what was typed (`merge::merge`, as Obsidian does), one
    /// step of undo, written at the next save, and said.
    fn disk_text(&mut self, path: &Path, text: String, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t.path == path && t.asset.is_none()) else { return };
        let tab = &mut self.tabs[ix];
        if tab.saved == text {
            return;
        }
        let base = std::mem::replace(&mut tab.saved, text.clone());
        if !tab.dirty {
            tab.editor.update(cx, |e, cx| e.replace_text_quietly(&text, cx));
            return;
        }
        let mine = tab.editor.read(cx).text();
        let (merged, _) = onyx_core::merge::merge(&base, &mine, &text);
        if merged != mine {
            // One edit, the stretch between what the two share at the
            // start and at the end, so the caret and the undo stay.
            let head = mine.bytes().zip(merged.bytes()).take_while(|(a, b)| a == b).count();
            let head = (0..=head).rev().find(|&i| mine.is_char_boundary(i) && merged.is_char_boundary(i)).unwrap_or(0);
            let tail = mine[head..].bytes().rev().zip(merged[head..].bytes().rev()).take_while(|(a, b)| a == b).count();
            let tail = (0..=tail).rev().find(|&i| mine.is_char_boundary(mine.len() - i) && merged.is_char_boundary(merged.len() - i)).unwrap_or(0);
            let edit = (head..mine.len() - tail, merged[head..merged.len() - tail].to_string());
            tab.editor.update(cx, |e, cx| e.apply_edits(vec![edit], cx));
        }
        let name = vault::title(path);
        self.say_quietly(tf("{name} changed outside Onyx; the changes are joined with yours.", &[("name", &name)]), "icons/git-merge.svg", cx);
    }

    /// Whether a change on the disk can show in the window: a note, a
    /// folder, or something gone whose name does not say it was neither.
    /// A picture copied in, a sync's bookkeeping, Obsidian's own folder
    /// and a temp file cost nothing: no reading, no frame.
    fn change_matters(root: &Path, path: &Path) -> bool {
        let hidden = path.strip_prefix(root).ok().is_some_and(|rest| rest.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')));
        if hidden {
            return false;
        }
        vault::is_note(path) || path.is_dir() || (!path.exists() && path.extension().is_none())
    }

    fn reread_tree(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = self.vault.as_mut() {
            v.tree = vault::tree(&v.root);
        }
        cx.notify();
    }

    /// Ask the system for a folder to be the vault.
    pub(crate) fn choose_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(t("Open").into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = cx.update(|window, cx| {
                let _ = this.update(cx, |s, cx| s.open_vault(dir, window, cx));
            });
        })
        .detach();
    }

    fn close_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_all(cx);
        self.library = Library::default();
        self.tabs.clear();
        self.shown = None;
        self.closed_tabs.clear();
        self.vault = None;
        let mut ui = UiState::load();
        ui.vault = None;
        ui.note = None;
        ui.tabs.clear();
        ui.save_later();
        self.save_ui(window);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // -- a new vault ------------------------------------------------------------

    /// The new-vault sheet: a name, and where it goes. iCloud Drive is the
    /// first choice where the machine has it, else the documents folder.
    fn start_new_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menus.close(cx);
        let place = if Self::cloud_place().is_some() { Place::Cloud } else { Place::Folder };
        self.new_vault = Some(NewVaultSheet { place, folder: onyx_core::paths::documents() });
        self.vault_input.update(cx, |i, cx| {
            i.set_value(t("Notes"), window, cx);
            i.select_all(window, cx);
        });
        window.focus(&self.vault_input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// Where a vault goes in iCloud: Onyx's own app library when the
    /// bundle is signed for one (`sys::icloud_container`), else an `Onyx`
    /// folder at iCloud Drive's root; nothing where the machine has no
    /// iCloud Drive.
    fn cloud_place() -> Option<PathBuf> {
        crate::sys::icloud_container().or_else(|| onyx_core::paths::icloud_drive().map(|d| d.join("Onyx")))
    }

    /// Where a vault named `name` would go for the sheet's choice.
    fn new_vault_parent(sheet: &NewVaultSheet) -> PathBuf {
        match sheet.place {
            Place::Cloud => Self::cloud_place().unwrap_or_else(onyx_core::paths::documents),
            Place::Folder => sheet.folder.clone(),
        }
    }

    /// Ask the system for the folder a new vault goes in.
    fn pick_vault_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(t("Choose").into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = cx.update(|window, cx| {
                let _ = this.update(cx, |s, cx| {
                    if let Some(sheet) = s.new_vault.as_mut() {
                        sheet.folder = dir;
                        sheet.place = Place::Folder;
                    }
                    window.focus(&s.vault_input.read(cx).focus_handle(cx), cx);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn finish_new_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sheet) = self.new_vault.take() else { return };
        let name = self.vault_input.read(cx).value().to_string();
        match vault::new_vault(&Self::new_vault_parent(&sheet), &name) {
            Ok(root) => self.open_vault(root, window, cx),
            Err(e) => {
                self.say(tf("The vault was not made: {why}", &[("why", &e)]), cx);
                self.focus_editor_or_shell(window, cx);
            }
        }
        cx.notify();
    }

    fn cancel_new_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_vault = None;
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    // -- the notes: the tabs ----------------------------------------------------

    /// The tab showing, and its editor.
    fn shown_tab(&self) -> Option<&Tab> {
        let shown = self.shown.as_ref()?;
        self.tabs.iter().find(|t| t.path == *shown)
    }

    pub(crate) fn shown_editor(&self) -> Option<Entity<Editor>> {
        self.shown_tab().map(|t| t.editor.clone())
    }

    /// An editor for a note, set as every editor is, and listened to.
    fn make_editor(&self, path: &Path, text: &str, cx: &mut Context<Self>) -> (Entity<Editor>, Subscription) {
        let editor = cx.new(Editor::new);
        let look = self.look.clone();
        let mode = self.mode;
        let root = self.vault_root().map(Path::to_path_buf);
        let (writing, words) = (self.writing_language(), self.words.clone());
        editor.update(cx, |e, cx| {
            e.set_look(look, cx);
            e.set_mode(mode, cx);
            e.set_place(path.to_path_buf(), root, cx);
            e.set_writing(writing, words, cx);
            e.set_text(text, cx);
        });
        let sub = Self::listen(&editor, path.to_path_buf(), cx);
        (editor, sub)
    }

    /// What a tab's editor says, heard by the note's path.
    fn listen(editor: &Entity<Editor>, path: PathBuf, cx: &mut Context<Self>) -> Subscription {
        cx.subscribe(editor, move |this, editor, ev: &EditorEvent, cx| match ev {
            EditorEvent::Changed => this.note_changed(&path, cx),
            EditorEvent::OpenLink(url) => this.open_link(url, cx),
            EditorEvent::OpenNote(name) => this.open_wikilink(name, cx),
            EditorEvent::Menu(at, what) => this.canvas_menu(editor, *at, what.clone(), cx),
            EditorEvent::View(viewing) => this.open_viewer(viewing, cx),
            EditorEvent::Found => cx.notify(),
            EditorEvent::Rename(file, name) => {
                this.pending_rename = Some((file.clone(), name.clone()));
                cx.notify();
            }
        })
    }

    /// A note's name as a field: it wraps as the name did and is kept
    /// when ↩ is pressed or the keyboard leaves. Focus events come after
    /// the frame that moved the keyboard, so the field is found by what
    /// it is, never by the note showing, which may be another by then.
    fn make_title(&self, path: &Path, window: &mut Window, cx: &mut Context<Self>) -> (Entity<TextareaState>, Subscription) {
        let name = vault::title(path);
        let title = cx.new(|cx| {
            let mut field = TextareaState::new(window, cx).auto_grow(1, TITLE_ROWS).soft_wrap(true).submit_on_enter(true).placeholder(t("Untitled").to_string());
            field.set_value(name, window, cx);
            field
        });
        let sub = cx.subscribe_in(&title, window, |this, field, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { .. } => {
                this.keep_title(field, window, cx);
                this.focus_editor_or_shell(window, cx);
                cx.notify();
            }
            InputEvent::Blur => this.keep_title(field, window, cx),
            _ => {}
        });
        (title, sub)
    }

    fn keep_title(&mut self, field: &Entity<TextareaState>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.iter().position(|t| t.title == *field) {
            self.keep_title_at(ix, window, cx);
        }
    }

    /// The name in a tab's field goes to the file. One that cannot be
    /// used puts the file's own name back. A line break, from ⇧↩ or a
    /// paste, is a space: a file's name has none.
    fn keep_title_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.tabs[ix].path.clone();
        let field = self.tabs[ix].title.clone();
        let typed = field.read(cx).value().to_string();
        let name = one_line(&typed);
        let kept = name.is_empty() || name == vault::title(&path) || self.rename_entry(&path, &name, window, cx);
        if !kept || name.is_empty() || name != typed {
            let shown = self.tabs.get(ix).map(|t| vault::title(&t.path)).unwrap_or_default();
            field.update(cx, |f, cx| f.set_value(shown, window, cx));
        }
    }

    /// Esc in a note's name: what was typed goes, and the keyboard with it.
    fn drop_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.shown_tab() {
            let (field, name) = (tab.title.clone(), vault::title(&tab.path));
            field.update(cx, |f, cx| f.set_value(name, window, cx));
        }
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    /// A note opened: its tab shows, made if it was not open, at the end
    /// of its folder's pill.
    fn open_note(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.iter().any(|t| t.path == path) {
            self.show_tab(&path, window, cx);
            return;
        }
        // Any other file opens as an asset, read only.
        if !vault::is_note(&path) {
            self.open_asset(path, window, cx);
            return;
        }
        match vault::read(&path) {
            Ok(text) => {
                let (editor, sub) = self.make_editor(&path, &text, cx);
                let (title, title_sub) = self.make_title(&path, window, cx);
                self.tabs.push(Tab { path: path.clone(), editor, title, dirty: false, saved: text, save_due: None, _sub: sub, _title_sub: title_sub, asset: None });
                self.closed_tabs.retain(|c| *c != path);
                self.sort_tabs();
                self.show_tab(&path, window, cx);
            }
            Err(e) => {
                self.say(tf("The note could not be read: {why}", &[("why", &e)]), cx);
                cx.notify();
            }
        }
    }

    /// The row's order: a folder's tabs together (`Tabs::group`).
    fn sort_tabs(&mut self) {
        let order = Tabs::order(&Tabs::group(&self.tabs.iter().map(|t| t.path.clone()).collect::<Vec<_>>()));
        self.tabs.sort_by_key(|t| order.iter().position(|p| *p == t.path).unwrap_or(usize::MAX));
    }

    /// The order a drag gave the row (`Tabs::drag_tab_to`,
    /// `Tabs::drag_pill_to`), kept and remembered.
    pub(crate) fn reorder_tabs(&mut self, order: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.sort_by_key(|t| order.iter().position(|p| *p == t.path).unwrap_or(usize::MAX));
        self.save_ui(window);
        cx.notify();
    }

    /// The tab shows, and the keyboard goes to its editor.
    pub(crate) fn show_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.rail_fly_close(cx);
        let Some(tab) = self.tabs.iter().find(|t| t.path == path) else { return };
        let _ = tab;
        // The note left is written, as Obsidian writes one its editor leaves.
        if let Some(ix) = self.shown.as_ref().filter(|s| s.as_path() != path).and_then(|s| self.tabs.iter().position(|t| t.path == *s)) {
            self.save_tab(ix, cx);
        }
        self.shown = Some(path.to_path_buf());
        // The note comes up with no caret: the keyboard is the window's
        // until a press in the words asks for it.
        window.focus(&self.focus, cx);
        self.save_ui(window);
        cx.notify();
    }

    /// A tab closed: its note is written when it has changes, and the
    /// next tab shows in its place, or with none left the vault's page.
    pub(crate) fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t.path == path) else { return };
        // A name being typed is kept first; the note's path may move with it.
        self.keep_title_at(ix, window, cx);
        let path = self.tabs[ix].path.clone();
        let path = path.as_path();
        self.save_tab(ix, cx);
        self.tabs[ix].editor.update(cx, |e, cx| e.let_pictures_go(window, cx));
        let gone = self.tabs.remove(ix);
        Self::let_asset_go(gone.asset, window, cx);
        self.closed_tabs.retain(|c| c != path);
        self.closed_tabs.push(path.to_path_buf());
        if self.shown.as_deref() == Some(path) {
            self.shown = None;
            match self.tabs.get(ix.min(self.tabs.len().saturating_sub(1))).map(|t| t.path.clone()) {
                Some(next) => self.show_tab(&next, window, cx),
                None => window.focus(&self.focus, cx),
            }
        }
        self.save_ui(window);
        cx.notify();
    }

    /// A tab let go without a write: its file is gone, or on its way to
    /// the trash with what was written already.
    fn drop_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t.path == path) else { return };
        self.tabs.remove(ix);
        if self.shown.as_deref() == Some(path) {
            self.shown = self.tabs.get(ix.min(self.tabs.len().saturating_sub(1))).map(|t| t.path.clone());
        }
        cx.notify();
    }

    /// ⌘W: the tab showing closes; with none, the window.
    /// ⌘W: the tab showing closes; on the vault's page with tabs still
    /// open, the page gives way to the tab shown last, or the row's last;
    /// on the page with none, the window closes.
    fn close_nearest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.shown.clone() {
            Some(path) => self.close_tab(&path, window, cx),
            None if !self.tabs.is_empty() => {
                let back = self.last_shown.clone().filter(|p| self.tabs.iter().any(|t| t.path == *p)).or_else(|| self.tabs.last().map(|t| t.path.clone()));
                if let Some(back) = back {
                    self.show_tab(&back, window, cx);
                }
            }
            None => {
                self.save_all(cx);
                self.save_ui(window);
                window.remove_window();
            }
        }
    }

    /// ⌘⇧T: the tab closed last is open again and showing, and at each
    /// further press the one closed before it. One whose note is gone
    /// since, or that is open again, is passed over.
    fn reopen_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(path) = self.closed_tabs.pop() {
            if !self.tabs.iter().any(|t| t.path == path) && path.is_file() {
                self.open_note(path, window, cx);
                return;
            }
        }
    }

    /// ⌘1 to ⌘9: that tab in the row's order, or the last when there
    /// are fewer.
    fn go_tab(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(n.saturating_sub(1).min(self.tabs.len().saturating_sub(1))) else { return };
        let path = tab.path.clone();
        self.show_tab(&path, window, cx);
    }

    /// An editor's text changed: its note has what the disk has not, and
    /// is written `SAVE_AFTER` from this first change. Later changes do not
    /// put the write off, as Obsidian's do not: typing without a pause is
    /// a write every two seconds, never one a keystroke, and never more
    /// than two seconds of words that only the window holds.
    fn note_changed(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|t| t.path == path && t.asset.is_none()) else { return };
        tab.dirty = true;
        if tab.save_due.is_none() {
            // Found again by its editor, which a rename meanwhile keeps.
            let editor = tab.editor.entity_id();
            tab.save_due = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SAVE_AFTER).await;
                let _ = this.update(cx, |s, cx| {
                    if let Some(ix) = s.tabs.iter().position(|t| t.editor.entity_id() == editor) {
                        s.save_tab(ix, cx);
                    }
                });
            }));
        }
    }

    /// One tab's note written now, when it has changes.
    fn save_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(ix) else { return };
        // A write due is this one.
        tab.save_due = None;
        // An asset is never written: what shows is read from it.
        if !tab.dirty || tab.asset.is_some() {
            return;
        }
        let text = tab.editor.read(cx).text();
        // Typed and taken back: the disk has it already.
        if text == tab.saved {
            tab.dirty = false;
            return;
        }
        match vault::write(&tab.path, &text) {
            Ok(()) => {
                tab.dirty = false;
                tab.saved = text;
            }
            Err(e) => {
                let why = e.to_string();
                self.say(tf("The note could not be written: {why}", &[("why", &why)]), cx);
            }
        }
    }

    /// Every note with changes written: as the window is left for another
    /// app, before the vault closes or changes, when the window closes, at
    /// quit.
    pub(crate) fn save_all(&mut self, cx: &mut Context<Self>) {
        for ix in 0..self.tabs.len() {
            self.save_tab(ix, cx);
        }
    }

    /// A new note in `dir`: its name asked first on the name sheet,
    /// offered as the first free "Untitled" and selected to type over;
    /// nothing is made until it is given (`finish_making`).
    fn new_note_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = vault::free_stem(&dir, t("Untitled"), false);
        self.start_making(dir, false, name, window, cx);
    }

    fn new_folder_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = vault::free_stem(&dir, t("New folder"), true);
        self.start_making(dir, true, name, window, cx);
    }

    fn start_making(&mut self, dir: PathBuf, folder: bool, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.rename = None;
        self.making = Some((dir, folder));
        self.rename_input.update(cx, |i, cx| {
            i.set_value(name.clone(), window, cx);
            i.select_all(window, cx);
        });
        window.focus(&self.rename_input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// The name given: the note or the folder made under it, and a note
    /// opened. A name refused keeps the sheet up, saying why.
    fn finish_making(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((dir, folder)) = self.making.clone() else { return };
        let name = self.rename_input.read(cx).value().to_string();
        match vault::create(&dir, &name, folder) {
            Ok(path) => {
                self.making = None;
                self.reread_tree(cx);
                if let Some(v) = self.vault.as_mut() {
                    let mut up = if folder { Some(path.as_path()) } else { path.parent() };
                    while let Some(d) = up {
                        v.folded.remove(d);
                        up = d.parent();
                    }
                }
                if folder {
                    self.focus_editor_or_shell(window, cx);
                } else {
                    self.open_note(path, window, cx);
                }
                self.save_ui(window);
            }
            Err(e) if folder => self.say(tf("The folder could not be made: {why}", &[("why", &e)]), cx),
            Err(e) => self.say(tf("The note could not be made: {why}", &[("why", &e)]), cx),
        }
        cx.notify();
    }

    /// Where a new note goes: the open note's folder, else the vault; an
    /// asset's folder is the vault's own, not a place for notes.
    fn new_note_dir(&self) -> Option<PathBuf> {
        let root = self.vault_root()?;
        Some(self.shown.as_ref().filter(|n| vault::is_note(n)).and_then(|n| n.parent()).map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf()))
    }

    /// A link out of the app, to the system. A probe says it and opens
    /// nothing: a copy under a script stays in its window.
    fn open_outside(&self, target: &str) {
        if std::env::var_os("ONYX_GO").is_some() {
            eprintln!("open {target}");
        } else {
            crate::sys::open(target);
        }
    }

    fn open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        if url.contains("://") || url.starts_with("mailto:") {
            self.open_outside(url);
            return;
        }
        // A path from the note, or a file of the vault by its name: a
        // note opens here, anything else with the system's app.
        let Some(from) = self.shown.clone() else { return };
        let Some(path) = vault::find_file(self.vault_root(), &from, url) else { return };
        if vault::is_note(&path) {
            let weak = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = cx.active_window().map(|w| {
                    w.update(cx, |_, window, cx| {
                        let _ = weak.update(cx, |s, cx| s.open_note(path, window, cx));
                    })
                });
            });
        } else {
            self.open_outside(&path.to_string_lossy());
        }
    }

    /// A `[[note]]` link: the note it names, or a new note of that name
    /// in the vault's folder, as Obsidian makes one.
    fn open_wikilink(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(root) = self.vault_root().map(Path::to_path_buf) else { return };
        let from = self.shown.clone().unwrap_or_else(|| root.clone());
        // `[[plan.pdf]]`: a file that is not a note opens with the system's app.
        let stem = name.split(['#', '|']).next().unwrap_or(name).trim();
        if Path::new(stem).extension().is_some_and(|_| !vault::is_note(Path::new(stem))) {
            if let Some(file) = vault::find_file(Some(&root), &from, stem) {
                self.open_outside(&file.to_string_lossy());
                return;
            }
        }
        let target = vault::resolve_link(&root, &from, name).or_else(|| {
            vault::new_note(&root, stem).ok()
        });
        let Some(path) = target else { return };
        let weak = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = cx.active_window().map(|w| {
                w.update(cx, |_, window, cx| {
                    let _ = weak.update(cx, |s, cx| {
                        s.reread_tree(cx);
                        s.open_note(path, window, cx)
                    });
                })
            });
        });
    }

    pub(crate) fn say(&mut self, words: String, cx: &mut Context<Self>) {
        self.notice_of(Notice { words, at: Instant::now(), done: None, icon: None }, cx);
    }

    /// News that is no failure, quiet, with its icon before it.
    pub(crate) fn say_quietly(&mut self, words: String, icon: &'static str, cx: &mut Context<Self>) {
        self.notice_of(Notice { words, at: Instant::now(), done: None, icon: Some(icon) }, cx);
    }

    /// What was asked for is done: a quiet line, and a press on it shows
    /// the file.
    pub(crate) fn say_done(&mut self, words: String, file: PathBuf, cx: &mut Context<Self>) {
        self.notice_of(Notice { words, at: Instant::now(), done: Some(file), icon: None }, cx);
    }

    fn notice_of(&mut self, notice: Notice, cx: &mut Context<Self>) {
        self.last_notice = Some(notice.clone());
        self.notice = Some(notice);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(6)).await;
            let _ = this.update(cx, |s, cx| {
                if s.notice.as_ref().is_some_and(|n| n.at.elapsed() >= Duration::from_secs(6)) {
                    s.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    // -- renaming and the menus -------------------------------------------------

    fn start_rename(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = if path.is_dir() { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() } else { vault::title(&path) };
        self.rename = Some(path);
        self.rename_input.update(cx, |i, cx| {
            i.set_value(name.clone(), window, cx);
            i.select_all(window, cx);
        });
        window.focus(&self.rename_input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    fn finish_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.rename.take() else { return };
        let name = self.rename_input.read(cx).value().to_string();
        self.rename_entry(&path, &name, window, cx);
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    /// An entry takes a new name, and everything that knows it by its
    /// path follows: an open note, one inside a renamed folder, and what
    /// is remembered of them. False when the name could not be used,
    /// which is said.
    fn rename_entry(&mut self, path: &Path, name: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let renamed = if self.is_asset(path) { self.rename_asset(path, name, cx) } else { vault::rename(path, name) };
        let new_path = match renamed {
            Ok(new_path) => new_path,
            Err(e) => {
                self.say(tf("The name was not changed: {why}", &[("why", &e)]), cx);
                return false;
            }
        };
        self.follow_moves(&[(path.to_path_buf(), new_path)], window, cx);
        true
    }

    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rename = None;
        self.making = None;
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    pub(crate) fn focus_editor_or_shell(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.shown_editor() {
            Some(editor) => window.focus(&editor.read(cx).focus_handle(cx), cx),
            None => window.focus(&self.focus, cx),
        }
    }

    fn trash(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // An open note going to the trash is written first, so what was
        // typed goes with it, and its tab closes.
        let going: Vec<PathBuf> = self.tabs.iter().map(|t| t.path.clone()).filter(|p| *p == path || p.starts_with(&path)).collect();
        for p in going {
            self.close_tab(&p, window, cx);
        }
        // A probe says it: the Trash is the person's.
        if std::env::var_os("ONYX_GO").is_some() {
            eprintln!("trash {}", path.display());
        } else if let Err(e) = vault::trash(&path) {
            self.say(tf("It was not moved to the trash: {why}", &[("why", &e)]), cx);
        }
        self.reread_tree(cx);
        self.save_ui(window);
    }

    /// A note's or a folder's menu, one menu wherever it is opened: its
    /// row, its tab. In the order every menu here keeps: what changes the
    /// thing's name, what copies it, what opens or shows it, what closes
    /// it, and what cannot be taken back last, a line of its own.
    fn entry_menu(&self, path: &Path) -> Vec<Entry> {
        let copies = |into: &mut Vec<Entry>| {
            into.push(Entry::choice("copy-path", "icons/link.svg", t("Copy Path")));
            into.push(Entry::choice("copy-rel", "icons/link.svg", t("Copy Relative Path")));
        };
        let mut items = vec![Entry::choice("rename", "icons/pencil-simple.svg", t("Rename")), Entry::Rule];
        if path.is_dir() {
            items.push(Entry::choice("new-note", "icons/file-plus.svg", t("New Note")));
            items.push(Entry::choice("new-folder", "icons/folder-plus.svg", t("New Folder")));
            items.push(Entry::Rule);
            copies(&mut items);
            items.push(Entry::Rule);
            items.push(Entry::choice("open-finder", "icons/folder-open.svg", t(crate::sys::OPEN_FOLDER_LABEL)));
        } else {
            copies(&mut items);
            items.push(Entry::Rule);
            if self.shown.as_deref() != Some(path) {
                items.push(Entry::choice("open", "icons/external-link.svg", t("Open")));
            }
            items.push(Entry::choice("reveal", "icons/folder-open.svg", t(crate::sys::REVEAL_LABEL)));
            if vault::is_note(path) {
                items.push(Entry::choice("export-pdf", "icons/file-export.svg", t("Export…")));
            }
            if self.tabs.iter().any(|t| t.path == path) {
                items.push(Entry::Rule);
                items.push(Entry::choice("close-tab", "icons/close.svg", t("Close Tab")));
                if self.tabs.len() > 1 {
                    items.push(Entry::choice("close-others", "icons/circle-x.svg", t("Close Other Tabs")));
                }
            }
        }
        items.push(Entry::Rule);
        items.push(Entry::choice("trash", "icons/trash.svg", t("Move to Trash")));
        items
    }

    /// The vault's name: which vault is open.
    fn show_vault_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let mut entries = vec![Entry::choice("new", "icons/plus.svg", t("New Vault…")), Entry::choice("open-other", "icons/folder-open.svg", t("Open Vault…"))];
        if self.vault.is_some() {
            entries.push(Entry::choice("reveal", "icons/folder-open.svg", t(crate::sys::REVEAL_LABEL)));
            entries.push(Entry::Rule);
            entries.push(Entry::choice("close", "icons/arrow-left.svg", t("Close Vault")));
        }
        self.menus.show("vault", at, entries);
        cx.notify();
    }

    /// The menu of a note or a folder, at the pointer.
    pub(crate) fn show_entry_menu(&mut self, path: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        // A right click on one of several chosen is their menu; on any
        // other row it chooses that row alone.
        if self.choice.marked.len() > 1 && self.choice.marked.contains(&path) {
            return self.show_marked_menu(at, cx);
        }
        if !self.choice.marked.is_empty() {
            self.choice.marked = vec![path.clone()];
            self.choice.anchor = Some(path.clone());
            self.choice.cursor = Some(path.clone());
        }
        let entries = self.entry_menu(&path);
        self.pressed = Some(path);
        self.menus.show("entry", at, entries);
        cx.notify();
    }

    /// The vault's own menu, in the tree's empty room.
    fn show_vault_room_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(root) = self.vault_root().map(Path::to_path_buf) else { return };
        let entries = vec![
            Entry::choice("new-note", "icons/file-plus.svg", t("New Note")),
            Entry::choice("new-folder", "icons/folder-plus.svg", t("New Folder")),
            Entry::Rule,
            Entry::choice("copy-path", "icons/link.svg", t("Copy Path")),
            Entry::Rule,
            Entry::choice("open-finder", "icons/folder-open.svg", t(crate::sys::OPEN_FOLDER_LABEL)),
            Entry::Rule,
            Entry::choice("collapse", "icons/copy-minus.svg", t("Collapse All Folders")),
        ];
        self.pressed = Some(root);
        self.menus.show("entry", at, entries);
        cx.notify();
    }

    /// A folder's handle on the tabs' row: where it is, and its tabs.
    pub(crate) fn show_pill_menu(&mut self, folder: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        let entries = vec![
            Entry::choice("copy-path", "icons/link.svg", t("Copy Path")),
            Entry::choice("copy-rel", "icons/link.svg", t("Copy Relative Path")),
            Entry::Rule,
            Entry::choice("open-finder", "icons/folder-open.svg", t(crate::sys::OPEN_FOLDER_LABEL)),
            Entry::Rule,
            Entry::choice("close-folder", "icons/circle-x.svg", t("Close Tabs")),
        ];
        self.pressed = Some(folder);
        self.menus.show("entry", at, entries);
        cx.notify();
    }

    /// The path over the page.
    fn show_path_menu(&mut self, target: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        let mut entries = vec![Entry::choice("copy-path", "icons/link.svg", t("Copy Path"))];
        if self.vault_root() != Some(target.as_path()) {
            entries.push(Entry::choice("copy-rel", "icons/link.svg", t("Copy Relative Path")));
        }
        entries.push(Entry::Rule);
        entries.push(if target.is_dir() { Entry::choice("open-finder", "icons/folder-open.svg", t(crate::sys::OPEN_FOLDER_LABEL)) } else { Entry::choice("reveal", "icons/folder-open.svg", t(crate::sys::REVEAL_LABEL)) });
        self.pressed = Some(target);
        self.menus.show("entry", at, entries);
        cx.notify();
    }

    /// A note found in the search.
    fn show_found_menu(&mut self, path: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        let entries = vec![
            Entry::choice("copy-path", "icons/link.svg", t("Copy Path")),
            Entry::choice("copy-rel", "icons/link.svg", t("Copy Relative Path")),
            Entry::Rule,
            Entry::choice("open", "icons/external-link.svg", t("Open")),
            Entry::choice("reveal", "icons/folder-open.svg", t(crate::sys::REVEAL_LABEL)),
        ];
        self.pressed = Some(path);
        self.menus.show("found", at, entries);
        cx.notify();
    }

    /// A vault offered on the landing page.
    fn show_recent_menu(&mut self, path: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        let entries = vec![
            Entry::choice("copy-path", "icons/link.svg", t("Copy Path")),
            Entry::Rule,
            Entry::choice("open", "icons/external-link.svg", t("Open")),
            Entry::choice("open-finder", "icons/folder-open.svg", t(crate::sys::OPEN_FOLDER_LABEL)),
        ];
        self.pressed = Some(path);
        self.menus.show("recent", at, entries);
        cx.notify();
    }

    /// The note's words: on a link, what it goes to and what it gives the
    /// clipboard; anywhere else what a text offers, in a note that takes
    /// no change only what leaves it as it is.
    fn canvas_menu(&mut self, editor: Entity<Editor>, at: Point<Pixels>, what: MenuAt, cx: &mut Context<Self>) {
        // A frame's words: copied, never changed, and only the frame's.
        if let Some(frame) = what.frame {
            self.menu_focus = Some(frame.focus);
            self.menu_picture = None;
            self.menu_link = None;
            self.menu_issue = None;
            self.menu_cell = None;
            self.menu_editor = Some(editor);
            self.menus.show("canvas", at, crate::menu::edit_entries(frame.selection, false, true));
            cx.notify();
            return;
        }
        let (selection, locked, focus) = {
            let e = editor.read(cx);
            (e.has_selection(), e.locked(), e.focus_handle(cx))
        };
        let mut items = Vec::new();
        // How a piece's link shows, first: a card's size, the whole file.
        // Then a picture's size and place, as its bar has them: a row each.
        let mut rows = Vec::new();
        // A piece (a card, a frame) has it first; a link among words, after
        // the link's own choices.
        let piece = what.remove.is_some();
        if let Some(l) = what.layout.as_ref().filter(|_| !locked && (piece || what.link.is_none())) {
            rows.extend(Self::layout_entries(l));
        }
        // A file of the vault, a picture or a link's: what the sidebar's
        // asset menu offers first.
        // A picture's size and place, as its bar has them, and the viewer.
        if let Some((_, across, asked)) = what.picture {
            use crate::menu::Tool;
            use onyx_core::html::{Across, Width};
            let full = asked == Some(Width::Share(100));
            // The viewer is a picture's; a file has its frame.
            if !what.file.as_deref().is_some_and(crate::layout::is_framed) {
                items.push(Entry::choice("pic-view", "icons/maximize.svg", t("View")));
                items.push(Entry::Rule);
            }
            rows.push(Entry::Icons { label: t("Size").into(), items: vec![Tool::new("pic-auto", "icons/image.svg", t("Original Size"), asked.is_none()), Tool::new("pic-full", "icons/picture-full.svg", t("Full Width"), full)] });
            if !full {
                rows.push(Entry::Icons { label: t("Align").into(), items: vec![Tool::new("pic-left", "icons/align-left.svg", t("Align Left"), across == Across::Start), Tool::new("pic-center", "icons/align-center.svg", t("Align Center"), across == Across::Center), Tool::new("pic-right", "icons/align-right.svg", t("Align Right"), across == Across::End)] });
            }
        }
        if !rows.is_empty() {
            items.extend(rows);
            items.push(Entry::Rule);
        }
        if let Some(file) = &what.file {
            items.push(Entry::choice("file-rename", "icons/pencil-simple.svg", t("Rename")));
            if what.link.is_none() {
                items.push(Entry::Rule);
                items.push(Entry::choice("file-open", "icons/eye.svg", t("Open")));
                items.push(Entry::choice("file-open-app", "icons/external-link.svg", t("Open with Default App")));
            }
            items.push(Entry::choice("file-reveal", "icons/folder-open.svg", t(crate::sys::REVEAL_LABEL)));
            items.push(Entry::Rule);
            if what.link.is_none() {
                items.push(Entry::choice("file-copy-path", "icons/link.svg", t("Copy Path")));
            }
            self.pressed = Some(file.clone());
        }
        if let Some(link) = &what.link {
            items.push(Entry::choice("open-link", "icons/external-link.svg", if link.note.is_some() { t("Open Note") } else { t("Open Link") }));
            items.push(Entry::Rule);
            if link.url.is_some() {
                items.push(Entry::choice("copy-link", "icons/link.svg", t("Copy Link")));
            }
            if !link.words.trim().is_empty() {
                items.push(Entry::choice("copy-text", "icons/typography.svg", t("Copy Text")));
            }
            if !link.raw.trim().is_empty() {
                items.push(Entry::choice("copy-markdown", "icons/markdown.svg", t("Copy Markdown")));
            }
            if let Some(l) = what.layout.as_ref().filter(|_| !locked && !piece) {
                items.push(Entry::Rule);
                items.extend(Self::layout_entries(l));
            }
        } else if what.file.is_none() {
            if let Some(issue) = &what.issue {
                items.extend(Self::issue_entries(issue));
                items.push(Entry::Rule);
            }
            let scope = what.table.as_ref().map(|(_, scope, _)| *scope);
            if !matches!(scope, Some(Scope::Row | Scope::Column)) {
                items.extend(crate::menu::edit_entries(selection, cx.read_from_clipboard().is_some(), locked));
            }
            if let Some((_, scope, facts)) = what.table.as_ref().filter(|_| !locked) {
                if !items.is_empty() {
                    items.push(Entry::Rule);
                }
                let at = what.table.as_ref().map_or(0, |(cell, _, _)| cell.row);
                let col = what.table.as_ref().map_or(0, |(cell, _, _)| cell.col);
                items.extend(Self::table_entries(*scope, facts, at, col));
            } else if what.blank && !locked {
                items.push(Entry::Rule);
                items.push(Entry::choice("table-new", "icons/table.svg", t("Insert Table")));
            }
            if self.shown.is_some() && what.table.as_ref().is_none_or(|(_, s, _)| *s == Scope::Cell) {
                items.push(Entry::Rule);
                items.push(Entry::choice("canvas-export", "icons/file-export.svg", t("Export…")));
                items.push(Entry::choice("canvas-print", "icons/printer.svg", t("Print…")));
            }
        }
        // Delete, last, for a picture or a frame: out of the note, the
        // file left where it is.
        if let Some(line) = what.remove.filter(|_| !locked) {
            if !items.is_empty() && !matches!(items.last(), Some(Entry::Rule)) {
                items.push(Entry::Rule);
            }
            items.push(Entry::danger("remove", "icons/trash.svg", t("Delete")));
            self.menu_remove = Some(line);
        } else {
            self.menu_remove = None;
        }
        self.menu_layout = what.layout.filter(|_| !locked);
        self.menu_focus = Some(focus);
        self.menu_picture = what.picture.map(|(line, _, _)| line);
        self.menu_link = what.link;
        self.menu_issue = what.issue;
        self.menu_cell = what.table.map(|(cell, _, _)| cell);
        self.menu_editor = Some(editor);
        self.menus.show("canvas", at, items);
        cx.notify();
    }

    /// How a link can show: among words, as a card of a size, or for a
    /// file a frame shows, whole; the way it shows now ticked.
    fn layout_entries(l: &crate::editor::LayoutAt) -> Vec<Entry> {
        use crate::editor::Layout;
        use crate::menu::Tool;
        use onyx_core::html::CardSize;
        let mut tools = Vec::new();
        if l.preview {
            tools.push(Tool::new("layout-preview", "icons/card-preview.svg", t("Preview"), l.now == Layout::Preview));
        }
        tools.push(Tool::new("layout-large", "icons/card-large.svg", t("Large Card"), l.now == Layout::Card(CardSize::Large)));
        tools.push(Tool::new("layout-medium", "icons/card-medium.svg", t("Medium Card"), l.now == Layout::Card(CardSize::Medium)));
        tools.push(Tool::new("layout-small", "icons/card-small.svg", t("Small Card"), l.now == Layout::Card(CardSize::Small)));
        tools.push(Tool::new("layout-link", "icons/link.svg", t("Link"), l.now == Layout::Link));
        let mut items = vec![Entry::Icons { label: t("Show As").into(), items: tools }];
        // A large card, narrower than the words, stands where it is placed.
        if let Some(across) = l.across {
            use onyx_core::html::Across;
            items.push(Entry::Icons { label: t("Align").into(), items: vec![Tool::new("layout-left", "icons/align-left.svg", t("Align Left"), across == Across::Start), Tool::new("layout-center", "icons/align-center.svg", t("Align Center"), across == Across::Center), Tool::new("layout-right", "icons/align-right.svg", t("Align Right"), across == Across::End)] });
        }
        items
    }

    /// A mark's menu: what could stand there, and what settles the word.
    fn issue_entries(issue: &MenuIssue) -> Vec<Entry> {
        let mut items: Vec<Entry> = issue.fixes.iter().take(5).enumerate().map(|(i, fix)| Entry::choice(format!("fix-{i}"), "icons/replace.svg", fix.clone())).collect();
        if issue.fixes.is_empty() {
            items.push(Entry::choice("no-fix", "icons/replace.svg", t("No spelling found")));
        }
        items.push(Entry::Rule);
        if issue.spelling {
            items.push(Entry::choice("learn-word", "icons/book-open.svg", t("Learn Spelling")));
        }
        items.push(Entry::choice("ignore-word", "icons/eye-off.svg", t("Ignore")));
        items
    }

    /// A table's menu. On a cell, what it most often wants; on a row's or
    /// a column's handle, all that row or column can do. What takes the
    /// table apart comes last, after a line of its own.
    fn table_entries(scope: Scope, facts: &TableFacts, row: usize, col: usize) -> Vec<Entry> {
        use crate::markdown::Align;
        let aligns = |items: &mut Vec<Entry>| {
            let choice = |id: &'static str, icon: &'static str, word: &'static str, on: bool| Entry::Choice { id: id.into(), icon, label: word.into(), picture: false, chosen: on };
            items.push(choice("align-left", "icons/align-left.svg", t("Align Left"), facts.align == Align::Left));
            items.push(choice("align-center", "icons/align-center.svg", t("Align Center"), facts.align == Align::Center));
            items.push(choice("align-right", "icons/align-right.svg", t("Align Right"), facts.align == Align::Right));
        };
        let sorts = |items: &mut Vec<Entry>| {
            items.push(Entry::choice("sort-up", "icons/sort-ascending.svg", t("Sort A to Z")));
            items.push(Entry::choice("sort-down", "icons/sort-descending.svg", t("Sort Z to A")));
        };
        let mut items = Vec::new();
        match scope {
            Scope::Cell => {
                items.push(Entry::choice("row-above", "icons/row-insert-top.svg", t("Insert Row Above")));
                items.push(Entry::choice("row-below", "icons/row-insert-bottom.svg", t("Insert Row Below")));
                items.push(Entry::choice("col-left", "icons/column-insert-left.svg", t("Insert Column Left")));
                items.push(Entry::choice("col-right", "icons/column-insert-right.svg", t("Insert Column Right")));
                items.push(Entry::Rule);
                aligns(&mut items);
                items.push(Entry::Rule);
                if row > 0 {
                    items.push(Entry::choice("row-delete", "icons/row-remove.svg", t("Delete Row")));
                }
                items.push(Entry::choice("col-delete", "icons/column-remove.svg", t("Delete Column")));
                items.push(Entry::choice("table-delete", "icons/trash.svg", t("Delete Table")));
            }
            Scope::Row => {
                items.push(Entry::choice("row-above", "icons/row-insert-top.svg", t("Insert Row Above")));
                items.push(Entry::choice("row-below", "icons/row-insert-bottom.svg", t("Insert Row Below")));
                items.push(Entry::choice("row-duplicate", "icons/copy.svg", t("Duplicate Row")));
                if facts.rows > 1 {
                    items.push(Entry::Rule);
                    if row > 0 {
                        items.push(Entry::choice("row-up", "icons/arrow-up.svg", t("Move Row Up")));
                    }
                    if row + 1 < facts.rows {
                        items.push(Entry::choice("row-down", "icons/arrow-down.svg", t("Move Row Down")));
                    }
                }
                if row > 0 {
                    items.push(Entry::Rule);
                    items.push(Entry::choice("row-delete", "icons/row-remove.svg", t("Delete Row")));
                }
            }
            Scope::Column => {
                items.push(Entry::choice("col-left", "icons/column-insert-left.svg", t("Insert Column Left")));
                items.push(Entry::choice("col-right", "icons/column-insert-right.svg", t("Insert Column Right")));
                if facts.columns > 1 {
                    items.push(Entry::Rule);
                    if col > 0 {
                        items.push(Entry::choice("col-move-left", "icons/arrow-left.svg", t("Move Column Left")));
                    }
                    if col + 1 < facts.columns {
                        items.push(Entry::choice("col-move-right", "icons/arrow-right.svg", t("Move Column Right")));
                    }
                }
                items.push(Entry::Rule);
                aligns(&mut items);
                items.push(Entry::Rule);
                sorts(&mut items);
                items.push(Entry::Rule);
                items.push(Entry::choice("col-delete", "icons/column-remove.svg", t("Delete Column")));
            }
        }
        items
    }

    /// The language a note is checked in, while the check is on.
    pub(crate) fn writing_language(&self) -> Option<String> {
        self.cfg.editor.check_writing.then(|| self.cfg.editor.writing_language.clone())
    }

    /// The check's settings or the words settled changed: every note hears.
    pub(crate) fn push_writing(&mut self, cx: &mut Context<Self>) {
        let (writing, words) = (self.writing_language(), self.words.clone());
        for tab in self.tabs.iter().filter(|t| t.asset.is_none()) {
            tab.editor.update(cx, |e, cx| e.set_writing(writing.clone(), words.clone(), cx));
        }
    }

    fn settle_word(&mut self, change: impl FnOnce(&mut Words), cx: &mut Context<Self>) {
        let mut words = (*self.words).clone();
        change(&mut words);
        self.words = Rc::new(words);
        self.push_writing(cx);
    }

    /// A text field's menu, handed to the field for its right click. The
    /// field calls this from inside its own update, where it cannot be
    /// read, so the menu is made a moment later.
    pub(crate) fn input_menu<M: gpui_base::input::InputModeKind>(input: &Entity<gpui_base::input::InputBaseState<M>>, what: &'static str, cx: &mut Context<Self>) -> impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static {
        let (this, input) = (cx.entity().downgrade(), input.clone());
        move |at, window, cx| {
            let (this, input) = (this.clone(), input.clone());
            window.defer(cx, move |_, cx| {
                let (selection, focus) = {
                    let state = input.read(cx);
                    (!state.selected_range().is_empty(), gpui::Focusable::focus_handle(state, cx))
                };
                let clipboard = cx.read_from_clipboard().is_some();
                let _ = this.update(cx, |this, cx| {
                    this.menu_focus = Some(focus);
                    this.menus.show(what, at, crate::menu::edit_entries(selection, clipboard, false));
                    cx.notify();
                });
            });
        }
    }

    fn copy_text(text: impl Into<String>, cx: &mut Context<Self>) {
        crate::widgets::to_clipboard(text, cx);
    }

    fn menu_pick(&mut self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let pressed = self.pressed.take();
        self.menus.close(cx);
        match (what, id.as_ref()) {
            ("marked", id) => self.marked_pick(id, window, cx),
            ("asset", id) => {
                if let Some(p) = pressed {
                    self.asset_pick(id, p, window, cx);
                }
            }
            ("entry", "open") => {
                if let Some(p) = pressed {
                    self.open_note(p, window, cx);
                }
            }
            ("entry", "reveal") => {
                if let Some(p) = pressed {
                    crate::sys::reveal(&p);
                }
            }
            ("entry", "export-pdf") => {
                if let Some(p) = pressed {
                    self.open_export(p, window, cx);
                }
            }
            ("pdf-paper" | "pdf-face" | "pdf-mono", id) => self.pick_export(what, id, window, cx),
            ("entry" | "found" | "recent", "copy-path") => {
                if let Some(p) = pressed {
                    Self::copy_text(p.to_string_lossy(), cx);
                }
            }
            ("entry" | "found", "copy-rel") => {
                if let (Some(p), Some(root)) = (pressed, self.vault_root()) {
                    Self::copy_text(p.strip_prefix(root).unwrap_or(&p).to_string_lossy(), cx);
                }
            }
            ("entry" | "recent", "open-finder") => {
                if let Some(p) = pressed {
                    self.open_outside(&p.to_string_lossy());
                }
            }
            ("entry", "close-tab") => {
                if let Some(p) = pressed {
                    self.close_tab(&p, window, cx);
                }
            }
            ("entry", "close-others") => {
                if let Some(p) = pressed {
                    for other in self.tabs.iter().map(|t| t.path.clone()).filter(|o| *o != p).collect::<Vec<_>>() {
                        self.close_tab(&other, window, cx);
                    }
                }
            }
            ("entry", "close-folder") => {
                if let Some(p) = pressed {
                    for tab in self.tabs.iter().map(|t| t.path.clone()).filter(|o| o.parent() == Some(p.as_path())).collect::<Vec<_>>() {
                        self.close_tab(&tab, window, cx);
                    }
                }
            }
            ("entry", "collapse") => {
                fn folders(entries: &[vault::Entry], into: &mut HashSet<PathBuf>) {
                    for e in entries.iter().filter(|e| e.dir) {
                        into.insert(e.path.clone());
                        folders(&e.children, into);
                    }
                }
                if let Some(v) = self.vault.as_mut() {
                    let children = v.tree.children.clone();
                    folders(&children, &mut v.folded);
                }
                self.save_ui(window);
            }
            ("found", "open") => {
                if let Some(p) = pressed {
                    self.close_search(window, cx);
                    self.open_note(p, window, cx);
                }
            }
            ("found", "reveal") => {
                if let Some(p) = pressed {
                    crate::sys::reveal(&p);
                }
            }
            ("recent", "open") => {
                if let Some(p) = pressed {
                    self.open_vault(p, window, cx);
                }
            }
            ("canvas", "remove") => {
                if let (Some(line), Some(editor)) = (self.menu_remove.take(), self.menu_editor.clone()) {
                    editor.update(cx, |e, cx| e.remove_object(line, cx));
                }
            }
            ("canvas", id) if id.starts_with("layout-") => {
                if let (Some(at), Some(editor)) = (self.menu_layout.take(), self.menu_editor.clone()) {
                    let what = id["layout-".len()..].to_string();
                    editor.update(cx, |e, cx| e.layout_choice(at.line, at.at, &what, cx));
                }
            }
            ("canvas", id) if id.starts_with("pic-") => {
                if let (Some(line), Some(editor)) = (self.menu_picture.take(), self.menu_editor.clone()) {
                    let what = id["pic-".len()..].to_string();
                    editor.update(cx, |e, cx| e.picture_choice(line, &what, cx));
                }
            }
            ("canvas", id) if id.starts_with("file-") => {
                if let Some(p) = pressed {
                    self.asset_pick(&id["file-".len()..], p, window, cx);
                }
            }
            ("canvas", "open-link") => {
                if let Some(link) = self.menu_link.take() {
                    if let Some(url) = &link.url {
                        self.open_link(url, cx);
                    } else if let Some(note) = &link.note {
                        self.open_wikilink(note, cx);
                    }
                }
            }
            ("canvas", "copy-link") => {
                if let Some(url) = self.menu_link.take().and_then(|l| l.url) {
                    Self::copy_text(url, cx);
                }
            }
            ("canvas", "copy-text") => {
                if let Some(link) = self.menu_link.take() {
                    Self::copy_text(link.words, cx);
                }
            }
            ("canvas", "copy-markdown") => {
                if let Some(link) = self.menu_link.take() {
                    Self::copy_text(link.raw, cx);
                }
            }
            ("canvas", "canvas-export") => {
                if let Some(p) = self.shown.clone() {
                    self.open_export(p, window, cx);
                }
            }
            ("canvas", "canvas-print") => {
                if let Some(p) = self.shown.clone() {
                    self.print_note(p, window, cx);
                }
            }
            ("canvas", "table-new") => {
                if let Some(editor) = self.menu_editor.clone() {
                    editor.update(cx, |e, cx| e.insert_table(cx));
                }
            }
            ("canvas", "no-fix") => {}
            ("canvas", "learn-word") => {
                if let Some(issue) = self.menu_issue.take() {
                    if let Err(e) = onyx_core::check::learn(&issue.words) {
                        eprintln!("onyx: the word was not kept: {e}");
                    }
                    self.settle_word(|w| w.learned = onyx_core::check::learned(), cx);
                }
            }
            ("canvas", "ignore-word") => {
                if let Some(issue) = self.menu_issue.take() {
                    if issue.spelling {
                        self.settle_word(|w| {
                            w.passed.insert(issue.words.to_lowercase());
                        }, cx);
                    } else {
                        if let Err(e) = onyx_core::check::ignore(&issue.rule, &issue.words) {
                            eprintln!("onyx: the word was not kept: {e}");
                        }
                        self.settle_word(|w| w.ignored = onyx_core::check::ignored(), cx);
                    }
                }
            }
            ("canvas", id) if id.starts_with("fix-") => {
                let fix = id["fix-".len()..].parse::<usize>().ok();
                if let (Some(issue), Some(editor), Some(fix)) = (self.menu_issue.take(), self.menu_editor.clone(), fix) {
                    if let Some(with) = issue.fixes.get(fix) {
                        editor.update(cx, |e, cx| e.fix_issue(issue.range.clone(), with, cx));
                    }
                }
            }
            ("canvas", id) if self.menu_cell.is_some() && !matches!(id, "cut" | "copy" | "paste" | "select-all") => {
                if let (Some(cell), Some(editor)) = (self.menu_cell.take(), self.menu_editor.clone()) {
                    let id = id.to_string();
                    editor.update(cx, |e, cx| e.table_do(&id, cell, cx));
                }
            }
            ("canvas" | "edit", id) => {
                if let Some(focus) = self.menu_focus.take() {
                    crate::menu::do_edit(id, &focus, window, cx);
                }
            }
            ("entry", "rename") => {
                if let Some(p) = pressed {
                    self.start_rename(p, window, cx);
                }
            }
            ("entry", "trash") => {
                if let Some(p) = pressed {
                    self.trash(p, window, cx);
                }
            }
            ("entry", "new-note") => {
                if let Some(p) = pressed {
                    self.new_note_in(p, window, cx);
                }
            }
            ("entry", "new-folder") => {
                if let Some(p) = pressed {
                    self.new_folder_in(p, window, cx);
                }
            }
            ("vault", "new") => self.start_new_vault(window, cx),
            ("vault", "open-other") => self.choose_vault(window, cx),
            ("vault", "reveal") => {
                if let Some(r) = self.vault_root() {
                    crate::sys::reveal(r);
                }
            }
            ("vault", "close") => self.close_vault(window, cx),
            _ => {}
        }
        cx.notify();
    }

    // -- the sidebar ----------------------------------------------------------

    pub fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // From as far as the move before had got, so a second press while
        // it moves turns it back without a jump.
        let begun = match self.side_anim {
            Some((_, at)) if at.elapsed() < SLIDE => Instant::now() - (SLIDE - at.elapsed()),
            _ => Instant::now(),
        };
        self.sidebar_open = !self.sidebar_open;
        self.side_anim = Some((self.sidebar_open, begun));
        self.save_ui(window);
        cx.notify();
    }

    /// How much of the sidebar is beside the content, from 0 to 1, on one
    /// eased clock that every term of the row's layout goes by.
    fn side_t(&self) -> f32 {
        match self.side_anim {
            Some((coming, at)) if at.elapsed() < SLIDE => {
                let t = ease_out_quint()(at.elapsed().as_secs_f32() / SLIDE.as_secs_f32());
                if coming { t } else { 1. - t }
            }
            _ => {
                if self.sidebar_open {
                    1.
                } else {
                    0.
                }
            }
        }
    }

    fn sidebar_moving(&self) -> bool {
        self.side_anim.is_some_and(|(_, at)| at.elapsed() < SLIDE) || self.side_w_anim.is_some_and(|(_, at)| at.elapsed() < SLIDE)
    }

    /// How wide the sidebar stands now, on its way there after a double
    /// click on its edge.
    fn sidebar_width(&self) -> Pixels {
        match self.side_w_anim {
            Some((from, at)) if at.elapsed() < SLIDE => from + (self.sidebar_w - from) * ease_out_quint()(at.elapsed().as_secs_f32() / SLIDE.as_secs_f32()),
            _ => self.sidebar_w,
        }
    }

    /// The pointer moved with the sidebar's edge held: the sidebar is as
    /// wide as where the pointer is, between its least and most, and
    /// folds away once the pointer is well left of the least. Brought
    /// back out in the same drag, it is there again.
    fn side_drag_to(&mut self, e: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            self.side_drag_end(window, cx);
            return;
        }
        let open = e.position.x >= SIDEBAR_FOLD_AT;
        if open != self.sidebar_open {
            self.toggle_sidebar(window, cx);
        }
        if open {
            self.sidebar_w = e.position.x.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
            self.side_w_anim = None;
            cx.notify();
        }
    }

    fn side_drag_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.side_drag) {
            self.save_ui(window);
            cx.notify();
        }
    }

    /// The width at which the tree's rows in view of the folds are not
    /// cut short, long outliers held to `SIDEBAR_FIT_MAX`. A double click
    /// on the edge goes there.
    fn sidebar_fit(&self, window: &Window, cx: &App) -> Pixels {
        // Round a name: the card's margin and outline and the list's
        // padding on both sides, then a row's padding, chevron, icon and
        // gaps (`render_row`).
        const CHROME: f32 = 2. * (8. + 1. + 8.) + 8. + 12. + 6. + 16. + 6. + 8.;
        const DEPTH: f32 = 14.;
        fn widest(entries: &[vault::Entry], depth: usize, folded: &HashSet<PathBuf>, measure: &dyn Fn(&str) -> f32) -> f32 {
            entries.iter().fold(0., |most: f32, e| {
                let inside = if e.dir && !folded.contains(&e.path) { widest(&e.children, depth + 1, folded, measure) } else { 0. };
                most.max(DEPTH * depth as f32 + measure(&e.name)).max(inside)
            })
        }
        let font = font(cx.theme().font_family.clone());
        let measure = |text: &str| {
            let run = TextRun { len: text.len(), font: font.clone(), color: gpui::black(), background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(SharedString::from(text.to_string()), px(13.), &[run], None).width())
        };
        let names = self.vault.as_ref().map_or(0., |v| widest(&v.tree.children, 0, &v.folded, &measure));
        px((CHROME + names).ceil()).clamp(SIDEBAR_MIN, SIDEBAR_FIT_MAX)
    }

    /// The strip over the card's left edge that takes the drag and the
    /// double click, after emaki's: the sidebar's edge, or the rail's
    /// while it is folded, from where the same drag brings it out. It
    /// lights a line on the edge under the pointer, and the pointer keeps
    /// its look wherever a drag takes it.
    fn render_side_grip(&self, edge: Pixels, cx: &mut Context<Self>) -> impl IntoElement {
        let line = cx.theme().primary.opacity(0.55);
        let held = self.side_drag;
        div()
            .id("side-grip")
            .group("side-grip")
            .absolute()
            .top(FRAME)
            .bottom(FRAME)
            .left(edge - SIDEBAR_GRIP / 2.)
            .w(SIDEBAR_GRIP)
            .occlude()
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    swallow_click(window, cx);
                    if ev.click_count == 2 {
                        this.side_drag = false;
                        this.side_w_anim = this.sidebar_open.then(|| (this.sidebar_width(), Instant::now()));
                        this.sidebar_w = this.sidebar_fit(window, cx);
                        if this.sidebar_open {
                            this.save_ui(window);
                        } else {
                            this.toggle_sidebar(window, cx);
                        }
                    } else {
                        this.side_drag = true;
                    }
                    cx.notify();
                }),
            )
            .child(div().absolute().top(CARD_R).bottom(CARD_R).left(SIDEBAR_GRIP / 2. - px(1.5)).w(px(2.)).when(held, |d| d.bg(line)).group_hover("side-grip", move |s| s.bg(line)))
            .when(held, |d| d.child(canvas(|_, _, _| (), |_, _, window, _| window.set_window_cursor_style(CursorStyle::ResizeLeftRight)).absolute().size_0()))
    }

    /// The frame's theme: the sidebar stands on the frame, which is a dark
    /// graphite in both appearances, so it draws with a dark theme's inks
    /// whatever the page is.
    fn frame_theme(&self, cx: &App) -> gpui_component::Theme {
        let mut theme = cx.theme().clone();
        if !theme.mode.is_dark() {
            theme.foreground = rgb(0xF3F1EA).into();
            theme.muted_foreground = rgb(0xA09D94).into();
            theme.mode = gpui_component::theme::ThemeMode::Dark;
            let (accent, on) = crate::look::on_frame(&self.cfg.app.accent);
            theme.primary = Rgba::try_from(accent).map(Hsla::from).unwrap_or(theme.primary);
            theme.primary_foreground = Rgba::try_from(on).map(Hsla::from).unwrap_or(theme.primary_foreground);
        }
        theme
    }

    /// The tree as drawn: a row for each entry in view, a folder's
    /// children under its row, and the rows out of view as one empty box
    /// above and one below, of their height, so the list keeps its length
    /// and the scroll its place. Every row is `TREE_ROW_H` tall, so where
    /// each stands is a sum and nothing is measured; a vault of a thousand
    /// notes costs the frame no more than the rows in view.
    fn render_tree(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(v) = &self.vault else { return Vec::new() };
        {
            let rows = self.visible_rows();
            let lit: Vec<bool> = rows.iter().map(|(p, _)| self.row_lit(p)).collect();
            let mut joins = self.lit_joins.borrow_mut();
            joins.clear();
            for (i, (p, _)) in rows.iter().enumerate() {
                if lit[i] {
                    joins.insert(p.clone(), (i > 0 && lit[i - 1], lit.get(i + 1).copied().unwrap_or(false)));
                }
            }
        }
        let mut pieces = Vec::new();
        self.tree_pieces(&v.tree.children, 0, cx, &mut pieces);
        // The list's box as last painted; before the first paint, more
        // rows than any sidebar holds.
        let view = f32::from(self.side_scroll.bounds().size.height);
        let top = -f32::from(self.side_scroll.offset().y);
        let (from, to) = if view > 0. { (top - TREE_OVER, top + view + TREE_OVER) } else { (0., TREE_FIRST) };
        let mut out = Vec::new();
        let (mut y, mut gap) = (0., 0.);
        for p in &pieces {
            let h = p.height();
            if y + h > from && y < to {
                if gap > 0. {
                    out.push(div().h(px(gap)).flex_shrink_0().into_any_element());
                    gap = 0.;
                }
                out.push(self.render_piece(p, theme, cx));
            } else {
                gap += h;
            }
            y += h;
        }
        if gap > 0. {
            out.push(div().h(px(gap)).flex_shrink_0().into_any_element());
        }
        out
    }

    /// The tree flattened: a row for each entry, an open folder's
    /// children after its row, and a folder's on their way in or out as
    /// one piece, so the fold is drawn as a box wherever it stands.
    fn tree_pieces<'a>(&self, entries: &'a [vault::Entry], depth: usize, cx: &mut Context<Self>, out: &mut Vec<Piece<'a>>) {
        let folded = self.vault.as_ref().map(|v| &v.folded);
        for e in entries {
            out.push(Piece::Row(e, depth));
            if !e.dir {
                continue;
            }
            let open = !folded.is_some_and(|f| f.contains(&e.path));
            let key = fold_key(&e.path);
            match self.motion.presence(&key, open, cx) {
                Some((opening, serial)) => {
                    let mut inside = Vec::new();
                    self.tree_pieces(&e.children, depth + 1, cx, &mut inside);
                    let t = self.motion.progress(&key);
                    out.push(Piece::Fold { folder: e, inside, opening, serial, at: if opening { t } else { 1. - t } });
                }
                None if open => self.tree_pieces(&e.children, depth + 1, cx, out),
                None => {}
            }
        }
    }

    fn render_piece(&self, piece: &Piece, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        match piece {
            Piece::Row(e, depth) => self.render_row(e, *depth, theme, cx),
            // A folder's children under its row, in a box that runs up to
            // their height and back, the content fading with it, keyed on
            // the folder's path so each change plays once and the first
            // sight plays nothing (`Motion::presence`).
            Piece::Fold { folder, inside, opening, serial, .. } => {
                let full = inside.iter().map(Piece::rows).sum::<usize>() as f32 * TREE_ROW_H;
                let opening = *opening;
                let children: Vec<AnyElement> = inside.iter().map(|p| self.render_piece(p, theme, cx)).collect();
                v_flex()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(v_flex().flex_shrink_0().children(children))
                    .with_animation(ElementId::NamedInteger(SharedString::from(fold_key(&folder.path)), *serial as u64), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                        let t = if opening { t } else { 1. - t };
                        d.h(px(full * t)).opacity(t)
                    })
                    .into_any_element()
            }
        }
    }

    /// A row: the chevron (a folder), Catppuccin's icon, the name, on a
    /// plate while it is the open note. Every row and every fold is
    /// `flex_shrink_0`: a scroll box's children shrink to fit it
    /// otherwise, and a tree taller than the sidebar drew its folders
    /// empty and its rows squeezed.
    fn render_row(&self, e: &vault::Entry, depth: usize, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let dark = true;
        let folded = self.vault.as_ref().map(|v| &v.folded);
        let path = e.path.clone();
        let is_open_folder = e.dir && !folded.is_some_and(|f| f.contains(&e.path));
        let chosen = self.row_lit(&e.path);
        // Rows lit one under another are one block, round at its two ends.
        let (joined_up, joined_down) = self.lit_joins.borrow().get(&e.path).copied().unwrap_or((false, false));
        let cut = self.choice.cut.iter().any(|c| c == &e.path);
        let into = if e.dir { e.path.clone() } else { e.path.parent().map(Path::to_path_buf).unwrap_or_default() };
        let wash = theme.primary.opacity(0.22);
        let icon = crate::file_icons::path(&if e.dir { e.name.clone() } else { format!("{}.md", e.name) }, e.dir, is_open_folder, dark);
        let id = ElementId::Name(SharedString::from(format!("entry-{}", e.path.display())));
        let menu_path = path.clone();
            // The row keeps its height and takes the press and the drag;
            // its plate, which is lit, stands a little in from its top and
            // foot, so lit rows breathe apart, but where two chosen rows
            // join into one block.
            let hover_bg = theme.foreground.opacity(0.07);
            let plate = h_flex()
                .size_full()
                .pl(px(8. + 14. * depth as f32))
                .pr(px(8.))
                .gap(px(6.))
                .items_center()
                .rounded(px(7.))
                .when(chosen && joined_up, |d| d.rounded_t(px(0.)))
                .when(chosen && joined_down, |d| d.rounded_b(px(0.)))
                .when(chosen, |d| d.bg(theme.foreground.opacity(0.13)))
                .when(!chosen, |d| d.group_hover("tree-row", move |s| s.bg(hover_bg)))
                .when(self.pressed.as_ref() == Some(&e.path), |d| d.bg(theme.foreground.opacity(0.1)))
                .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().when(e.dir, |d| d.child(Icon::default().path(if is_open_folder { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" }).with_size(px(12.)).text_color(theme.muted_foreground))))
                .child(img(icon).size(px(16.)).flex_shrink_0())
                .child(div().flex_1().min_w_0().truncate().child(e.name.clone()));
            let row = div()
                .id(id)
                .group("tree-row")
                .h(px(TREE_ROW_H))
                .flex_shrink_0()
                .when(!(chosen && joined_up), |d| d.pt(px(ROW_BREATH)))
                .when(!(chosen && joined_down), |d| d.pb(px(ROW_BREATH)))
                .text_size(px(13.))
                .text_color(theme.foreground.opacity(if chosen { 1. } else { 0.88 }))
                .when(cut, |d| d.opacity(0.45))
                .cursor_pointer()
                .child(plate)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener({
                    let path = path.clone();
                    let dir = e.dir;
                    move |this, ev: &ClickEvent, window, cx| {
                        swallow_click(window, cx);
                        this.tree_click(path.clone(), dir, ev.modifiers(), window, cx);
                    }
                }))
                // Dragged, it moves with the rows chosen with it; another
                // let go over it goes into its folder, which is lit.
                .on_drag(self.drag_of(&path), |drag: &TreeDrag, _, _, cx| cx.new(|_| drag.clone()))
                .drag_over::<TreeDrag>(move |style, _, _, _| style.bg(wash))
                .on_drop(cx.listener(move |this, drag: &TreeDrag, window, cx| {
                    cx.stop_propagation();
                    this.tree_drop(drag, into.clone(), window, cx)
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        swallow_click(window, cx);
                        this.show_entry_menu(menu_path.clone(), ev.position, cx);
                    }),
                );
        row.into_any_element()
    }

    /// The sidebar, after emaki's: the brand on the title row, the New
    /// Note row and the New Folder row, the notes card with the tree,
    /// and the foot with the person's picture, name and the settings.
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let waiting = self.update.found.is_some();
        let gear_dot = self.motion.presence("gear-dot", waiting, cx);
        let theme = self.frame_theme(cx);
        let accent = theme.foreground.opacity(0.1);
        let brand = h_flex().gap(px(5.)).items_center().child(img("icon/icon-128.png").size(px(31.)).flex_shrink_0()).child(crate::widgets::wordmark("Onyx", 23.5, theme.primary, cx));
        let header = Self::drag_region(h_flex().h(TITLEBAR_H).flex_shrink_0().pt(px(2.) + FRAME).pr(px(16.)).justify_end().items_center().child(brand), cx);
        let new_row = h_flex()
            .id("side-new-note")
            .h(px(36.))
            .px(px(10.))
            .gap(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(move |s| s.bg(accent.opacity(0.6)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.new_note(&NewNote, window, cx);
            }))
            .child(div().size(px(22.)).rounded_full().bg(theme.primary).flex().items_center().justify_center().child(Icon::default().path("icons/plus.svg").with_size(px(13.)).text_color(theme.primary_foreground)))
            .child(div().flex_1().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).child(t("New Note")))
            .child(kbd_hint("⌘N", &theme));
        let folder_row = h_flex()
            .id("side-new-folder")
            .h(px(32.))
            .px(px(10.))
            .gap(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(move |s| s.bg(accent.opacity(0.6)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.new_folder(&NewFolder, window, cx);
            }))
            .child(div().w(px(22.)).flex().justify_center().child(Icon::default().path("icons/folder-plus.svg").with_size(px(16.)).text_color(theme.muted_foreground)))
            .child(div().flex_1().text_size(px(13.5)).child(t("New Folder")))
            .child(kbd_hint("⌘⇧N", &theme));
        let top = v_flex().px(px(10.)).pt(px(8.)).gap(px(2.)).child(new_row).child(folder_row);
        let name = self.user_name.clone();
        let family = crate::fonts::wordmark_family(cx);
        let footer = h_flex()
            .flex_shrink_0()
            .px(px(14.))
            .py(px(10.))
            .gap(px(10.))
            .items_center()
            .border_t_1()
            .border_color(accent)
            .child(
                self.avatar(28., &theme)
                    .id("side-avatar")
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.85))
                    .managed_tooltip(|window, cx| tip(t("Your picture and name"), window, cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.open_settings(Some("appearance"), window, cx);
                    })),
            )
            .child(
                v_flex().flex_1().min_w_0().child(
                    div()
                        .relative()
                        .text_size(px(16.))
                        .text_color(theme.foreground)
                        .font_family(family)
                        .child(div().truncate().child(name.clone()))
                        .child(div().absolute().top_0().left(px(0.5)).right(px(-0.5)).truncate().child(name)),
                ),
            )
            .child(
                div()
                    .id("settings-gear")
                    .relative()
                    .size(px(26.))
                    .flex_shrink_0()
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(accent))
                    .managed_tooltip(|window, cx| tip(t("Settings (⌘,)"), window, cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.open_settings(None, window, cx);
                    }))
                    .child(Icon::new(IconName::Settings).with_size(px(16.)).text_color(theme.muted_foreground))
                    // A newer version waiting: seen with the settings shut,
                    // as About's dot in the rail is with them open.
                    .children(crate::widgets::waiting_dot(waiting, gear_dot, theme.primary)),
            );
        v_flex()
            .w(self.sidebar_width())
            .h_full()
            .flex_shrink_0()
            .text_color(theme.foreground)
            .child(header)
            .when(self.vault.is_some(), |d| d.child(top))
            .child(self.render_notes_card(&theme, cx))
            .child(footer)
            .into_any_element()
    }

    /// The notes card: the vault's name on its title strip, whose press
    /// opens the vault's list, and the tree under it. In the sidebar and
    /// flown out from the rail alike.
    fn render_notes_card(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let frame = cx.theme().title_bar;
        let dark = cx.theme().mode.is_dark();
        let ground = frame.blend(if dark { cx.theme().background.opacity(0.55) } else { theme.foreground.opacity(0.045) });
        self.scrollers.name(&self.side_scroll);
        if self.side_view == SideView::Assets && self.vault.is_some() {
            let assets = self.render_assets(theme, ground, cx);
            return self.render_side_stack(div().flex().flex_col().child(assets), ground, theme, cx);
        }
        let rows = self.render_tree(theme, cx);
        let empty = self.vault.as_ref().is_some_and(|v| v.tree.children.is_empty());
        let list = div().flex().flex_col().child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        self.register_tree_keys(v_flex().id("side-list"), cx)
                            .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                                swallow_click(window, cx);
                                this.show_vault_room_menu(ev.position, cx);
                            }))
                            // A press in the room around the rows lets the
                            // chosen go and starts a rectangle that chooses
                            // the rows it reaches; a drag let go there goes
                            // to the vault's top.
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                                let y = this.list_y(ev.position);
                                this.band_start(y, ev.modifiers, window, cx);
                            }))
                            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                                if ev.pressed_button == Some(MouseButton::Left) && this.choice.band.is_some() {
                                    let y = this.list_y(ev.position);
                                    this.band_to(y, cx);
                                }
                            }))
                            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.band_end(cx)))
                            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.band_end(cx)))
                            .on_drop(cx.listener(|this, drag: &TreeDrag, window, cx| {
                                if let Some(root) = this.vault_root().map(Path::to_path_buf) {
                                    this.tree_drop(drag, root, window, cx);
                                }
                            }))
                            .size_full()
                            .px(px(8.))
                            .pt(px(LIST_TOP))
                            .pb(px(12.))
                            .overflow_y_scroll()
                            .track_scroll(&self.side_scroll)
                            .children(rows)
                            .when(empty, |d| d.child(div().px(px(10.)).py(px(8.)).text_size(px(12.)).text_color(theme.muted_foreground).child(t("No notes yet. Make one with the button above.")))),
                    )
                    .children(self.render_band(theme))
                    // What is cut says so: the list fades into the
                    // card at an edge with more beyond it.
                    .child(edge_fade(&self.side_scroll, ground, true, px(0.)))
                    // The card's radius less its border.
                    .child(edge_fade(&self.side_scroll, ground, false, px(9.)))
                    .vertical_scrollbar(&self.side_scroll),
            );
        self.render_side_stack(list, ground, theme, cx)
    }

    /// The person's picture on a disc, or their initial on the accent.
    pub(crate) fn avatar(&self, size: f32, theme: &gpui_component::Theme) -> Div {
        let master = PathBuf::from(&self.cfg.app.avatar);
        let sized = avatar_sized(&master, size as u32);
        let path = if sized.is_file() { sized } else { master };
        let disc = div().size(px(size)).rounded_full().flex_shrink_0().overflow_hidden();
        if !self.cfg.app.avatar.is_empty() && path.is_file() {
            return disc.child(img(path).size(px(size)).rounded_full().object_fit(ObjectFit::Cover));
        }
        let initial = self.user_name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "O".into());
        disc.bg(theme.primary).flex().items_center().justify_center().text_size(px(size * 0.46)).font_weight(FontWeight::SEMIBOLD).text_color(theme.primary_foreground).child(initial)
    }

    /// The name the window calls the person: the settings' own, or the
    /// account's first name.
    pub(crate) fn name_of(cfg: &Config) -> String {
        if cfg.app.user_name.trim().is_empty() { crate::sys::user_first_name() } else { cfg.app.user_name.trim().to_string() }
    }

    /// Ask for a picture and keep a copy of it as the person's own. The
    /// copy has a name of its own each time, so the old picture is not
    /// what the image cache answers with.
    pub(crate) fn pick_avatar(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(t("Choose").into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(from) = paths.into_iter().next() else { return };
            this.update(cx, |this, cx| {
                let dir = onyx_core::paths::root().join("avatar");
                let to = dir.join(format!("{}.png", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)));
                if let Err(e) = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| avatar_keep(&from, &to)) {
                    this.say(tf("The picture could not be used: {why}", &[("why", &e)]), cx);
                    return;
                }
                this.set_avatar(to.to_string_lossy().to_string(), cx);
            })
            .ok();
        })
        .detach();
    }

    /// Take that file as the picture, or none; the copy kept before goes.
    pub(crate) fn set_avatar(&mut self, path: String, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.cfg.app.avatar, path);
        if !old.is_empty() && old != self.cfg.app.avatar && PathBuf::from(&old).starts_with(onyx_core::paths::root().join("avatar")) {
            let old = PathBuf::from(old);
            for size in AVATAR_SIZES {
                let _ = std::fs::remove_file(avatar_sized(&old, size));
            }
            let _ = std::fs::remove_file(&old);
        }
        let avatar = self.cfg.app.avatar.clone();
        let _ = Config::edit(move |c| c.app.avatar = avatar);
        cx.notify();
    }

    /// The person's name as the settings' field has it.
    pub(crate) fn set_user_name(&mut self, name: String, cx: &mut Context<Self>) {
        let name = name.trim().to_string();
        if name == self.cfg.app.user_name {
            return;
        }
        self.cfg.app.user_name = name.clone();
        self.user_name = Self::name_of(&self.cfg);
        let _ = Config::edit(move |c| c.app.user_name = name);
        cx.notify();
    }

    // -- the rail ------------------------------------------------------------

    /// The sidebar folded: a column at the window's left with the New
    /// Note button and the notes card's button, and the app's icon and
    /// the person's picture at its foot. It fades as the sidebar comes.
    fn render_rail(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = (1. - self.side_t() * 1.6).max(0.);
        if shown <= 0. {
            return None;
        }
        let theme = self.frame_theme(cx);
        let accent = theme.foreground.opacity(0.1);
        let mid = RAIL_W / 2.;
        let top = RAIL_TOP + (RAIL_ENTRY + RAIL_GAP) * 2. + px(12.);
        let fly = self.rail_fly.is_some_and(|(_, open)| open);
        let entry = |id: &'static str, glyph: AnyElement, words: SharedString, active: bool, i: f32| {
            div()
                .id(id)
                .absolute()
                .left(mid - RAIL_ENTRY / 2.)
                .top(top + (RAIL_ENTRY + RAIL_GAP) * i)
                .size(RAIL_ENTRY)
                .rounded(px(9.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(active, |d| d.bg(accent))
                .hover(move |s| s.bg(accent.opacity(0.6)))
                .managed_tooltip_at(gpui_component::Placement::Right, move |window, cx| tip(words.clone(), window, cx))
                // The press is the entry's, not the rail's to move the
                // window by.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(glyph)
        };
        let plus = div().size(px(24.)).rounded_full().bg(theme.primary).flex().items_center().justify_center().child(Icon::default().path("icons/plus.svg").with_size(px(14.)).text_color(theme.primary_foreground)).into_any_element();
        let notes = Icon::default().path("icons/folder.svg").with_size(px(18.)).text_color(if fly { theme.foreground } else { theme.muted_foreground }).into_any_element();
        let rule = |y: Pixels| div().absolute().left(mid - px(12.)).top(y).w(px(24.)).h(px(1.)).bg(accent);
        let mut rail = Self::drag_region(div(), cx).absolute().top_0().left_0().h_full().w(RAIL_W).opacity(shown);
        if self.vault.is_some() {
            rail = rail
                .child(rule(top - px(7.)))
                .child(entry("rail-new", plus, t("New Note (⌘N)").into(), false, 0.).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.new_note(&NewNote, window, cx);
                })))
                .child(entry("rail-notes", notes, t("Notes").into(), fly, 1.).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.rail_fly_toggle(cx);
                })));
        }
        rail = rail.child(img("icon/icon-128.png").absolute().left(mid - px(16.)).bottom(px(60.)).size(px(32.))).child(
            self.avatar(28., &theme)
                .id("rail-avatar")
                .absolute()
                .left(mid - px(14.))
                .bottom(px(18.))
                .cursor_pointer()
                .hover(|s| s.opacity(0.85))
                .managed_tooltip_at(gpui_component::Placement::Right, |window, cx| tip(t("Your picture and name"), window, cx))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.open_settings(Some("appearance"), window, cx);
                })),
        );
        Some(rail.into_any_element())
    }

    fn rail_fly_toggle(&mut self, cx: &mut Context<Self>) {
        let open = !self.rail_fly.is_some_and(|(_, open)| open);
        self.rail_fly = Some((Instant::now(), open));
        cx.notify();
    }

    fn rail_fly_close(&mut self, cx: &mut Context<Self>) {
        if self.rail_fly.is_some_and(|(_, open)| open) {
            self.rail_fly = Some((Instant::now(), false));
            cx.notify();
        }
    }

    /// The notes card flown out beside the rail, over the page, for a
    /// note picked without the sidebar; a press beside it lets it go.
    fn render_rail_fly(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (at, open) = self.rail_fly?;
        let done = at.elapsed() >= SLIDE;
        if !open && done {
            return None;
        }
        if !done {
            window.request_animation_frame();
        }
        let t = ease_out_quint()((at.elapsed().as_secs_f32() / SLIDE.as_secs_f32()).min(1.));
        let shown = if open { t } else { 1. - t };
        let theme = self.frame_theme(cx);
        let frame = cx.theme().title_bar;
        let dark = cx.theme().mode.is_dark();
        let shadow = vec![BoxShadow { color: gpui::black().opacity(if dark { 0.5 } else { 0.18 }), offset: point(px(0.), px(8.)), blur_radius: px(28.), spread_radius: px(-6.), inset: false }];
        // The card is over the page: the page under it does not scroll,
        // the card's own list does.
        self.scrollers.lift(open);
        let card = self.render_notes_card(&theme, cx);
        self.scrollers.lift(false);
        let scrim = open.then(|| {
            div().id("rail-fly-scrim").absolute().inset_0().occlude().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.rail_fly_close(cx);
                }),
            )
        });
        Some(
            div()
                .absolute()
                .inset_0()
                .children(scrim)
                .child(
                    div()
                        .id("rail-fly")
                        .absolute()
                        .left(RAIL_W + px(4.) - px(12.) * (1. - shown))
                        .top(FLY_TOP)
                        .bottom(FRAME * 2.)
                        .w(self.sidebar_w)
                        .rounded(px(12.))
                        .overflow_hidden()
                        .bg(frame)
                        .border_1()
                        .border_color(theme.foreground.opacity(0.1))
                        .shadow(shadow)
                        .occlude()
                        .opacity(shown)
                        .text_color(theme.foreground)
                        .child(v_flex().size_full().child(card)),
                )
                .into_any_element(),
        )
    }

    // -- the search and the find bar ----------------------------------------

    pub(crate) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_open {
            self.close_search(window, cx)
        } else {
            self.open_search(window, cx)
        }
    }

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.vault.is_none() {
            return;
        }
        self.search_open = true;
        self.search_input.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = false;
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    /// The phrase typed looked for in every note, off the main thread;
    /// a search still running when the next is asked is dropped.
    fn run_search(&mut self, cx: &mut Context<Self>) {
        let q = self.search_input.read(cx).value().to_string();
        let Some(root) = self.vault_root().map(Path::to_path_buf) else { return };
        if q.trim().is_empty() {
            self.search_results = None;
            cx.notify();
            return;
        }
        let task = cx.background_spawn(async move { onyx_core::search::search(&root, &q, SEARCH_LINES) });
        self.search_task = Some(cx.spawn(async move |this, cx| {
            let res = task.await;
            this.update(cx, |this, cx| {
                this.search_results = Some(res);
                cx.notify();
            })
            .ok();
        }));
    }

    /// A note the search found, opened with the phrase in its find bar.
    fn open_found(&mut self, path: PathBuf, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_search(window, cx);
        self.open_note(path, window, cx);
        let query = query.to_string();
        self.find_input.update(cx, |s, cx| s.set_value(query, window, cx));
        self.find_open = true;
        self.run_find(cx);
        cx.notify();
    }

    pub(crate) fn find_in_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find_open && self.find_input.read(cx).focus_handle(cx).is_focused(window) {
            self.close_find(window, cx)
        } else {
            self.open_find(window, cx)
        }
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown.is_none() {
            return;
        }
        self.find_open = true;
        self.find_input.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        self.run_find(cx);
        cx.notify();
    }

    pub(crate) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = false;
        if let Some(editor) = self.shown_editor() {
            editor.update(cx, |e, cx| e.clear_find(cx));
        }
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    fn run_find(&mut self, cx: &mut Context<Self>) {
        let q = self.find_input.read(cx).value().to_string();
        if let Some(editor) = self.shown_editor() {
            editor.update(cx, |e, cx| e.set_find(&q, cx));
        }
        cx.notify();
    }

    pub(crate) fn find_step(&mut self, delta: i64, cx: &mut Context<Self>) {
        if !self.find_open {
            return;
        }
        if let Some(editor) = self.shown_editor() {
            editor.update(cx, |e, cx| e.find_step(delta, cx));
        }
        cx.notify();
    }

    fn render_search(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.scrollers.name_over(&self.search_scroll);
        let theme = cx.theme().clone();
        let focus = self.search_input.read(cx).focus_handle(cx);
        let results = self.search_results.clone();
        let mut snippet_style = window.text_style();
        snippet_style.font_size = px(11.5).into();
        snippet_style.color = theme.muted_foreground;
        let mark = HighlightStyle { color: Some(theme.foreground), background_color: Some(theme.warning.opacity(0.35)), ..Default::default() };
        div()
            .id("search-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(theme.overlay)
            .flex()
            .flex_col()
            .items_center()
            .pt(px(80.))
            .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx)))
            .child(
                v_flex()
                    .id("search-panel")
                    .occlude()
                    .key_context(SEARCH_CONTEXT)
                    .on_action(cx.listener(|this, _: &Escape, window, cx| {
                        cx.stop_propagation();
                        this.close_search(window, cx)
                    }))
                    .on_click(|_, window, cx| swallow_click(window, cx))
                    .w(px(680.))
                    .max_w(relative(0.94))
                    .max_h(px(520.))
                    .rounded(px(18.))
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(float_shadow(&theme))
                    .child(
                        h_flex()
                            .px(px(16.))
                            .h(px(50.))
                            .flex_shrink_0()
                            .gap(px(10.))
                            .items_center()
                            .border_b_1()
                            .border_color(theme.border)
                            .child(Icon::default().path("icons/search.svg").with_size(px(16.)).text_color(theme.muted_foreground))
                            .child(div().id("search-field").track_focus(&focus).flex_1().min_w_0().text_size(px(14.)).child(Input::new(&self.search_input).appearance(false).bordered(false).on_secondary_click(Self::input_menu(&self.search_input, "edit", cx)))),
                    )
                    .child(
                        v_flex().relative().flex_1().min_h_0().pb(px(8.)).vertical_scrollbar(&self.search_scroll).child(v_flex().id("search-results").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.search_scroll).px(px(8.)).pt(px(8.)).map(|d| match results {
                            None => d.child(div().p(px(12.)).text_size(px(12.5)).text_color(theme.muted_foreground).child(t("Type to search every note's name and words."))),
                            Some(res) if res.notes.is_empty() => d.child(div().p(px(12.)).text_size(px(12.5)).text_color(theme.muted_foreground).child(t("No matches."))),
                            Some(res) => d.children(res.notes.into_iter().take(40).enumerate().map(|(i, n)| {
                                let query = res.query.clone();
                                let path = n.path.clone();
                                let where_ = if n.folder.is_empty() { crate::i18n::count(n.hits, "{n} hit", "{n} hits") } else { format!("{} · {}", n.folder, crate::i18n::count(n.hits, "{n} hit", "{n} hits")) };
                                let style = snippet_style.clone();
                                v_flex()
                                    .id(("hit", i))
                                    .px(px(12.))
                                    .py(px(8.))
                                    .gap(px(3.))
                                    .rounded(px(10.))
                                    .cursor_pointer()
                                    .hover(|st| st.bg(theme.list_hover))
                                    .on_mouse_down(MouseButton::Right, cx.listener({
                                        let path = path.clone();
                                        move |this, ev: &MouseDownEvent, window, cx| {
                                            swallow_click(window, cx);
                                            this.show_found_menu(path.clone(), ev.position, cx);
                                        }
                                    }))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        swallow_click(window, cx);
                                        this.open_found(path.clone(), &query, window, cx);
                                    }))
                                    .child(h_flex().gap(px(8.)).child(div().flex_1().min_w_0().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(n.title.clone())).child(div().flex_shrink_0().text_size(px(11.)).text_color(theme.muted_foreground).child(where_)))
                                    .children(n.snippets.into_iter().map(|sn| {
                                        let marks: Vec<(std::ops::Range<usize>, HighlightStyle)> = sn.marks.into_iter().map(|r| (r, mark)).collect();
                                        div().w_full().truncate().child(StyledText::new(sn.text).with_default_highlights(&style, marks))
                                    }))
                            })),
                        })),
                    ),
            )
            .into_any_element()
    }

    /// The find bar over the note: the phrase, how many of it and which
    /// one, and the way to the next and the one before.
    fn render_find_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let focus = self.find_input.read(cx).focus_handle(cx);
        let (n, at) = self.shown_editor().map(|e| {
            let e = e.read(cx);
            (e.find_count(), e.find_at())
        }).unwrap_or((0, 0));
        let empty = self.find_input.read(cx).value().trim().is_empty();
        let count = if empty {
            String::new()
        } else if n == 0 {
            t("no matches").to_string()
        } else {
            tf("{n} of {total}", &[("n", &(at + 1)), ("total", &n)])
        };
        let button = |id: &'static str, icon: &'static str, label: SharedString| {
            let hover = theme.foreground.opacity(0.07);
            div()
                .id(id)
                .size(px(22.))
                .flex_shrink_0()
                .rounded(px(5.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .managed_tooltip(move |window, cx| tip(label.clone(), window, cx))
                .child(Icon::default().path(icon).with_size(px(14.)).text_color(theme.muted_foreground))
        };
        h_flex()
            .w_full()
            .justify_center()
            .px(SHEET_MARGIN)
            .py(px(8.))
            .child(
                h_flex()
                    .id("find-bar")
                    .key_context(FIND_CONTEXT)
                    .on_action(cx.listener(|this, _: &Escape, window, cx| {
                        cx.stop_propagation();
                        this.close_find(window, cx)
                    }))
                    .w(px(sheet_width(self.width_level(), self.page_w)))
                    .max_w_full()
                    .h(px(36.))
                    .px(px(10.))
                    .gap(px(8.))
                    .items_center()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .shadow_sm()
                    .child(Icon::default().path("icons/search.svg").with_size(px(14.)).text_color(theme.muted_foreground))
                    .child(div().id("find-field").track_focus(&focus).flex_1().min_w_0().text_size(px(13.)).child(Input::new(&self.find_input).appearance(false).bordered(false).on_secondary_click(Self::input_menu(&self.find_input, "edit", cx))))
                    .child(div().text_size(px(11.5)).flex_shrink_0().text_color(if n == 0 && !empty { theme.danger } else { theme.muted_foreground }).child(count))
                    .child(button("find-prev", "icons/chevron-up.svg", t("Previous (⇧↩)").into()).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.find_step(-1, cx)
                    })))
                    .child(button("find-next", "icons/chevron-down.svg", t("Next (↩)").into()).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.find_step(1, cx)
                    })))
                    .child(button("find-close", "icons/close.svg", t("Close (Esc)").into()).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.close_find(window, cx)
                    }))),
            )
            .into_any_element()
    }

    // -- the strip --------------------------------------------------------------

    /// The sidebar's button and the search's, drawn once over the window:
    /// beside the traffic lights on the sidebar's title row while the
    /// sidebar is there, and at the rail's head, a size larger, while it
    /// is folded; between the two they slide on a line.
    fn render_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.frame_theme(cx);
        let k = self.side_t();
        let size = STRIP_BTN + (RAIL_ENTRY - STRIP_BTN) * (1. - k);
        let beside = k < 0.5;
        let button = |id: &'static str, icon: &'static str, label: SharedString| {
            let hover = theme.foreground.opacity(0.07);
            div()
                .id(id)
                .size(size)
                .rounded(px(6.) + px(2.) * (1. - k))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .hover(move |s| s.bg(hover))
                .map(|d| if beside { d.managed_tooltip_at(gpui_component::Placement::Right, move |window, cx| tip(label.clone(), window, cx)) } else { d.managed_tooltip(move |window, cx| tip(label.clone(), window, cx)) })
                .child(Icon::default().path(icon).with_size(px(16.) + px(2.) * (1. - k)).text_color(theme.muted_foreground))
        };
        let row_y = px(1.) + FRAME / 2. + (TITLEBAR_H - STRIP_BTN) / 2.;
        let rail_x = RAIL_W / 2. - RAIL_ENTRY / 2.;
        let strip_left = if cfg!(target_os = "macos") { TRAFFIC_W + px(2.) } else { px(10.) };
        let at = |i: f32| {
            let (x, y) = (strip_left + STRIP_BTN * i, row_y);
            let (rx, ry) = (rail_x, RAIL_TOP + (RAIL_ENTRY + RAIL_GAP) * i);
            (rx + (x - rx) * k, ry + (y - ry) * k)
        };
        let ((sx, sy), (qx, qy)) = (at(0.), at(1.));
        div()
            .absolute()
            .top_0()
            .left_0()
            .child(button("strip-sidebar", "icons/panel-left.svg", t("Sidebar (⌘⇧B)").into()).absolute().left(sx).top(sy).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.toggle_sidebar(window, cx)
            })))
            .child(button("strip-search", "icons/search.svg", t("Search (⌘K)").into()).absolute().left(qx).top(qy).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.open_search(window, cx)
            })))
    }

    /// The window's top is the app's to drag by: a press on it moves the
    /// window once the pointer moves with the button down, unless a tab
    /// or a button on the strip took the press first (`press_taken`: a
    /// child's listener runs before its parent's, and a tab's press is
    /// the start of its own drag), and a double click is the title bar's.
    fn drag_region(el: Div, cx: &mut Context<Self>) -> Div {
        el.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, window, _| {
                this.win_move = !std::mem::take(&mut this.press_taken);
                if this.win_move && ev.click_count == 2 {
                    this.win_move = false;
                    window.titlebar_double_click();
                }
            }),
        )
        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, _| {
            if !this.win_move {
                return;
            }
            this.win_move = false;
            if ev.pressed_button == Some(MouseButton::Left) {
                window.start_window_move();
            }
        }))
    }

    /// The line under the strip: it opens and goes over a moment, in the
    /// danger's ink, or quiet when it is news (an icon before it) or says
    /// something is done (a press from the file).
    fn render_notice(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        const NOTICE_H: f32 = 30.;
        let moving = self.motion.presence("notice", self.notice.is_some(), cx);
        let n = self.notice.clone().or_else(|| moving.and_then(|_| self.last_notice.clone()))?;
        let done = n.done.clone();
        let quiet = done.is_some() || n.icon.is_some();
        let line = div()
            .id("notice")
            .h(px(NOTICE_H))
            .flex_shrink_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(7.))
            .px(px(16.))
            .text_size(px(12.5))
            .text_color(if quiet { theme.muted_foreground } else { theme.danger })
            .when_some(n.icon, |d, icon| d.child(Icon::default().path(icon).with_size(px(14.)).text_color(theme.muted_foreground)))
            .when_some(done, |d, file| {
                let hover = theme.foreground;
                d.cursor_pointer().hover(move |s| s.text_color(hover)).on_click(move |_, window, cx| {
                    swallow_click(window, cx);
                    crate::sys::reveal(&file);
                })
            })
            .child(n.words);
        Some(match moving {
            Some((coming, serial)) => line.with_animation(ElementId::NamedInteger("notice".into(), serial as u64), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                let k = if coming { t } else { 1. - t };
                d.h(px(NOTICE_H * k)).opacity(k)
            }).into_any_element(),
            None => line.into_any_element(),
        })
    }

    /// What the editor holds of a note open in a tab, changes and all.
    pub(crate) fn text_of_tab(&self, path: &Path, cx: &App) -> Option<String> {
        self.tabs.iter().find(|t| t.path == path).map(|t| t.editor.read(cx).text())
    }

    /// The vault's page, with every tab left open.
    pub(crate) fn show_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown.is_none() {
            return;
        }
        self.last_shown = self.shown.take();
        self.find_open = false;
        window.focus(&self.focus, cx);
        self.save_ui(window);
        cx.notify();
    }

    /// The bar under the strip, after emaki's path line: at the left the
    /// note's name with its folder before it, from the vault's name, or
    /// the vault's own place while its page shows, each a press away from
    /// the file manager; at the right how the note is shown and how wide.
    fn render_path_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let root = self.vault_root()?.to_path_buf();
        let theme = cx.theme().clone();
        let (icon, parents, name, target) = match &self.shown {
            Some(path) => {
                let folder = path.parent().and_then(|d| d.strip_prefix(&root).ok()).map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
                let vault = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let parents = if folder.is_empty() { format!("{vault}/") } else { format!("{vault}/{folder}/") };
                (if vault::is_note(path) { "icons/file-text.svg" } else { "icons/file.svg" }, parents, Self::tab_title(path), path.clone())
            }
            None => {
                let shown = crate::sys::short_path(&root);
                let (parents, name) = match shown.rfind('/') {
                    Some(at) => (shown[..at + 1].to_string(), shown[at + 1..].to_string()),
                    None => (String::new(), shown),
                };
                ("icons/folder.svg", parents, name, root.clone())
            }
        };
        let hover_bg = theme.muted;
        let chip = h_flex()
            .id("path-chip")
            .max_w_full()
            .min_w_0()
            .h(px(22.))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .font_family(self.look.mono_family.clone())
            .text_size(px(11.))
            .managed_tooltip(|window, cx| tip(t(crate::sys::REVEAL_LABEL), window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, cx.listener({
                let target = target.clone();
                move |this, ev: &MouseDownEvent, window, cx| {
                    swallow_click(window, cx);
                    this.show_path_menu(target.clone(), ev.position, cx);
                }
            }))
            .on_click(cx.listener(move |_, _, window, cx| {
                swallow_click(window, cx);
                crate::sys::reveal(&target);
            }))
            .child(Icon::default().path(icon).with_size(px(12.)).text_color(theme.muted_foreground).flex_shrink_0())
            .child(h_flex().min_w_0().child(div().min_w_0().truncate().text_color(theme.muted_foreground).child(parents)).child(div().flex_shrink_0().font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(name)));
        Some(
            h_flex()
                .w_full()
                .h(PATH_BAR_H)
                .flex_shrink_0()
                // No ground and no line of its own: it stands on the
                // page's tint, which begins under the tabs' row.
                .px(px(PATH_BAR_PAD))
                .justify_center()
                .items_center()
                .relative()
                .child(chip)
                .when(self.shown.as_deref().is_some_and(vault::is_note), |d| d.child(h_flex().absolute().right(px(PATH_BAR_PAD)).top_0().bottom_0().gap(px(2.)).items_center().child(self.path_button("path-export", "icons/file-export.svg", t("Export (⇧⌘E)"), Self::press_export, &theme, cx))))
                // An asset: the system's app for it, where the note has its paper.
                .when(self.shown.as_deref().is_some_and(|p| !vault::is_note(p)), |d| {
                    d.child(
                        h_flex()
                            .absolute()
                            .right(px(PATH_BAR_PAD))
                            .top_0()
                            .bottom_0()
                            .gap(px(2.))
                            .items_center()
                            .children(self.render_zoom(&theme, cx))
                            .child(self.path_button("path-open", "icons/external-link.svg", t("Open with Default App"), Self::press_open_app, &theme, cx)),
                    )
                })
                .into_any_element(),
        )
    }

    /// A quiet square at the path bar's end.
    fn path_button(&self, id: &'static str, icon: &'static str, label: &'static str, press: fn(&mut Self, &mut Window, &mut Context<Self>), theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let hover = theme.muted;
        div()
            .id(id)
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .managed_tooltip(move |window, cx| tip(label, window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                swallow_click(window, cx);
                press(this, window, cx);
            }))
            .child(Icon::default().path(icon).with_size(px(13.)).text_color(theme.muted_foreground))
    }

    fn press_open_app(&mut self, _: &mut Window, _: &mut Context<Self>) {
        if let Some(path) = self.shown.clone() {
            self.open_outside(&path.to_string_lossy());
        }
    }

    fn press_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.shown.clone() {
            self.open_export(path, window, cx);
        }
    }

    /// How the note is shown, three squares in a group at the bar's
    /// left, each in its own color: the pencil in amber, the source in
    /// blue, the book in green.
    /// The ends of the tabs' row: how notes are shown at the left and how
    /// wide at the right, each a square with its choice on it, only while
    /// a note shows, since neither says anything to the vault's page. Both
    /// ends keep their width with or without them, so the tabs keep one
    /// center line, and the squares fade as they come and go.
    fn render_strip_ends(&self, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let on = self.shown.is_some();
        let moving = self.motion.presence("strip-ends", on, cx);
        let fade = |square: AnyElement, id: &'static str| -> AnyElement {
            match moving {
                Some((coming, serial)) => div().child(square).with_animation((id, serial as u64), Animation::new(crate::widgets::FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element(),
                None => square,
            }
        };
        let drawn = on || moving.is_some();
        let modes = h_flex().w(STRIP_CAP).flex_shrink_0().items_center().when(drawn, |d| d.child(fade(self.render_strip_square(StripEnd::Modes, cx), "strip-modes"))).into_any_element();
        let widths = h_flex().w(STRIP_CAP).flex_shrink_0().justify_end().items_center().when(drawn, |d| d.child(fade(self.render_strip_square(StripEnd::Widths, cx), "strip-widths"))).into_any_element();
        (modes, widths)
    }

    /// A group at rest: the group's line with the choice in it and no
    /// plate, which says which of several is chosen and here there is one.
    /// The group that opens is this same line grown.
    fn render_strip_square(&self, end: StripEnd, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let (id, icon, label, tint): (&'static str, &'static str, SharedString, Hsla) = match end {
            StripEnd::Modes => match self.mode {
                Mode::Live => ("strip-modes-square", "icons/pencil-simple.svg", t("Edit (⌃1)").into(), theme.warning),
                Mode::Source => ("strip-modes-square", "icons/file-code.svg", t("Source (⌃2)").into(), theme.info),
                Mode::Reading => ("strip-modes-square", "icons/book-open.svg", t("Read (⌃3)").into(), theme.success),
            },
            StripEnd::Widths => match self.width {
                PageWidth::Narrow => ("strip-widths-square", "icons/width-narrow.svg", t("Narrow (⌥1)").into(), theme.primary),
                PageWidth::Middle => ("strip-widths-square", "icons/width-middle.svg", t("Middle (⌥2)").into(), theme.primary),
                PageWidth::Wide => ("strip-widths-square", "icons/width-wide.svg", t("Wide (⌥3)").into(), theme.primary),
            },
        };
        let hover_bg = theme.muted;
        div()
            .id(id)
            .size(STRIP_CAP)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(STRIP_CAP_R)
            .border_1()
            .border_color(theme.border)
            .cursor_pointer()
            .hover(|s| s.bg(hover_bg))
            .managed_tooltip(move |window, cx| tip(label.clone(), window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                swallow_click(window, cx);
                this.strip_fly = Some((end, Instant::now(), true, this.strip_fly_open()));
                cx.notify();
            }))
            .child(Icon::default().path(icon).with_size(px(14.)).text_color(tint))
            .into_any_element()
    }

    /// How open the group is, from 0 at its square to 1 at its three,
    /// eased on the window's clock.
    fn strip_fly_open(&self) -> f32 {
        let Some((_, at, open, from)) = self.strip_fly else { return 0. };
        let to = if open { 1. } else { 0. };
        from + (to - from) * ease_out_quint()((at.elapsed().as_secs_f32() / SLIDE.as_secs_f32()).min(1.))
    }

    /// It folds back from wherever it stands, so a press while it opens
    /// turns it without a jump.
    fn strip_fly_close(&mut self, cx: &mut Context<Self>) {
        if let Some((end, _, true, _)) = self.strip_fly {
            self.strip_fly = Some((end, Instant::now(), false, self.strip_fly_open()));
            cx.notify();
        }
    }

    /// A group, open in place: its line widens along the row to hold the
    /// three, the chosen square sliding from where it stood to its own
    /// place on a plate that comes as it opens, while the others come out
    /// from behind it. It lies over the tabs and moves nothing, solid, with a line and a small shadow that
    /// comes as it opens, so a tab under it passes behind a clean edge. A
    /// choice folds it, and so does a press anywhere else.
    fn render_strip_fly(&mut self, card_x: Pixels, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (end, at, open, _) = self.strip_fly?;
        let done = at.elapsed() >= SLIDE;
        if !open && done {
            self.strip_fly = None;
            return None;
        }
        if !done {
            window.request_animation_frame();
        }
        let shown = self.strip_fly_open();
        if open {
            self.scrollers.over();
        }
        let theme = cx.theme().clone();
        // Where the choice stands in its group, counted from the end the
        // group opens away from.
        let (group, from_end) = match end {
            StripEnd::Modes => (
                self.render_mode_group(shown, cx),
                match self.mode {
                    Mode::Live => 0.,
                    Mode::Source => 1.,
                    Mode::Reading => 2.,
                },
            ),
            StripEnd::Widths => (self.render_width_group(shown, cx), 2. - self.width.level()),
        };
        let scrim = open.then(|| {
            div().id("strip-fly-scrim").absolute().inset_0().occlude().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.strip_fly_close(cx);
                }),
            )
        });
        let dark = theme.mode.is_dark();
        let lift = vec![BoxShadow { color: gpui::black().opacity(if dark { 0.4 } else { 0.1 } * shown), offset: point(px(0.), px(2.)), blur_radius: px(8.), spread_radius: px(0.), inset: false }];
        let w = STRIP_CAP + (GROUP_W - STRIP_SQUARE) * shown;
        let slide = (STRIP_SQUARE + px(2.)) * from_end * (1. - shown);
        let top = FRAME + (TITLEBAR_H - FRAME - STRIP_CAP) / 2.;
        let right = FRAME + STRIP_END + crate::csd::room(window, cx).1;
        Some(
            div()
                .absolute()
                .inset_0()
                .children(scrim)
                .child(
                    div()
                        .id("strip-fly")
                        .occlude()
                        .absolute()
                        .top(top)
                        .map(|d| if end == StripEnd::Modes { d.left(card_x + STRIP_END) } else { d.right(right) })
                        .w(w)
                        .h(STRIP_CAP)
                        .rounded(STRIP_CAP_R)
                        .bg(theme.background)
                        .border_1()
                        .border_color(theme.border)
                        .shadow(lift)
                        // The clip is the inside of the line, which the
                        // squares do not cross on their way out.
                        .child(div().relative().size_full().overflow_hidden().child(div().absolute().top(STRIP_INSET).w(GROUP_W).flex().map(|d| if end == StripEnd::Modes { d.left(STRIP_INSET - slide).justify_start() } else { d.right(STRIP_INSET - slide).justify_end() }).child(group))),
                )
                .into_any_element(),
        )
    }

    fn render_mode_group(&self, lift: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let modes = self.motion.segmented_tinted(
            "mode",
            vec![("live", "icons/pencil-simple.svg", t("Edit (⌃1)").into(), theme.warning), ("source", "icons/file-code.svg", t("Source (⌃2)").into(), theme.info), ("reading", "icons/book-open.svg", t("Read (⌃3)").into(), theme.success)],
            self.mode.key(),
            lift,
            Rc::new(|this: &mut Self, key, window, cx| this.set_mode(Mode::from_key(key), window, cx)),
            cx,
        );
        div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(modes).into_any_element()
    }

    /// How wide the note runs, Normal or Wide, a group of two at the
    /// bar's right in the accent.
    fn render_width_group(&self, lift: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let widths = self.motion.segmented_tinted(
            "width",
            vec![("narrow", "icons/width-narrow.svg", t("Narrow (⌥1)").into(), theme.primary), ("middle", "icons/width-middle.svg", t("Middle (⌥2)").into(), theme.primary), ("wide", "icons/width-wide.svg", t("Wide (⌥3)").into(), theme.primary)],
            self.width.key(),
            lift,
            Rc::new(|this: &mut Self, key, window, cx| this.set_width(PageWidth::from_key(key), window, cx)),
            cx,
        );
        div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(widths).into_any_element()
    }

    pub(crate) fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        self.strip_fly_close(cx);
        // An asset reads as it is, whatever the mode.
        for tab in self.tabs.iter().filter(|t| t.asset.is_none()) {
            tab.editor.update(cx, |e, cx| e.set_mode(mode, cx));
        }
        let mut ui = UiState::load();
        ui.mode = Some(mode.key().to_string());
        ui.save_later();
        // The keyboard stays where it is: a caret placed stays, and a
        // note left by a click in the margin keeps reading whole.
        let _ = window;
        cx.notify();
    }

    /// How wide every note runs. The app's own, as the mode is: only the
    /// person changes it, and it is kept. The move starts from wherever
    /// the sheet is, so a second press while it moves turns it without a
    /// jump.
    fn set_width(&mut self, width: PageWidth, window: &mut Window, cx: &mut Context<Self>) {
        self.strip_fly_close(cx);
        if width == self.width {
            return;
        }
        self.width_anim = Some((self.width_level(), Instant::now()));
        self.width = width;
        self.save_ui(window);
        cx.notify();
    }

    /// Where the sheet stands from Narrow (0) to Wide (2), eased on the
    /// window's clock.
    fn width_level(&self) -> f32 {
        let to = self.width.level();
        match &self.width_anim {
            Some((from, at)) if at.elapsed() < SLIDE => from + (to - from) * ease_out_quint()(at.elapsed().as_secs_f32() / SLIDE.as_secs_f32()),
            _ => to,
        }
    }

    fn width_moving(&self) -> bool {
        self.shown.is_some() && self.width_anim.as_ref().is_some_and(|(_, at)| at.elapsed() < SLIDE)
    }

    // -- the page ---------------------------------------------------------------

    /// With no vault: the way in. A vault is a folder of notes.
    fn render_landing(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let recent: Vec<PathBuf> = UiState::load().recent_vaults.into_iter().filter(|p| p.is_dir()).collect();
        let rows = recent.into_iter().enumerate().map(|(i, p)| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
            let where_ = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
            h_flex()
                .id(("recent", i))
                .h(px(44.))
                .px(px(14.))
                .gap(px(10.))
                .items_center()
                .cursor_pointer()
                .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                .hover(|s| s.bg(theme.foreground.opacity(0.04)))
                .on_mouse_down(MouseButton::Right, cx.listener({
                    let p = p.clone();
                    move |this, ev: &MouseDownEvent, window, cx| {
                        swallow_click(window, cx);
                        this.show_recent_menu(p.clone(), ev.position, cx);
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.open_vault(p.clone(), window, cx)
                }))
                .child(Icon::default().path("icons/folder.svg").with_size(px(16.)).text_color(theme.muted_foreground))
                .child(v_flex().flex_1().min_w_0().child(div().text_size(px(13.)).child(name)).child(div().text_size(px(11.)).text_color(theme.muted_foreground).truncate().child(where_)))
        });
        let has_recent = !UiState::load().recent_vaults.iter().any(|p| p.is_dir());
        v_flex()
            .w(px(420.))
            .max_w_full()
            .mx_auto()
            .pt(px(80.))
            .px(px(24.))
            .gap(px(22.))
            .items_center()
            .child(crate::widgets::wordmark("Onyx", 34., theme.primary, cx))
            .child(div().text_size(px(13.5)).text_color(theme.muted_foreground).text_center().child(t("Notes live in a folder of your own, a vault, as plain markdown files.")))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(choice_pill("new-vault", "icons/plus.svg", t("New Vault…"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.start_new_vault(window, cx)
                    })))
                    .child(choice_pill("open-vault", "icons/folder-open.svg", t("Open Vault…"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.choose_vault(window, cx)
                    }))),
            )
            .when(!has_recent, |d| {
                d.child(
                    v_flex()
                        .w_full()
                        .gap(px(6.))
                        .child(div().px(px(14.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(t("Recent")))
                        .child(v_flex().w_full().rounded(px(12.)).border_1().border_color(theme.border).bg(if theme.mode.is_dark() { theme.sidebar } else { theme.muted.opacity(0.4) }).overflow_hidden().children(rows)),
                )
            })
            .into_any_element()
    }

    /// A vault with no note open.
    /// The vault's page while no note is open, as emaki's home page is
    /// made and Obsidian's new tab is filled: the drawing, the greeting in
    /// the note's face, and under them what can be done next, each on a
    /// pill with its key at the right.
    fn render_no_note(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let face = self.look.text_family.clone();
        let choice = |id: &'static str, icon: &'static str, label: &str, keys: &str, theme: &gpui_component::Theme| choice_pill(id, icon, label.to_string(), Press::Quiet, false, theme).w(px(240.)).h(px(34.)).px(px(14.)).gap(px(8.)).text_size(px(13.)).child(div().flex_1()).child(kbd_hint(keys, theme));
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .pb(px(48.))
            .gap(px(26.))
            .child(v_flex().items_center().gap(px(12.)).child(self.home_figure(&theme, cx)).child(div().text_size(px(34.)).font_family(face).child(greeting(&self.user_name))))
            .child(
                v_flex()
                    .gap(px(8.))
                    .child(choice("home-new-note", "icons/file-plus.svg", t("New Note"), "⌘N", &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.new_note(&NewNote, window, cx)
                    })))
                    .child(choice("home-new-folder", "icons/folder-plus.svg", t("New Folder"), "⌘⇧N", &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.new_folder(&NewFolder, window, cx)
                    })))
                    .when(!self.closed_tabs.is_empty(), |d| {
                        d.child(choice("home-reopen", "icons/history.svg", t("Reopen Closed Tab"), "⌘⇧T", &theme).on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.reopen_tab(window, cx)
                        })))
                    })
                    .child(choice("home-open-vault", "icons/folder-open.svg", t("Open Vault…"), "⌘⇧O", &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.choose_vault(window, cx)
                    }))),
            )
            .into_any_element()
    }

    /// The drawing over the greeting: the icon's own picture, after
    /// emaki's handscroll. A notebook rests on a tile of onyx, a thin cut
    /// slab seen from above and a little to the left, its thickness at the
    /// front and a vein of the band's teal across its top, and a pen lies
    /// by. All of it is in the icon's own colors whatever the accent or
    /// the appearance, in layers of one color each, since a picture is
    /// painted in one color: the tile's base, its two sides, its top, its
    /// vein, its glint, its shadow on the ground; the notebook's pages,
    /// its cover, its lines, the bright line. The pen is one color, the
    /// ink, so it shows on either ground, with its nib's slit in the
    /// paper's. It answers the pointer as emaki's does: the notebook tilts
    /// and lifts toward it, and the pen comes to the page and writes the
    /// bright line as the pointer crosses, then goes back to lie by it,
    /// the line written whole.
    ///
    /// The notebook is drawn from text made here, its tilt written into
    /// the picture and rounded to half a degree: gpui turns a picture
    /// after it is rasterized, and a line turned that way blurs. Each
    /// tilt is rasterized once and kept by its text's hash (`svg().data`).
    fn home_figure(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> impl IntoElement {
        const W: f32 = 220.;
        const H: f32 = 124.;
        const PEN: f32 = 64.;
        let (wind, lift, near, moving) = {
            let mut fig = self.home_fig.borrow_mut();
            let moving = fig.step();
            let (wind, lift, near) = fig.at();
            (wind, lift, near, moving)
        };
        let c = |hex: u32| -> Hsla { rgb(hex).into() };
        let (deep, stone, top, vein, pages, paper, line, mark) = (c(0x1C1F3A), c(0x262A4C), c(0x3B4170), c(0x5FB8CA), c(0xDAD8CF), c(0xF8F7F2), c(0x474747), c(0xE8A93A));
        // The pen in the drawing's own inks, the lines' gray by day and the
        // pages' warm white by night: the window's foreground is near black
        // and near white, too hard beside the picture's softer colors.
        let pen = if theme.mode.is_dark() { pages } else { line };
        let layer = |path: &'static str, color: Hsla| svg().path(path).absolute().left_0().top_0().w(px(W)).h(px(H)).text_color(color);
        // The notebook in a box with room to turn in, the cover's center
        // the turn's; the pages a step down and right of the cover.
        let tilt = ((-8. + 5. * wind) * 2.).round() / 2.;
        let turn = format!("rotate({tilt:.1} 50 55)");
        let (book_x, book_y) = (60. + 4. * wind, -6. + 6. * lift);
        let book = move |body: String, color: Hsla| svg().data(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 110">{body}</svg>"#).as_bytes()).absolute().left(px(book_x)).top(px(book_y)).w(px(100.)).h(px(110.)).text_color(color);
        // The bright line: whole at rest, and as far as the pointer has
        // come across while the pen is at it.
        let written = 28. * (1. - near) + 28. * ((wind + 1.) / 2.) * near;
        let pages_fill = format!(r#"<rect x="20" y="18" width="68" height="82" rx="6" fill="currentColor" transform="{turn}"/>"#);
        let pages_edge = format!(r#"<rect x="20" y="18" width="68" height="82" rx="6" fill="none" stroke="currentColor" stroke-width="1.5" transform="{turn}"/>"#);
        let cover = format!(r#"<rect x="16" y="14" width="68" height="82" rx="6" fill="currentColor" transform="{turn}"/>"#);
        let lines = format!(r#"<g fill="none" stroke="currentColor" stroke-linecap="round" transform="{turn}"><rect x="16" y="14" width="68" height="82" rx="6" stroke-width="1.5"/><path d="M30 39 H70" stroke-width="3.5"/><path d="M30 71 H70" stroke-width="3.5"/></g>"#);
        let bright = format!(r#"<path d="M30 55 H{:.1}" fill="none" stroke="currentColor" stroke-width="3.5" stroke-linecap="round" transform="{turn}"/>"#, 30. + written.max(0.5));
        // The pen's nib at the bright line's end, on the cover as it is
        // turned; it lies by the tile while the pointer is away, and
        // comes as far as the pointer is near.
        let (sin, cos) = tilt.to_radians().sin_cos();
        let reach = 30. + written - 50.;
        let (nib_x, nib_y) = (book_x + 50. + reach * cos, book_y + 55. + reach * sin);
        let (write_x, write_y) = (nib_x - 7. / 40. * PEN, nib_y - 33. / 40. * PEN);
        let (rest_x, rest_y) = (152. + 3. * wind, 0. + 6. * lift);
        let (pen_x, pen_y) = (rest_x + (write_x - rest_x) * near, rest_y + (write_y - rest_y) * near);
        let pen_part = move |path: &'static str, color: Hsla| svg().path(path).absolute().left(px(pen_x)).top(px(pen_y)).size(px(PEN)).text_color(color);
        let fig = self.home_fig.clone();
        div()
            .id("home-fig")
            .relative()
            .w(px(W))
            .h(px(H))
            .flex_shrink_0()
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                this.home_fig.borrow_mut().ask(Some(e.position));
                cx.notify();
            }))
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if !*over {
                    this.home_fig.borrow_mut().ask(None);
                    cx.notify();
                }
            }))
            // Where it is, for the pointer's place on it; and the next
            // frame, while the spring has not settled.
            .child(
                canvas(move |bounds, _, _| fig.borrow_mut().bounds = bounds, move |_, _, window, _| {
                    if moving {
                        window.request_animation_frame();
                    }
                })
                .absolute()
                .inset_0(),
            )
            .child(layer("icons/home/shadow.svg", theme.foreground.opacity(0.1)))
            .child(layer("icons/home/tile-base.svg", deep))
            .child(layer("icons/home/tile-sides.svg", stone))
            .child(layer("icons/home/tile-top.svg", top))
            .child(layer("icons/home/tile-vein.svg", vein))
            .child(layer("icons/home/tile-glint.svg", gpui::white().opacity(0.28)))
            .child(book(pages_fill, pages))
            .child(book(pages_edge, line))
            .child(book(cover, paper))
            .child(book(lines, line))
            .child(book(bright, mark))
            .child(pen_part("icons/home/pen-edge.svg", theme.background))
            .child(pen_part("icons/home/pen.svg", pen))
            .child(pen_part("icons/home/pen-slit.svg", paper))
    }

    /// The note as a sheet on the page: a column of the card's ground
    /// with a hairline round it, on a tint of the muted shade with
    /// `SHEET_MARGIN` of it showing round the sheet, so the words' room
    /// and the margins read apart. Normal, the sheet is `READABLE_W`
    /// wide, centered; Wide, it runs to the page's edges less the
    /// margin, with more room inside it for the words; between the two
    /// it eases over `SLIDE`, the words wrapping as it goes.
    fn render_note(&mut self, editor: Entity<Editor>, field: Entity<TextareaState>, path: &Path, cx: &mut Context<Self>) -> AnyElement {
        let level = self.width_level();
        let scroll = editor.read(cx).scroll.clone();
        self.scrollers.name(&scroll);
        let theme = cx.theme().clone();
        let title: SharedString = Self::tab_title(path).into();
        let text_px = self.cfg.editor.text_sizes().note;
        let family = self.look.text_family.clone();
        let title_font = Font { family: family.clone(), features: FontFeatures::default(), fallbacks: self.look.text_fallbacks.clone(), weight: FontWeight::BOLD, style: FontStyle::Normal };
        let ground = page_ground(&theme);
        let title_px = px(text_px * onyx_core::rhythm::TITLE);
        let line = (text_px * onyx_core::rhythm::LINE).round();
        // Where the words cannot change neither can the name; the two
        // draw alike, so a change of mode moves nothing.
        let title_el = if editor.read(cx).locked() {
            div().font(title_font).text_size(title_px).line_height(relative(onyx_core::rhythm::TITLE_LINE)).text_color(theme.foreground).child(title).into_any_element()
        } else {
            div()
                .id("note-title")
                .key_context(TITLE_CONTEXT)
                .on_action(cx.listener(|this, _: &Escape, window, cx| {
                    cx.stop_propagation();
                    this.drop_title(window, cx)
                }))
                // Not a press in the margin, which takes the keyboard back.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .child(Textarea::new(&field).appearance(false).bordered(false).flush(true).on_secondary_click(Self::input_menu(&field, "edit", cx)).font(title_font).text_size(title_px).line_height(relative(onyx_core::rhythm::TITLE_LINE)).text_color(theme.foreground).p_0())
                .into_any_element()
        };
        let sheet_w = sheet_width(level, self.page_w);
        let gutter = SHEET_GUTTER.0 + (SHEET_GUTTER.1 - SHEET_GUTTER.0) * (level - 1.).max(0.);
        div()
            .relative()
            .size_full()
            .child(
                // A press in the margin, off the words, takes the keyboard
                // out of the editor: the note reads whole, with no caret.
                div().id("note-scroll").size_full().overflow_y_scroll().track_scroll(&scroll).on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    crate::editor::stamp("margin press");
                    window.focus(&this.focus, cx);
                    cx.notify();
                })).on_mouse_down(MouseButton::Right, cx.listener({
                    // Off the words nothing moves until a choice asks for
                    // the keyboard (`menu::do_edit`).
                    let editor = editor.clone();
                    move |this, ev: &MouseDownEvent, window, cx| {
                        swallow_click(window, cx);
                        this.canvas_menu(editor.clone(), ev.position, MenuAt::default(), cx);
                    }
                })).child(
                    v_flex().min_h_full().p(SHEET_MARGIN).child(
                        v_flex()
                            .flex_1()
                            .w(px(sheet_w))
                            .max_w_full()
                            .mx_auto()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.background)
                            .px(px(gutter))
                            .pt(px(line))
                            .pb(px(line * 4.))
                            .gap(px((line * 0.4).round()))
                            // The note's name over its words, in the words'
                            // face, and a soft line that says it is the name.
                            .child(title_el)
                            .child(div().w_full().h(px(1.)).flex_shrink_0().mb(px((line * 0.25).round())).bg(theme.border.opacity(0.7)))
                            .child(editor),
                    ),
                ),
            )
            .child(edge_fade(&scroll, ground, true, px(0.)))
            .child(edge_fade(&scroll, ground, false, CARD_R))
            .vertical_scrollbar(&scroll)
            .into_any_element()
    }

    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.sync_players(window);
        if self.vault.is_none() {
            self.scrollers.name(&self.page_scroll);
            let theme = cx.theme().clone();
            return div()
                .relative()
                .size_full()
                .child(div().id("page").size_full().overflow_y_scroll().track_scroll(&self.page_scroll).child(page_in("landing", div().child(self.render_landing(cx)))))
                .child(edge_fade(&self.page_scroll, page_ground(&theme), false, CARD_R))
                .vertical_scrollbar(&self.page_scroll)
                .into_any_element();
        }
        let Some((editor, field, path, reads)) = self.shown_tab().map(|t| (t.editor.clone(), t.title.clone(), t.path.clone(), t.asset.as_ref().map(|a| a.reads_as_words()))) else {
            return page_in("no-note", div().size_full().child(self.render_no_note(cx)));
        };
        // An asset that is not words has a page of its own.
        if reads == Some(false) {
            let key = format!("asset-{}", editor.entity_id());
            return page_in(key, div().size_full().child(self.render_asset(&path, window, cx)));
        }
        // By the note and not its path: a rename is not a new page.
        let key = format!("note-{}", editor.entity_id());
        page_in(key, div().size_full().child(self.render_note(editor, field, &path, cx)))
    }

    /// A sheet: a card over a scrim, with a title, what the sheet asks,
    /// and a line of keys under it. Esc or a press off the card lets it go.
    /// The card occludes, so a press in it let go outside does not close it.
    fn render_sheet(&self, id: &'static str, title: SharedString, body: Vec<AnyElement>, hint: SharedString, cancel: fn(&mut Self, &mut Window, &mut Context<Self>), cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        crate::widgets::fade_in(
            id,
            div()
                .id("sheet-scrim")
                .absolute()
                .inset_0()
                .occlude()
                .bg(theme.overlay)
                .flex()
                .flex_col()
                .items_center()
                .pt(px(140.))
                .on_click(cx.listener(move |this, _, window, cx| cancel(this, window, cx)))
                .child(
                    v_flex()
                        .id("sheet-card")
                        .occlude()
                        .key_context(SHEET_CONTEXT)
                        .on_action(cx.listener(move |this, _: &Escape, window, cx| cancel(this, window, cx)))
                        .on_click(|_, window, cx| swallow_click(window, cx))
                        .w(px(420.))
                        .max_w(relative(0.9))
                        .p(px(16.))
                        .gap(px(10.))
                        .rounded(px(18.))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.border)
                        .shadow(float_shadow(&theme))
                        .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(title))
                        .children(body)
                        .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(key_text(&hint))),
                ),
        )
        .into_any_element()
    }

    /// A field on a sheet: the toolkit's input on a plate, keyed to the
    /// window it was made in.
    fn sheet_field(id: &'static str, input: &Entity<InputState>, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let focus = input.read(cx).focus_handle(cx);
        div().id(id).track_focus(&focus).px(px(10.)).h(px(36.)).flex().items_center().rounded(px(8.)).bg(theme.muted).text_size(px(14.)).child(Input::new(input).appearance(false).on_secondary_click(Self::input_menu(input, "edit", cx))).into_any_element()
    }

    /// A rename: the name in a field. ↩ keeps it.
    fn render_rename(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some((_, folder)) = &self.making {
            let theme = cx.theme().clone();
            let title = if *folder { t("New Folder") } else { t("New Note") };
            let body = vec![
                Self::sheet_field("rename-field", &self.rename_input, &theme, cx),
                h_flex()
                    .gap(px(8.))
                    .justify_end()
                    .child(small_pill("make-cancel", "icons/close.svg", t("Cancel"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| this.cancel_rename(window, cx))))
                    .child(small_pill("make-ok", "icons/check.svg", t("Create"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| this.finish_making(window, cx))))
                    .into_any_element(),
            ];
            return Some(self.render_sheet("make-sheet", title.into(), body, t("↩ to create, Esc to cancel").into(), Self::cancel_rename, cx));
        }
        let path = self.rename.as_ref()?;
        let theme = cx.theme().clone();
        let title = if path.is_dir() {
            t("Rename Folder")
        } else if vault::is_note(path) {
            t("Rename Note")
        } else {
            t("Rename File")
        };
        let body = vec![
            Self::sheet_field("rename-field", &self.rename_input, &theme, cx),
            h_flex()
                .gap(px(8.))
                .justify_end()
                .child(small_pill("rename-cancel", "icons/close.svg", t("Cancel"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| this.cancel_rename(window, cx))))
                .child(small_pill("rename-ok", "icons/check.svg", t("Rename"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| this.finish_rename(window, cx))))
                .into_any_element(),
        ];
        Some(self.render_sheet("rename-sheet", title.into(), body, t("↩ to rename, Esc to cancel").into(), Self::cancel_rename, cx))
    }

    /// A new vault: its name in a field, and where it goes as two rows,
    /// iCloud Drive (where the machine has it) and a folder of one's own
    /// with a pill to choose it. ↩ makes it and opens it.
    fn render_new_vault(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sheet = self.new_vault.as_ref()?;
        let theme = cx.theme().clone();
        let cloud = Self::cloud_place();
        let row = |id: &'static str, icon: &'static str, name: SharedString, where_: SharedString, place: Place, on: bool, theme: &gpui_component::Theme, cx: &mut Context<Self>| {
            h_flex()
                .id(id)
                .h(px(48.))
                .px(px(12.))
                .gap(px(10.))
                .items_center()
                .cursor_pointer()
                .when(on, |d| d.bg(theme.primary.opacity(0.1)))
                .when(!on, |d| d.hover(|s| s.bg(theme.foreground.opacity(0.04))))
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    if let Some(sheet) = this.new_vault.as_mut() {
                        sheet.place = place;
                    }
                    window.focus(&this.vault_input.read(cx).focus_handle(cx), cx);
                    cx.notify();
                }))
                .child(Icon::default().path(icon).with_size(px(16.)).text_color(if on { theme.primary } else { theme.muted_foreground }))
                .child(v_flex().flex_1().min_w_0().child(div().text_size(px(13.)).child(name)).child(div().text_size(px(11.)).text_color(theme.muted_foreground).truncate().child(where_)))
        };
        let folder_where: SharedString = crate::sys::short_path(&sheet.folder).into();
        let mut rows: Vec<AnyElement> = Vec::new();
        if cloud.is_some() {
            rows.push(row("place-cloud", "icons/cloud.svg", t("iCloud Drive").into(), format!("{} › Onyx", t("iCloud Drive")).into(), Place::Cloud, sheet.place == Place::Cloud, &theme, cx).into_any_element());
        }
        rows.push(
            row("place-folder", "icons/device-laptop.svg", t("On this computer").into(), folder_where, Place::Folder, sheet.place == Place::Folder, &theme, cx)
                .when(cloud.is_some(), |d| d.border_t_1().border_color(theme.border))
                .child(small_pill("place-choose", "icons/folder-open.svg", t("Choose…"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.pick_vault_folder(window, cx)
                })))
                .into_any_element(),
        );
        let body = vec![
            Self::sheet_field("vault-field", &self.vault_input, &theme, cx),
            v_flex().w_full().rounded(px(12.)).border_1().border_color(theme.border).overflow_hidden().children(rows).into_any_element(),
            h_flex()
                .gap(px(8.))
                .justify_end()
                .child(small_pill("vault-cancel", "icons/close.svg", t("Cancel"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| this.cancel_new_vault(window, cx))))
                .child(small_pill("vault-ok", "icons/check.svg", t("Create"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| this.finish_new_vault(window, cx))))
                .into_any_element(),
        ];
        Some(self.render_sheet("vault-sheet", t("New Vault").into(), body, t("↩ to create, Esc to cancel").into(), Self::cancel_new_vault, cx))
    }

    // -- the actions ------------------------------------------------------------

    pub(crate) fn new_note(&mut self, _: &NewNote, window: &mut Window, cx: &mut Context<Self>) {
        match self.new_note_dir() {
            Some(dir) => self.new_note_in(dir, window, cx),
            None => self.choose_vault(window, cx),
        }
    }

    fn new_folder(&mut self, _: &NewFolder, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dir) = self.new_note_dir() {
            self.new_folder_in(dir, window, cx);
        }
    }

    // -- the settings and updates ------------------------------------------------

    pub fn open_settings(&mut self, section: Option<&'static str>, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = section {
            self.settings_section = s;
        }
        // After this update: opening reads the view this call is inside.
        let weak = cx.entity().downgrade();
        cx.defer(move |cx| crate::settings::open(weak, cx));
        cx.notify();
    }

    fn check_updates_later(&self, cx: &mut Context<Self>) {
        if update::from_store() {
            return;
        }
        let state = update::UpdateState::load();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.);
        if now - state.last_check < 86_400. {
            return;
        }
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| this.check_updates(cx));
        })
        .detach();
    }

    /// Ask for the newest version, off the main thread.
    pub(crate) fn check_updates(&mut self, cx: &mut Context<Self>) {
        self.update.checking = true;
        self.update.said = None;
        cx.notify();
        let task = cx.background_executor().spawn(async { update::latest() });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                this.update.checking = false;
                match found {
                    Ok(latest) => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.);
                        let _ = update::UpdateState { last_check: now, latest: latest.clone() }.save();
                        if update::is_newer(&latest, update::current_version()) {
                            this.update.found = Some(latest);
                        } else {
                            this.update.found = None;
                            this.update.said = Some(t("Up to date.").into());
                        }
                    }
                    Err(e) => this.update.said = Some(tf("The check did not go through: {why}", &[("why", &e)])),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch the newest version and put it in place, then quit: the new
    /// app opens once this one has gone.
    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        let Some(version) = self.update.found.clone() else { return };
        self.update.downloading = Some(0);
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel::<u32>();
        let task = cx.background_executor().spawn(async move {
            update::download(&version, &|done, total| {
                if let Some(total) = total.filter(|t| *t > 0) {
                    let _ = tx.send((done * 100 / total) as u32);
                }
            })
            .and_then(|file| update::install(&file))
        });
        cx.spawn(async move |this, cx| {
            let ticker = async {
                loop {
                    cx.background_executor().timer(Duration::from_millis(200)).await;
                    let p = rx.try_iter().last();
                    if this.update(cx, |this, cx| {
                        if let Some(p) = p {
                            this.update.downloading = Some(p);
                            cx.notify();
                        }
                    }).is_err() {
                        break;
                    }
                }
            };
            let done = futures_lite_select(task, ticker).await;
            let _ = this.update(cx, |this, cx| {
                this.update.downloading = None;
                match done {
                    Ok(()) => {
                        this.save_all(cx);
                        cx.quit()
                    }
                    Err(e) => this.update.said = Some(tf("The update did not go through: {e}", &[("e", &e)])),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// A fade at a scroller's top or foot, there while there is more that way.
/// It comes and goes with the scroll, which draws the view at each step.
/// The sizes the person's picture is drawn at, in points: the sidebar's
/// foot and the rail, and the settings row.
const AVATAR_SIZES: [u32; 2] = [28, 40];

/// Where the copy of the picture made for one of those sizes is kept:
/// beside the picture, `<name>@<size>.png`.
fn avatar_sized(master: &Path, size: u32) -> PathBuf {
    let stem = master.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    master.with_file_name(format!("{stem}@{size}.png"))
}

/// Keep `from` as the person's picture at `to`: turned the way its
/// camera says, cut to the square at its middle, 512 pixels a side, and
/// with it a copy for each size it is drawn at. (After emaki's.)
fn avatar_keep(from: &Path, to: &Path) -> Result<(), String> {
    use image::ImageDecoder as _;
    let err = |e: image::ImageError| e.to_string();
    let mut decoder = image::ImageReader::open(from).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?.into_decoder().map_err(err)?;
    let turn = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = image::DynamicImage::from_decoder(decoder).map_err(err)?;
    picture.apply_orientation(turn);
    let side = picture.width().min(picture.height());
    let square = picture.crop_imm((picture.width() - side) / 2, (picture.height() - side) / 2, side, side);
    let master = if side > 512 { square.resize_exact(512, 512, image::imageops::FilterType::Lanczos3) } else { square };
    master.save(to).map_err(err)?;
    avatar_sizes(to)
}

/// The copies of the picture at the sizes it is drawn at, each twice the
/// points across for a dense screen and scaled down with a filter that
/// averages: handed the whole picture, the GPU samples a few of its
/// pixels for each one it draws, and a photograph came out jagged.
fn avatar_sizes(master: &Path) -> Result<(), String> {
    let picture = image::open(master).map_err(|e| e.to_string())?;
    for size in AVATAR_SIZES {
        let px = size * 2;
        picture.resize_to_fill(px, px, image::imageops::FilterType::Lanczos3).save(avatar_sized(master, size)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The page's ground under the tabs' row: the card's color on a tint of
/// the muted shade, so a sheet's own ground reads as the words' room.
pub(crate) fn page_ground(theme: &gpui_component::Theme) -> Hsla {
    theme.background.blend(theme.muted.opacity(0.3))
}

/// The sheet's width at a level from Narrow (0) to Wide (2) on a page
/// this wide.
fn sheet_width(level: f32, page: f32) -> f32 {
    let wide = (page - 2. * f32::from(SHEET_MARGIN)).max(NARROW_W);
    let middle = ((NARROW_W + wide) / 2.).min(MIDDLE_MOST);
    if level <= 1. {
        NARROW_W + (middle - NARROW_W) * level
    } else {
        middle + (wide - middle) * (level - 1.)
    }
}

/// A name typed in a field that wraps, as a file's name: its lines joined
/// by a space, and no space at its ends.
fn one_line(typed: &str) -> String {
    typed.split(['\n', '\r']).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// How strong an edge's fade stands with this much left to scroll past
/// it: none at the end, whole once `FADE_ROOM` is left, so it comes and
/// goes with the scroll instead of snapping on at the first pixel.
fn fade_strength(room: f32) -> f32 {
    let t = (room / FADE_ROOM).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// A scroller's edge fading into its ground while there is more beyond
/// it. What is under the fade counts as out of sight, as if a hard edge
/// stood there: the zone takes no press and no hover, only the wheel, so
/// a caret cannot be put in it (the editor keeps it out the other ways,
/// `SCROLL_MARGIN`). The zone is as tall as the fade is strong: what
/// reads whole is live.
///
/// Set after the scroller in its parent, and before the scrollbar, which
/// stays on top. It reads the handle as it is placed, since a scroller's
/// length is known only once it is laid out: read in `render` it would
/// be a frame late, and missing from the first frame of a long note.
///
/// `corner` is the radius of the rounded card its foot stands in, none
/// where its ends meet no corner: a square fade would paint the corner
/// out, since the card's clip is square.
pub(crate) fn edge_fade(handle: &ScrollHandle, ground: Hsla, top: bool, corner: Pixels) -> AnyElement {
    let handle = handle.clone();
    canvas(
        move |bounds, window, _| {
            let (at, most) = (-handle.offset().y, handle.max_offset().y);
            let strength = fade_strength(f32::from(if top { at } else { most - at }));
            let tall = FADE_H * strength;
            let y = if top { bounds.top() } else { bounds.bottom() - tall };
            let dead = (strength > 0.).then(|| window.insert_hitbox(Bounds::new(point(bounds.origin.x, y), size(bounds.size.width, tall)), HitboxBehavior::BlockMouseExceptScroll));
            (strength, dead)
        },
        move |bounds, (strength, dead), window, _| {
            let Some(dead) = dead else { return };
            window.set_cursor_style(CursorStyle::Arrow, &dead);
            let (from, to) = if top { (ground.opacity(strength), ground.opacity(0.)) } else { (ground.opacity(0.), ground.opacity(strength)) };
            window.paint_quad(fill(bounds, linear_gradient(180., linear_color_stop(from, 0.), linear_color_stop(to, 1.))).corner_radii(Corners { bottom_right: corner, bottom_left: corner, ..Default::default() }));
        },
    )
    .absolute()
    .left_0()
    .right_0()
    .h(FADE_H)
    .map(|d| if top { d.top_0() } else { d.bottom_0() })
    .into_any_element()
}

/// The first of a task and a loop that never ends: the task's answer.
async fn futures_lite_select<T: 'static>(task: Task<T>, ticker: impl std::future::Future<Output = ()>) -> T {
    use std::future::Future as _;
    use std::pin::pin;
    let mut task = pin!(task);
    let mut ticker = pin!(ticker);
    std::future::poll_fn(move |cx| {
        if let std::task::Poll::Ready(v) = task.as_mut().poll(cx) {
            return std::task::Poll::Ready(v);
        }
        let _ = ticker.as_mut().poll(cx);
        std::task::Poll::Pending
    })
    .await
}

/// The greeting over the vault's page, by the hour and the name, as emaki
/// says it.
fn greeting(name: &str) -> String {
    use chrono::Timelike as _;
    let hour = chrono::Local::now().hour();
    let part = if hour < 5 { t("Still up") } else if hour < 12 { t("Good morning") } else if hour < 18 { t("Good afternoon") } else { t("Good evening") };
    if name.is_empty() { part.to_string() } else { tf("{part}, {name}", &[("part", &part), ("name", &name)]) }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some((file, name)) = self.pending_rename.take() {
            self.rename_entry(&file, &name, window, cx);
        }
        crate::editor::stamp("shell render begins");
        self.scrollers.begin();
        let theme = cx.theme().clone();
        let frame = theme.title_bar;
        if self.ground.get() != Some(frame) {
            self.ground.set(Some(frame));
            crate::sys::set_window_ground(window, frame.to_rgb());
        }
        if self.sidebar_moving() || self.width_moving() {
            window.request_animation_frame();
        }
        let t_side = self.side_t();
        // The sidebar's room in the row, and the lights' place: in gpui's
        // row on the sidebar's title row, a column at the rail's head.
        let side_w = (self.sidebar_width() * t_side).round();
        crate::sys::fold_lights(window, t_side, f32::from(RAIL_W / 2.));
        let side_shown = ((t_side - 0.375) / 0.625).clamp(0., 1.);
        // The tabs, their widths set for this draw; a frame more while
        // any of them moves.
        let items: Vec<TabItem> = self.tabs.iter().map(|t| TabItem { path: t.path.clone(), title: Self::tab_title(&t.path) }).collect();
        let card_x = side_w.max(RAIL_W);
        self.page_w = f32::from(window.viewport_size().width - card_x - FRAME);
        let strip_inner = self.page_w - f32::from(STRIP_END * 2. + crate::csd::room(window, cx).1 + (STRIP_CAP + STRIP_END_GAP) * 2.);
        self.tabs_row.sync(strip_inner - crate::tabs::STRIP_GAP * 2., &items, self.shown.as_deref(), window, cx);
        if self.tabs_row.moving() {
            window.request_animation_frame();
        }
        let root = self.vault_root().map(Path::to_path_buf);
        // No tab, no row: the plus alone on the strip said nothing.
        let center = if items.is_empty() { div().flex_1().min_w_0().into_any_element() } else { self.tabs_row.render(&items, self.shown.as_deref(), root.as_deref(), cx) };
        if self.shown.is_none() {
            self.strip_fly = None;
        }
        let (modes, widths) = self.render_strip_ends(cx);
        let notice = self.render_notice(&theme, cx);
        let path_bar = self.render_path_bar(cx);
        let find_bar = (self.find_open && self.shown.is_some()).then(|| {
            div().w_full().flex_shrink_0().overflow_hidden().child(self.render_find_bar(cx)).with_animation("find-bar-in", Animation::new(crate::widgets::MENU_IN).with_easing(ease_out_quint()), |d, t| if t >= 1. { d } else { d.max_h(px(64. * t)).opacity(t) })
        });
        let page = self.render_page(window, cx);
        // Under the tabs' row the card is the page's tint, one ground for
        // the path bar, the find bar and the page, with no line between
        // them: the sheet's edge is the one line below the tabs. The card
        // is its two grounds and paints none of its own, each rounding its
        // two corners: `overflow_hidden` clips to a square, so nothing in
        // the card rounds a corner for it.
        let card = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .text_color(theme.foreground)
            .overflow_hidden()
            .child(Self::drag_region(h_flex().h(TITLEBAR_H - FRAME).flex_shrink_0().pl(STRIP_END).pr(STRIP_END + crate::csd::room(window, cx).1).items_center().gap(STRIP_END_GAP).rounded_t(CARD_R).bg(theme.background), cx).child(modes).child(center).child(widths))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .rounded_b(CARD_R)
                    .bg(page_ground(&theme))
                    .children(path_bar)
                    .children(notice)
                    .children(find_bar)
                    .child(div().relative().flex_1().min_h_0().child(page)),
            );
        let pick = Rc::new(|this: &mut Self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>| this.menu_pick(what, id, window, cx));
        let close = Rc::new(|this: &mut Self, cx: &mut Context<Self>| {
            this.pressed = None;
            this.menus.close(cx);
            cx.notify();
        });
        // The settings window's are drawn there.
        let ours = |m: &crate::menu::Menu| m.what != "language" && m.what != SETTINGS_EDIT;
        // Over the window nothing under it scrolls (`scroll.rs`).
        if self.menus.open.as_ref().is_some_and(ours) || self.new_vault.is_some() || self.rename.is_some() || self.export.is_some() || self.search_open || self.viewer.is_some() {
            self.scrollers.over();
        }
        let menu = if self.menus.open.as_ref().is_some_and(ours) || self.menus.gone.as_ref().is_some_and(|(m, _)| ours(m)) { self.menus.render(pick, close, cx) } else { None };
        let sheet = self.render_new_vault(cx).or_else(|| self.render_rename(cx)).or_else(|| self.render_export(window, cx));
        let search = self.search_open.then(|| self.render_search(window, cx));
        let viewer = self.render_viewer(window, cx);
        let rail = self.render_rail(cx);
        let fly = self.render_rail_fly(window, cx);
        let strip = self.render_strip(cx);
        let strip_fly = self.render_strip_fly(card_x, window, cx);
        let this = cx.entity().downgrade();
        let ambient = self.ambient.clone();
        crate::editor::stamp("shell render ends");
        div()
            .id("shell")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(frame)
            .on_modifiers_changed(cx.listener(|this, e: &ModifiersChangedEvent, _, cx| this.tabs_row.modifiers_changed(&e.modifiers, cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)))
            .on_action(cx.listener(Self::new_note))
            .on_action(cx.listener(Self::new_folder))
            .on_action(cx.listener(|this, _: &NewVault, window, cx| this.start_new_vault(window, cx)))
            .on_action(cx.listener(|this, _: &OpenVault, window, cx| this.choose_vault(window, cx)))
            .on_action(cx.listener(|this, _: &CloseVault, window, cx| this.close_vault(window, cx)))
            .on_action(cx.listener(|this, _: &ExportPdf, window, cx| {
                if let Some(path) = this.shown.clone() {
                    this.open_export(path, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_shown(Some(true), cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_shown(Some(false), cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, _, cx| this.zoom_shown(None, cx)))
            .on_action(cx.listener(|this, _: &PrintNote, window, cx| {
                if let Some(path) = this.export.as_ref().map(|e| e.path().to_path_buf()).or_else(|| this.shown.clone()) {
                    this.print_note(path, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_nearest(window, cx)))
            .on_action(cx.listener(|this, _: &ReopenTab, window, cx| this.reopen_tab(window, cx)))
            .on_action(cx.listener(|this, _: &Tab1, window, cx| this.go_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &Tab2, window, cx| this.go_tab(2, window, cx)))
            .on_action(cx.listener(|this, _: &Tab3, window, cx| this.go_tab(3, window, cx)))
            .on_action(cx.listener(|this, _: &Tab4, window, cx| this.go_tab(4, window, cx)))
            .on_action(cx.listener(|this, _: &Tab5, window, cx| this.go_tab(5, window, cx)))
            .on_action(cx.listener(|this, _: &Tab6, window, cx| this.go_tab(6, window, cx)))
            .on_action(cx.listener(|this, _: &Tab7, window, cx| this.go_tab(7, window, cx)))
            .on_action(cx.listener(|this, _: &Tab8, window, cx| this.go_tab(8, window, cx)))
            .on_action(cx.listener(|this, _: &Tab9, window, cx| this.go_tab(9, window, cx)))
            .on_action(cx.listener(|this, _: &ShowLive, window, cx| this.set_mode(Mode::Live, window, cx)))
            .on_action(cx.listener(|this, _: &ShowSource, window, cx| this.set_mode(Mode::Source, window, cx)))
            .on_action(cx.listener(|this, _: &ShowReading, window, cx| this.set_mode(Mode::Reading, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleReading, window, cx| {
                let mode = if this.mode == Mode::Reading { Mode::Live } else { Mode::Reading };
                this.set_mode(mode, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowNarrow, window, cx| this.set_width(PageWidth::Narrow, window, cx)))
            .on_action(cx.listener(|this, _: &ShowMiddle, window, cx| this.set_width(PageWidth::Middle, window, cx)))
            .on_action(cx.listener(|this, _: &ShowWide, window, cx| this.set_width(PageWidth::Wide, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleSearch, window, cx| this.toggle_search(window, cx)))
            .on_action(cx.listener(|this, _: &FindInNote, window, cx| this.find_in_note(window, cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| this.find_step(1, cx)))
            .on_action(cx.listener(|this, _: &FindPrev, _, cx| this.find_step(-1, cx)))
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.rename.is_some() {
                    this.cancel_rename(window, cx);
                }
                if this.new_vault.is_some() {
                    this.cancel_new_vault(window, cx);
                }
                if this.search_open {
                    this.close_search(window, cx);
                }
                this.rail_fly_close(cx);
                this.strip_fly_close(cx);
                this.pressed = None;
                this.menus.close(cx);
                cx.notify();
            }))
            // The scroll router, a raw listener in the capture phase,
            // registered at paint, before any scroller hears the wheel; and
            // the clock's step, asked for at the end of the frame.
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, cx| {
                        let router = this.clone();
                        // The sidebar's edge, once held, follows the
                        // pointer wherever it goes, and nothing under it
                        // hears the move.
                        let (mover, lifter) = (this.clone(), this.clone());
                        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                // Under the system's panel nothing follows the pointer.
                                if crate::sys::sheet_over(window) {
                                    cx.stop_propagation();
                                    return;
                                }
                                let held = mover.update(cx, |s, cx| {
                                    if s.side_drag {
                                        s.side_drag_to(e, window, cx);
                                    }
                                    s.side_drag
                                });
                                if held.unwrap_or(false) {
                                    cx.stop_propagation();
                                }
                            }
                        });
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = lifter.update(cx, |s, cx| s.side_drag_end(window, cx));
                            }
                        });
                        window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                let system = crate::sys::sheet_over(window);
                                let taken = router.update(cx, |s, _| s.scrollers.route(e, system)).unwrap_or(false);
                                if taken {
                                    cx.stop_propagation();
                                    let _ = router.update(cx, |_, cx| cx.notify());
                                }
                            }
                        });
                        crate::ambient::after_frame(&ambient, window, cx);
                    },
                )
                .absolute()
                .size_0(),
            )
            // The sidebar's column grows from nothing to its width while
            // the sidebar itself stays put and fades in over the last
            // part of the way; the card's left edge waits at the rail's
            // until the column is past it.
            .child(
                h_flex()
                    .size_full()
                    .items_stretch()
                    .when(t_side > 0., |d| d.child(div().h_full().flex_shrink_0().w(side_w).overflow_hidden().child(div().h_full().w(self.sidebar_width()).opacity(side_shown).child(self.render_sidebar(cx)))))
                    .child(div().flex_1().min_w_0().h_full().pt(FRAME).pb(FRAME).pr(FRAME).pl((RAIL_W - side_w).max(px(0.))).child(card)),
            )
            .children(rail)
            .child(self.render_side_grip(card_x, cx))
            .children(fly)
            .child(strip)
            .children(strip_fly)
            .children(crate::csd::chrome(window, px(1.) + FRAME / 2. + TITLEBAR_H / 2. - px(12.), |window, _| window.remove_window(), cx))
            .children(sheet)
            .children(search)
            .children(menu)
            .children(viewer)
            .child(self.ambient.clone())
    }
}

impl Shell {
    /// For a script: a picture of the window once it is drawn, then quit.
    /// The settings window when it is open, else the main one.
    pub fn shoot_later(&self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let wait = (self.go_ends + 600).max(1100);
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(wait)).await;
            let settings = this.update(cx, |s, _| s.settings).ok().flatten();
            let _ = cx.update(|window, cx| {
                let shot = match settings {
                    Some(h) => h.update(cx, |_, w, _| crate::sys::shoot_window(w, &path)).unwrap_or_else(|e| Err(e.to_string())),
                    None => crate::sys::shoot_window(window, &path),
                };
                if let Err(e) = shot {
                    eprintln!("onyx: ONYX_SHOT: {e}");
                }
                cx.quit();
            });
        })
        .detach();
    }

    /// For a script: steps by name (`ONYX_GO=note;sidebar;source`): the
    /// vault's first note opened, the sidebar folded, a mode, a row's menu,
    /// the tabs' numbers.
    pub fn go(&mut self, steps: &str, window: &mut Window, cx: &mut Context<Self>) {
        // Each press and key lands in its own time, on what the one before
        // it drew, and the picture waits for the last.
        let mut later = 500;
        for step in steps.split(';') {
            match step {
                "note" => {
                    if let Some(root) = self.vault_root().map(Path::to_path_buf) {
                        if let Some(first) = vault::notes(&root).into_iter().next() {
                            self.open_note(first, window, cx);
                        }
                    }
                }
                "sidebar" => self.toggle_sidebar(window, cx),
                "home" => self.show_home(window, cx),
                "narrow" | "middle" | "wide" => self.set_width(PageWidth::from_key(step), window, cx),
                s if s.starts_with("key=") => {
                    if let Ok(keystroke) = Keystroke::parse(&s["key=".len()..]) {
                        // As `press=`: into this window alone.
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = cx.update(|window, cx| {
                                window.dispatch_event(PlatformInput::KeyDown(KeyDownEvent { keystroke, is_held: false, prefer_character_input: false }), cx);
                            });
                        })
                        .detach();
                    }
                }
                s if s.starts_with("title=") => {
                    if let Some(tab) = self.shown_tab() {
                        let (field, name) = (tab.title.clone(), s["title=".len()..].to_string());
                        window.focus(&field.read(cx).focus_handle(cx), cx);
                        field.update(cx, |f, cx| f.set_value(name, window, cx));
                    }
                }
                s if s.starts_with("size=") => {
                    // The window to that size with no pointer in it, as a
                    // tiling key does, in its turn among the presses.
                    let n: Vec<f32> = s["size=".len()..].split(',').filter_map(|n| n.parse().ok()).collect();
                    if n.len() == 2 {
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = cx.update(|window, _| window.resize(size(px(n[0]), px(n[1]))));
                        })
                        .detach();
                    }
                }
                s if s.starts_with("press=") || s.starts_with("rpress=") || s.starts_with("drag=") => {
                    // `press=x,y` or `press=x,y,clicks`; `rpress=x,y`, the
                    // right button; `drag=x,y,to_x,to_y`.
                    // Once the frame it aims at is drawn, into this window
                    // alone: nothing reaches the screen.
                    let dragging = s.starts_with("drag=");
                    let right = s.starts_with("rpress=");
                    let n: Vec<f32> = s.split_once('=').map(|(_, v)| v.split(',').filter_map(|n| n.parse().ok()).collect()).unwrap_or_default();
                    if n.len() >= 2 {
                        let position = point(px(n[0]), px(n[1]));
                        let to = if dragging && n.len() == 4 { point(px(n[2]), px(n[3])) } else { position };
                        let click_count = if !dragging && n.len() == 3 { n[2] as usize } else { 1 };
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = cx.update(|window, cx| {
                                let (button, modifiers) = (if right { MouseButton::Right } else { MouseButton::Left }, Modifiers::default());
                                window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers }), cx);
                                window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button, position, modifiers, click_count, first_mouse: false }), cx);
                                if dragging {
                                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: to, pressed_button: Some(button), modifiers }), cx);
                                }
                                window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button, position: to, modifiers, click_count }), cx);
                            });
                        })
                        .detach();
                    }
                }
                s if s.starts_with("point=") || s.starts_with("wheel=") || s.starts_with("cmdwheel=") || s.starts_with("pinch=") => {
                    // `point=x,y`: the pointer there, nothing pressed;
                    // `wheel=x,y,dy`: a wheel's turn there, a gesture of
                    // its own, `cmdwheel=` the same with ⌘ held; `pinch=x,y,d`
                    // a pinch there by `d` (0.1 is a tenth larger). Into this
                    // window alone.
                    let wheel = s.starts_with("wheel=") || s.starts_with("cmdwheel=");
                    let cmd = s.starts_with("cmdwheel=");
                    let pinch = s.starts_with("pinch=");
                    let n: Vec<f32> = s.split_once('=').map(|(_, v)| v.split(',').filter_map(|n| n.parse().ok()).collect()).unwrap_or_default();
                    if n.len() >= 2 {
                        let position = point(px(n[0]), px(n[1]));
                        let dy = n.get(2).copied().unwrap_or(0.);
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = cx.update(|window, cx| {
                                let modifiers = Modifiers { platform: cmd, ..Default::default() };
                                window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers }), cx);
                                if wheel {
                                    let delta = ScrollDelta::Pixels(point(px(0.), px(dy)));
                                    window.dispatch_event(PlatformInput::ScrollWheel(ScrollWheelEvent { position, delta, modifiers, touch_phase: TouchPhase::Started }), cx);
                                }
                                if pinch {
                                    window.dispatch_event(PlatformInput::Pinch(PinchEvent { position, delta: dy, modifiers, phase: TouchPhase::Moved }), cx);
                                }
                            });
                        })
                        .detach();
                    }
                }
                "where" => {
                    // Where the caret and the selection's other end stand,
                    // line and byte, on stderr, and the note's scroll.
                    let wait = later;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update(cx, |s, cx| {
                            if let Some(e) = s.shown_editor() {
                                let e = e.read(cx);
                                let ((al, ac), (hl, hc)) = e.selection_ends();
                                eprintln!("where {al}:{ac} {hl}:{hc} scroll {}", f32::from(e.scroll.offset().y));
                            }
                        });
                    })
                    .detach();
                }
                "sel" => {
                    // What is selected, on stderr: what ⌘C would copy,
                    // with no clipboard touched.
                    let wait = later;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update(cx, |s, cx| {
                            if let Some(e) = s.shown_editor() {
                                eprintln!("sel {:?}", e.read(cx).copied_text());
                            }
                        });
                    })
                    .detach();
                }
                s if s.starts_with("caret=") => {
                    if let (Some(e), Ok(line)) = (self.shown_editor(), s["caret=".len()..].parse::<usize>()) {
                        e.update(cx, |e, cx| e.move_to_line(line, cx));
                    }
                }
                "assets" => self.set_side_view(SideView::Assets, window, cx),
                // `renamefile=<path>::<name>`: a file renamed as the sheet renames it.
                s if s.starts_with("renamefile=") => {
                    if let Some((path, name)) = s["renamefile=".len()..].split_once("::") {
                        self.rename_entry(Path::new(path), name, window, cx);
                    }
                }
                // What the preview's player has read of its file, on stderr.
                "media" => {
                    let wait = later + 1500;
                    later = wait + GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update(cx, |s, _| s.print_player());
                    })
                    .detach();
                }
                s if s.starts_with("preview=") => {
                    let file = PathBuf::from(&s["preview=".len()..]);
                    let wait = later;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update_in(cx, |s, window, cx| s.open_asset(file, window, cx));
                    })
                    .detach();
                }
                s if s.starts_with("attach=") => {
                    if let Some(e) = self.shown_editor() {
                        let file = PathBuf::from(&s["attach=".len()..]);
                        e.update(cx, |e, cx| e.attach_files(vec![file], cx));
                    }
                }
                s if s.starts_with("scroll=") => {
                    if let (Some(e), Ok(y)) = (self.shown_editor(), s["scroll=".len()..].parse::<f32>()) {
                        let scroll = e.read(cx).scroll.clone();
                        scroll.set_offset(point(px(0.), px(-y)));
                    }
                }
                "search" => {
                    self.open_search(window, cx);
                    self.search_input.update(cx, |s, cx| s.set_value("note", window, cx));
                    self.run_search(cx);
                }
                "find" => {
                    self.open_find(window, cx);
                    self.find_input.update(cx, |s, cx| s.set_value("the", window, cx));
                    self.run_find(cx);
                }
                "vault" => self.start_new_vault(window, cx),
                "export" => {
                    if let Some(path) = self.shown.clone() {
                        self.open_export(path, window, cx);
                    }
                }
                "print" => {
                    if let Some(path) = self.shown.clone() {
                        self.print_note(path, window, cx);
                    }
                }
                s if s.starts_with("cell=") => {
                    // `cell=row,col`: the caret in that cell of the note's first table.
                    let mut at = s["cell=".len()..].split(',').filter_map(|n| n.parse::<usize>().ok());
                    if let (Some(editor), Some(row), Some(col)) = (self.shown_editor(), at.next(), at.next()) {
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = cx.update(|window, cx| editor.update(cx, |e, cx| e.go_cell_probe(row, col, window, cx)));
                        })
                        .detach();
                    }
                }
                s if s.starts_with("hover=") => {
                    // `hover=row,col`: the pointer over that cell of the first table.
                    let mut at = s["hover=".len()..].split(',').filter_map(|n| n.parse::<usize>().ok());
                    if let (Some(editor), Some(row), Some(col)) = (self.shown_editor(), at.next(), at.next()) {
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = editor.update(cx, |e, cx| e.hover_probe(row, col, cx));
                        })
                        .detach();
                    }
                }
                s if s.starts_with("picture=") || s.starts_with("pichover=") => {
                    // `picture=<line>,<what>`: the picture on that line placed
                    // (`left`, `center`, `right`), sized (`auto`, `full`, a
                    // width), or in the viewer (`view`), as its bar does;
                    // `pichover=<line>`: the pointer over it, so its bar shows.
                    let hover = s.starts_with("pichover=");
                    let rest = s.split_once('=').map_or("", |(_, r)| r).to_string();
                    if let Some(editor) = self.shown_editor() {
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let mut parts = rest.split(',');
                            let Some(line) = parts.next().and_then(|n| n.trim().parse().ok()) else { return };
                            let what = parts.next().unwrap_or("").to_string();
                            let _ = editor.update(cx, |e, cx| if hover { e.picture_hover_probe(line, cx) } else { e.picture_choice(line, &what, cx) });
                        })
                        .detach();
                    }
                }
                s if s.starts_with("layout=") => {
                    // `layout=<line>,<byte>,<what>`: the link at that byte of
                    // the line shown as `link`, `small`, `medium`, `large` or
                    // `preview`, as its menu does.
                    let rest = s["layout=".len()..].to_string();
                    if let Some(editor) = self.shown_editor() {
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let mut parts = rest.split(',');
                            let (Some(line), Some(at)) = (parts.next().and_then(|n| n.trim().parse().ok()), parts.next().and_then(|n| n.trim().parse().ok())) else { return };
                            let what = parts.next().unwrap_or("").to_string();
                            let _ = editor.update(cx, |e, cx| e.layout_choice(line, at, &what, cx));
                        })
                        .detach();
                    }
                }
                s if s.starts_with("table=") => {
                    // `table=what,row,col`: a table's menu choice.
                    let mut parts = s["table=".len()..].split(',');
                    if let (Some(editor), Some(what), Some(row), Some(col)) = (self.shown_editor(), parts.next(), parts.next().and_then(|n| n.parse().ok()), parts.next().and_then(|n| n.parse().ok())) {
                        let what = what.to_string();
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = editor.update(cx, |e, cx| e.table_probe(&what, row, col, cx));
                        })
                        .detach();
                    }
                }
                "focus" => {
                    // The keyboard in the note, as a press in its words puts it.
                    if let Some(editor) = self.shown_editor() {
                        let focus = editor.read(cx).focus_handle(cx);
                        window.focus(&focus, cx);
                    }
                }
                "tops" => {
                    // Where each line stands, on stderr, for a probe to measure.
                    let wait = later;
                    later += GO_STEP;
                    if let Some(editor) = self.shown_editor() {
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = editor.update(cx, |e, _| eprintln!("tops {:?}", e.line_tops()));
                        })
                        .detach();
                    }
                }
                "text" => {
                    // What the editor holds, on stderr, for a probe to read.
                    let wait = later;
                    later += GO_STEP;
                    if let Some(editor) = self.shown_editor() {
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = editor.update(cx, |e, _| eprintln!("text<<<{}>>>", e.text()));
                        })
                        .detach();
                    }
                }
                s if s.starts_with("type=") => {
                    if let Some(editor) = self.shown_editor() {
                        let words = s["type=".len()..].to_string();
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = editor.update(cx, |e, cx| e.type_text(&words, cx));
                        })
                        .detach();
                    }
                }
                s if s.starts_with("pdf=") => self.probe_export(&s["pdf=".len()..], cx),
                s if s.starts_with("pdfpage=") => {
                    let page = s["pdfpage=".len()..].parse().unwrap_or(0);
                    let wait = later;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update(cx, |s, cx| s.go_export_page(page, cx));
                    })
                    .detach();
                }
                s if s.starts_with("find=") => {
                    let words = s["find=".len()..].to_string();
                    self.open_find(window, cx);
                    self.find_input.update(cx, |s, cx| s.set_value(words, window, cx));
                    self.run_find(cx);
                }
                s if s.starts_with("pdfto=") => {
                    let to = PathBuf::from(&s["pdfto=".len()..]);
                    let wait = later + 800;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update_in(cx, |s, window, cx| s.export_to(&to, window, cx));
                    })
                    .detach();
                }
                "notes" => {
                    // The vault's first three notes, as tabs.
                    if let Some(root) = self.vault_root().map(Path::to_path_buf) {
                        for note in vault::notes(&root).into_iter().take(3) {
                            self.open_note(note, window, cx);
                        }
                    }
                }
                "close" => self.close_nearest(window, cx),
                // A newer version waits, as a check finding one says: the
                // gear's and About's dots. Nothing is fetched.
                "updatefound" => self.update.found = Some("9.9.9".into()),
                "hints" => self.tabs_row.hints = true,
                "end" => {
                    if let Some(e) = self.shown_editor() {
                        e.update(cx, |e, cx| e.move_to_end(cx));
                    }
                }
                "type" => {
                    if let Some(e) = self.shown_editor() {
                        e.update(cx, |e, cx| e.type_text("Hello, probe. ", cx));
                    }
                }
                "live" => self.set_mode(Mode::Live, window, cx),
                "source" => self.set_mode(Mode::Source, window, cx),
                "reading" => self.set_mode(Mode::Reading, window, cx),
                "menu" => {
                    let first = self.vault.as_ref().and_then(|v| v.tree.children.iter().find(|e| !e.dir).map(|e| e.path.clone()));
                    if let Some(p) = first {
                        self.pressed = Some(p.clone());
                        let entries = self.entry_menu(&p);
                        self.menus.show("entry", point(px(120.), px(200.)), entries);
                    }
                }
                "rename" => {
                    let first = self.vault.as_ref().and_then(|v| v.tree.children.iter().find(|e| !e.dir).map(|e| e.path.clone()));
                    if let Some(p) = first {
                        self.start_rename(p, window, cx);
                    }
                }
                _ => {}
            }
        }
        cx.notify();
        self.go_ends = later;
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(test)]
mod tests {
    // By name: a glob here would bring in gpui's own `test`.
    use super::{FADE_ROOM, NARROW_W, fade_strength, one_line, sheet_width};

    #[test]
    fn narrow_is_narrowest_and_wide_is_the_page() {
        // A window with room to spare, a laptop's whole screen, a big
        // monitor's.
        for page in [900., 1456., 3000.] {
            let (narrow, middle, wide) = (sheet_width(0., page), sheet_width(1., page), sheet_width(2., page));
            assert_eq!(narrow, NARROW_W);
            assert!(middle - narrow > 40. && wide - middle > 40., "{page}: {narrow} {middle} {wide}");
            assert_eq!(wide, page - 32.);
        }
    }

    #[test]
    fn middle_stays_a_line_to_read_on_a_big_monitor() {
        assert_eq!(sheet_width(1., 3000.), super::MIDDLE_MOST);
    }

    #[test]
    fn a_change_of_width_passes_through_every_width_between() {
        let page = 1456.;
        let steps: Vec<f32> = (0..=40).map(|i| sheet_width(i as f32 / 20., page)).collect();
        assert!(steps.windows(2).all(|w| w[1] > w[0] && w[1] - w[0] < 40.));
    }

    #[test]
    fn with_no_room_every_width_is_narrow() {
        for page in [0., 500., 731.] {
            assert_eq!(sheet_width(2., page), NARROW_W);
            assert_eq!(sheet_width(1., page), NARROW_W);
        }
    }

    #[test]
    fn a_typed_name_is_one_line() {
        assert_eq!(one_line("  March trip "), "March trip");
        assert_eq!(one_line("March\ntrip"), "March trip");
        assert_eq!(one_line("March\r\n\n  trip\n"), "March trip");
        assert_eq!(one_line(" \n "), "");
    }

    #[test]
    fn an_edge_fade_grows_with_the_room_left() {
        assert_eq!(fade_strength(0.), 0.);
        assert_eq!(fade_strength(-40.), 0.);
        assert_eq!(fade_strength(FADE_ROOM), 1.);
        assert_eq!(fade_strength(FADE_ROOM * 4.), 1.);
        let steps: Vec<f32> = (0..=56).map(|r| fade_strength(r as f32 * FADE_ROOM / 56.)).collect();
        assert!(steps.windows(2).all(|w| w[1] > w[0]), "it rises at every step");
        assert!(steps.windows(2).all(|w| w[1] - w[0] < 0.05), "no step is a jump");
    }
}
