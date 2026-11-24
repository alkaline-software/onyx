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
use std::collections::{HashMap, HashSet};
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
use crate::editor::{Editor, EditorEvent, Look, MenuAt, MenuIssue, Words};
use crate::source::SourcePane;
use crate::terminal::{TermEvent, TermPanel};
use crate::i18n::{t, tf};
use crate::menu::{Entry, Menus};
use crate::scroll::Scrollers;
use crate::tabs::{Item as TabItem, Tabs};
use crate::ui_state::{Rect, UiState};
use crate::widgets::{choice_pill, float_shadow, hand, kbd_hint, key_text, page_in, small_pill, swallow_click, tip, Fig, Motion, Press, FOLD_ANIM, SLIDE};

actions!(onyx, [ExportPdf, PrintNote, ToggleSidebar, OpenSettings, Escape, NewNote, NewFolder, NewVault, CloseVault, ChooseContainer, ToggleSource, ToggleTerminal, ToggleSearch, FindInNote, FindNext, FindPrev, CloseTab, ReopenTab, ZoomIn, ZoomOut, ZoomReset, Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, Tab9]);

/// The key context of the main window.
pub const KEY_CONTEXT: &str = "Shell";
/// The key context of a sheet over the window.
pub(crate) const SHEET_CONTEXT: &str = "Sheet";

pub(crate) mod library;
pub(crate) mod places;
mod tree;
pub(crate) use tree::{Choice, TreeDrag};

/// The tree's keys, for the window's key map.
pub fn tree_keys(cx: &App) -> Vec<KeyBinding> {
    tree::key_bindings(cx)
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
pub(crate) const TITLE_ROWS: usize = 6;
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
pub const CARD_R: Pixels = px(if cfg!(any(target_os = "macos", target_os = "ios")) { 12. } else { 4. });
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
/// The path's row under the tabs, and how far its path keeps from the
/// card's sides.
const PATH_BAR_H: Pixels = px(30.);
const PATH_BAR_PAD: f32 = 6.;
/// How far the tabs' row keeps its squares from the card's sides: what
/// they keep from its top and foot, so the room round them is even.
const STRIP_END: Pixels = px((48. - 4. - 28.) / 2.);
/// A script's presses and keys land this many milliseconds apart.
const GO_STEP: u64 = 400;
/// A button at an end of the tabs' row, as Emaki's panel buttons are.
const TOGGLE_W: Pixels = px(28.);
/// The least the source pane and the note beside it are drawn.
const SOURCE_LEAST: f32 = 320.;
const NOTE_LEAST: f32 = 420.;
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

/// The sheet's own width is halfway from `READ_W`, a comfortable line of
/// reading, to the page's edges, as far as `SHEET_MOST`, past which a big
/// monitor would stretch it; full width it runs to the page's edges. A
/// page no wider than `READ_W` has room for one width, and both are it.
/// The sheet is full width while a pane stands beside it, and its own
/// width while it is alone on the page: nobody chooses it.
const READ_W: f32 = 700.;
const SHEET_MOST: f32 = 960.;
/// The tinted page shows this much round the sheet, and the words stand
/// `SHEET_GUTTER` in from its edge at every width, so a full sheet in a
/// page with no more room than its own width has the same room for words.
const SHEET_MARGIN: Pixels = px(16.);
/// The source's sheet's corner, and the room over its first line.
const SOURCE_ROUND: f32 = 10.;
const SOURCE_HEAD: f32 = 8.;
/// The gap between the note's sheet and a pane beside it, the source's or
/// the terminal's: half the page's margin, so the three read as one row.
const PANE_GAP: f32 = 8.;
/// The vaults the landing page offers, the last opened first.
const SHEET_GUTTER: f32 = 32.;
/// The least the window is dragged to.
pub const MIN_W: Pixels = px(720.);
/// A burst of the vault's changes, read as one: those that come this long
/// after the first are read with it.
const WATCH_GATHER: Duration = Duration::from_millis(300);

/// The vault shown: its folder, its tree as last read, the folders open
/// in it (the rest are shut), and the watcher that says when to read it
/// again.
struct Vault {
    root: PathBuf,
    tree: vault::Entry,
    unfolded: HashSet<PathBuf>,
    _watcher: Option<vault::Watcher>,
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
    /// The note's source, once the source pane has shown it (`source.rs`):
    /// its own, as the editor is, with its caret, scroll and history.
    source: Option<Entity<SourcePane>>,
}

/// The new-vault sheet while it is up: its name is in `vault_input`.
struct NewVaultSheet;

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
    /// What the system keeps of the window at this frame: the status bar
    /// and the home indicator on an iPhone and an iPad, the keyboard while
    /// it is out. Nothing elsewhere. `room` is the window less these, and
    /// everything is laid out in it.
    safe: Edges<Pixels>,
    room: Size<Pixels>,
    /// A window too narrow for the sidebar beside the note (`MIN_W`: an
    /// iPhone, an iPad's narrow window): the two are pages of the whole
    /// window, and the sidebar's coming and going slides one over to the
    /// other, with no rail.
    compact: bool,
    /// Whether the keyboard is out, which takes the window's foot.
    keys_out: bool,
    /// The swipe from the screen's edge that brings the sidebar back in a
    /// compact window: how far it has come, while a finger holds it.
    side_pull: Option<f32>,
    side_scroll: ScrollHandle,
    /// The list on the sidebar's card before a vault is shown (`places.rs`).
    place_scroll: ScrollHandle,
    /// The page before a vault, when its vaults outgrow it.
    place_page_scroll: ScrollHandle,
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
    /// A folder's icon and color being chosen (`folder_look.rs`).
    folder_look: Option<(Entity<crate::folder_look::Picker>, Subscription)>,
    win_move: bool,
    /// What every editor is set in, and how every note is shown.
    pub(crate) look: Look,
    /// When a script's last press has landed, for its picture.
    go_ends: u64,
    /// The note's source in a pane at the page's left (`source.rs`), for
    /// every note, kept: whether it shows; its coming or going, how far
    /// out it was then, when, and its number; its share of the card; and
    /// whether its edge is held.
    source_on: bool,
    source_anim: Option<(f32, Instant, u32)>,
    source_share: f32,
    source_drag: bool,
    /// The terminal at the page's right (`terminal.rs`), the vault's
    /// shells: made the first time it shows; whether it shows, not kept
    /// past a launch, as its shells are not; its coming or going as the
    /// source's; its width, kept; and whether its edge is held.
    terminal: Option<Entity<TermPanel>>,
    term_on: bool,
    term_anim: Option<(f32, Instant, u32)>,
    term_w: f32,
    term_drag: bool,
    _term_sub: Option<Subscription>,
    /// Whether the terminal was asked for after the source: in a card too
    /// narrow for both, the one asked for last shows.
    term_latest: bool,
    /// The card less the terminal at this frame: the room the source's
    /// share is of, for a drag of its edge to measure by.
    source_room: f32,
    /// The card's place and width, and the note's page beside the source,
    /// figured as each frame is built, so the sheet is never a frame
    /// behind a window being resized.
    card_x: Pixels,
    card_w: f32,
    page_w: f32,
    /// The page's room at the sheet's left and right at this draw: the
    /// margin, or the gap to a pane beside it, eased as the pane slides.
    page_pad: (f32, f32),
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
    /// The file each size of the person's picture is drawn from, by the
    /// picture it was found for: two looks at the disk a draw, else.
    avatar_files: RefCell<HashMap<u32, (String, Option<PathBuf>)>>,
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
    /// The rows chosen, as one list every chosen row's drag shares, made
    /// as the tree is drawn (`TreeDrag`).
    drag_marked: RefCell<Option<Rc<[PathBuf]>>>,
    /// The sidebar card's view, the vault's assets as it lists them, and
    /// an asset shown over the page.
    side_view: SideView,
    /// The last change of view: the view left, and when.
    side_moved: Option<(SideView, Instant)>,
    /// Where the vaults' list rises from: the button, as it was last drawn.
    vaults_at: Rc<std::cell::Cell<Bounds<Pixels>>>,
    /// Where a compact window's foot bar was last drawn, for More's menu to rise from.
    foot_at: Rc<std::cell::Cell<Bounds<Pixels>>>,
    /// The folder's row the pointer is on, which shows its New Note and New Folder.
    hover_row: Option<PathBuf>,
    library: Library,
    /// The field a text field's menu was opened on, for what is chosen.
    pub(crate) menu_focus: Option<FocusHandle>,
    /// A file a bar renamed in place, renamed at the next draw, where
    /// the window is to hand (`Shell::rename_entry`).
    pending_rename: Option<(PathBuf, String)>,
    /// Entries the menu bar note renamed, for everything here that knows
    /// them by their path to follow at the next frame (`moved_elsewhere`).
    pending_moves: Vec<(PathBuf, PathBuf)>,
    /// The tabs let go of as their files left the vault, whether each was
    /// the one showing, and its place in the row: open again there when
    /// ⌘Z brings the file back.
    gone_tabs: Vec<(PathBuf, bool, usize)>,
    /// What a note's menu was opened on (`note_menu.rs`).
    canvas: crate::note_menu::NoteMenu,
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
    /// The settings as a sheet of this window, where there is no second
    /// window (an iPhone, an iPad), and where the keyboard is while it is up.
    pub(crate) settings_in: bool,
    pub(crate) settings_own: FocusHandle,
    pub(crate) settings_section: &'static str,
    /// Settings, Menu Bar: the shortcut being recorded, what the row says
    /// of the last keys pressed, and whether Onyx opens at login, read from
    /// the system as the section shows.
    pub(crate) recording: bool,
    pub(crate) record_said: Option<SharedString>,
    /// Why the system would not put Onyx in Login Items, at the last try:
    /// a mark before the switch, the reason in its tooltip.
    pub(crate) login_said: Option<SharedString>,
    pub(crate) at_login: bool,
    /// The list of the system's faces a Custom tile opens (Settings,
    /// Fonts): its search field, made with the settings window; which
    /// tile it is open for and the row ↑ and ↓ are on; the families once
    /// read, and whether they are being read; each Custom tile's place,
    /// for the list to hang from; the list's scroll and its box.
    pub(crate) font_query: Option<Entity<InputState>>,
    pub(crate) font_list: Option<(crate::fonts::Slot, usize)>,
    pub(crate) font_families: Option<Rc<crate::fonts::Families>>,
    pub(crate) font_families_reading: bool,
    /// The font list's rows for the last tile and search asked, kept: they
    /// were matched against every family of the system at every draw
    /// while the list is open.
    pub(crate) font_rows: RefCell<Option<(crate::fonts::Slot, String, usize, Rc<Vec<String>>)>>,
    pub(crate) font_tiles: Rc<RefCell<HashMap<crate::fonts::Slot, Bounds<Pixels>>>>,
    pub(crate) font_scroll: ScrollHandle,
    pub(crate) font_list_box: Rc<Cell<Bounds<Pixels>>>,
    /// The settings window's own focus, given back when its list closes.
    pub(crate) settings_focus: Option<FocusHandle>,
    pub(crate) settings_scroll: ScrollHandle,
    pub(crate) lang_serial: u32,
    pub(crate) update: UpdateRow,
}

/// What the Updates row says and does. Every state is said on the one
/// button, in its fixed width: nothing is added under the row.
#[derive(Default, Clone)]
pub(crate) struct UpdateRow {
    pub checking: bool,
    pub found: Option<String>,
    /// While a download runs: how far, from 0 to 100.
    pub downloading: Option<u32>,
    /// How the last check or install ended, until the next press.
    pub outcome: Option<Outcome>,
}

/// How a check or an install ended: the button's label, and on a failure
/// the reason, in its tooltip.
#[derive(Clone)]
pub(crate) enum Outcome {
    UpToDate,
    CheckFailed(String),
    InstallFailed(String),
}

/// How much of a compact window its sidebar takes: the rest is the
/// note's page, darkened, a press on which puts the sidebar away.
const DRAWER: f32 = 0.84;
/// How dark the note's page is beside the sidebar's drawer.
const DRAWER_DIM: f32 = 0.38;
/// The bar at a compact window's foot: its height, and the room under
/// and over it.
const FOOT_BAR_H: f32 = 52.;
const FOOT_BAR_GAP: f32 = 10.;
/// A compact window's title row: taller than the Mac's, for a finger.
const HAND_TITLE_H: Pixels = px(54.);

/// Every row of the tree is this tall for a pointer (`tree_row_h`).
pub(crate) const TREE_ROW_H: f32 = 30.;

/// How tall every row of the sidebar's tree is: `TREE_ROW_H`, and 40 in
/// a compact window, where a finger presses it. Asked wherever a row's
/// place is figured, so the sums agree with what is drawn.
pub(crate) fn tree_row_h() -> f32 {
    if crate::widgets::in_hand() { 40. } else { TREE_ROW_H }
}
/// How long after a note's first unsaved change it is written.
const SAVE_AFTER: Duration = Duration::from_secs(2);
/// The room over the tree's first row, under the card's band.
pub(crate) const LIST_TOP: f32 = 6.;
/// The room between a row's plate and its edge, at its top and foot.
pub(crate) const ROW_BREATH: f32 = 1.5;
/// Rows drawn beyond the list's box at either end, so a scroll of a few
/// rows shows no blank before the next frame.
pub(crate) const TREE_OVER: f32 = TREE_ROW_H * 6.;
/// Before the list's box has been painted, this much of the tree is
/// drawn from its top.
pub(crate) const TREE_FIRST: f32 = 4000.;

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
            Piece::Row(..) => tree_row_h(),
            Piece::Fold { at, .. } => self.rows() as f32 * tree_row_h() * at,
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
        // Drawn again either way: the caret blinks only in the window in
        // front (`Editor::active`), and stops or starts here.
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.save_all(cx);
            } else if let Some(watcher) = this.vault.as_ref().and_then(|v| v._watcher.as_ref()) {
                // Back in front: what another app wrote in place meanwhile
                // is found where the watcher cannot hear it (an iPhone, an
                // iPad; `onyx_core::watch`).
                vault::look_again(watcher);
            }
            cx.notify();
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
            "system-mono" => cfg.editor.mono_face = crate::fonts::MONOS[crate::fonts::SYSTEM_MONO].key.into(),
            _ => {}
        }
        let mut shell = Self {
            look: Self::look_of(&cfg.editor),
            source_on: ui.source_open.unwrap_or(false),
            source_anim: None,
            source_share: ui.source_share.unwrap_or(0.5).clamp(0.1, 0.9),
            source_drag: false,
            terminal: None,
            term_on: false,
            term_anim: None,
            term_w: ui.term_w.unwrap_or(crate::terminal::TERM_W).max(crate::terminal::TERM_LEAST),
            term_drag: false,
            _term_sub: None,
            term_latest: false,
            source_room: 0.,
            go_ends: 0,
            card_x: px(0.),
            card_w: 0.,
            page_w: 0.,
            page_pad: (16., 16.),
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
            vaults_at: Rc::default(),
            foot_at: Rc::default(),
            hover_row: None,
            choice: Choice::default(),
            tree_focus: cx.focus_handle(),
            lit_joins: RefCell::new(std::collections::HashMap::new()),
            drag_marked: RefCell::new(None),
            library: Library::default(),
            side_anim: None,
            safe: Edges::default(),
            room: Size::default(),
            compact: false,
            keys_out: false,
            side_pull: None,
            sidebar_w: ui.sidebar_w.map(px).unwrap_or(SIDEBAR_W).clamp(SIDEBAR_MIN, SIDEBAR_MAX),
            side_w_anim: None,
            side_drag: false,
            side_scroll: ScrollHandle::new(),
            place_scroll: ScrollHandle::new(),
            place_page_scroll: ScrollHandle::new(),
            vault: None,
            tabs: Vec::new(),
            shown: None,
            last_shown: None,
            closed_tabs: Vec::new(),
            tabs_row: Tabs::default(),
            press_taken: false,
            folder_look: None,
            win_move: false,
            ground: Cell::new(None),
            home_fig: Rc::default(),
            user_name: String::new(),
            rail_fly: None,
            search_open: false,
            search_input,
            search_scroll: ScrollHandle::new(),
            search_results: None,
            avatar_files: RefCell::default(),
            search_task: None,
            find_open: false,
            find_input,
            name_input: None,
            pressed: None,
            menu_focus: None,
            pending_rename: None,
            pending_moves: Vec::new(),
            gone_tabs: Vec::new(),
            canvas: Default::default(),
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
            settings_section: crate::settings::section_named(ui.settings_section.as_deref().unwrap_or("general")),
            recording: false,
            record_said: None,
            login_said: None,
            at_login: false,
            font_query: None,
            font_list: None,
            font_families: None,
            font_families_reading: false,
            font_rows: RefCell::new(None),
            font_tiles: Rc::default(),
            font_scroll: ScrollHandle::new(),
            font_list_box: Rc::default(),
            settings_focus: None,
            settings_in: false,
            settings_own: cx.focus_handle(),
            settings_scroll: ScrollHandle::new(),
            lang_serial: 0,
            update: UpdateRow::default(),
        };
        shell.user_name = Self::name_of(&shell.cfg);
        // A probe's vault stands over the remembered one.
        let probing = std::env::var_os("ONYX_VAULT").is_some();
        let vault = std::env::var_os("ONYX_VAULT").map(PathBuf::from).or_else(|| ui.vault.clone()).filter(|p| p.is_dir());
        // Before there was a container, the vault open was anywhere:
        // the folder it is in becomes the container, so it opens as before.
        if shell.cfg.app.container.is_empty() && crate::sys::fixed_container().is_none() {
            if let Some(parent) = vault.as_ref().and_then(|v| v.parent()) {
                shell.set_container(parent.to_path_buf());
            }
        }
        // A vault remembered from another container stays shut; a probe's opens.
        let in_folder = |v: &PathBuf| probing || library::container_of(&shell.cfg).is_some_and(|f| v.parent() == Some(f.as_path()));
        if let Some(root) = vault.filter(in_folder) {
            shell.show_vault(root.clone(), window, cx);
            if let Some(v) = shell.vault.as_mut() {
                v.unfolded = ui.unfolded.iter().filter(|p| p.starts_with(&root)).cloned().collect();
            }
            shell.save_ui(window);
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
        shell.read_at_login(cx);
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
        ui.source_open = Some(self.source_on);
        ui.source_share = Some(self.source_share);
        ui.term_w = Some(self.term_w);
        if std::env::var_os("ONYX_VAULT").is_none() {
            ui.vault = self.vault.as_ref().map(|v| v.root.clone());
            ui.note = self.shown.clone();
            ui.tabs = self.tabs.iter().map(|t| t.path.clone()).collect();
            ui.unfolded = self.vault.as_ref().map(|v| v.unfolded.iter().cloned().collect()).unwrap_or_default();
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
    pub(crate) fn look_of(e: &onyx_core::config::EditorConfig) -> Look {
        let sizes = e.text_sizes();
        let (text_family, mono_family) = (crate::fonts::text_family(&e.text_face, &e.text_custom), crate::fonts::mono_family(&e.mono_face, &e.mono_custom));
        let (text_fallbacks, mono_fallbacks) = (Some(crate::fonts::cjk_fallbacks(&text_family, &e.cjk_face, &e.cjk_custom)), Some(crate::fonts::cjk_fallbacks(&mono_family, &e.cjk_face, &e.cjk_custom)));
        let cjk_family = crate::fonts::cjk_text_family(&e.cjk_face, &e.cjk_custom, &text_family);
        let cjk_fallbacks = Some(crate::fonts::cjk_behind(&text_family));
        crate::fonts::set_fields_cjk(cjk_family.clone(), cjk_fallbacks.clone());
        Look { text_family, mono_family, text_px: sizes.note, mono_px: sizes.code, text_fallbacks, mono_fallbacks, cjk_family, cjk_fallbacks }
    }

    /// The settings changed: every editor takes the faces and size.
    pub(crate) fn push_look(&mut self, cx: &mut Context<Self>) {
        self.look = Self::look_of(&self.cfg.editor);
        let look = self.look.clone();
        for tab in &self.tabs {
            tab.editor.update(cx, |ed, cx| ed.set_look(look.clone(), cx));
            if let Some(pane) = &tab.source {
                pane.update(cx, |p, cx| p.set_look(look.clone(), cx));
            }
        }
        if let Some(term) = &self.terminal {
            let cfg = self.cfg.terminal.clone();
            term.update(cx, |t, cx| t.set_look(cfg, look.mono_family.clone(), cx));
        }
        crate::menubar::push_look(look, cx);
    }

    // -- the vault ------------------------------------------------------------

    pub(crate) fn vault_root(&self) -> Option<&Path> {
        self.vault.as_ref().map(|v| v.root.as_path())
    }

    /// Every tab goes, as a vault is shown or closed: each one's pictures
    /// and frames let go of on the GPU too, as a tab closed lets go of its
    /// own (`close_tab`). Dropped with the tabs alone, they stayed in the
    /// window's atlas until the app quit.
    fn let_tabs_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for tab in std::mem::take(&mut self.tabs) {
            tab.editor.update(cx, |e, cx| e.let_pictures_go(window, cx));
            Self::let_asset_go(tab.asset, window, cx);
        }
    }

    fn show_vault(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.save_all(cx);
        self.let_tabs_go(window, cx);
        self.shown = None;
        self.closed_tabs.clear();
        self.forget_files(cx);
        let (watcher, changes) = self.watch_vault(&root, cx);
        let tree = vault::tree(&root);
        library::forget_vaults();
        crate::folder_look::ensure(&root, cx);
        // Its folders come shut (at launch, `Shell::new` opens again the ones left open).
        self.vault = Some(Vault { root: root.clone(), tree, unfolded: HashSet::new(), _watcher: watcher });
        let mut ui = UiState::load();
        ui.remember_vault(&root);
        ui.save_later();
        self.watch_changes(changes, cx);
        self.library = Library::default();
        self.reread_assets(cx);
        self.save_ui(window);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// A watcher on the vault's folder, and where it says what changed.
    fn watch_vault(&mut self, root: &Path, cx: &mut Context<Self>) -> (Option<vault::Watcher>, futures::channel::mpsc::UnboundedReceiver<Vec<PathBuf>>) {
        let (tx, rx) = futures::channel::mpsc::unbounded::<Vec<PathBuf>>();
        let watcher = match vault::watch(root, move |paths| {
            let _ = tx.unbounded_send(paths);
        }) {
            Ok(w) => Some(w),
            Err(e) => {
                self.say(tf("The vault is not being watched: {why}", &[("why", &e)]), cx);
                None
            }
        };
        (watcher, rx)
    }

    /// The vault's folder takes a new name. The open notes are written
    /// first; then the vault is the folder at its new place, watched
    /// there, and everything that knew a path in it follows, as for a
    /// folder renamed in it (`follow_moves`), the recent vaults too.
    fn rename_vault(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(old) = self.vault_root().map(Path::to_path_buf) else { return };
        self.save_all(cx);
        let new = match vault::rename(&old, name) {
            Ok(new) => new,
            Err(e) => {
                self.say(tf("The name was not changed: {why}", &[("why", &e)]), cx);
                return;
            }
        };
        if new == old {
            return;
        }
        library::forget_vaults();
        let (watcher, changes) = self.watch_vault(&new, cx);
        if let Some(v) = self.vault.as_mut() {
            v.root = new.clone();
            v.tree = vault::tree(&new);
            v._watcher = watcher;
        }
        self.watch_changes(changes, cx);
        let mut ui = UiState::load();
        for p in ui.recent_vaults.iter_mut().chain(ui.unfolded.iter_mut()) {
            *p = vault::moved_path(p, &[(old.clone(), new.clone())]);
        }
        ui.save_later();
        self.follow_moves(&[(old, new)], window, cx);
        self.reread_assets(cx);
    }

    /// The vault's changes, waited for: the first of a burst wakes the
    /// task, the rest of it within `WATCH_GATHER` come with it, and they
    /// are one reading. Nothing wakes while nothing changes, and the task
    /// ends with its watcher (the vault closed, renamed or another shown).
    fn watch_changes(&mut self, mut rx: futures::channel::mpsc::UnboundedReceiver<Vec<PathBuf>>, cx: &mut Context<Self>) {
        use futures::StreamExt as _;
        cx.spawn(async move |this, cx| {
            while let Some(first) = rx.next().await {
                cx.background_executor().timer(WATCH_GATHER).await;
                let mut paths = first;
                while let Ok(p) = rx.try_recv() {
                    paths.extend(p);
                }
                if this.update(cx, |s, cx| s.disk_changed(&paths, cx)).is_err() {
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
        // The open notes the change touched, read again with the tree. A
        // note's own words changed, its own save among them, change no
        // tree: then the tree is not walked again.
        let touched: Vec<PathBuf> = self.tabs.iter().map(|t| t.path.clone()).filter(|t| paths.iter().any(|p| p == t)).collect();
        let only_words = self.vault.as_ref().is_some_and(|v| paths.iter().filter(|p| Self::change_matters(&root, p)).all(|p| p.is_file() && v.tree.holds(p)));
        if only_words && touched.is_empty() {
            return;
        }
        let task = cx.background_executor().spawn(async move {
            let tree = (!only_words).then(|| vault::tree(&root));
            let texts: Vec<(PathBuf, Option<String>)> = touched.into_iter().map(|p| (p.clone(), vault::read(&p).ok())).collect();
            (tree, texts)
        });
        cx.spawn(async move |this, cx| {
            let (tree, texts) = task.await;
            let _ = this.update(cx, |s, cx| {
                if let (Some(v), Some(tree)) = (s.vault.as_mut(), tree) {
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
            crate::menubar::follow(path, text, cx);
            return;
        }
        let mine = tab.editor.read(cx).text();
        let (merged, _) = onyx_core::merge::merge(&base, &mine, &text);
        // One edit, so the caret and the undo stay.
        if let Some(edit) = crate::editor::one_edit(&mine, &merged) {
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

    /// A folder's icon and color, chosen on a sheet of their own.
    fn open_folder_look(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault_root().map(Path::to_path_buf) else { return };
        self.open_look(root, folder, window, cx);
    }

    /// The sheet for a folder of `root`, or for the vault itself when the
    /// two are one: its mark's icon and color.
    fn open_look(&mut self, root: PathBuf, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let picker = cx.new(|cx| crate::folder_look::Picker::new(root, folder, 90., 0., window, cx));
        let sub = cx.subscribe_in(&picker, window, |this, _, _: &crate::folder_look::Dismissed, window, cx| {
            this.folder_look = None;
            this.focus_editor_or_shell(window, cx);
            cx.notify();
        });
        self.folder_look = Some((picker, sub));
        cx.notify();
    }

    fn reread_tree(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = self.vault.as_mut() {
            v.tree = vault::tree(&v.root);
            let root = v.root.clone();
            crate::folder_look::ensure(&root, cx);
        }
        cx.notify();
    }

    /// Ask the system for the container, in its own folder panel: a
    /// folder there already, its folders each a vault as they are, or a
    /// new one made with the panel's New Folder. There is no sheet of
    /// Onyx's own for it: the panel goes through every folder.
    pub(crate) fn choose_container(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menus.close(cx);
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(t("Choose").into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = cx.update(|window, cx| {
                let _ = this.update(cx, |s, cx| {
                    s.use_container(dir.clone(), window, cx);
                });
            });
        })
        .detach();
    }

    /// The container kept in the settings.
    fn set_container(&mut self, dir: PathBuf) {
        let path = dir.to_string_lossy().to_string();
        self.cfg.app.container = path.clone();
        let _ = Config::edit(move |c| c.app.container = path);
        library::forget_vaults();
    }

    /// A container taken from the system's panel (the page, Settings,
    /// File): a vault shown from another closes.
    pub(crate) fn use_container(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.menus.close(cx);
        self.set_container(dir.clone());
        if self.vault_root().is_some_and(|v| v.parent() != Some(dir.as_path())) {
            self.close_vault(window, cx);
        }
        cx.notify();
    }

    /// The container let go, from its row in Settings: the vault shown
    /// closes and the window is as at its first launch, the page offering
    /// Choose Container…. The vaults stay where they are on the disk.
    pub(crate) fn close_container(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.vault.is_some() {
            self.close_vault(window, cx);
        }
        self.cfg.app.container.clear();
        let _ = Config::edit(|c| c.app.container.clear());
        library::forget_vaults();
        cx.notify();
    }

    /// Close Vault: back to the page before a vault, the vaults on the card.
    fn close_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menus.close(cx);
        self.save_all(cx);
        self.library = Library::default();
        self.let_tabs_go(window, cx);
        self.shown = None;
        self.closed_tabs.clear();
        self.forget_files(cx);
        self.vault = None;
        library::forget_vaults();
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

    /// The new-vault sheet, its name alone: the vault goes in the
    /// container. With no container, the container is asked for first.
    fn start_new_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.container().is_none() {
            return self.choose_container(window, cx);
        }
        self.menus.close(cx);
        self.new_vault = Some(NewVaultSheet);
        self.vault_input.update(cx, |i, cx| {
            i.set_value(t("Notes"), window, cx);
            i.select_all(window, cx);
        });
        window.focus(&self.vault_input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    fn finish_new_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.new_vault.take().is_none() {
            return;
        }
        let name = self.vault_input.read(cx).value().to_string();
        let Some(parent) = self.container() else { return };
        library::forget_vaults();
        match vault::new_vault(&parent, &name) {
            Ok(root) => self.show_vault(root, window, cx),
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
        self.shown_index().map(|i| &self.tabs[i])
    }

    fn shown_index(&self) -> Option<usize> {
        let shown = self.shown.as_ref()?;
        self.tabs.iter().position(|t| t.path == *shown)
    }

    pub(crate) fn shown_editor(&self) -> Option<Entity<Editor>> {
        self.shown_tab().map(|t| t.editor.clone())
    }

    /// An editor for a note, set as every editor is, and listened to.
    fn make_editor(&self, path: &Path, text: &str, cx: &mut Context<Self>) -> (Entity<Editor>, Subscription) {
        let editor = cx.new(Editor::new);
        let look = self.look.clone();
        let root = self.vault_root().map(Path::to_path_buf);
        let (writing, words) = (self.writing_language(), self.words.clone());
        editor.update(cx, |e, cx| {
            e.set_look(look, cx);
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
            // A press in a note's words is work in it: it is the file chosen.
            EditorEvent::Pressed => this.choose(&path, cx),
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
            // Its Chinese, Japanese and Korean in the note's CJK face, as
            // the words under it.
            field.set_split_runs(Some(Rc::new(crate::fonts::split_for_fields)));
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
                self.tabs.push(Tab { path: path.clone(), editor, title, dirty: false, saved: text, save_due: None, _sub: sub, _title_sub: title_sub, asset: None, source: None });
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
        if crate::sys::one_file() {
            self.keep_one(path, window, cx);
        }
        // The tab showing is the file chosen, however it came to show: its
        // row is lit, and the files' keys are for it.
        self.choose(path, cx);
        // The note comes up with no caret: the keyboard is the window's
        // until a press in the words asks for it.
        window.focus(&self.focus, cx);
        self.page_forward(window, cx);
        self.save_ui(window);
        cx.notify();
    }

    /// On an iPhone one file is open at a time: the one shown is the one
    /// kept, and every other is let go as a closed tab is, written first
    /// where it has changes, but not kept to be opened again: it was left,
    /// not closed.
    fn keep_one(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        for ix in (0..self.tabs.len()).rev() {
            if self.tabs[ix].path == path {
                continue;
            }
            self.keep_title_at(ix, window, cx);
            if self.tabs.get(ix).is_none_or(|t| t.path == path) {
                continue;
            }
            self.save_tab(ix, cx);
            self.tabs[ix].editor.update(cx, |e, cx| e.let_pictures_go(window, cx));
            let gone = self.tabs.remove(ix);
            Self::let_asset_go(gone.asset, window, cx);
        }
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
                // The last tab gone, the panes beside it go with it: the
                // vault's page stands alone, as it opens. The note stays
                // the file chosen, its row lit, so the files' keys still
                // act on it.
                None => {
                    self.choose(path, cx);
                    window.focus(&self.focus, cx);
                    self.put_panes_away(window, cx);
                }
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
        self.mark_changed(path, cx);
        // The menu bar note, when it is this one, follows as it is typed.
        if crate::menubar::holds(path, cx) {
            if let Some(text) = self.text_of_tab(path, cx) {
                crate::menubar::follow(path, text, cx);
            }
        }
    }

    /// A tab's note has changes: it is written `SAVE_AFTER` on.
    fn mark_changed(&mut self, path: &Path, cx: &mut Context<Self>) {
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
                tab.saved = text.clone();
                let path = tab.path.clone();
                crate::menubar::written(&path, text, cx);
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

    // -- the menu bar note (`menubar.rs`) ---------------------------------------

    /// Whether a note is open here in a tab, which then writes it.
    pub(crate) fn has_note(&self, path: &Path) -> bool {
        self.tabs.iter().any(|t| t.path == path && t.asset.is_none())
    }

    /// Typed in the menu bar note: the tab takes it, and writes it on its
    /// own clock, as if typed here.
    pub(crate) fn follow_from_menu(&mut self, path: &Path, text: &str, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|t| t.path == path && t.asset.is_none()) else { return };
        tab.editor.update(cx, |e, cx| e.follow_text(text, cx));
        self.mark_changed(path, cx);
    }

    /// A tab's note written now, when it has changes: as the menu bar note
    /// closes over it.
    pub(crate) fn save_note_now(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(ix) = self.tabs.iter().position(|t| t.path == path && t.asset.is_none()) {
            self.save_tab(ix, cx);
        }
    }

    /// A file renamed from a bar in the menu bar note: as one here is, its
    /// links followed through the vault, at the next drawing.
    pub(crate) fn ask_rename(&mut self, file: PathBuf, name: String, cx: &mut Context<Self>) {
        self.pending_rename = Some((file, name));
        cx.notify();
    }

    /// The menu bar note renamed a note from its name: its tab here, and
    /// all else that knows it by its path, follow at the next frame.
    pub(crate) fn moved_elsewhere(&mut self, moves: &[(PathBuf, PathBuf)], cx: &mut Context<Self>) {
        self.pending_moves.extend_from_slice(moves);
        cx.notify();
    }

    /// Open in Onyx, from the menu bar note: the note's vault open here
    /// (the one open when it holds the note), the note in a tab, and a link
    /// of it followed when one was pressed.
    pub(crate) fn show_from_menu(&mut self, path: PathBuf, link: Option<crate::menubar::Link>, window: &mut Window, cx: &mut Context<Self>) {
        let inside = self.vault_root().is_some_and(|r| path.starts_with(r));
        if !inside {
            if let Some(root) = crate::menubar::vault_of(&path) {
                self.show_vault(root, window, cx);
            }
        }
        self.open_note(path, window, cx);
        match link {
            Some(crate::menubar::Link::Path(url)) => self.open_link(&url, cx),
            Some(crate::menubar::Link::Note(name)) => self.open_wikilink(&name, cx),
            Some(crate::menubar::Link::File(what, file)) => self.asset_pick(&what, file, window, cx),
            Some(crate::menubar::Link::Export) => {
                if let Some(p) = self.shown.clone() {
                    self.open_export(p, window, cx);
                }
            }
            Some(crate::menubar::Link::Print) => {
                if let Some(p) = self.shown.clone() {
                    self.print_note(p, window, cx);
                }
            }
            None => {}
        }
        window.activate_window();
        cx.notify();
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
                        v.unfolded.insert(d.to_path_buf());
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
        if self.vault_root() == Some(path.as_path()) {
            self.rename_vault(&name, window, cx);
        } else {
            self.rename_entry(&path, &name, window, cx);
        }
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    /// An entry takes a new name, and everything that knows it by its
    /// path follows: an open note, one inside a renamed folder, and what
    /// is remembered of them. False when the name could not be used,
    /// which is said.
    fn rename_entry(&mut self, path: &Path, name: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.rename_quietly(path, name, window, cx) {
            Ok(to) => {
                if to != path {
                    crate::file_undo::record(crate::file_undo::Kind::Renamed, vec![(path.to_path_buf(), to)], cx);
                }
                true
            }
            Err(e) => {
                self.say(tf("The name was not changed: {why}", &[("why", &e)]), cx);
                false
            }
        }
    }

    /// The rename itself, with nothing written down of it: what ⌘Z and
    /// ⌘⇧Z run, and `rename_entry`.
    pub(crate) fn rename_quietly(&mut self, path: &Path, name: &str, window: &mut Window, cx: &mut Context<Self>) -> Result<PathBuf, String> {
        let renamed = if self.is_asset(path) { self.rename_asset(path, name, cx) } else { vault::rename(path, name) };
        let to = renamed.map_err(|e| e.to_string())?;
        self.follow_moves(&[(path.to_path_buf(), to.clone())], window, cx);
        Ok(to)
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
        self.remove(vec![path], window, cx);
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
            items.push(Entry::choice("folder-look", "tabler/palette.svg", t("Icon and Color…")));
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
        if self.vault.is_none() {
            return self.show_vaults_at(at, None, false, cx);
        }
        let entries = vec![Entry::choice("rename", "icons/pencil-simple.svg", t("Rename")), Entry::choice("look", "tabler/palette.svg", t("Icon and Color…")), Entry::Rule, Entry::choice("reveal", "icons/folder-open.svg", t(crate::sys::REVEAL_LABEL))];
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
        let entries = vec![Entry::choice("look", "tabler/palette.svg", t("Icon and Color…")), Entry::choice("copy-path", "icons/link.svg", t("Copy Path")), Entry::Rule, Entry::choice("open-finder", "icons/folder-open.svg", t(crate::sys::OPEN_FOLDER_LABEL))];
        self.pressed = Some(path);
        self.menus.show("recent", at, entries);
        cx.notify();
    }

    /// The note's words: on a link, what it goes to and what it gives the
    /// clipboard; anywhere else what a text offers, in a note that takes
    /// no change only what leaves it as it is.
    fn canvas_menu(&mut self, editor: Entity<Editor>, at: Point<Pixels>, what: MenuAt, cx: &mut Context<Self>) {
        let (items, file) = self.canvas.entries(editor, what, self.shown.is_some(), cx);
        if file.is_some() {
            self.pressed = file;
        }
        self.menus.show("canvas", at, items);
        cx.notify();
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
        crate::menubar::push_writing(writing, words, cx);
    }

    /// A marked word learned for every note, or let be: a spelling for
    /// as long as the app runs, a grammar's rule for good.
    pub(crate) fn settle_issue(&mut self, issue: MenuIssue, learn: bool, cx: &mut Context<Self>) {
        if learn {
            if let Err(e) = onyx_core::check::learn(&issue.words) {
                eprintln!("onyx: the word was not kept: {e}");
            }
            self.settle_word(|w| w.learned = onyx_core::check::learned(), cx);
        } else if issue.spelling {
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

    /// Every folder of the vault shut.
    fn collapse_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(v) = self.vault.as_mut() {
            v.unfolded.clear();
        }
        self.save_ui(window);
        cx.notify();
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
            ("entry", "folder-look") => {
                if let Some(p) = pressed {
                    self.open_folder_look(p, window, cx);
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
            ("entry", "collapse") => self.collapse_all(window, cx),
            ("vault" | "vaults", "new") => self.start_new_vault(window, cx),
            ("vaults", "container") => self.choose_container(window, cx),
            ("vaults", "close") => self.close_vault(window, cx),
            ("vaults", path) => {
                if self.vault_root() != Some(Path::new(path)) {
                    self.show_vault(PathBuf::from(path), window, cx);
                }
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
            ("canvas", id) => match self.canvas.pick(id, window, cx) {
                crate::note_menu::Asked::Done => {}
                crate::note_menu::Asked::Link(link) => {
                    if let Some(url) = &link.url {
                        self.open_link(url, cx);
                    } else if let Some(note) = &link.note {
                        self.open_wikilink(note, cx);
                    }
                }
                crate::note_menu::Asked::File(what) => {
                    if let Some(p) = pressed {
                        self.asset_pick(&what, p, window, cx);
                    }
                }
                crate::note_menu::Asked::Export => {
                    if let Some(p) = self.shown.clone() {
                        self.open_export(p, window, cx);
                    }
                }
                crate::note_menu::Asked::Print => {
                    if let Some(p) = self.shown.clone() {
                        self.print_note(p, window, cx);
                    }
                }
                crate::note_menu::Asked::Learn(issue) => self.settle_issue(issue, true, cx),
                crate::note_menu::Asked::Ignore(issue) => self.settle_issue(issue, false, cx),
            },
            ("terminal", id) => {
                if let Some(term) = self.terminal.clone() {
                    term.update(cx, |t, cx| t.menu_do(id, window, cx));
                }
            }
            ("edit", id) => {
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
            ("vault", "rename") => {
                if let Some(root) = self.vault_root().map(Path::to_path_buf) {
                    self.start_rename(root, window, cx);
                }
            }
            ("vault", "look") => {
                if let Some(root) = self.vault_root().map(Path::to_path_buf) {
                    self.open_look(root.clone(), root, window, cx);
                }
            }
            ("recent", "look") => {
                if let Some(p) = pressed {
                    self.open_look(p.clone(), p, window, cx);
                }
            }
            ("vault", "reveal") => {
                if let Some(r) = self.vault_root() {
                    crate::sys::reveal(r);
                }
            }
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

    /// In a compact window the note's page comes forward: a note opened
    /// from the sidebar's page is what was asked to be seen.
    fn page_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.compact && self.sidebar_open {
            self.toggle_sidebar(window, cx);
        }
    }

    /// The swipe from the screen's edge over the note's page of a compact
    /// window, which brings the sidebar back as a finger pulls it: the
    /// page follows the finger, and let go it goes on to where the system
    /// says the swipe ended, from where it had got.
    fn side_pulled(&mut self, e: &BackGestureEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.compact || self.sidebar_open {
            return;
        }
        match e.phase {
            BackGesturePhase::Started | BackGesturePhase::Progressed => self.side_pull = Some(e.progress.clamp(0., 1.)),
            BackGesturePhase::Committed | BackGesturePhase::Cancelled => {
                let Some(at) = self.side_pull.take() else { return };
                let coming = e.phase == BackGesturePhase::Committed;
                // The eased clock set to where the finger left the page.
                let done = if coming { at } else { 1. - at };
                let passed = SLIDE.mul_f32(1. - (1. - done.clamp(0., 1.)).powf(0.2));
                self.side_anim = Some((coming, Instant::now() - passed));
                if coming {
                    self.sidebar_open = true;
                    self.save_ui(window);
                }
            }
        }
        cx.notify();
    }

    /// Whether this is a compact window, and the room it has, for what is
    /// drawn in it from another file.
    pub(crate) fn is_compact(&self) -> bool {
        self.compact
    }

    pub(crate) fn room_size(&self) -> Size<Pixels> {
        self.room
    }

    /// The page's room round the sheet, and the sheet's round its words:
    /// less of both in a compact window, where every point is the words'.
    fn sheet_margin(&self) -> f32 {
        if self.compact { 0. } else { f32::from(SHEET_MARGIN) }
    }

    /// How far a compact window's page runs under the room, to the
    /// screen's foot: what the system keeps for the home indicator, and
    /// nothing while the keyboard is out, whose top the page ends at.
    fn foot(&self) -> Pixels {
        if self.compact && !self.keys_out { self.safe.bottom } else { px(0.) }
    }

    /// Whether the bar at a compact window's foot shows (`render_foot_bar`).
    fn foot_bar_on(&self) -> bool {
        self.compact && self.vault.is_some() && !self.keys_out
    }

    /// The room a compact window's page keeps clear at its foot, for the
    /// home indicator and the bar over it.
    fn foot_room(&self) -> f32 {
        f32::from(self.foot()) + if self.foot_bar_on() { FOOT_BAR_H + 2. * FOOT_BAR_GAP } else { 0. }
    }

    fn sheet_gutter(&self) -> f32 {
        if self.compact { 20. } else { SHEET_GUTTER }
    }

    /// How much of the sidebar is beside the content, from 0 to 1, on one
    /// eased clock that every term of the row's layout goes by.
    fn side_t(&self) -> f32 {
        if let Some(pulled) = self.side_pull {
            return pulled;
        }
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
        // A compact window's sidebar is a drawer: it leaves a strip of the
        // note's page in sight at its right, as Obsidian's does.
        if self.compact {
            return (self.room.width * DRAWER).round();
        }
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
        fn widest(entries: &[vault::Entry], depth: usize, unfolded: &HashSet<PathBuf>, measure: &dyn Fn(&str) -> f32) -> f32 {
            entries.iter().fold(0., |most: f32, e| {
                let inside = if e.dir && unfolded.contains(&e.path) { widest(&e.children, depth + 1, unfolded, measure) } else { 0. };
                most.max(DEPTH * depth as f32 + measure(&e.name)).max(inside)
            })
        }
        let font = font(cx.theme().font_family.clone());
        let measure = |text: &str| {
            let run = TextRun { len: text.len(), font: font.clone(), color: gpui::black(), background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(SharedString::from(text.to_string()), px(13.), &[run], None).width())
        };
        let names = self.vault.as_ref().map_or(0., |v| widest(&v.tree.children, 0, &v.unfolded, &measure));
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
            // The chosen looked up in a set made once, not by a walk of
            // them for every row: with all of a long tree chosen, that was
            // rows × rows a frame.
            let marked: HashSet<&Path> = self.choice.marked.iter().map(PathBuf::as_path).collect();
            let lit: Vec<bool> = rows.iter().map(|(p, _)| if marked.is_empty() { self.shown.as_deref() == Some(p.as_path()) } else { marked.contains(p.as_path()) }).collect();
            *self.drag_marked.borrow_mut() = (!self.choice.marked.is_empty()).then(|| Rc::from(self.choice.marked.clone()));
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
        let unfolded = self.vault.as_ref().map(|v| &v.unfolded);
        for e in entries {
            out.push(Piece::Row(e, depth));
            if !e.dir {
                continue;
            }
            let open = unfolded.is_some_and(|f| f.contains(&e.path));
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
                let full = inside.iter().map(Piece::rows).sum::<usize>() as f32 * tree_row_h();
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
        let unfolded = self.vault.as_ref().map(|v| &v.unfolded);
        let path = e.path.clone();
        let is_open_folder = e.dir && unfolded.is_some_and(|f| f.contains(&e.path));
        let chosen = self.row_lit(&e.path);
        // Rows lit one under another are one block, round at its two ends.
        let (joined_up, joined_down) = self.lit_joins.borrow().get(&e.path).copied().unwrap_or((false, false));
        let cut = self.choice.cut.iter().any(|c| c == &e.path);
        let into = if e.dir { e.path.clone() } else { e.path.parent().map(Path::to_path_buf).unwrap_or_default() };
        let wash = theme.primary.opacity(0.22);
        // A folder's icon; a note has none, every one being markdown, and
        // its name stands where a folder's icon would.
        let icon = e.dir.then(|| crate::folder_look::icon(&e.path, &e.name, is_open_folder, dark, theme, cx));
        let id = ElementId::Name(SharedString::from(format!("entry-{}", e.path.display())));
        let menu_path = path.clone();
            // The row keeps its height and takes the press and the drag;
            // its plate, which is lit, stands a little in from its top and
            // foot, so lit rows breathe apart, but where two chosen rows
            // join into one block.
            let hover_bg = theme.foreground.opacity(0.07);
            let plate = h_flex()
                .size_full()
                .pl(px(hand(8.) + hand(14.) * depth as f32))
                .pr(px(hand(8.)))
                .gap(px(hand(6.)))
                .items_center()
                .rounded(px(hand(7.)))
                .when(chosen && joined_up, |d| d.rounded_t(px(0.)))
                .when(chosen && joined_down, |d| d.rounded_b(px(0.)))
                .when(chosen, |d| d.bg(theme.foreground.opacity(0.13)))
                .when(!chosen, |d| d.group_hover("tree-row", move |s| s.bg(hover_bg)))
                .when(self.pressed.as_ref() == Some(&e.path), |d| d.bg(theme.foreground.opacity(0.1)))
                .child(div().w(px(hand(12.))).flex_shrink_0().flex().justify_center().when(e.dir, |d| d.child(Icon::default().path(if is_open_folder { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" }).with_size(px(hand(12.))).text_color(theme.muted_foreground))))
                .children(icon)
                .child(div().flex_1().min_w_0().truncate().child(e.name.clone()))
                // A folder's New Note and New Folder, there while the
                // pointer is on its row (`hover_row`): no room is kept for
                // them else, so a name is cut only while they show. A hover
                // style cannot do it, being applied as the row is painted,
                // after its layout.
                .when(e.dir && self.hover_row.as_ref() == Some(&e.path), |d| {
                    let (note_in, folder_in) = (path.clone(), path.clone());
                    d.child(
                        h_flex()
                            .flex_shrink_0()
                            .mr(px(-4.))
                            .gap(px(2.))
                            .child(library::row_tool(ElementId::Name(format!("row-note-{}", e.path.display()).into()), "icons/file-plus.svg", t("New Note").into(), theme).on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.new_note_in(note_in.clone(), window, cx);
                            })))
                            .child(library::row_tool(ElementId::Name(format!("row-folder-{}", e.path.display()).into()), "icons/folder-plus.svg", t("New Folder").into(), theme).on_click(cx.listener(move |this, _, window, cx| {
                                swallow_click(window, cx);
                                this.new_folder_in(folder_in.clone(), window, cx);
                            }))),
                    )
                });
            let row = div()
                .id(id)
                .group("tree-row")
                .h(px(tree_row_h()))
                .flex_shrink_0()
                .when(!(chosen && joined_up), |d| d.pt(px(ROW_BREATH)))
                .when(!(chosen && joined_down), |d| d.pb(px(ROW_BREATH)))
                .text_size(px(hand(13.)))
                .text_color(theme.foreground.opacity(if chosen { 1. } else { 0.88 }))
                .when(cut, |d| d.opacity(0.45))
                .cursor_pointer()
                .child(plate)
                .when(e.dir, |d| {
                    let hovered = path.clone();
                    d.on_hover(cx.listener(move |this, on: &bool, _, cx| {
                        if *on {
                            this.hover_row = Some(hovered.clone());
                        } else if this.hover_row.as_ref() == Some(&hovered) {
                            this.hover_row = None;
                        }
                        cx.notify();
                    }))
                })
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

    /// The sidebar, after emaki's: the brand on the title row, the
    /// vault's card with the tree and the vaults' button under it
    /// (`render_side_stack`), and the foot with the person's picture,
    /// name and the settings.
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let waiting = self.update.found.is_some();
        let gear_dot = self.motion.presence("gear-dot", waiting, cx);
        let theme = self.frame_theme(cx);
        let accent = theme.foreground.opacity(0.1);
        let brand = h_flex().gap(px(5.)).items_center().child(img("icon/icon-128.png").size(px(31.)).flex_shrink_0()).child(crate::widgets::wordmark("Onyx", 23.5, theme.primary, cx));
        let header = Self::drag_region(h_flex().h(TITLEBAR_H).flex_shrink_0().pt(px(2.) + FRAME).pr(px(16.)).justify_end().items_center().child(brand), cx);
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
                self.avatar(if self.compact { 34. } else { 28. }, &theme)
                    .id("side-avatar")
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.85))
                    .managed_tooltip(|window, cx| tip(t("Your picture and name"), window, cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.open_settings(Some("general"), window, cx);
                    })),
            )
            .child(
                v_flex().flex_1().min_w_0().child(
                    div()
                        .relative()
                        .text_size(px(if self.compact { 19. } else { 16. }))
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
                    .size(px(if self.compact { 36. } else { 26. }))
                    .flex_shrink_0()
                    .rounded(px(hand(6.)))
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
                    .child(Icon::new(IconName::Settings).with_size(px(hand(16.))).text_color(theme.muted_foreground))
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
        if self.vault.is_none() {
            return self.render_place_card(ground, theme, cx);
        }
        self.scrollers.name(&self.side_scroll);
        if self.side_view == SideView::Assets {
            let assets = self.render_assets(theme, ground, cx);
            return self.render_side_stack(div().flex().flex_col().child(assets), ground, theme, cx);
        }
        let rows = self.render_tree(theme, cx);
        // A compact window names the vault once, on its block at the
        // drawer's top, which has the row's menu and takes its drops.
        let head = if self.compact { None } else { self.render_vault_root(theme, cx) };
        let list = div().flex().flex_col().children(head).child(
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
                            .children(rows),
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
        let found = {
            let mut files = self.avatar_files.borrow_mut();
            match files.get(&(size as u32)) {
                Some((for_, path)) if *for_ == self.cfg.app.avatar => path.clone(),
                _ => {
                    let master = PathBuf::from(&self.cfg.app.avatar);
                    let sized = avatar_sized(&master, size as u32);
                    let path = if sized.is_file() { sized } else { master };
                    let path = (!self.cfg.app.avatar.is_empty() && path.is_file()).then_some(path);
                    files.insert(size as u32, (self.cfg.app.avatar.clone(), path.clone()));
                    path
                }
            }
        };
        let disc = div().size(px(size)).rounded_full().flex_shrink_0().overflow_hidden();
        if let Some(path) = found {
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
        self.avatar_files.borrow_mut().clear();
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
    /// The sidebar folded to a rail, with what the sidebar has, each as
    /// an entry with its tip at the right. In a vault: New Note, then
    /// Notes and Assets, each flying the sidebar's card out beside the
    /// rail on its tab (`render_rail_fly`), and at the foot the vault's
    /// mark, a press on which lists the vaults as the vault's button does.
    /// Before a vault: the container's vaults by their marks, a press
    /// showing one and a right click giving its menu, then New Vault…; a
    /// list longer than the rail ends in an entry that lists them all.
    /// With no container, Choose Container…. The app's icon and the
    /// person's picture stand at the foot of either.
    fn render_rail(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = (1. - self.side_t() * 1.6).max(0.);
        if shown <= 0. || self.compact {
            return None;
        }
        let theme = self.frame_theme(cx);
        let accent = theme.foreground.opacity(0.1);
        let mid = RAIL_W / 2.;
        let top = RAIL_TOP + (RAIL_ENTRY + RAIL_GAP) * 2. + px(12.);
        let fly = self.rail_fly.is_some_and(|(_, open)| open);
        let at = move |i: f32| top + (RAIL_ENTRY + RAIL_GAP) * i;
        let entry = |id: ElementId, glyph: AnyElement, words: SharedString, active: bool| {
            div()
                .id(id)
                .absolute()
                .left(mid - RAIL_ENTRY / 2.)
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
        let glyph = |path: &'static str, lit: bool| Icon::default().path(path).with_size(px(18.)).text_color(if lit { theme.foreground } else { theme.muted_foreground }).into_any_element();
        let rule = |y: Pixels| div().absolute().left(mid - px(12.)).top(y).w(px(24.)).h(px(1.)).bg(accent);
        let mut rail = Self::drag_region(div(), cx).absolute().top_0().left_0().h_full().w(RAIL_W).opacity(shown).child(rule(top - px(7.)));
        // The foot: the vault's mark in a vault, then the icon and the picture.
        let foot = px(104.);
        if let Some(root) = self.vault_root().map(Path::to_path_buf) {
            let plus = div().size(px(24.)).rounded_full().bg(theme.primary).flex().items_center().justify_center().child(Icon::default().path("icons/plus.svg").with_size(px(14.)).text_color(theme.primary_foreground)).into_any_element();
            let notes_on = fly && self.side_view == SideView::Notes;
            let assets_on = fly && self.side_view == SideView::Assets;
            let listed = self.menus.open.as_ref().is_some_and(|m| m.what == "vaults");
            let name = self.vault.as_ref().map(|v| v.tree.name.clone()).unwrap_or_default();
            let mark = library::vault_badge(&root, &name, 22., &theme, cx).into_any_element();
            rail = rail
                .child(entry("rail-new".into(), plus, t("New Note (⌘N)").into(), false).top(at(0.)).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.new_note(&NewNote, window, cx);
                })))
                .child(entry("rail-notes".into(), glyph("icons/folder.svg", notes_on), t("Notes").into(), notes_on).top(at(1.)).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.rail_fly_to(SideView::Notes, window, cx);
                })))
                .child(entry("rail-assets".into(), glyph("icons/image.svg", assets_on), t("Assets").into(), assets_on).top(at(2.)).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.rail_fly_to(SideView::Assets, window, cx);
                })))
                // The list rises beside the rail, its foot level with the mark's.
                .child(entry("rail-vault".into(), mark, name.into(), listed).bottom(foot).on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    if this.menus.open.as_ref().is_some_and(|m| m.what == "vaults") {
                        this.menus.close(cx);
                        cx.notify();
                        return;
                    }
                    let y = this.safe.top + this.room.height - foot;
                    this.show_vaults_at(point(this.safe.left + RAIL_W + px(6.), y), Some(px(240.)), true, cx);
                })));
        } else if let Some(dir) = self.container() {
            // As many vaults as the rail has room for above its foot, the
            // last place given to the whole list when they do not all fit.
            let room = self.room.height - top - px(150.);
            let fit = ((f32::from(room) / f32::from(RAIL_ENTRY + RAIL_GAP)).floor() as usize).max(2) - 1;
            let vaults = library::vault_list(Some(&dir), None);
            let more = vaults.len() > fit;
            let take = if more { fit - 1 } else { vaults.len() };
            let mut i = 0.;
            for p in vaults.into_iter().take(take) {
                let name = library::vault_name(&p);
                let mark = library::vault_badge(&p, &name, 22., &theme, cx).into_any_element();
                let menu_p = p.clone();
                rail = rail.child(
                    entry(ElementId::Name(format!("rail-vault-{}", p.display()).into()), mark, name.into(), false)
                        .top(at(i))
                        .on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                            swallow_click(window, cx);
                            this.show_recent_menu(menu_p.clone(), ev.position, cx);
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            this.show_vault(p.clone(), window, cx);
                        })),
                );
                i += 1.;
            }
            if more {
                let y = at(i) + RAIL_ENTRY;
                rail = rail.child(entry("rail-vaults".into(), glyph("icons/ellipsis.svg", false), t("Vaults").into(), false).top(at(i)).on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.show_vaults_at(point(this.safe.left + RAIL_W + px(6.), this.safe.top + y), Some(px(240.)), true, cx);
                })));
                i += 1.;
            }
            rail = rail.child(entry("rail-new-vault".into(), glyph("icons/plus.svg", false), t("New Vault…").into(), false).top(at(i)).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.start_new_vault(window, cx);
            })));
        } else {
            rail = rail.child(entry("rail-container".into(), glyph("icons/folder-open.svg", false), t("Choose Container…").into(), false).top(at(0.)).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.choose_container(window, cx);
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
                    this.open_settings(Some("general"), window, cx);
                })),
        );
        Some(rail.into_any_element())
    }

    /// The card flown out on a tab: Notes or Assets. A press on the entry
    /// of the tab showing puts it away; on the other, it turns to that
    /// tab and stays.
    fn rail_fly_to(&mut self, view: SideView, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.rail_fly.is_some_and(|(_, open)| open);
        if open && self.side_view == view {
            return self.rail_fly_close(cx);
        }
        self.set_side_view(view, window, cx);
        if !open {
            self.rail_fly_toggle(cx);
        }
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
        if self.compact {
            return None;
        }
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
        // Borrowed, not copied: every note found, each with its words,
        // copied at each draw (the field's caret draws twice a second) to
        // draw the first 40.
        let results = self.search_results.as_ref();
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
            .children(crate::widgets::reaches(theme.overlay))
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
                            .child(Icon::default().path("icons/search.svg").with_size(px(hand(16.))).text_color(theme.muted_foreground))
                            .child(div().id("search-field").track_focus(&focus).flex_1().min_w_0().text_size(px(hand(14.))).child(Input::new(&self.search_input).appearance(false).bordered(false).text_size(px(hand(14.))).on_secondary_click(Self::input_menu(&self.search_input, "edit", cx)))),
                    )
                    .child(
                        v_flex().relative().flex_1().min_h_0().pb(px(8.)).vertical_scrollbar(&self.search_scroll).child(v_flex().id("search-results").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.search_scroll).px(px(8.)).pt(px(8.)).map(|d| match results {
                            None => d.child(div().p(px(12.)).text_size(px(hand(12.5))).text_color(theme.muted_foreground).child(t("Type to search every note's name and words."))),
                            Some(res) if res.notes.is_empty() => d.child(div().p(px(12.)).text_size(px(hand(12.5))).text_color(theme.muted_foreground).child(t("No matches."))),
                            Some(res) => d.children(res.notes.iter().take(40).enumerate().map(|(i, n)| {
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
                                    .child(h_flex().gap(px(8.)).child(div().flex_1().min_w_0().truncate().text_size(px(hand(13.))).font_weight(FontWeight::MEDIUM).child(n.title.clone())).child(div().flex_shrink_0().text_size(px(hand(11.))).text_color(theme.muted_foreground).child(where_)))
                                    .children(n.snippets.iter().map(|sn| {
                                        let marks: Vec<(std::ops::Range<usize>, HighlightStyle)> = sn.marks.iter().map(|r| (r.clone(), mark)).collect();
                                        div().w_full().truncate().child(StyledText::new(sn.text.clone()).with_default_highlights(&style, marks))
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
            .pl(px(self.page_pad.0))
            .pr(px(self.page_pad.1))
            .py(px(8.))
            .child(
                h_flex()
                    .id("find-bar")
                    .key_context(FIND_CONTEXT)
                    .on_action(cx.listener(|this, _: &Escape, window, cx| {
                        cx.stop_propagation();
                        this.close_find(window, cx)
                    }))
                    .w(px(self.sheet_w(self.width_level())))
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
        let k = self.side_t();
        let compact = self.compact;
        // A compact window's button stands on the note's page, in its inks.
        let theme = if compact { cx.theme().clone() } else { self.frame_theme(cx) };
        // Beside the drawer the page is darkened and the button, the way
        // back, is not: it stands on a plate of the page's ground.
        let plate = (compact && k > 0.).then(|| theme.background.opacity(k));
        let size = if compact { self.toggle_w() } else { STRIP_BTN + (RAIL_ENTRY - STRIP_BTN) * (1. - k) };
        let icon_size = if compact { px(20.) } else { px(16.) + px(2.) * (1. - k) };
        let beside = k < 0.5 && !compact;
        let button = |id: &'static str, icon: &'static str, label: SharedString| {
            let hover = theme.foreground.opacity(0.07);
            div()
                .id(id)
                .size(size)
                .rounded(px(6.) + px(2.) * (1. - k))
                .when_some(plate, |d, ground| d.rounded_full().bg(ground))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.press_taken = true))
                .hover(move |s| s.bg(hover))
                .map(|d| if beside { d.managed_tooltip_at(gpui_component::Placement::Right, move |window, cx| tip(label.clone(), window, cx)) } else { d.managed_tooltip(move |window, cx| tip(label.clone(), window, cx)) })
                .child(Icon::default().path(icon).with_size(icon_size).text_color(theme.muted_foreground))
        };
        let row_y = px(1.) + FRAME / 2. + (TITLEBAR_H - STRIP_BTN) / 2.;
        let rail_x = RAIL_W / 2. - RAIL_ENTRY / 2.;
        let strip_left = if cfg!(target_os = "macos") { TRAFFIC_W + px(2.) } else { px(10.) };
        // In a compact window the two go to the ends of the tabs' row as
        // the note's page comes: the sidebar's at its left, where the way
        // back is looked for, the search's at its right.
        // In a compact window the sidebar's button is the note's page's:
        // at its title row's left end, a finger's size, and it goes with
        // the page as the drawer comes, so it is in the strip of the page
        // left in sight, where a press puts the drawer away. There is no
        // search's button there: Search is in the bar at the page's foot
        // and among the tools at the drawer's (`render_foot_bar`,
        // `render_side_tools`).
        let hand_y = (HAND_TITLE_H - size) / 2.;
        let at = |i: f32| {
            if compact {
                return (self.card_x + STRIP_END + size * i, hand_y);
            }
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
            .when(!compact, |d| {
                d.child(button("strip-search", "icons/search.svg", t("Search (⌘K)").into()).absolute().left(qx).top(qy).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.open_search(window, cx)
                })))
            })
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

    /// A notice: a pill hung under the strip, over the page, coming and
    /// going over a moment, in the danger's ink, or quiet when it is news
    /// (an icon before it) or says something is done (a press from the
    /// file). It takes no room: the page under it never moves, as a line
    /// opened under the strip moved the whole note down.
    fn render_notice(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        const NOTICE_H: f32 = 30.;
        let moving = self.motion.presence("notice", self.notice.is_some(), cx);
        let n = self.notice.clone().or_else(|| moving.and_then(|_| self.last_notice.clone()))?;
        let done = n.done.clone();
        let quiet = done.is_some() || n.icon.is_some();
        let pill = h_flex()
            .id("notice")
            .occlude()
            .h(px(NOTICE_H))
            .max_w(relative(0.9))
            .flex_shrink_0()
            .items_center()
            .gap(px(7.))
            .px(px(14.))
            .rounded_full()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow(crate::widgets::float_shadow(theme))
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
            .child(div().min_w_0().truncate().child(n.words));
        let hung = div().absolute().top(px(8.)).left_0().right_0().flex().justify_center().child(pill);
        Some(match moving {
            Some((coming, serial)) => hung.with_animation(ElementId::NamedInteger("notice".into(), serial as u64), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                let k = if coming { t } else { 1. - t };
                d.top(px(8. - 6. * (1. - k))).opacity(k)
            }).into_any_element(),
            None => hung.into_any_element(),
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

    /// The bar at a compact window's foot, over the note's page and above
    /// the home indicator, where a thumb reaches: Search, New Note and
    /// More, the menu of the note showing (`entry_menu`, the one its row
    /// in the sidebar opens), which rises from the bar. More keeps its
    /// place with no note showing, dimmed, so the bar is one width. It
    /// goes while the keyboard is out, which has the window's foot then,
    /// and there is none before a vault, with nothing to search or to
    /// make a note in.
    fn render_foot_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.compact {
            return None;
        }
        let on = self.foot_bar_on();
        let theme = cx.theme().clone();
        let wash = theme.foreground.opacity(0.08);
        let button = |id: &'static str, icon: &'static str| div().id(id).size(px(FOOT_BAR_H - 8.)).rounded_full().flex().items_center().justify_center().cursor_pointer().hover(move |s| s.bg(wash)).active(move |s| s.bg(wash)).child(Icon::default().path(icon).with_size(px(22.)).text_color(theme.foreground.opacity(0.8)));
        let note = self.shown.as_deref().is_some_and(vault::is_note);
        let listed = note && self.menus.open.as_ref().is_some_and(|m| m.what == "entry") && self.pressed.is_some() && self.pressed == self.shown;
        let place = self.foot_at.clone();
        let bar = h_flex()
            .id("foot-bar")
            .relative()
            .occlude()
            .h(px(FOOT_BAR_H))
            .px(px(4.))
            .gap(px(8.))
            .items_center()
            .rounded_full()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow(float_shadow(&theme))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(button("foot-search", "icons/search.svg").on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.open_search(window, cx)
            })))
            .child(button("foot-new-note", "icons/file-plus.svg").on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.new_note(&NewNote, window, cx)
            })))
            .child(button("foot-more", "icons/menu.svg").when(!note, |d| d.opacity(0.35).cursor_default()).when(listed, |d| d.bg(wash)).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                let Some(path) = this.shown.clone().filter(|p| vault::is_note(p)) else { return };
                if this.menus.open.as_ref().is_some_and(|m| m.what == "entry") {
                    this.menus.close(cx);
                    cx.notify();
                    return;
                }
                // The menu's bottom right over the bar's top right, under the button pressed.
                let b = this.foot_at.get();
                this.show_entry_menu(path, point(b.right(), b.top() - px(6.)), cx);
                if let Some(m) = this.menus.open.as_mut() {
                    m.corner = Anchor::BottomRight;
                }
            })))
            .child(canvas(move |b, _, _| place.set(b), |_, _, _, _| ()).absolute().top_0().left_0().size_full())
            .into_any_element();
        let bar = match self.motion.presence("foot-bar", on, cx) {
            Some((coming, serial)) => div().child(bar).with_animation(("foot-bar", serial as u64), Animation::new(crate::widgets::FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element(),
            None if on => bar,
            None => return None,
        };
        Some(div().absolute().left_0().right_0().bottom(self.foot() + px(FOOT_BAR_GAP)).flex().justify_center().child(bar).into_any_element())
    }

    /// The room a button at an end of the tabs' row has: `TOGGLE_W`, and
    /// in a compact window a finger's, where the sidebar's button and the
    /// search's stand there.
    fn toggle_w(&self) -> Pixels {
        if self.compact { px(36.) } else { TOGGLE_W }
    }

    /// The bar under the strip, after emaki's path line: at the left the
    /// note's name with its folder before it, from the vault's name, or
    /// the vault's own place while its page shows, each a press away from
    /// the file manager; at the right how the note is shown and how wide.
    fn render_path_bar(&self, room: Pixels, cx: &mut Context<Self>) -> Option<AnyElement> {
        // Before a vault is shown, the container's.
        let root = self.vault_root().map(Path::to_path_buf).or_else(|| self.container())?;
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
            .h(px(if self.compact { 32. } else { 22. }))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .font_family(self.look.mono_family.clone())
            // Eleven points is small print on an iPhone, where the path
            // is the title row's middle.
            .text_size(px(if self.compact { 14. } else { 11. }))
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
            .child(Icon::default().path(icon).with_size(px(if self.compact { 14. } else { 12. })).text_color(theme.muted_foreground).flex_shrink_0())
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
                // In a compact window More is at the title row's right end (`render_strip_ends`).
                .when(self.shown.as_deref().is_some_and(vault::is_note) && !self.compact, |d| d.child(h_flex().absolute().right(STRIP_END + room).top_0().bottom_0().items_center().child(self.render_more(&theme, cx))))
                // An asset: the system's app for it, where the note has its
                // paper; a picture or a PDF has its frame's bar and menu.
                .when(self.shown.as_deref().is_some_and(|p| !vault::is_note(p)) && !self.shown_framed() && cfg!(not(target_os = "ios")), |d| {
                    d.child(
                        h_flex()
                            .absolute()
                            .right(px(PATH_BAR_PAD))
                            .top_0()
                            .bottom_0()
                            .gap(px(2.))
                            .items_center()
                            .child(self.path_button("path-open", "icons/external-link.svg", t("Open with Default App"), Self::press_open_app, &theme, cx)),
                    )
                })
                .into_any_element(),
        )
    }

    /// Whether the shown tab is a picture or a PDF in its frame.
    fn shown_framed(&self) -> bool {
        let Some(shown) = self.shown.as_deref() else { return false };
        self.tabs.iter().find(|t| t.path == shown).and_then(|t| t.asset.as_ref()).is_some_and(|a| a.framed())
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

    /// The note's own menu at the path bar's end: the one its row in the
    /// sidebar opens on a right click (`entry_menu`), Export… with the rest,
    /// opened at the button. It stands in the strip's column of buttons,
    /// square and placed as the terminal's over it (`toggle_button`,
    /// `STRIP_END`), so the two line up; lit while its menu is open. A
    /// compact window has no path row: its More is on the bar at the
    /// page's foot (`render_foot_bar`).
    fn render_more(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let (lit, wash) = (theme.foreground, theme.muted);
        let open = self.menus.open.as_ref().is_some_and(|m| m.what == "entry") && self.pressed.is_some() && self.pressed == self.shown;
        div()
            .id("path-more")
            .size(TOGGLE_W)
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .cursor_pointer()
            .text_color(if open { lit } else { theme.muted_foreground })
            .when(open, |d| d.bg(wash))
            .hover(move |s| s.bg(wash).text_color(lit))
            .managed_tooltip(|window, cx| tip(t("More"), window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, ev: &ClickEvent, window, cx| {
                swallow_click(window, cx);
                if let Some(path) = this.shown.clone() {
                    this.show_entry_menu(path, ev.position(), cx);
                }
            }))
            .child(Icon::default().path("icons/file-dots.svg").with_size(px(15.)))
    }

    /// The ends of the tabs' row, after Emaki's panel buttons: at the
    /// left the note's source (a note's only), at the right the terminal
    /// (while a vault is open), each its icon alone, only while what it
    /// acts on shows. Both ends keep their width with or without them, so
    /// the tabs keep one center line, and the buttons fade as they come
    /// and go.
    fn render_strip_ends(&self, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let note = self.shown_tab().is_some_and(|t| t.asset.is_none());
        let end = |id: &'static str, on: bool, button: AnyElement, cx: &mut Context<Self>| -> Option<AnyElement> {
            match self.motion.presence(id, on, cx) {
                Some((coming, serial)) => Some(div().child(button).with_animation((id, serial as u64), Animation::new(crate::widgets::FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| d.opacity(if coming { t } else { 1. - t })).into_any_element()),
                None => on.then_some(button),
            }
        };
        let source = self.toggle_button("strip-source", "icons/file-code.svg", t("Markdown Source (⌘⇧M)"), self.pressed_line(self.source_anim, self.source_shown()), Rc::new(|this, window, cx| this.toggle_source(window, cx)), cx);
        let term = self.toggle_button("strip-term", "icons/terminal.svg", t("Shell Terminal (⌘⇧S)"), self.pressed_line(self.term_anim, self.term_shown()), Rc::new(|this, window, cx| this.toggle_terminal(window, cx)), cx);
        // A compact window has room for no pane beside the note, and its
        // row's ends are the sidebar's button's and the search's
        // (`render_strip`); an iPhone and an iPad have no shell.
        let source = end("strip-source", note && !self.compact, source, cx);
        let term = end("strip-term", (self.vault.is_some() || self.term_on) && !self.compact && cfg!(not(target_os = "ios")), term, cx);
        let left = h_flex().w(self.toggle_w()).flex_shrink_0().items_center().children(source).into_any_element();
        let right = h_flex().w(self.toggle_w()).flex_shrink_0().justify_end().items_center().children(term).into_any_element();
        (left, right)
    }

    /// What a button's line grows or shrinks from, whether it shows, and
    /// the press's number: what was before the press while the press is a
    /// moment old, after that what is, as Emaki's `pressed_from`.
    fn pressed_line(&self, press: Option<(f32, Instant, u32)>, on: bool) -> (bool, bool, u32) {
        match press {
            Some((from, at, serial)) => (if at.elapsed() < Duration::from_millis(300) { from >= 0.5 } else { on }, on, serial + 1),
            None => (on, on, 0),
        }
    }

    /// A button that shows a thing and puts it away, as Emaki's: its icon
    /// alone, gray and washed under the pointer, in the ink while what it
    /// shows is out, with a short line of the accent under it that grows
    /// out from its middle and shrinks back.
    #[allow(clippy::type_complexity)]
    fn toggle_button(&self, id: &'static str, icon: &'static str, label: &'static str, (was, on, serial): (bool, bool, u32), click: Rc<dyn Fn(&mut Self, &mut Window, &mut Context<Self>)>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let lit = theme.foreground;
        let wash = theme.muted;
        let label: SharedString = label.into();
        let accent = theme.primary;
        h_flex()
            .id(id)
            .relative()
            .size(TOGGLE_W)
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .cursor_pointer()
            .text_color(if on { lit } else { theme.muted_foreground })
            .hover(move |s| s.bg(wash).text_color(lit))
            .managed_tooltip(move |window, cx| tip(label.clone(), window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| {
                swallow_click(window, cx);
                click(this, window, cx);
            }))
            .child(Icon::default().path(icon).with_size(px(15.)))
            .children((on || was).then(|| {
                let (a, b) = (if was { 1. } else { 0. }, if on { 1. } else { 0. });
                let w = 14.;
                div().absolute().bottom(px(1.)).left(TOGGLE_W / 2.).h(px(2.)).rounded(px(1.)).bg(accent).with_animation(ElementId::Name(format!("{id}-mark-{serial}").into()), Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()), move |d, t| {
                    let now = w * (a + (b - a) * t);
                    d.w(px(now)).ml(px(-now / 2.))
                })
            }))
            .into_any_element()
    }

    /// The note's source beside it, or put away: ⌘⇧M, the View menu and
    /// the strip's button, for every note, and kept. The keyboard goes
    /// with it: in the note as it opens, it goes to the same place in the
    /// source; in the source as it goes, back to the note.
    pub(crate) fn toggle_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let was = self.panes_now();
        // Put away for the terminal in a card too narrow for both, it comes
        // back in the terminal's place.
        if !(self.source_on && !self.source_shown()) {
            self.source_on = !self.source_on;
        }
        self.term_latest = false;
        self.panes_moved(was);
        if let Some(ix) = self.shown_index().filter(|&i| self.tabs[i].asset.is_none()) {
            let editor = self.tabs[ix].editor.clone();
            if self.source_shown() {
                let in_note = editor.read(cx).focus_handle(cx).is_focused(window);
                let pane = self.source_pane(ix, window, cx);
                if in_note {
                    let at = editor.read(cx).caret_byte();
                    pane.update(cx, |p, cx| p.put_caret(at, cx));
                    window.focus(&pane.focus_handle(cx), cx);
                }
            } else if let Some(pane) = self.tabs[ix].source.clone() {
                if pane.focus_handle(cx).contains_focused(window, cx) {
                    let at = pane.read(cx).caret(cx);
                    editor.update(cx, |e, cx| e.put_caret_at(at, window, cx));
                }
            }
        }
        self.save_ui(window);
        cx.notify();
    }

    /// How far the source pane is out, 0 to 1, eased on the window's clock.
    fn source_t(&self) -> f32 {
        let to = if self.source_shown() { 1. } else { 0. };
        match self.source_anim {
            Some((from, at, _)) if at.elapsed() < SLIDE => from + (to - from) * ease_out_quint()(at.elapsed().as_secs_f32() / SLIDE.as_secs_f32()),
            _ => to,
        }
    }

    fn source_moving(&self) -> bool {
        self.source_anim.is_some_and(|(_, at, _)| at.elapsed() < SLIDE)
    }

    /// The source pane's width when all the way out, in a card this wide:
    /// its share of the card, each side keeping its least; in a card too
    /// narrow for both, half of it.
    fn source_full_w(&self, card_w: f32) -> f32 {
        if card_w < SOURCE_LEAST + NOTE_LEAST {
            return (card_w / 2.).round();
        }
        (card_w * self.source_share).clamp(SOURCE_LEAST, card_w - NOTE_LEAST).round()
    }

    /// The tab's source pane, made the first time it shows.
    fn source_pane(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> Entity<SourcePane> {
        if let Some(pane) = &self.tabs[ix].source {
            return pane.clone();
        }
        let editor = self.tabs[ix].editor.clone();
        let look = self.look.clone();
        let this = cx.entity().downgrade();
        let pane = cx.new(|cx| SourcePane::new(&editor, look, |input| Self::source_menu(this.clone(), &editor, input), window, cx));
        self.tabs[ix].source = Some(pane.clone());
        pane
    }

    /// A right click in the source: a marked word's corrections, then the
    /// text's own choices, acting on the pane's words; a correction is
    /// made in the note, and so comes here.
    fn source_menu(this: WeakEntity<Self>, editor: &Entity<Editor>, input: &Entity<gpui_component::input::EditorState>) -> crate::source::MenuAt {
        let (editor, input) = (editor.clone(), input.clone());
        Rc::new(move |at, window, cx| {
            let (this, editor, input) = (this.clone(), editor.clone(), input.clone());
            // Called from inside the editor's own update: what it does
            // waits for that to end.
            window.defer(cx, move |window, cx| {
                // With nothing selected, the word under the pointer is,
                // as a right click in Emaki's editor selects it.
                let selection = input.update(cx, |s, cx| s.select_word_at(at, window, cx));
                let (focus, offset, text) = {
                    let s = input.read(cx);
                    (gpui::Focusable::focus_handle(s, cx), s.offset_at(at), s.value().to_string())
                };
                let issue = (editor.read(cx).text() == text).then(|| editor.read(cx).issue_at(offset)).flatten();
                let clipboard = cx.read_from_clipboard().is_some();
                let _ = this.update(cx, |this, cx| {
                    let items = this.canvas.for_source(editor, focus, issue, selection, clipboard);
                    this.menus.show("canvas", at, items);
                    cx.notify();
                });
            });
        })
    }

    /// The source pane at the page's left, `w` wide at this draw: a sheet
    /// as the note's is, its words at the width it has all the way out,
    /// so they do not wrap anew as it slides.
    ///
    /// The sheet reads as the note's does while it scrolls: with more to
    /// scroll past an edge, the sheet runs off the page there, no margin
    /// and no border, under the page's fade; at the file's start and its
    /// end, the margin and the sheet's edge stand. The editor keeps its
    /// own offset, so the sheet's edges go by what is left to scroll
    /// (`scroll_room`): the top edge and the words' top rise with the
    /// first of the scroll until the words reach the page's top; the
    /// editor always runs to the page's foot, and at the file's end the
    /// sheet's bottom edge comes up over the empty rows the editor keeps
    /// under the last line.
    fn render_source(&mut self, w: f32, full_w: f32, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if w < 1. {
            return None;
        }
        let ix = self.shown_index().filter(|&i| self.tabs[i].asset.is_none())?;
        let pane = self.source_pane(ix, window, cx);
        let theme = cx.theme().clone();
        let shown = self.source_t();
        let margin = f32::from(SHEET_MARGIN);
        // How far a sheet's edge goes to be out of sight, its corner too.
        let gone = margin + SOURCE_ROUND + 1.;
        let (at, most) = pane.read(cx).state.read(cx).scroll_room();
        let (at, most) = (f32::from(at), f32::from(most));
        // The words' top: under the margin and the sheet's own room at the
        // file's start, at the page's top once that much is scrolled.
        let head = (margin + SOURCE_HEAD - at).max(0.);
        let top = margin - gone * (1. - head / (margin + SOURCE_HEAD));
        // What is left to scroll under the foot, with the head as it is
        // now: the editor measured it with the last draw's.
        let was = pane.read(cx).head.replace(head);
        let left = (most - at + if was < 0. { 0. } else { head - was }).max(0.);
        let bottom = margin - left.min(gone);
        if was >= 0. && (was - head).abs() > 0.5 {
            // The editor lays out at its new height in this draw; the foot
            // goes by that at the next.
            window.request_animation_frame();
        }
        // The page's own fades at its edges, by what is left to scroll
        // past each, as the note's page has them.
        let state = pane.read(cx).state.downgrade();
        let room = move |cx: &App| state.upgrade().map_or((px(0.), px(0.)), |s| s.read(cx).scroll_room());
        let ground = page_ground(&theme);
        let foot = Corners { bottom_left: CARD_R, ..Default::default() };
        Some(
            div()
                .w(px(w))
                .h_full()
                .flex_shrink_0()
                .overflow_hidden()
                .opacity(shown)
                .child(
                    div()
                        .w(px(full_w))
                        .h_full()
                        .relative()
                        .child(
                            div()
                                .absolute()
                                .left(SHEET_MARGIN)
                                .right_0()
                                .top(px(top))
                                .bottom(px(bottom))
                                .rounded(px(SOURCE_ROUND))
                                .border_1()
                                .border_color(theme.border)
                                .bg(crate::look::source_ground(&theme))
                                .overflow_hidden()
                                // A press in it is its own, not the margin's.
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                // The editor from the words' top to the page's
                                // foot, whatever the sheet's edges: placed
                                // from the sheet's inside, past its border.
                                .child(div().absolute().left_0().right_0().top(px(head - top - 1.)).bottom(px(-bottom - 1.)).child(pane)),
                        )
                        .child(edge_fade_by(room.clone(), ground, true, Corners::default()))
                        .child(edge_fade_by(room, ground, false, foot)),
                )
                .into_any_element(),
        )
    }

    /// The edge between the source and the note, which takes a drag as
    /// the sidebar's does: the source as wide as where the pointer is,
    /// between its least and the note's; a double click shares the page
    /// half and half. A line of the accent lights on it under the pointer.
    fn render_source_grip(&self, x: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let line = cx.theme().primary.opacity(0.55);
        let held = self.source_drag;
        div()
            .id("source-grip")
            .group("source-grip")
            .absolute()
            .top(SHEET_MARGIN)
            .bottom(SHEET_MARGIN)
            .left(px(x + self.page_pad.0 / 2.) - SIDEBAR_GRIP / 2.)
            .w(SIDEBAR_GRIP)
            .occlude()
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    swallow_click(window, cx);
                    if ev.click_count == 2 {
                        this.source_drag = false;
                        this.source_share = 0.5;
                        this.save_ui(window);
                    } else {
                        this.source_drag = true;
                    }
                    cx.notify();
                }),
            )
            .child(div().absolute().top(CARD_R).bottom(CARD_R).left(SIDEBAR_GRIP / 2. - px(1.)).w(px(2.)).rounded(px(1.)).when(held, |d| d.bg(line)).group_hover("source-grip", move |s| s.bg(line)))
            .when(held, |d| d.child(canvas(|_, _, _| (), |_, _, window, _| window.set_window_cursor_style(CursorStyle::ResizeLeftRight)).absolute().size_0()))
    }

    /// The pointer moved with the source's edge held.
    fn source_drag_to(&mut self, e: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            self.source_drag_end(window, cx);
            return;
        }
        // The share is of the room beside the terminal, as the pane is
        // drawn (`source_full_w(rest)`): of the whole card, the edge stood
        // left of the pointer by the terminal's share.
        let room = self.source_room;
        if room >= SOURCE_LEAST + NOTE_LEAST {
            let x = f32::from(e.position.x - self.card_x) - self.page_pad.0 / 2.;
            self.source_share = x.clamp(SOURCE_LEAST, room - NOTE_LEAST) / room;
            cx.notify();
        }
    }

    fn source_drag_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.source_drag) {
            self.save_ui(window);
            cx.notify();
        }
    }

    /// The sheet's width at a level, in the room between the page's
    /// margins or its gaps to the panes beside it, and never over that
    /// room. A width over it, left to `max_w_full` to cut, was laid out
    /// first as it was asked and then again as the room cut it: beside
    /// the source pane the whole note was laid out twice at every frame,
    /// each layout taking the other's place in the editor's one kept
    /// layout (`docs/optimize.md`).
    fn sheet_w(&self, level: f32) -> f32 {
        let room = self.page_w - self.page_pad.0 - self.page_pad.1;
        sheet_width_in(level, room).min(room.max(0.))
    }

    /// Where the sheet stands from its own width (0) to the page's (1):
    /// full while a pane stands beside it, moving as the pane does.
    fn width_level(&self) -> f32 {
        let note = self.shown_tab().is_some_and(|t| t.asset.is_none());
        let source = if note { self.source_t() } else { 0. };
        source.max(self.term_t())
    }

    /// The terminal at the page's right, or put away: ⌘⇧S, the View menu
    /// and the strip's button. The keyboard goes with it, and back to the
    /// note as it goes.
    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // With no vault there is no folder to open one in; one open still
        // goes away.
        let root = self.vault_root().map(Path::to_path_buf);
        if (root.is_none() && !self.term_on) || cfg!(target_os = "ios") {
            return;
        }
        let was = self.panes_now();
        // Put away for the source in a card too narrow for both, it comes
        // back in the source's place.
        let back = self.term_on && !self.term_shown();
        if !back {
            self.term_on = !self.term_on;
        }
        self.term_latest = true;
        self.panes_moved(was);
        let term = self.term_panel(window, cx);
        if back {
            window.focus(&term.focus_handle(cx), cx);
        } else if self.term_on {
            term.update(cx, |t, cx| {
                if let Some(root) = root {
                    t.set_cwd(root);
                }
                t.set_shown(true, cx);
            });
            window.focus(&term.focus_handle(cx), cx);
        } else {
            let had = term.focus_handle(cx).contains_focused(window, cx);
            term.update(cx, |t, cx| t.set_shown(false, cx));
            if had {
                match self.shown_editor() {
                    Some(e) => window.focus(&e.focus_handle(cx), cx),
                    None => window.focus(&self.focus, cx),
                }
            }
            // The terminal left is drawn while it goes: once more when it has.
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SLIDE + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        cx.notify();
    }

    /// The terminal, made the first time it is asked for: its menu is the
    /// window's, and ⌘W with no tab left in it puts it away.
    fn term_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<TermPanel> {
        if let Some(t) = &self.terminal {
            return t.clone();
        }
        let this = cx.entity().downgrade();
        let menu: crate::terminal::TermMenu = Rc::new(move |at, items, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.menu_focus = None;
                this.menus.show("terminal", at, items);
                cx.notify();
            });
        });
        let root = self.vault_root().map(Path::to_path_buf).unwrap_or_else(onyx_core::paths::home);
        let (cfg, mono) = (self.cfg.terminal.clone(), self.look.mono_family.clone());
        let term = cx.new(|cx| TermPanel::new(root, cfg, mono, menu, cx));
        self._term_sub = Some(cx.subscribe_in(&term, window, |this: &mut Self, _, ev: &TermEvent, window, cx| match ev {
            TermEvent::Away => {
                if this.term_on {
                    this.toggle_terminal(window, cx);
                }
            }
        }));
        self.terminal = Some(term.clone());
        term
    }

    /// How far the terminal is out, 0 to 1, eased on the window's clock.
    fn term_t(&self) -> f32 {
        let to = if self.term_shown() { 1. } else { 0. };
        match self.term_anim {
            Some((from, at, _)) if at.elapsed() < SLIDE => from + (to - from) * ease_out_quint()(at.elapsed().as_secs_f32() / SLIDE.as_secs_f32()),
            _ => to,
        }
    }

    /// Whether the source and the terminal both fit beside the note, each
    /// at its least.
    fn panes_fit(&self) -> bool {
        self.card_w >= crate::terminal::TERM_LEAST + SOURCE_LEAST + NOTE_LEAST
    }

    /// Whether the source pane shows: asked for, on a note, and not put
    /// away for the terminal asked for after it in a card too narrow for
    /// both, as Emaki's panels give way. It comes back when there is room;
    /// its button says what shows.
    fn source_shown(&self) -> bool {
        let note = self.shown_tab().is_some_and(|t| t.asset.is_none());
        self.source_on && note && !(self.term_on && self.term_latest && !self.panes_fit())
    }

    /// Whether the terminal shows: asked for, and not put away for the
    /// source asked for after it in a card too narrow for both.
    fn term_shown(&self) -> bool {
        let note = self.shown_tab().is_some_and(|t| t.asset.is_none());
        self.term_on && !(self.source_on && note && !self.term_latest && !self.panes_fit())
    }

    /// What shows of the two, and how far out each is, before a change.
    fn panes_now(&self) -> ((bool, f32), (bool, f32)) {
        ((self.source_shown(), self.source_t()), (self.term_shown(), self.term_t()))
    }

    /// After a change, each pane whose showing changed comes or goes from
    /// where it stood.
    fn panes_moved(&mut self, ((source, s_at), (term, t_at)): ((bool, f32), (bool, f32))) {
        let now = Instant::now();
        if self.source_shown() != source {
            self.source_anim = Some((s_at, now, self.source_anim.map_or(0, |(_, _, n)| n + 1)));
        }
        if self.term_shown() != term {
            self.term_anim = Some((t_at, now, self.term_anim.map_or(0, |(_, _, n)| n + 1)));
        }
    }

    /// The source and the terminal put away together, each from where it
    /// stood, the terminal's shells kept for when it comes back.
    fn put_panes_away(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.source_on && !self.term_on {
            return;
        }
        let was = self.panes_now();
        self.source_on = false;
        self.term_on = false;
        self.panes_moved(was);
        if let Some(term) = self.terminal.clone() {
            term.update(cx, |t, cx| t.set_shown(false, cx));
        }
        self.save_ui(window);
        cx.notify();
    }

    fn term_moving(&self) -> bool {
        self.term_anim.is_some_and(|(_, at, _)| at.elapsed() < SLIDE)
    }

    /// The terminal's width when all the way out, in a card this wide: its
    /// own, leaving the note and the source their least; in a card too
    /// narrow for them all, a share as the others have.
    fn term_full_w(&self, card_w: f32) -> f32 {
        let source = self.source_shown();
        let others = NOTE_LEAST + if source { SOURCE_LEAST } else { 0. };
        if card_w < crate::terminal::TERM_LEAST + others {
            return (card_w / if source { 3. } else { 2. }).round();
        }
        self.term_w.clamp(crate::terminal::TERM_LEAST, card_w - others).round()
    }

    /// The terminal at the page's right, `w` wide at this draw: a sheet of
    /// its own ground, full height, at the width it has all the way out,
    /// so its screen keeps its columns as it slides.
    fn render_terminal(&mut self, w: f32, full_w: f32, cx: &mut Context<Self>) -> Option<AnyElement> {
        if w < 1. {
            return None;
        }
        let term = self.terminal.clone()?;
        let theme = cx.theme().clone();
        let ground = term.read(cx).ground(cx);
        Some(
            div()
                .w(px(w))
                .h_full()
                .flex_shrink_0()
                .overflow_hidden()
                .opacity(self.term_t())
                .child(
                    div().w(px(full_w)).h_full().pr(SHEET_MARGIN).py(SHEET_MARGIN).child(
                        div()
                            .size_full()
                            .rounded(px(crate::terminal::SHEET_R))
                            .border_1()
                            .border_color(theme.border)
                            .bg(ground)
                            .overflow_hidden()
                            // A press in it is its own, not the margin's.
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(term),
                    ),
                )
                .into_any_element(),
        )
    }

    /// The edge between the note and the terminal, as the source's: the
    /// terminal as wide as from the pointer to the card's edge; a double
    /// click puts its width back.
    fn render_term_grip(&self, x: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let line = cx.theme().primary.opacity(0.55);
        let held = self.term_drag;
        div()
            .id("term-grip")
            .group("term-grip")
            .absolute()
            .top(SHEET_MARGIN)
            .bottom(SHEET_MARGIN)
            .left(px(x - self.page_pad.1 / 2.) - SIDEBAR_GRIP / 2.)
            .w(SIDEBAR_GRIP)
            .occlude()
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    swallow_click(window, cx);
                    if ev.click_count == 2 {
                        this.term_drag = false;
                        this.term_w = crate::terminal::TERM_W;
                        this.save_ui(window);
                    } else {
                        this.term_drag = true;
                    }
                    cx.notify();
                }),
            )
            .child(div().absolute().top(CARD_R).bottom(CARD_R).left(SIDEBAR_GRIP / 2. - px(1.)).w(px(2.)).rounded(px(1.)).when(held, |d| d.bg(line)).group_hover("term-grip", move |s| s.bg(line)))
            .when(held, |d| d.child(canvas(|_, _, _| (), |_, _, window, _| window.set_window_cursor_style(CursorStyle::ResizeLeftRight)).absolute().size_0()))
    }

    /// The pointer moved with the terminal's edge held.
    fn term_drag_to(&mut self, e: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if e.pressed_button != Some(MouseButton::Left) {
            if std::mem::take(&mut self.term_drag) {
                self.save_ui(window);
                cx.notify();
            }
            return;
        }
        let w = f32::from(self.card_x) + self.card_w - f32::from(e.position.x) - self.page_pad.1 / 2.;
        let source = self.source_shown();
        let most = self.card_w - NOTE_LEAST - if source { SOURCE_LEAST } else { 0. };
        if most >= crate::terminal::TERM_LEAST {
            self.term_w = w.clamp(crate::terminal::TERM_LEAST, most);
            cx.notify();
        }
    }

    // -- the page ---------------------------------------------------------------

    /// The words over a page with nothing open (the greeting, what to do
    /// first), in the note's face with its CJK face behind it. In a
    /// compact window they are smaller, to stay on a line, and where a
    /// long language still takes two, each line is centered and they
    /// stand close.
    pub(crate) fn page_words(&self, words: impl Into<SharedString>) -> Div {
        let words = div().max_w_full().text_center().font(self.look.words()).child(words.into());
        if self.compact { words.text_size(px(26.)).line_height(relative(1.3)) } else { words.text_size(px(34.)) }
    }

    /// A vault with no note open.
    /// The vault's page while no note is open, as emaki's home page is
    /// made and Obsidian's new tab is filled: the drawing, the greeting in
    /// the note's face, and under them what can be done next, each on a
    /// pill with its key at the right.
    fn render_no_note(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        // In a compact window each is at a finger's size.
        let compact = self.compact;
        let choice = |id: &'static str, icon: &'static str, label: &str, keys: &str, theme: &gpui_component::Theme| {
            let pill = choice_pill(id, icon, label.to_string(), Press::Quiet, false, theme);
            let pill = if compact { pill.min_w(px(280.)).h(px(44.)).px(px(18.)).gap(px(10.)).text_size(px(15.)) } else { pill.min_w(px(240.)).h(px(34.)).px(px(14.)).gap(px(8.)).text_size(px(13.)) };
            pill.child(div().flex_1()).child(kbd_hint(keys, theme))
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .pb(px(48.))
            .gap(px(26.))
            .child(v_flex().max_w_full().px(px(24.)).items_center().gap(px(12.)).child(Self::home_figure(&self.home_fig, &theme, cx.entity_id())).child(self.page_words(greeting(&self.user_name))))
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
                    }),
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
    /// Drawn by the vault's page and by the menu bar note's landing page
    /// (`menubar.rs`), each with its own spring, and `view` redrawn as the
    /// pointer moves it.
    pub(crate) fn home_figure(home_fig: &Rc<RefCell<Fig>>, theme: &gpui_component::Theme, view: EntityId) -> impl IntoElement {
        const W: f32 = 220.;
        const H: f32 = 124.;
        const PEN: f32 = 64.;
        let (wind, lift, near, moving) = {
            let mut fig = home_fig.borrow_mut();
            let moving = fig.step();
            let (wind, lift, near) = fig.at();
            (wind, lift, near, moving)
        };
        let c = |hex: u32| -> Hsla { rgb(hex).into() };
        let (deep, glow, teal, pages, paper, line, mark) = (c(0x171431), c(0x4A4588), c(0x6FB9C4), c(0xDAD8CF), c(0xF8F7F2), c(0x474747), c(0xE8A93A));
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
        let fig = home_fig.clone();
        let (moved, left) = (home_fig.clone(), home_fig.clone());
        div()
            .id("home-fig")
            .relative()
            .w(px(W))
            .h(px(H))
            .flex_shrink_0()
            .on_mouse_move(move |e: &MouseMoveEvent, _, cx| {
                moved.borrow_mut().ask(Some(e.position));
                cx.notify(view);
            })
            .on_hover(move |over: &bool, _, cx| {
                if !*over {
                    left.borrow_mut().ask(None);
                    cx.notify(view);
                }
            })
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
            // The stone, the icon's plump pebble lit from within: a teal
            // glow on the page under it, the stone in its deepest navy, a
            // lighter navy rising to the far shoulder, the teal caught in
            // its lower edge, and light along its rim. Each layer's shading
            // is its picture's alpha; its color is given here.
            .child(layer("icons/home/stone-glow.svg", teal))
            .child(layer("icons/home/stone-base.svg", deep))
            .child(layer("icons/home/stone-lift.svg", glow))
            .child(layer("icons/home/stone-rim.svg", teal))
            .child(layer("icons/home/stone-edge.svg", gpui::white()))
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
        let sheet_w = self.sheet_w(level);
        let gutter = self.sheet_gutter();
        // A compact window's note is the page itself: no sheet's edge or
        // corners, and room at its foot for what stands over it there.
        let compact = self.compact;
        let foot_room = self.foot_room();
        let ground = if compact { theme.background } else { ground };
        editor.update(cx, |e, _| e.fill_page((line * 4. - gutter).max(0.)));
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
                    v_flex().min_h_full().py(px(self.sheet_margin())).pl(px(self.page_pad.0)).pr(px(self.page_pad.1)).child(
                        v_flex()
                            .flex_1()
                            .w(px(sheet_w))
                            .max_w_full()
                            .mx_auto()
                            .when(!compact, |d| d.rounded(px(10.)).border_1().border_color(theme.border))
                            .bg(theme.background)
                            .px(px(gutter))
                            .pt(px(if compact { (line * 0.5).round() } else { line }))
                            // The editor fills the sheet from its first line
                            // down, the room under the words its own
                            // (`Editor::fill_page`), so what takes a press is
                            // one rectangle, the gutter round it.
                            .pb(px(gutter + foot_room))
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
            .child(edge_fade(&scroll, ground, false, if compact { px(0.) } else { CARD_R }))
            .vertical_scrollbar(&scroll)
            .into_any_element()
    }

    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.sync_players(window);
        if self.vault.is_none() {
            return page_in("place", div().size_full().child(self.render_place_page(cx)));
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
                .children(crate::widgets::reaches(theme.overlay))
                .flex()
                .flex_col()
                .items_center()
                .pt(px(if self.compact { 96. } else { 140. }))
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
                        .p(px(hand(16.)))
                        .gap(px(hand(10.)))
                        .rounded(px(hand(18.)))
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.border)
                        .shadow(float_shadow(&theme))
                        .child(div().text_size(px(hand(13.))).font_weight(FontWeight::SEMIBOLD).child(title))
                        .children(body)
                        // The line of keys is for a keyboard: a compact
                        // window, held in a hand, has the two buttons.
                        .when(!self.compact, |d| d.child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(key_text(&hint)))),
                ),
        )
        .into_any_element()
    }

    /// A field on a sheet: the toolkit's input on a plate, keyed to the
    /// window it was made in.
    fn sheet_field(id: &'static str, input: &Entity<InputState>, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let focus = input.read(cx).focus_handle(cx);
        div().id(id).track_focus(&focus).px(px(hand(10.))).h(px(hand(36.))).flex().items_center().rounded(px(hand(8.))).bg(theme.muted).text_size(px(hand(14.))).child(Input::new(input).appearance(false).text_size(px(hand(14.))).on_secondary_click(Self::input_menu(input, "edit", cx))).into_any_element()
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
        let title = if self.vault_root() == Some(path.as_path()) {
            t("Rename Vault")
        } else if path.is_dir() {
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

    /// A new vault: its name in a field, Cancel and Create; ↩ makes it
    /// in the container and shows it.
    fn render_new_vault(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.new_vault.as_ref()?;
        let theme = cx.theme().clone();
        let body = vec![
            Self::sheet_field("vault-field", &self.vault_input, &theme, cx),
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
            None => self.start_new_vault(window, cx),
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
        self.update.outcome = None;
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
                            this.update.outcome = Some(Outcome::UpToDate);
                        }
                    }
                    Err(e) => this.update.outcome = Some(Outcome::CheckFailed(tf("The check did not go through: {why}", &[("why", &e)]))),
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
        self.update.outcome = None;
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
                    Err(e) => this.update.outcome = Some(Outcome::InstallFailed(tf("The update did not go through: {e}", &[("e", &e)]))),
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

/// The sheet's width at a level from its own (0) to full width (1) on a
/// page this wide.
#[cfg(test)]
fn sheet_width(level: f32, page: f32) -> f32 {
    sheet_width_in(level, page - 2. * f32::from(SHEET_MARGIN))
}

/// The same in the room the page leaves the sheet, its margins or gaps
/// taken off already.
fn sheet_width_in(level: f32, room: f32) -> f32 {
    let wide = room.max(READ_W);
    let own = ((READ_W + wide) / 2.).min(SHEET_MOST);
    own + (wide - own) * level
}

/// A name typed in a field that wraps, as a file's name: its lines joined
/// by a space, and no space at its ends.
pub(crate) fn one_line(typed: &str) -> String {
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
    edge_fade_at(handle, ground, top, Corners { bottom_right: corner, bottom_left: corner, ..Default::default() })
}

/// An edge's fade with its own corners: where a scroller's foot is a
/// window's corner on one side only, the fade is rounded there alone, or
/// it paints the square the corner cuts away.
pub(crate) fn edge_fade_at(handle: &ScrollHandle, ground: Hsla, top: bool, corners: Corners<Pixels>) -> AnyElement {
    let handle = handle.clone();
    edge_fade_by(move |_| (-handle.offset().y, handle.max_offset().y), ground, top, corners)
}

/// An edge's fade over a scroller that keeps its own offset (the source
/// pane's editor): `room` says how far it is scrolled down and how far it
/// can go.
pub(crate) fn edge_fade_by(room: impl Fn(&App) -> (Pixels, Pixels) + 'static, ground: Hsla, top: bool, corners: Corners<Pixels>) -> AnyElement {
    canvas(
        move |bounds, window, cx| {
            let (at, most) = room(cx);
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
            window.paint_quad(fill(bounds, linear_gradient(180., linear_color_stop(from, 0.), linear_color_stop(to, 1.))).corner_radii(corners));
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
impl crate::tabs::TabHost for Shell {
    fn tabs_row(&mut self) -> &mut crate::tabs::Tabs {
        &mut self.tabs_row
    }

    fn tab_press(&mut self) {
        self.press_taken = true;
    }

    fn tabs_reorder(&mut self, order: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        self.reorder_tabs(order, window, cx);
    }

    fn tab_show(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.show_tab(path, window, cx);
    }

    fn tab_close(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(path, window, cx);
    }

    fn tab_menu(&mut self, path: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.show_entry_menu(path, at, cx);
    }

    fn pill_menu(&mut self, folder: PathBuf, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.show_pill_menu(folder, at, cx);
    }

    fn tab_plus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_note(&NewNote, window, cx);
    }
}

pub(crate) fn greeting(name: &str) -> String {
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
        let moves = std::mem::take(&mut self.pending_moves);
        if !moves.is_empty() {
            self.follow_moves(&moves, window, cx);
        }
        crate::editor::stamp("shell render begins");
        self.scrollers.begin();
        // The router is the window's being drawn: the menu bar note's
        // window has scrollers of its own (`menubar.rs`), and a frame names
        // itself to whichever window draws it.
        cx.set_global(crate::scroll::Router(self.scrollers.clone()));
        let theme = cx.theme().clone();
        let frame = theme.title_bar;
        if self.ground.get() != Some(frame) {
            self.ground.set(Some(frame));
            crate::sys::set_window_ground(window, frame.to_rgb());
        }
        if self.sidebar_moving() || self.source_moving() || self.term_moving() {
            window.request_animation_frame();
        }
        // The window less what the system keeps of it, and whether the
        // sidebar and the note are pages of it (`compact`).
        let insets = window.insets();
        self.safe = Edges { bottom: insets.safe_area.bottom.max(insets.ime.bottom), ..insets.safe_area };
        self.keys_out = insets.ime.bottom > insets.safe_area.bottom;
        let whole = window.viewport_size();
        self.room = size(whole.width - self.safe.left - self.safe.right, whole.height - self.safe.top - self.safe.bottom);
        let compact = self.room.width < MIN_W;
        // A compact window with no vault begins on the page that says what
        // to do, not on a list with nothing in it.
        if compact && !self.compact && self.vault.is_none() && self.side_anim.is_none() {
            self.sidebar_open = false;
        }
        self.compact = compact;
        crate::widgets::set_in_hand(compact);
        // A compact window's page is the whole screen, on its own
        // ground under the status bar and the home indicator.
        let (reach_top, reach_foot) = if compact { (self.safe.top, self.foot()) } else { (px(0.), px(0.)) };
        crate::widgets::set_reach(reach_top, reach_foot);
        let t_side = self.side_t();
        // The status bar's ink: light on the frame, which the sidebar's
        // page and every wider window have under it, and on a dark page.
        crate::sys::status_bar(!compact || cx.theme().mode.is_dark() || (t_side > 0.5 && !self.settings_in));
        // The sidebar's room in the row, and the lights' place: in gpui's
        // row on the sidebar's title row, a column at the rail's head.
        let side_w = (self.sidebar_width() * t_side).round();
        crate::sys::fold_lights(window, t_side, f32::from(RAIL_W / 2.));
        let side_shown = ((t_side - 0.375) / 0.625).clamp(0., 1.);
        // The tabs, their widths set for this draw; a frame more while
        // any of them moves.
        let items: Vec<TabItem> = self.tabs.iter().map(|t| TabItem { path: t.path.clone(), title: Self::tab_title(&t.path) }).collect();
        let card_x = if compact { side_w } else { side_w.max(RAIL_W) };
        self.card_x = card_x;
        // A compact window's card is the window's width whatever the
        // sidebar's page covers of it: it slides, and is never squeezed.
        self.card_w = f32::from(if compact { self.room.width } else { self.room.width - card_x - FRAME });
        // The source pane's room, as far out as it is, a note's only.
        // A compact window has no room for it: a pane left out there shows no button to put it away by.
        let note = self.shown_tab().is_some_and(|t| t.asset.is_none()) && !compact;
        // The terminal's room at the right, as far out as it is.
        let term_full = self.term_full_w(self.card_w);
        let term_w = if self.terminal.is_some() { (term_full * self.term_t()).round() } else { 0. };
        let rest = self.card_w - term_w;
        self.source_room = rest;
        let source_full = self.source_full_w(rest);
        let source_w = if note { (source_full * self.source_t()).round() } else { 0. };
        self.page_w = rest - source_w;
        let margin = self.sheet_margin();
        let ease = |t: f32| margin - (margin - PANE_GAP.min(margin)) * t;
        self.page_pad = (if note { ease(self.source_t()) } else { margin }, if self.terminal.is_some() { ease(self.term_t()) } else { margin });
        let strip_inner = self.card_w - f32::from(STRIP_END * 2. + crate::csd::room(window, cx).1 + (self.toggle_w() + STRIP_END_GAP) * 2.);
        self.tabs_row.sync(strip_inner - crate::tabs::STRIP_GAP * 2., &items, self.shown.as_deref(), window, cx);
        if self.tabs_row.moving() {
            window.request_animation_frame();
        }
        let root = self.vault_root().map(Path::to_path_buf);
        // No tab, no row: the plus alone on the strip said nothing.
        let path_bar = self.render_path_bar(crate::csd::room(window, cx).1, cx);
        // A compact window has no tabs' row: one file shows at a time, and
        // where it is, the path, is the title row's middle.
        let (center, path_bar) = if compact {
            (div().flex_1().min_w_0().children(path_bar).into_any_element(), None)
        } else if items.is_empty() {
            (div().flex_1().min_w_0().into_any_element(), path_bar)
        } else {
            (self.tabs_row.render(&items, self.shown.as_deref(), root.as_deref(), cx), path_bar)
        };
        let (modes, widths) = self.render_strip_ends(cx);
        let notice = self.render_notice(&theme, cx);
        let find_bar = (self.find_open && self.shown.is_some()).then(|| {
            div().w_full().flex_shrink_0().overflow_hidden().child(self.render_find_bar(cx)).with_animation("find-bar-in", Animation::new(crate::widgets::MENU_IN).with_easing(ease_out_quint()), |d, t| if t >= 1. { d } else { d.max_h(px(64. * t)).opacity(t) })
        });
        let page = self.render_page(window, cx);
        let source = self.render_source(source_w, source_full, window, cx);
        let source_grip = (source.is_some() && self.source_shown()).then(|| self.render_source_grip(source_w, cx));
        let terminal = self.render_terminal(term_w, term_full, cx);
        let term_grip = (terminal.is_some() && self.term_shown()).then(|| self.render_term_grip(rest, cx));
        // Under the tabs' row the card is the page's tint, one ground for
        // the path bar, the find bar and the page, with no line between
        // them: the sheet's edge is the one line below the tabs. The card
        // is its two grounds and paints none of its own, each rounding its
        // two corners: `overflow_hidden` clips to a square, so nothing in
        // the card rounds a corner for it.
        let foot_ground = if compact && self.vault.is_some() && self.shown_tab().is_some_and(|t| t.asset.as_ref().is_none_or(|a| a.reads_as_words())) { theme.background } else { page_ground(&theme) };
        let card = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .text_color(theme.foreground)
            .overflow_hidden()
            .child(Self::drag_region(h_flex().h(if compact { HAND_TITLE_H + reach_top } else { TITLEBAR_H - FRAME }).pt(reach_top).flex_shrink_0().pl(STRIP_END).pr(STRIP_END + crate::csd::room(window, cx).1).items_center().gap(STRIP_END_GAP).when(!compact, |d| d.rounded_t(CARD_R)).bg(theme.background), cx).child(modes).child(center).child(widths))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .when(!compact, |d| d.rounded_b(CARD_R))
                    // A compact window's note is its paper from the tabs
                    // to the screen's foot, with no sheet laid on a page.
                    .bg(foot_ground)
                    .children(path_bar)
                    .child(
                        // The source at the left, the note beside it.
                        h_flex()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .items_stretch()
                            .children(source)
                            .child(v_flex().flex_1().min_w_0().h_full().children(find_bar).child(div().relative().flex_1().min_h_0().child(page)))
                            .children(terminal)
                            .children(source_grip)
                            .children(term_grip)
                            .children(notice),
                    ),
            );
        let pick = Rc::new(|this: &mut Self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>| this.menu_pick(what, id, window, cx));
        let close = Rc::new(|this: &mut Self, cx: &mut Context<Self>| {
            this.pressed = None;
            this.menus.close(cx);
            cx.notify();
        });
        // The settings window's are drawn there.
        let ours = |m: &crate::menu::Menu| !crate::settings::menu_is_ours(m.what);
        // Over the window nothing under it scrolls (`scroll.rs`).
        if self.menus.open.as_ref().is_some_and(ours) || self.new_vault.is_some() || self.rename.is_some() || self.export.is_some() || self.search_open || self.viewer.is_some() || self.settings_in {
            self.scrollers.over();
        }
        if self.settings_in {
            self.scrollers.name_over(&self.settings_scroll);
        }
        if let Some((picker, _)) = &self.folder_look {
            self.scrollers.name_over(&picker.read(cx).scroll);
        }
        let menu = if self.menus.open.as_ref().is_some_and(ours) || self.menus.gone.as_ref().is_some_and(|(m, _)| ours(m)) { self.menus.render(pick, close, cx) } else { None };
        let sheet = self.render_new_vault(cx).or_else(|| self.render_rename(cx)).or_else(|| self.render_export(window, cx));
        let search = self.search_open.then(|| self.render_search(window, cx));
        let viewer = self.render_viewer(window, cx);
        let settings = self.render_settings_sheet(window, cx);
        let rail = self.render_rail(cx);
        let fly = self.render_rail_fly(window, cx);
        let strip = self.render_strip(cx);
        let this = cx.entity().downgrade();
        let ambient = self.ambient.clone();
        // The sidebar's column: it grows from nothing to its width while
        // the sidebar itself stays put and fades in over the last part of
        // the way. In a compact window it is a page: it comes whole from
        // the left with its right edge on the note's page, which it pushes
        // off the window.
        let side = (t_side > 0.).then(|| {
            let sidebar = div().h_full().w(self.sidebar_width()).flex_shrink_0().when(!compact, |d| d.opacity(side_shown)).child(self.render_sidebar(cx));
            div().h_full().flex_shrink_0().w(side_w).overflow_hidden().when(compact, |d| d.flex().justify_end().pt(reach_top).pb(reach_foot)).child(sidebar)
        });
        let foot_bar = self.render_foot_bar(cx);
        // Beside the drawer the page is darkened, and a press on what is
        // left of it puts the drawer away.
        let dim = (compact && t_side > 0.).then(|| {
            div().id("page-dim").absolute().inset_0().occlude().bg(gpui::black().opacity(DRAWER_DIM * t_side)).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                if this.sidebar_open {
                    this.toggle_sidebar(window, cx);
                }
            }))
        });
        let page = if compact {
            div().relative().w(self.room.width).flex_shrink_0().h_full().on_back_gesture(cx.listener(Self::side_pulled)).child(card).children(foot_bar).children(dim)
        } else {
            div().flex_1().min_w_0().h_full().pt(FRAME).pb(FRAME).pr(FRAME).pl((RAIL_W - side_w).max(px(0.))).child(card)
        };
        let safe = self.safe;
        crate::editor::stamp("shell render ends");
        let shell = div()
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
            .on_action(cx.listener(|this, _: &CloseVault, window, cx| this.close_vault(window, cx)))
            .on_action(cx.listener(|this, _: &ChooseContainer, window, cx| this.choose_container(window, cx)))
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
            // The files' keys, while the keyboard is in no words: ⌘⌫ puts
            // the file chosen in the trash, ⌘Z takes the last thing done
            // to the files back and ⌘⇧Z does it again. Anywhere else each
            // goes on to whatever has the keyboard.
            .on_action(cx.listener(|this, _: &tree::TreeTrash, window, cx| if this.files_keys(window) { this.trash_chosen(window, cx) } else { cx.propagate() }))
            .on_action(cx.listener(|this, _: &gpui_component::input::Undo, window, cx| if this.files_keys(window) { this.files_step(false, window, cx) } else { cx.propagate() }))
            .on_action(cx.listener(|this, _: &gpui_component::input::Redo, window, cx| if this.files_keys(window) { this.files_step(true, window, cx) } else { cx.propagate() }))
            .on_action(cx.listener(|this, _: &Tab1, window, cx| this.go_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &Tab2, window, cx| this.go_tab(2, window, cx)))
            .on_action(cx.listener(|this, _: &Tab3, window, cx| this.go_tab(3, window, cx)))
            .on_action(cx.listener(|this, _: &Tab4, window, cx| this.go_tab(4, window, cx)))
            .on_action(cx.listener(|this, _: &Tab5, window, cx| this.go_tab(5, window, cx)))
            .on_action(cx.listener(|this, _: &Tab6, window, cx| this.go_tab(6, window, cx)))
            .on_action(cx.listener(|this, _: &Tab7, window, cx| this.go_tab(7, window, cx)))
            .on_action(cx.listener(|this, _: &Tab8, window, cx| this.go_tab(8, window, cx)))
            .on_action(cx.listener(|this, _: &Tab9, window, cx| this.go_tab(9, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleSource, window, cx| this.toggle_source(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleTerminal, window, cx| this.toggle_terminal(window, cx)))
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
                                    if s.source_drag {
                                        s.source_drag_to(e, window, cx);
                                    }
                                    if s.term_drag {
                                        s.term_drag_to(e, window, cx);
                                    }
                                    s.side_drag || s.source_drag || s.term_drag
                                });
                                if held.unwrap_or(false) {
                                    cx.stop_propagation();
                                }
                            }
                        });
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                let _ = lifter.update(cx, |s, cx| {
                                    s.side_drag_end(window, cx);
                                    s.source_drag_end(window, cx);
                                });
                            }
                        });
                        window.on_mouse_event(move |e: &ScrollEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                let system = crate::sys::sheet_over(window);
                                match router.update(cx, |s, _| s.scrollers.route(e, system)) {
                                    Ok(crate::scroll::Route::Stop) => {
                                        cx.stop_propagation();
                                        let _ = router.update(cx, |_, cx| cx.notify());
                                    }
                                    Ok(crate::scroll::Route::Give(wheel)) => {
                                        cx.stop_propagation();
                                        wheel(e, window, cx);
                                    }
                                    _ => {}
                                }
                            }
                        });
                        // A finger held still is the right button: every
                        // menu a right click opens, a finger opens there. It
                        // is claimed as it begins, so lifting the finger is
                        // no tap, and the press is sent once this one is done.
                        window.on_mouse_event(move |e: &LongPressEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble && e.phase == TouchPhase::Started && !window.default_prevented() {
                                window.prevent_default();
                                let (position, modifiers) = (e.start_position, Modifiers::default());
                                window.defer(cx, move |window, cx| {
                                    window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Right, position, modifiers, click_count: 1, first_mouse: false }), cx);
                                    window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Right, position, modifiers, click_count: 1 }), cx);
                                });
                            }
                        });
                        crate::ambient::after_frame(&ambient, window, cx);
                    },
                )
                .absolute()
                .size_0(),
            )
            // The card's left edge waits at the rail's until the sidebar's
            // column is past it.
            // A compact window's two pages are as tall as the screen, past
            // the room at its top and its foot.
            .child(h_flex().items_stretch().map(|d| if compact { d.absolute().left_0().right_0().top(-reach_top).bottom(-reach_foot).overflow_hidden() } else { d.size_full() }).children(side).child(page))
            .children(rail)
            .when(!compact, |d| d.child(self.render_side_grip(card_x, cx)))
            .children(fly)
            .child(strip)
            .children(crate::csd::chrome(window, px(1.) + FRAME / 2. + TITLEBAR_H / 2. - px(12.), |window, _| window.remove_window(), cx))
            .children(sheet)
            .children(self.folder_look.as_ref().map(|(p, _)| p.clone()))
            .children(search)
            .children(menu)
            .children(viewer)
            .children(settings)
            .child(self.ambient.clone());
        // The frame's ground under what the system keeps of the window,
        // and the window's parts in the room that is left. While the
        // keyboard is out a compact window's page ends at its top, and
        // the page's ground goes on under it: the keyboard's corners are
        // round and what is behind it shows there.
        let under_keys = (compact && self.keys_out).then(|| div().absolute().bottom_0().left(safe.left + card_x).w(self.room.width).h(safe.bottom).bg(foot_ground));
        div().relative().size_full().bg(frame).pt(safe.top).pr(safe.right).pb(safe.bottom).pl(safe.left).children(under_keys).child(shell)
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
                // `ONYX_SHOT_OF=menunote`: the menu bar note's window.
                let of_note = std::env::var("ONYX_SHOT_OF").is_ok_and(|v| v == "menunote");
                let shot = match settings {
                    _ if of_note => crate::menubar::shoot(&path, cx),
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
                s if s.starts_with("key=") => {
                    if let Ok(keystroke) = Keystroke::parse_with_os(&s["key=".len()..], cx.platform_info().os) {
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
                s if s.starts_with("press=") || s.starts_with("rpress=") || s.starts_with("drag=") || s.starts_with("altdrag=") || s.starts_with("hold=") => {
                    // `press=x,y` or `press=x,y,clicks`; `rpress=x,y`, the
                    // right button; `drag=x,y,to_x,to_y`; `altdrag=` the
                    // same with ⌥ held; `hold=` the same with the button
                    // still down, for a picture mid-drag.
                    // Once the frame it aims at is drawn, into this window
                    // alone: nothing reaches the screen.
                    let holding = s.starts_with("hold=");
                    let alt = s.starts_with("altdrag=");
                    let dragging = s.starts_with("drag=") || alt || holding;
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
                                let (button, modifiers) = (if right { MouseButton::Right } else { MouseButton::Left }, Modifiers { alt, ..Modifiers::default() });
                                window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers }), cx);
                                window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button, position, modifiers, click_count, first_mouse: false }), cx);
                                if dragging {
                                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: to, pressed_button: Some(button), modifiers }), cx);
                                }
                                if !holding {
                                    window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button, position: to, modifiers, click_count }), cx);
                                }
                            });
                        })
                        .detach();
                    }
                }
                // A finger, as an iPhone's screen sends one: `tap=x,y` or
                // `tap=x,y,taps` (a touch begun and ended there), `pan=x,y,to
                // x,to y` (begun at the first place, moved to the second in
                // ten steps and lifted, so what it scrolls goes on by its own
                // momentum), `longtap=x,y` (held for 700 ms, then lifted) and
                // `back=<how far>` (the swipe from the screen's edge over the
                // middle of the window, 0 to 1, let go there: past a half it
                // goes back). Each goes through gpui's own reading of touches,
                // into this window alone.
                s if s.starts_with("tap=") || s.starts_with("pan=") || s.starts_with("longtap=") || s.starts_with("back=") => {
                    let (what, n) = s.split_once('=').map(|(what, v)| (what.to_string(), v.split(',').filter_map(|n| n.parse::<f32>().ok()).collect::<Vec<_>>())).unwrap_or_default();
                    let wait = later;
                    later += if what == "tap" { GO_STEP } else { GO_STEP * 3 };
                    cx.spawn_in(window, async move |_, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let touch = |phase: TouchPhase, id: u64, at: Point<Pixels>| PlatformInput::Touch(TouchEvent { id: TouchId(id), phase, position: at, predicted_position: None, force: None });
                        match (what.as_str(), &n[..]) {
                            ("tap", [x, y, ..]) => {
                                let at = point(px(*x), px(*y));
                                for i in 0..n.get(2).map_or(1, |taps| *taps as u64) {
                                    let _ = cx.update(|window, cx| {
                                        window.dispatch_event(touch(TouchPhase::Started, 100 + i, at), cx);
                                        window.dispatch_event(touch(TouchPhase::Ended, 100 + i, at), cx);
                                    });
                                    cx.background_executor().timer(Duration::from_millis(60)).await;
                                }
                            }
                            ("pan", [x, y, tx, ty]) => {
                                let (from, to) = (point(px(*x), px(*y)), point(px(*tx), px(*ty)));
                                let _ = cx.update(|window, cx| window.dispatch_event(touch(TouchPhase::Started, 200, from), cx));
                                for step in 1..=10 {
                                    cx.background_executor().timer(Duration::from_millis(16)).await;
                                    let at = from + (to - from) * (step as f32 / 10.);
                                    let _ = cx.update(|window, cx| window.dispatch_event(touch(TouchPhase::Moved, 200, at), cx));
                                }
                                let _ = cx.update(|window, cx| window.dispatch_event(touch(TouchPhase::Ended, 200, to), cx));
                            }
                            ("longtap", [x, y]) => {
                                let at = point(px(*x), px(*y));
                                let _ = cx.update(|window, cx| window.dispatch_event(touch(TouchPhase::Started, 300, at), cx));
                                cx.background_executor().timer(Duration::from_millis(700)).await;
                                let _ = cx.update(|window, cx| window.dispatch_event(touch(TouchPhase::Ended, 300, at), cx));
                            }
                            ("back", [far]) => {
                                let far = far.clamp(0., 1.);
                                let _ = cx.update(|window, cx| {
                                    let size = window.viewport_size();
                                    let start_position = point(px(1.), size.height / 2.);
                                    let step = |phase: BackGesturePhase, progress: f32| PlatformInput::BackGesture(BackGestureEvent { phase, start_position, progress });
                                    window.dispatch_event(step(BackGesturePhase::Started, 0.), cx);
                                    window.dispatch_event(step(BackGesturePhase::Progressed, far), cx);
                                    window.dispatch_event(step(if far > 0.5 { BackGesturePhase::Committed } else { BackGesturePhase::Cancelled }, far), cx);
                                });
                            }
                            _ => {}
                        }
                    })
                    .detach();
                }
                // `flick=x,y,to x,to y,dy`: a gesture begun at the first
                // place, its momentum carried to the second, as a finger
                // lifted and the pointer moved on.
                s if s.starts_with("flick=") => {
                    let n: Vec<f32> = s["flick=".len()..].split(',').filter_map(|n| n.parse().ok()).collect();
                    if let [x, y, tx, ty, dy] = n[..] {
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |_, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let delta = ScrollDelta::Pixels(point(px(0.), px(dy)));
                            let modifiers = Modifiers::default();
                            let _ = cx.update(|window, cx| {
                                let position = point(px(x), px(y));
                                window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers }), cx);
                                window.dispatch_event(PlatformInput::Scroll(ScrollEvent { position, delta, modifiers, touch_phase: TouchPhase::Started, source: ScrollSource::Pointer }), cx);
                            });
                            for _ in 0..10 {
                                cx.background_executor().timer(Duration::from_millis(16)).await;
                                let _ = cx.update(|window, cx| {
                                    let position = point(px(tx), px(ty));
                                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers }), cx);
                                    window.dispatch_event(PlatformInput::Scroll(ScrollEvent { position, delta, modifiers, touch_phase: TouchPhase::Moved, source: ScrollSource::Pointer }), cx);
                                });
                            }
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
                                    window.dispatch_event(PlatformInput::Scroll(ScrollEvent { position, delta, modifiers, touch_phase: TouchPhase::Started, source: ScrollSource::Pointer }), cx);
                                }
                                if pinch {
                                    window.dispatch_event(PlatformInput::Pinch(PinchEvent { position, delta: dy, modifiers, phase: TouchPhase::Moved }), cx);
                                }
                            });
                        })
                        .detach();
                    }
                }
                "files" => {
                    // The files as the window has them, in its turn among
                    // the presses and keys: the note showing, the tabs,
                    // the rows chosen, where the keyboard is, and the
                    // vault's notes on the disk.
                    let wait = later;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update_in(cx, |s, window, cx| {
                            let name = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            let keys = if s.tree_focus.is_focused(window) {
                                "tree"
                            } else if s.focus.is_focused(window) {
                                "window"
                            } else if s.shown_editor().is_some_and(|e| e.read(cx).focus_handle(cx).is_focused(window)) {
                                "words"
                            } else {
                                "elsewhere"
                            };
                            let on_disk: Vec<String> = s.vault_root().map(|r| vault::notes(r).iter().map(|p| p.strip_prefix(r).unwrap_or(p).to_string_lossy().to_string()).collect()).unwrap_or_default();
                            eprintln!(
                                "files shown {:?} tabs {:?} chosen {:?} lit {:?} keys {keys} disk {on_disk:?}",
                                s.shown.as_ref().map(name),
                                s.tabs.iter().map(|t| name(&t.path)).collect::<Vec<_>>(),
                                s.choice.marked.iter().map(name).collect::<Vec<_>>(),
                                s.visible_rows().iter().filter(|(p, _)| s.row_lit(p)).map(|(p, _)| name(p)).collect::<Vec<_>>(),
                            );
                        });
                    })
                    .detach();
                }
                "where" => {
                    // Where the caret and the selection's other end stand,
                    // line and byte, on stderr, and the note's scroll.
                    let wait = later;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update(cx, |s, cx| {
                            let open = s.tabs.len();
                            if let Some(e) = s.shown_editor() {
                                let e = e.read(cx);
                                let ((al, ac), (hl, hc)) = e.selection_ends();
                                eprintln!("where {al}:{ac} {hl}:{hc} scroll {} open {open}", f32::from(e.scroll.offset().y));
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
                // `renamefile=<path>::<name>`: a file renamed as the sheet
                // renames it, in its turn, after a `preview=` before it.
                s if s.starts_with("renamefile=") => {
                    if let Some((path, name)) = s["renamefile=".len()..].split_once("::") {
                        let (path, name) = (PathBuf::from(path), name.to_string());
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |this, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = this.update_in(cx, |s, window, cx| {
                                s.rename_entry(&path, &name, window, cx);
                                cx.notify();
                            });
                        })
                        .detach();
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
                // `uikeys=<words>`: typed as an iPhone's keyboard types
                // them, through UIKit into whatever has the keyboard
                // (`sys::keyboard_types`), and whether anything took them
                // said on stderr; `uidelete`, its delete key once.
                s if s.starts_with("uikeys=") || s == "uidelete" => {
                    let words = s.strip_prefix("uikeys=").map(str::to_string);
                    let wait = later;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |_, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = cx.update(|window, _| eprintln!("uikeys {}", crate::sys::keyboard_types(window, words.as_deref())));
                    })
                    .detach();
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
                // The CJK face changed as its tile changes it, a moment on,
                // the note open: `cjkface=<key>`.
                s if s.starts_with("cjkface=") => {
                    let key = s["cjkface=".len()..].to_string();
                    let wait = later + 300;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update(cx, |s, cx| s.probe_cjk_face(key, cx));
                    })
                    .detach();
                }
                // The list of the system's faces a Custom tile opens, in the
                // settings window: `fontlist=english`, `cjk` or `mono`, and
                // `fontlist=cjk:<words>` with words in its search.
                s if s.starts_with("fontlist=") => {
                    let rest = s["fontlist=".len()..].to_string();
                    let (which, words) = rest.split_once(':').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((rest.clone(), String::new()));
                    let slot = match which.as_str() {
                        "cjk" => crate::fonts::Slot::Cjk,
                        "mono" => crate::fonts::Slot::Mono,
                        _ => crate::fonts::Slot::English,
                    };
                    let wait = later + 600;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = cx.update(|_, cx| {
                            let Some(shell) = this.upgrade() else { return };
                            let Some(handle) = shell.read(cx).settings else { return };
                            let _ = cx.update_window(handle, |_, window, cx| {
                                shell.update(cx, |s, cx| {
                                    s.probe_font_list(slot, &words, window, cx);
                                })
                            });
                        });
                    })
                    .detach();
                }
                // A newer version waits, as a check finding one says: the
                // gear's and About's dots. Nothing is fetched.
                "updatefound" => self.update.found = Some("9.9.9".into()),
                // Each state the Updates button comes to, drawn with nothing
                // fetched; `check` is the button's own press, fetched.
                "check" => self.check_updates(cx),
                "checking" => self.update.checking = true,
                "uptodate" => self.update.outcome = Some(Outcome::UpToDate),
                "checkfailed" => self.update.outcome = Some(Outcome::CheckFailed(tf("The check did not go through: {why}", &[("why", &"GitHub answered 404")]))),
                "downloading" => {
                    self.update.found = Some("9.9.9".into());
                    self.update.downloading = Some(100);
                }
                "installfailed" => {
                    self.update.found = Some("9.9.9".into());
                    self.update.outcome = Some(Outcome::InstallFailed(tf("The update did not go through: {e}", &[("e", &"the disk image could not be opened")])));
                }
                // Open at login refused by the system, and a shortcut's
                // keys refused, as their presses leave them.
                "loginrefused" => self.login_said = Some("Operation not permitted".into()),
                // A notice, as a failure says one: over the page, moving nothing.
                "notice" => self.say(tf("The note could not be written: {why}", &[("why", &"Permission denied (os error 13)")]), cx),
                "shortcutwrong" => self.record_said = Some(t(onyx_core::shortcut::Invalid::NoModifier.said()).into()),
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
                s if s.starts_with("srctype=") => {
                    // As keys typed in the source pane: the keyboard there,
                    // the words through its input handler.
                    if let Some(ix) = self.shown_index().filter(|&i| self.source_on && self.tabs[i].asset.is_none()) {
                        let pane = self.source_pane(ix, window, cx);
                        let words = s["srctype=".len()..].to_string();
                        window.focus(&pane.focus_handle(cx), cx);
                        let state = pane.read(cx).state.clone();
                        state.update(cx, |s, cx| gpui::EntityInputHandler::replace_text_in_range(s, None, &words, window, cx));
                    }
                }
                "source" => self.toggle_source(window, cx),
                // A right click in the source pane at a point of the
                // window, as the mouse makes one.
                s if s.starts_with("srcmenu=") => {
                    let n: Vec<f32> = s["srcmenu=".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
                    if let [x, y] = n[..] {
                        let wait = later;
                        later += GO_STEP;
                        cx.spawn_in(window, async move |this, cx| {
                            cx.background_executor().timer(Duration::from_millis(wait)).await;
                            let _ = this.update_in(cx, |this, window, cx| {
                                if let Some(pane) = this.shown_index().and_then(|i| this.tabs[i].source.clone()) {
                                    let state = pane.read(cx).state.clone();
                                    state.update(cx, |s, cx| s.secondary_click_at(point(px(x), px(y)), window, cx));
                                }
                            });
                        })
                        .detach();
                    }
                }
                "term" => self.toggle_terminal(window, cx),
                // The menu bar note, in its turn (`menubar::probe`).
                s if s.starts_with("menunote") => {
                    let step = s.to_string();
                    let wait = later;
                    later += GO_STEP;
                    cx.spawn_in(window, async move |_, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = cx.update(|_, cx| crate::menubar::probe(&step, cx));
                    })
                    .detach();
                }
                // The pause before what comes next, in milliseconds.
                s if s.starts_with("wait=") => later += s["wait=".len()..].parse::<u64>().unwrap_or(0),
                // On the terminal's screen once it is drawn, in its turn.
                s if s.starts_with("term") => {
                    let (wait, step) = (later, s.to_string());
                    later += GO_STEP;
                    cx.spawn_in(window, async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(wait)).await;
                        let _ = this.update_in(cx, |this, window, cx| {
                            if let Some(term) = this.terminal.clone() {
                                term.update(cx, |t, cx| t.probe(&step, window, cx));
                            }
                        });
                    })
                    .detach();
                }
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
    use super::{FADE_ROOM, READ_W, SHEET_MOST, fade_strength, one_line, sheet_width};

    #[test]
    fn full_width_is_the_page() {
        // A window with room to spare, a laptop's whole screen, a big
        // monitor's.
        for page in [900., 1456., 3000.] {
            let (own, full) = (sheet_width(0., page), sheet_width(1., page));
            assert!(own > READ_W && full - own > 40., "{page}: {own} {full}");
            assert_eq!(full, page - 32.);
        }
    }

    #[test]
    fn its_own_width_stays_a_line_to_read_on_a_big_monitor() {
        assert_eq!(sheet_width(0., 3000.), SHEET_MOST);
    }

    #[test]
    fn a_change_of_width_passes_through_every_width_between() {
        let page = 1456.;
        let steps: Vec<f32> = (0..=20).map(|i| sheet_width(i as f32 / 20., page)).collect();
        assert!(steps.windows(2).all(|w| w[1] > w[0] && w[1] - w[0] < 40.));
    }

    #[test]
    fn with_no_room_both_widths_are_one() {
        for page in [0., 500., 731.] {
            assert_eq!(sheet_width(1., page), READ_W);
            assert_eq!(sheet_width(0., page), READ_W);
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
