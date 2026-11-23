//! The note in the menu bar: one markdown file, any on the disk, in a
//! small window under Onyx's item in the menu bar (`sys/status_item.rs`),
//! as MiniNotes was before Onyx took its place. Its look is MiniNotes':
//! a card hung from the item by an arrow. Its arrangement is the main
//! window's: a row over everything (the sidebar's button, the tabs, the
//! pin), a vault's tree at the left, which slides shut and open and is as
//! wide as its edge is dragged, as the main window's sidebar, and the note
//! at the right under its name, drawn as it reads. A note is held from the tree or named in place
//! in it; nothing in the window is a panel of the system's.
//! Everything under it is Onyx's: the note is the window's own `Editor`,
//! written as a tab's note is, and joined with what the disk has as a
//! tab's is.
//!
//! The window is a panel that does not make Onyx the app in front, so the
//! main window stays where it was under the other apps, and it floats at
//! the level an input method's candidates show over
//! (`sys::float_note_window`). It goes when another window or app takes
//! the keyboard, unless it is pinned; the item, the shortcut (`main.rs`)
//! and its ✕ also close it. The editor outlives the window: the caret, the
//! scroll and the history are where they were when it opens again.
//!
//! **One writer per note.** While the main window has the same note in a
//! tab, the two editors hold one text: what is typed in either is taken by
//! the other as it is typed (`Editor::follow_text`), and the tab alone
//! writes the file, on its own clock. With no such tab, this note writes
//! itself: two seconds after its first change, and as it closes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Root, Sizable as _};

actions!(onyx_menu_note, [TogglePin, OpenInOnyx, TrashNote]);

/// The note's own keys, bound only while its window has the keyboard
/// (`key_bindings` in `main.rs`): ⌘⇧P pins it, ⌘⇧O opens it in Onyx, and
/// ⌘⇧B, the main window's, shows its tree.
pub(crate) const CONTEXT: &str = "MenuNote";
use onyx_core::config::Config;
use onyx_core::vault;

use crate::editor::{Editor, EditorEvent, Look, Words};
use crate::i18n::{t, tf};
use crate::menu::{Entry, Menus};
use crate::scroll::Scrollers;
use crate::shell::{Shell, ROW_BREATH, TREE_ROW_H};
use crate::ui_state::UiState;
use crate::widgets::{choice_pill, kbd_hint, swallow_click, tip, Fig, Motion, Press, FOLD_ANIM, SLIDE};

/// MiniNotes' size, the card's.
pub(crate) const NOTE_W: f32 = 620.;
pub(crate) const NOTE_H: f32 = 500.;
/// The arrow under the item, from its tip to the card, and its width
/// where its sides run into the card's top, as a popover's: a round tip
/// and sides that bend out into the edge.
const ARROW_H: f32 = 10.;
const ARROW_W: f32 = 34.;
/// Room kept between the card and the screen's sides, and under the bar.
const SIDE: f32 = 8.;
const BELOW: f32 = 2.;
const RADIUS: f32 = 12.;
/// Room round the card, at its sides and under it, for the shadow it
/// casts, which it draws itself: the system's shadow for a window with
/// the keyboard is a deep one, and MiniNotes' popover cast a light one.
const SHADE: f32 = 32.;
const HEAD_H: f32 = 40.;
/// The gap between the row's parts.
const HEAD_GAP: f32 = 6.;
/// The room each side of the words, a little under the main window's
/// (`SHEET_GUTTER`): the tree takes a third of the card.
const GUTTER: f32 = 28.;
/// The tree's width beside the note, fixed, under a third of the card, so
/// the note keeps the most of it, and the vault's row keeps room for a
/// name beside its three tools. A name cut short says itself whole
/// under the pointer (`tree_row`).
const TREE_W: f32 = 170.;
use crate::tabs::{Item, TabHost, Tabs};
/// A note written this long after its first change, as a tab's is.
const SAVE_AFTER: Duration = Duration::from_secs(2);
/// In and out, as a menu comes and goes (`docs/design.md`, Timing).
const COME: Duration = Duration::from_millis(150);
const GO: Duration = Duration::from_millis(110);

/// The window fading in or out as a whole: from how much showed, to
/// how much will, over how long, from its first frame since the change.
#[derive(Clone, Copy)]
struct Fade {
    from: f32,
    to: f32,
    since: Option<Instant>,
    over: Duration,
}

impl Fade {
    fn now(&self) -> f32 {
        let Some(at) = self.since else { return self.from };
        let t = (at.elapsed().as_secs_f32() / self.over.as_secs_f32()).min(1.);
        self.from + (self.to - self.from) * (1. - (1. - t).powi(3))
    }

    fn done(&self) -> bool {
        self.since.is_some_and(|at| at.elapsed() >= self.over)
    }
}
/// A click on the item this soon after the note went for another window
/// is the one that took the keyboard from it: the note stays shut.
const JUST_SHUT: Duration = Duration::from_millis(300);

/// The note in the menu bar, for the app and the main window to reach,
/// and the files its tabs hold, which the main window reads without
/// reading the note: the note may be the one asking it, busy until it
/// returns.
pub(crate) struct MenuBarNote(pub Entity<MenuNote>, pub Rc<RefCell<Vec<PathBuf>>>);

impl Global for MenuBarNote {}

/// A tab: its file, its editor, and its writing, as the main window's tab
/// has them (`shell.rs`, `Tab`).
struct Held {
    path: PathBuf,
    editor: Entity<Editor>,
    /// The text as last written or read, which a change on the disk is
    /// weighed against.
    saved: String,
    dirty: bool,
    save_due: Option<Task<()>>,
    _sub: Subscription,
    /// The note's name as a field over its words, as the main window's
    /// (`Shell::make_title`): made the first time the tab is drawn, which
    /// a field needs the window for.
    title: Option<(Entity<TextareaState>, Subscription)>,
    /// The main window's tab typed in the note while this window was
    /// closed: its editor takes the text as the window opens, not at each
    /// key, which read and checked the whole note again for no one.
    behind: bool,
}

/// The notes page: a vault's folders and notes, for a note to be held
/// from or made in.
#[derive(Default)]
struct Browse {
    /// The vault shown, and its tree once read.
    vault: Option<PathBuf>,
    tree: Option<Rc<vault::Entry>>,
    /// The folders open; the rest are shut.
    open: HashSet<PathBuf>,
    naming: Option<Naming>,
    scroll: ScrollHandle,
    /// The folder the pointer is on, whose New Note and New Folder show.
    hover_row: Option<PathBuf>,
}

/// A new note being named, in its folder's place in the tree. Nothing is
/// made until the name is given.
struct Naming {
    dir: PathBuf,
    /// A folder's name being given, not a note's.
    folder: bool,
    /// A vault's, in the container (`dir`), not a note's or a folder's:
    /// asked in the vault's block at the tree's foot, or atop the list of
    /// vaults while none is shown.
    vault: bool,
    input: Entity<InputState>,
    /// Why the name given was refused.
    why: Option<SharedString>,
    _sub: Subscription,
}

pub(crate) struct MenuNote {
    /// The notes open, a tab each in the row's order, the one showing, and
    /// the ones closed, the latest last, for Reopen Closed Tab.
    tabs: Vec<Held>,
    shown: Option<usize>,
    closed: Vec<PathBuf>,
    /// The note of the last tab closed: still the file chosen, lit in the
    /// tree, until another shows or it goes (`chosen`).
    left: Option<PathBuf>,
    /// Whether the keyboard is in the note's words. A note chosen (its
    /// row, its tab, the tab shown in a closed one's place) is chosen with
    /// no caret, as in the main window, so the files' keys act on it; a
    /// press in the words, the window opened and a note made put it there.
    caret: bool,
    /// The tabs let go of as their files left the vault, whether each was
    /// the one showing, and its place in the row: open again there when
    /// ⌘Z brings the file back.
    gone: Vec<(PathBuf, bool, usize)>,
    /// The tabs' drawing and motion, the main window's (`tabs.rs`).
    tabs_row: Tabs,
    /// A folder's icon and color being chosen (`folder_look.rs`).
    folder_look: Option<(Entity<crate::folder_look::Picker>, Subscription)>,
    /// The landing page's drawing and the person's name for its greeting.
    home_fig: Rc<RefCell<Fig>>,
    user_name: String,
    /// The vault's tree beside the note, and its slide in or out: which
    /// way and since when.
    tree_open: bool,
    tree_anim: Option<(bool, Instant)>,
    /// Where the vault's block at the tree's foot stood when last drawn,
    /// which its list rises from.
    vault_at: Rc<std::cell::Cell<Bounds<Pixels>>>,
    /// The landing page's scroll, while it lists the vaults.
    landing_scroll: ScrollHandle,
    /// The tabs' folders watched, one watcher a folder (`rewatch`).
    watched: HashMap<PathBuf, (vault::Watcher, Task<()>)>,
    /// The window's alpha as last set on the system's window.
    alpha_set: std::cell::Cell<Option<f32>>,
    browse: Browse,
    motion: Motion,
    pinned: bool,
    window: Option<WindowHandle<Root>>,
    /// When the window came in, and when it began to go.
    came: Instant,
    going: Option<Instant>,
    fade: Fade,
    /// When it last went for another window, for the item's click that
    /// took the keyboard (`JUST_SHUT`).
    shut: Option<Instant>,
    /// Where the arrow's tip stands from the card's left.
    arrow_x: f32,
    menus: Menus,
    /// What the note's menu was opened on, and the file, if any.
    canvas: crate::note_menu::NoteMenu,
    pressed: Option<PathBuf>,
    scrollers: Rc<Scrollers>,
    focus: FocusHandle,
    look: Look,
    words: Rc<Words>,
    /// The files the tabs hold, shared with the global (`MenuBarNote`).
    pub(crate) held_paths: Rc<RefCell<Vec<PathBuf>>>,
}

impl MenuNote {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let cfg = Config::load();
        let mut this = Self {
            tabs: Vec::new(),
            shown: None,
            closed: Vec::new(),
            tabs_row: Tabs::from_start(),
            folder_look: None,
            home_fig: Rc::default(),
            user_name: Shell::name_of(&cfg),
            tree_open: UiState::load().menu_tree.unwrap_or(true),
            tree_anim: None,
            vault_at: Rc::default(),
            landing_scroll: ScrollHandle::default(),
            watched: HashMap::new(),
            alpha_set: std::cell::Cell::new(None),
            browse: Browse::default(),
            motion: Motion::default(),
            pinned: false,
            window: None,
            came: Instant::now(),
            going: None,
            fade: Fade { from: 0., to: 0., since: None, over: COME },
            shut: None,
            arrow_x: NOTE_W / 2.,
            menus: Menus::default(),
            canvas: Default::default(),
            pressed: None,
            scrollers: Rc::new(Scrollers::default()),
            focus: cx.focus_handle(),
            look: Shell::look_of(&cfg.editor),
            words: Rc::new(Words::load()),
            held_paths: Rc::new(RefCell::new(Vec::new())),
            left: None,
            caret: true,
            gone: Vec::new(),
        };
        // The tabs as they were left, the one showing showing.
        let ui = UiState::load();
        let mut open: Vec<PathBuf> = ui.menu_tabs.into_iter().filter(|p| p.is_file()).collect();
        if open.is_empty() {
            open.extend(ui.menu_note.clone().filter(|p| p.is_file()));
        }
        for path in Tabs::order(&Tabs::group(&open)) {
            if let Some(h) = this.make_tab(path, cx) {
                this.tabs.push(h);
            }
        }
        this.shown = (!this.tabs.is_empty()).then(|| ui.menu_note.as_ref().and_then(|n| this.tab_of(n)).unwrap_or(0));
        this.remember();
        this.rewatch(cx);
        // A quit writes the notes first, as the main window's.
        cx.on_app_quit(|this, cx| {
            this.save(cx);
            async {}
        })
        .detach();
        this
    }

    pub(crate) fn is_open(&self) -> bool {
        self.window.is_some() && self.going.is_none()
    }

    /// The tab showing.
    fn held(&self) -> Option<&Held> {
        self.shown.and_then(|i| self.tabs.get(i))
    }

    pub(crate) fn path(&self) -> Option<&Path> {
        self.held().map(|h| h.path.as_path())
    }

    fn tab_of(&self, path: &Path) -> Option<usize> {
        self.tabs.iter().position(|h| h.path == path)
    }

    // -- the window ---------------------------------------------------------

    /// The item's click or the shortcut: the note opens, or goes.
    pub(crate) fn toggle(&mut self, from_item: bool, cx: &mut Context<Self>) {
        if self.is_open() {
            self.close(cx);
        } else if from_item && self.shut.is_some_and(|at| at.elapsed() < JUST_SHUT) {
            // The click that took the keyboard from it, which shut it.
        } else {
            self.open(cx);
        }
    }

    /// The window under the item, coming in; with the item hidden or out of
    /// sight, at the top of the screen in front, in the middle. Made a
    /// moment on: gpui draws a window as it opens, and this note is busy
    /// until this returns.
    pub(crate) fn open(&mut self, cx: &mut Context<Self>) {
        // The window opens to be written in.
        self.caret = true;
        if self.window.is_some() {
            if self.going.take().is_some() {
                self.came = Instant::now();
                // Back from as far as it had gone.
                self.fade = Fade { from: self.fade.now(), to: 1., since: Some(Instant::now()), over: COME };
                crate::sys::set_menu_bar_open(true);
            }
            self.with_window(|this, window, cx| {
                window.activate_window();
                this.focus_in(window, cx);
            }, cx);
            cx.notify();
            return;
        }
        crate::sys::heal_menu_bar();
        let Some((display, bounds, arrow_x)) = Self::place(cx) else { return };
        self.arrow_x = arrow_x;
        self.came = Instant::now();
        self.going = None;
        // From its first frame, so none of the fade is spent before it.
        self.fade = Fade { from: 0., to: 1., since: None, over: COME };
        self.catch_up(cx);
        self.refresh_tree(cx);
        self.user_name = Shell::name_of(&Config::load());
        crate::sys::set_menu_bar_open(true);
        let me = cx.entity();
        cx.spawn(async move |this, cx| {
            let view = me.clone();
            let opened = cx.update(|cx| {
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        titlebar: None,
                        focus: true,
                        show: true,
                        kind: WindowKind::PopUp,
                        is_movable: false,
                        is_resizable: false,
                        is_minimizable: false,
                        display_id: Some(display),
                        window_background: WindowBackgroundAppearance::Transparent,
                        app_id: Some("Onyx".into()),
                        ..Default::default()
                    },
                    move |window, cx| {
                        crate::look::apply(&Config::load().app.appearance, Some(window), cx);
                        window
                            .observe_window_appearance(|window, cx| {
                                crate::look::apply(&Config::load().app.appearance, Some(window), cx);
                            })
                            .detach();
                        crate::sys::float_note_window(window);
                        // The toolkit's root lays the theme's ground under the
                        // whole window; here the card and its arrow are the
                        // window's shape, and round them is nothing.
                        cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
                    },
                )
            });
            let handle = match opened {
                Ok(handle) => handle,
                Err(e) => {
                    eprintln!("onyx: the menu bar note could not open: {e}");
                    let _ = this.update(cx, |_, _| crate::sys::set_menu_bar_open(false));
                    return;
                }
            };
            let _ = this.update(cx, |this, _| {
                this.window = Some(handle);
                this.alpha_set.set(None);
            });
            let weak = this.clone();
            let _ = handle.update(cx, |_, window, cx| {
                // Another window or app took the keyboard: the note goes,
                // unless it is pinned.
                cx.observe_window_activation(window, move |_, window, cx| {
                    let active = window.is_window_active();
                    let _ = weak.update(cx, |this, cx| {
                        if !active {
                            this.save(cx);
                            if !this.pinned && this.going.is_none() {
                                this.shut = Some(Instant::now());
                                this.close(cx);
                            }
                        }
                        // Pinned behind another app, its caret stops; in
                        // front again, it starts (`Editor::active`).
                        cx.notify();
                    });
                })
                .detach();
                let _ = this.update(cx, |this, cx| this.focus_in(window, cx));
            });
        })
        .detach();
        cx.notify();
    }

    /// Something done with the window a moment on, outside this note's
    /// update, which the window's own work may need.
    fn with_window(&self, f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static, cx: &mut Context<Self>) {
        let Some(handle) = self.window else { return };
        cx.spawn(async move |this, cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| f(this, window, cx));
            });
        })
        .detach();
    }

    /// Where the window goes: the display, its bounds there, and the
    /// arrow's place from the card's left. Under the item, centered on it
    /// and kept `SIDE` in from the screen's sides; without the item, under
    /// the top of the first screen's middle.
    fn place(cx: &App) -> Option<(DisplayId, Bounds<Pixels>, f32)> {
        let displays = cx.displays();
        let w = NOTE_W;
        let h = NOTE_H + ARROW_H;
        let (cx_at, below, display) = match crate::sys::menu_bar_frame() {
            Some((x, y, iw, ih)) => {
                let center = point(px(x + iw / 2.), px(y + ih / 2.));
                let d = displays.iter().find(|d| d.bounds().contains(&center)).or(displays.first())?.clone();
                (x + iw / 2., y + ih + BELOW, d)
            }
            None => {
                let d = cx.primary_display().or_else(|| displays.first().cloned())?;
                let b = d.bounds();
                let top = f32::from(d.visible_bounds().top());
                (f32::from(b.left()) + f32::from(b.size.width) / 2., top + BELOW, d)
            }
        };
        let b = display.bounds();
        let (left, right) = (f32::from(b.left()) + SIDE, f32::from(b.right()) - SIDE - w);
        let x = (cx_at - w / 2.).clamp(left, right.max(left));
        let arrow_x = (cx_at - x).clamp(RADIUS + ARROW_W / 2., w - RADIUS - ARROW_W / 2.);
        // The window's place on its display, from the display's top left.
        // Grown by the shadow's room, at the sides and under the card.
        let origin = point(px(x - SHADE) - b.left(), px(below) - b.top());
        Some((display.id(), Bounds::new(origin, size(px(w + 2. * SHADE), px(h + SHADE))), arrow_x))
    }

    /// The window goes, after its motion out. Its note is written first.
    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        if self.window.is_none() || self.going.is_some() {
            return;
        }
        self.save(cx);
        self.menus.close(cx);
        // The name's field is the window's; a new note is named again.
        self.browse.naming = None;
        self.going = Some(Instant::now());
        self.fade = Fade { from: self.fade.now(), to: 0., since: Some(Instant::now()), over: GO };
        crate::sys::set_menu_bar_open(false);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(GO).await;
            let gone = this
                .update(cx, |this, cx| {
                    if this.going.take().is_some() {
                        for h in &this.tabs {
                            h.editor.update(cx, |e, _| e.window_gone());
                        }
                        this.window.take()
                    } else {
                        None
                    }
                })
                .ok()
                .flatten();
            if let Some(handle) = gone {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        })
        .detach();
        cx.notify();
    }

    /// The keyboard to a name being given, else the note, else the card.
    fn focus_in(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(n) = &self.browse.naming {
            window.focus(&n.input.read(cx).focus_handle(cx), cx);
            return;
        }
        match self.held() {
            Some(h) if self.caret => {
                let focus = h.editor.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            _ => window.focus(&self.focus, cx),
        }
    }

    /// The file chosen: the note showing, or with no tab the note of the
    /// last one closed. The tree lights it and the files' keys act on it.
    fn chosen(&self) -> Option<&Path> {
        self.path().or(self.left.as_deref())
    }

    fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        self.pinned = !self.pinned;
        cx.notify();
    }

    // -- the tabs -------------------------------------------------------------

    /// A note in a tab: shown when it is open already, else read into a
    /// new tab at the end of its folder's pill, or of the row when its
    /// folder has none, as the main window's (`Shell::sort_tabs`).
    fn open_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.browse.naming = None;
        if let Some(i) = self.tab_of(&path) {
            self.show_tab(i, cx);
            return;
        }
        let Some(h) = self.make_tab(path.clone(), cx) else { return };
        let folder = path.parent();
        let at = self.tabs.iter().rposition(|t| t.path.parent() == folder).map_or(self.tabs.len(), |i| i + 1);
        self.tabs.insert(at, h);
        self.closed.retain(|p| *p != path);
        self.rewatch(cx);
        self.show_tab(at, cx);
    }

    /// A tab for a note: its editor set from the settings and holding what
    /// the main window's tab of it has, typed or not, else the file.
    fn make_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Option<Held> {
        let read = match vault::read(&path) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("onyx: the menu bar note could not be read: {e}");
                return None;
            }
        };
        let text = main_text(&path, cx).unwrap_or_else(|| read.clone());
        let editor = cx.new(Editor::new);
        let root = vault_of(&path);
        let (look, words) = (self.look.clone(), self.words.clone());
        let writing = writing_language();
        editor.update(cx, |e, cx| {
            e.set_look(look, cx);
            e.set_place(path.clone(), root, cx);
            e.set_writing(writing, words, cx);
            e.set_text(&text, cx);
        });
        let sub = cx.subscribe(&editor, |this, editor, ev: &EditorEvent, cx| this.heard(editor, ev, cx));
        Some(Held { path, editor, saved: read, dirty: false, save_due: None, _sub: sub, title: None, behind: false })
    }

    fn show_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.tabs.len() {
            return;
        }
        self.shown = Some(i);
        self.left = None;
        self.remember();
        self.with_window(|this, window, cx| this.focus_in(window, cx), cx);
        cx.notify();
    }

    /// ⌘ and a number: that tab, 9 the last, as the main window's.
    fn go_tab(&mut self, n: usize, cx: &mut Context<Self>) {
        if !self.tabs.is_empty() {
            self.caret = false;
            self.show_tab((n.saturating_sub(1)).min(self.tabs.len() - 1), cx);
        }
    }

    /// A tab closed, its note written first; the one after it shows, else
    /// the one before, else the landing page.
    fn close_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.tabs.len() {
            return;
        }
        // A name being typed is kept first; the note's path may move with it.
        self.keep_name(i, cx);
        self.save_tab(i, cx);
        let h = self.tabs.remove(i);
        // Its pictures let go of on the window's GPU, as a tab closed in
        // the main window lets go of its own.
        let editor = h.editor.clone();
        self.with_window(move |_, window, cx| editor.update(cx, |e, cx| e.let_pictures_go(window, cx)), cx);
        self.closed.retain(|p| *p != h.path);
        self.closed.push(h.path);
        // Reopen Closed Tab goes back this far.
        if self.closed.len() > 32 {
            self.closed.remove(0);
        }
        self.rewatch(cx);
        // The tab shown in its place is chosen as a click on it chooses
        // it; with none left the note stays the file chosen.
        self.caret = false;
        self.left = self.tabs.is_empty().then(|| self.closed.last().cloned()).flatten();
        self.shown = match self.shown {
            _ if self.tabs.is_empty() => None,
            Some(s) if s > i => Some(s - 1),
            Some(s) if s == i => Some(i.min(self.tabs.len() - 1)),
            other => other,
        };
        self.remember();
        self.with_window(|this, window, cx| this.focus_in(window, cx), cx);
        cx.notify();
    }

    /// ⌘W: the tab showing, and with none, the window.
    fn close_shown(&mut self, cx: &mut Context<Self>) {
        match self.shown {
            Some(i) => self.close_tab(i, cx),
            None => self.close(cx),
        }
    }

    /// ⌘⇧T: the last tab closed whose note is still there.
    fn reopen_tab(&mut self, cx: &mut Context<Self>) {
        self.caret = false;
        while let Some(path) = self.closed.pop() {
            if path.is_file() {
                self.open_tab(path, cx);
                return;
            }
        }
    }

    fn can_reopen(&self) -> bool {
        // The latest first: one look at the disk, as a rule.
        self.closed.iter().rev().any(|p| p.is_file())
    }

    /// The tabs kept for the next launch, and shared with the main window.
    fn remember(&self) {
        let paths: Vec<PathBuf> = self.tabs.iter().map(|h| h.path.clone()).collect();
        *self.held_paths.borrow_mut() = paths.clone();
        let mut ui = UiState::load();
        ui.menu_tabs = paths;
        ui.menu_note = self.path().map(Path::to_path_buf);
        ui.save_later();
    }

    /// The tabs' folders watched, one watcher a folder however many tabs
    /// hold notes in it (each one a stream of the system's and a thread),
    /// those no tab needs let go: as the tabs change.
    fn rewatch(&mut self, cx: &mut Context<Self>) {
        let needed: HashSet<PathBuf> = self.tabs.iter().filter_map(|h| h.path.parent().map(Path::to_path_buf)).collect();
        self.watched.retain(|dir, _| needed.contains(dir));
        for dir in needed {
            if !self.watched.contains_key(&dir) {
                if let Some(w) = self.watch(&dir, cx) {
                    self.watched.insert(dir, w);
                }
            }
        }
    }

    /// A folder watched: a change to a tab's note in it on the disk is
    /// taken as a tab's is (`take_disk`).
    fn watch(&self, dir: &Path, cx: &mut Context<Self>) -> Option<(vault::Watcher, Task<()>)> {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Vec<PathBuf>>();
        let watcher = match vault::watch_folder(dir, move |paths| {
            let _ = tx.unbounded_send(paths);
        }) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("onyx: the menu bar note is not being watched: {e}");
                return None;
            }
        };
        let task = cx.spawn(async move |this, cx| {
            while let Some(paths) = rx.next().await {
                let Ok(held) = this.read_with(cx, |this, _| this.tabs.iter().map(|h| h.path.clone()).filter(|t| paths.contains(t)).collect::<Vec<_>>()) else { break };
                if held.is_empty() {
                    continue;
                }
                // A write is a temp file and a rename: what the disk has
                // settles a moment on.
                cx.background_executor().timer(Duration::from_millis(80)).await;
                for path in held {
                    let read = path.clone();
                    let text = cx.background_executor().spawn(async move { vault::read(&read).ok() }).await;
                    let alive = this.update(cx, |this, cx| {
                        if let Some(text) = text {
                            this.take_disk(&path, text, cx);
                        }
                    });
                    if alive.is_err() {
                        return;
                    }
                }
            }
        });
        Some((watcher, task))
    }

    /// What the disk has for the note. Its own write, come back, is
    /// nothing; while the main window has it in a tab, the tab takes it
    /// and this note follows the tab. Otherwise as a tab: a note with no
    /// changes takes it whole, and one with changes joins it with what was
    /// typed (`merge::merge`), one step of undo, written at the next save.
    fn take_disk(&mut self, path: &Path, text: String, cx: &mut Context<Self>) {
        let Some(i) = self.tab_of(path) else { return };
        if main_has(path, cx) {
            self.tabs[i].saved = text;
            return;
        }
        let h = &mut self.tabs[i];
        if h.saved == text {
            return;
        }
        let base = std::mem::replace(&mut h.saved, text.clone());
        if !h.dirty {
            h.editor.update(cx, |e, cx| e.replace_text_quietly(&text, cx));
            return;
        }
        let mine = h.editor.read(cx).text();
        let (merged, _) = onyx_core::merge::merge(&base, &mine, &text);
        if let Some(edit) = crate::editor::one_edit(&mine, &merged) {
            h.editor.update(cx, |e, cx| e.apply_edits(vec![edit], cx));
        }
    }

    fn heard(&mut self, editor: Entity<Editor>, ev: &EditorEvent, cx: &mut Context<Self>) {
        match ev {
            EditorEvent::Changed => {
                if let Some(i) = self.tabs.iter().position(|h| h.editor == editor) {
                    self.changed(i, cx);
                }
            }
            EditorEvent::OpenLink(url) => self.open_link(url.clone(), cx),
            EditorEvent::OpenNote(name) => self.open_note_link(name.clone(), cx),
            EditorEvent::Menu(at, what) => self.canvas_menu(editor, *at, what.clone(), cx),
            // The viewer is the main window's: here the picture opens with
            // the system's app.
            EditorEvent::View(viewing) => {
                if let Some(seen) = viewing.pictures.get(viewing.at) {
                    crate::sys::open(&seen.path.to_string_lossy());
                }
            }
            EditorEvent::Found => cx.notify(),
            EditorEvent::Pressed => self.caret = true,
            // A file renamed from its bar: by the main window when it is
            // open, which follows its links through the vault; else the
            // file alone.
            EditorEvent::Rename(file, name) => match main_shell(cx) {
                Some(shell) => shell.update(cx, |s, cx| s.ask_rename(file.clone(), name.clone(), cx)),
                None => {
                    if let Err(e) = vault::rename(file, name) {
                        eprintln!("onyx: the file was not renamed: {e}");
                    }
                }
            },
        }
    }

    /// Typed here: while the main window has the note in a tab, the tab
    /// takes it and writes it; otherwise this note writes it, two seconds
    /// after its first change.
    fn changed(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(h) = self.tabs.get_mut(i) else { return };
        let path = h.path.clone();
        if let Some(shell) = main_shell(cx).filter(|s| s.read(cx).has_note(&path)) {
            let text = h.editor.read(cx).text();
            shell.update(cx, |s, cx| s.follow_from_menu(&path, &text, cx));
            return;
        }
        h.dirty = true;
        if h.save_due.is_none() {
            let path = path.clone();
            h.save_due = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SAVE_AFTER).await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(i) = this.tab_of(&path) {
                        this.save_tab(i, cx);
                    }
                });
            }));
        }
    }

    /// Every tab's note written now, those with changes.
    pub(crate) fn save(&mut self, cx: &mut Context<Self>) {
        for i in 0..self.tabs.len() {
            self.save_tab(i, cx);
        }
    }

    /// A tab's note written now, when it has changes. While the main
    /// window has it in a tab, that tab is asked to write it.
    fn save_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(h) = self.tabs.get_mut(i) else { return };
        h.save_due = None;
        let path = h.path.clone();
        if let Some(shell) = main_shell(cx).filter(|s| s.read(cx).has_note(&path)) {
            h.dirty = false;
            shell.update(cx, |s, cx| s.save_note_now(&path, cx));
            return;
        }
        if !h.dirty {
            return;
        }
        let text = h.editor.read(cx).text();
        if text == h.saved {
            h.dirty = false;
            return;
        }
        match vault::write(&h.path, &text) {
            Ok(()) => {
                h.dirty = false;
                h.saved = text;
            }
            Err(e) => eprintln!("onyx: the menu bar note could not be written: {e}"),
        }
    }

    /// The main window's tab of the note changed: this one follows.
    fn follow(&mut self, path: &Path, text: &str, cx: &mut Context<Self>) {
        let shut = self.window.is_none();
        if let Some(h) = self.tabs.iter_mut().find(|h| h.path == path) {
            if shut {
                h.behind = true;
            } else {
                h.editor.update(cx, |e, cx| e.follow_text(text, cx));
            }
        }
    }

    /// The tabs the main window typed in while this window was closed take
    /// its text, or with its tab gone, what was last written.
    fn catch_up(&mut self, cx: &mut Context<Self>) {
        for i in 0..self.tabs.len() {
            if !std::mem::take(&mut self.tabs[i].behind) {
                continue;
            }
            let path = self.tabs[i].path.clone();
            let text = main_text(&path, cx).unwrap_or_else(|| self.tabs[i].saved.clone());
            self.tabs[i].editor.update(cx, |e, cx| e.follow_text(&text, cx));
        }
    }

    /// The main window's tab wrote the note.
    fn written(&mut self, path: &Path, text: &str, cx: &mut Context<Self>) {
        if let Some(h) = self.tabs.iter_mut().find(|h| h.path == path) {
            h.saved = text.to_string();
            if h.editor.read(cx).text() == text {
                h.dirty = false;
                h.save_due = None;
            }
        }
    }

    // -- the note's name ---------------------------------------------------------

    /// The shown tab's name field, made the first time it is drawn, as the
    /// main window's (`Shell::make_title`): it wraps as the name does and
    /// is kept when ↩ is pressed or the keyboard leaves.
    fn title_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<TextareaState>> {
        let i = self.shown?;
        let h = self.tabs.get(i)?;
        if let Some((field, _)) = &h.title {
            return Some(field.clone());
        }
        let name = vault::title(&h.path);
        let field = cx.new(|cx| {
            let mut field = TextareaState::new(window, cx).auto_grow(1, crate::shell::TITLE_ROWS).soft_wrap(true).submit_on_enter(true).placeholder(t("Untitled").to_string());
            field.set_split_runs(Some(Rc::new(crate::fonts::split_for_fields)));
            field.set_value(name, window, cx);
            field
        });
        let sub = cx.subscribe_in(&field, window, |this, field, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { .. } => {
                this.keep_title(field, window, cx);
                this.caret = true;
                this.focus_in(window, cx);
                cx.notify();
            }
            InputEvent::Blur => this.keep_title(field, window, cx),
            _ => {}
        });
        self.tabs[i].title = Some((field.clone(), sub));
        Some(field)
    }

    /// The name in a tab's field goes to the file, as the main window's
    /// (`Shell::keep_title_at`): the note written first, then renamed, and
    /// everything that knows it by its path follows, here and in the main
    /// window. A line break is a space; a name that cannot be used leaves
    /// the file as it was.
    fn keep_name(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(h) = self.tabs.get(i) else { return };
        let Some((field, _)) = &h.title else { return };
        let name = crate::shell::one_line(&field.read(cx).value());
        let path = h.path.clone();
        if name.is_empty() || name == vault::title(&path) {
            return;
        }
        self.save_tab(i, cx);
        match vault::rename(&path, &name) {
            Ok(to) => {
                let moves = [(path, to)];
                crate::file_undo::record(crate::file_undo::Kind::Renamed, moves.to_vec(), cx);
                self.follow_moves(&moves, cx);
                if let Some(shell) = main_shell(cx) {
                    shell.update(cx, |s, cx| s.moved_elsewhere(&moves, cx));
                }
            }
            Err(e) => eprintln!("onyx: the note was not renamed: {e}"),
        }
    }

    /// The field found by what it is, never by the tab showing, which may
    /// be another by the time the keyboard has left it; then it shows the
    /// file's name, the new one or the one kept.
    fn keep_title(&mut self, field: &Entity<TextareaState>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.tabs.iter().position(|h| h.title.as_ref().is_some_and(|(f, _)| f == field)) else { return };
        self.keep_name(i, cx);
        let name = vault::title(&self.tabs[i].path);
        if field.read(cx).value() != name {
            field.update(cx, |f, cx| f.set_value(name, window, cx));
        }
    }

    /// Esc in the name: what was typed goes, and the keyboard with it.
    fn drop_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((field, path)) = self.held().and_then(|h| h.title.as_ref().map(|(f, _)| (f.clone(), h.path.clone()))) {
            field.update(cx, |f, cx| f.set_value(vault::title(&path), window, cx));
        }
        self.focus_in(window, cx);
        cx.notify();
    }

    /// Notes renamed or moved, here or in the main window: a tab's editor
    /// told where its note is now and its folder watched there, its name
    /// drawn anew, the tabs closed and the tree followed.
    fn follow_moves(&mut self, moves: &[(PathBuf, PathBuf)], cx: &mut Context<Self>) {
        let mut any = false;
        for i in 0..self.tabs.len() {
            let to = vault::moved_path(&self.tabs[i].path, moves);
            if to == self.tabs[i].path {
                continue;
            }
            any = true;
            let root = vault_of(&to);
            let place = to.clone();
            self.tabs[i].editor.update(cx, |e, cx| e.set_place(place, root, cx));
            let h = &mut self.tabs[i];
            h.path = to;
            h.title = None;
        }
        for p in self.closed.iter_mut().chain(self.left.iter_mut()).chain(self.gone.iter_mut().map(|(p, _, _)| p)) {
            *p = vault::moved_path(p, moves);
        }
        self.browse.open = self.browse.open.iter().map(|f| vault::moved_path(f, moves)).collect();
        if any {
            self.remember();
            self.rewatch(cx);
        }
        self.read_tree(cx);
        cx.notify();
    }

    pub(crate) fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        self.look = look.clone();
        for h in &self.tabs {
            h.editor.update(cx, |e, cx| e.set_look(look.clone(), cx));
        }
    }

    pub(crate) fn set_writing(&mut self, writing: Option<String>, words: Rc<Words>, cx: &mut Context<Self>) {
        self.words = words.clone();
        for h in &self.tabs {
            h.editor.update(cx, |e, cx| e.set_writing(writing.clone(), words.clone(), cx));
        }
    }

    // -- the tree --------------------------------------------------------------

    /// The tree as the window opens: the vault that holds the note, else
    /// the one shown last, else the main window's; read again, and the
    /// note's folders open.
    fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let vaults = known_vaults();
        let held = self.path().and_then(|p| vaults.iter().filter(|v| p.starts_with(v)).max_by_key(|v| v.components().count()).cloned());
        let shown = self.browse.vault.clone().filter(|v| vaults.contains(v));
        let last = UiState::load().menu_vault.filter(|v| vaults.contains(v));
        let vault = if UiState::load().menu_closed { None } else { held.or(shown).or(last).or_else(|| vaults.first().cloned()) };
        if let (Some(root), Some(path)) = (vault.as_ref(), self.path().map(Path::to_path_buf)) {
            let mut up = path.parent();
            while let Some(d) = up.filter(|d| d.starts_with(root) && *d != root.as_path()) {
                self.browse.open.insert(d.to_path_buf());
                up = d.parent();
            }
        }
        self.set_vault(vault, cx);
        self.with_window(|this, window, cx| this.focus_in(window, cx), cx);
    }

    /// Another vault on the notes page, kept for the next time.
    fn set_vault(&mut self, vault: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.browse.vault != vault {
            self.browse.tree = None;
            self.browse.scroll.set_offset(point(px(0.), px(0.)));
        }
        self.browse.vault = vault.clone();
        self.browse.naming = None;
        if let Some(v) = vault {
            let mut ui = UiState::load();
            ui.menu_vault = Some(v);
            ui.menu_closed = false;
            ui.save_later();
        }
        self.read_tree(cx);
        cx.notify();
    }

    /// Close Vault, as the main window's: the vault's notes written and
    /// their tabs closed, and the tree back to the list of vaults until
    /// one is chosen, kept so (`menu_closed`).
    fn close_vault(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.browse.vault.clone() else { return };
        for i in (0..self.tabs.len()).rev() {
            if self.tabs[i].path.starts_with(&root) {
                self.close_tab(i, cx);
            }
        }
        self.closed.retain(|p| !p.starts_with(&root));
        self.set_vault(None, cx);
        let mut ui = UiState::load();
        ui.menu_vault = None;
        ui.menu_closed = true;
        ui.save_later();
        self.with_window(|this, window, cx| this.focus_in(window, cx), cx);
    }

    /// The vault's tree, read off the window's thread.
    fn read_tree(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.browse.vault.clone() else { return };
        crate::folder_look::ensure(&root, cx);
        let read = cx.background_spawn(async move { vault::tree(&root) });
        cx.spawn(async move |this, cx| {
            let tree = read.await;
            let _ = this.update(cx, |this, cx| {
                if this.browse.vault.as_deref() == Some(tree.path.as_path()) {
                    this.browse.tree = Some(Rc::new(tree));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A note chosen in the tree: its tab shown, made if it has none.
    fn pick_note(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.caret = false;
        self.open_tab(path, cx);
    }

    // -- the files' keys ------------------------------------------------------

    /// ⌘⌫ with the keyboard in no words: the file chosen goes to the
    /// trash, held so ⌘Z puts it back (`file_undo`). The main window does
    /// it while it is open, its own tab of the note written and closed.
    fn trash_chosen(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.chosen().map(Path::to_path_buf) else { return };
        if main_window(move |s, window, cx| s.remove(vec![path.clone()], window, cx), cx) {
            return;
        }
        let Some(path) = self.chosen().map(Path::to_path_buf) else { return };
        let paths = [path];
        self.let_go(&paths, cx);
        let (_, failed) = crate::file_undo::remove(&paths, cx);
        if let Some(e) = failed {
            eprintln!("onyx: the note was not moved to the trash: {e}");
        }
        self.read_tree(cx);
        cx.notify();
    }

    /// ⌘Z and ⌘⇧Z with the keyboard in no words: the last thing done to
    /// the files is taken back, or the last taken back done again
    /// (`Shell::files_step`). The main window does it while it is open,
    /// since a move's links are written through its editors; shut, the
    /// step is run on the disk.
    fn files_step(&mut self, again: bool, cx: &mut Context<Self>) {
        use crate::file_undo::{self, Kind};
        if main_window(move |s, window, cx| s.files_step(again, window, cx), cx) {
            return;
        }
        let Some((kind, moves)) = file_undo::next(again, cx) else { return };
        let leaving: Vec<PathBuf> = moves.iter().filter(|(_, to)| file_undo::held(to, cx)).map(|(from, _)| from.clone()).collect();
        self.let_go(&leaving, cx);
        let Some(op) = file_undo::take(again, cx) else { return };
        let ran = match kind {
            Kind::Removed => file_undo::run_removal(&moves, cx),
            kind => file_undo::run_on_disk(kind, &moves, self.browse.vault.as_deref()),
        };
        file_undo::settle(op, again, ran.is_ok(), cx);
        match ran {
            Ok(()) if kind == Kind::Removed => {
                let here: Vec<PathBuf> = moves.iter().filter(|(_, to)| !file_undo::held(to, cx)).map(|(_, to)| to.clone()).collect();
                self.came_back(&here, cx);
            }
            Ok(()) => self.follow_moves(&moves, cx),
            Err(e) => eprintln!("onyx: the step was not run: {e}"),
        }
        self.read_tree(cx);
        cx.notify();
    }

    /// The tabs of what is on its way out of the vault, written and
    /// closed, and kept in mind to open again with it.
    fn let_go(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let under = |p: &Path| paths.iter().any(|g| p.starts_with(g));
        let was = self.path().map(Path::to_path_buf);
        let going: Vec<(usize, PathBuf)> = self.tabs.iter().map(|h| h.path.clone()).enumerate().filter(|(_, p)| under(p)).collect();
        for (ix, p) in going {
            self.gone.retain(|(g, _, _)| *g != p);
            self.gone.push((p.clone(), was.as_ref() == Some(&p), ix));
            if let Some(i) = self.tab_of(&p) {
                self.close_tab(i, cx);
            }
        }
        let extra = self.gone.len().saturating_sub(100);
        self.gone.drain(..extra);
        if self.left.as_deref().is_some_and(under) {
            self.left = None;
        }
        self.closed.retain(|p| !under(p));
    }

    /// What ⌘Z brought back: a note that was open as it left is open
    /// again, the one that showing; with no tab, the note that came back
    /// is the file chosen.
    fn came_back(&mut self, here: &[PathBuf], cx: &mut Context<Self>) {
        let (mut open, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.gone).into_iter().partition(|(p, _, _)| here.iter().any(|h| p.starts_with(h)));
        self.gone = rest;
        let shown = open.iter().find(|(_, shown, _)| *shown).map(|(p, _, _)| p.clone()).or_else(|| self.path().map(Path::to_path_buf));
        self.caret = false;
        // Each back in its place in the row, the leftmost first.
        open.sort_by_key(|(_, _, ix)| *ix);
        let mut last = None;
        for (p, _, ix) in open {
            if !p.is_file() {
                continue;
            }
            self.open_tab(p.clone(), cx);
            if let Some(at) = self.tab_of(&p) {
                let tab = self.tabs.remove(at);
                self.tabs.insert(ix.min(self.tabs.len()), tab);
            }
            last = Some(p);
        }
        // The tabs have changed places: the one showing is found by its
        // note again.
        if let Some(i) = shown.or(last).and_then(|p| self.tab_of(&p)) {
            self.show_tab(i, cx);
        }
        if self.shown.is_none() {
            self.left = here.iter().find(|p| vault::is_note(p) && p.is_file()).cloned();
        }
        self.read_tree(cx);
        cx.notify();
    }

    /// ⌘N, and the landing page's New Note: a name asked in the tree, in
    /// the folder of the note showing, else at the vault's top.
    fn new_note(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.browse.vault.clone() else { return };
        let dir = self.path().and_then(Path::parent).filter(|d| d.starts_with(&root)).map(Path::to_path_buf).unwrap_or(root);
        if !self.tree_open {
            self.toggle_tree(cx);
        }
        self.start_naming(dir, cx);
    }

    /// The tree shut or opened beside the note, from as far as a slide
    /// still running had got, so a second press turns it back without a
    /// jump (`Shell::toggle_sidebar`). Kept for the next time.
    fn toggle_tree(&mut self, cx: &mut Context<Self>) {
        let begun = match self.tree_anim {
            Some((_, at)) if at.elapsed() < SLIDE => Instant::now() - (SLIDE - at.elapsed()),
            _ => Instant::now(),
        };
        self.tree_open = !self.tree_open;
        self.tree_anim = Some((self.tree_open, begun));
        if !self.tree_open && self.browse.naming.take().is_some() {
            self.with_window(|this, window, cx| this.focus_in(window, cx), cx);
        }
        let mut ui = UiState::load();
        ui.menu_tree = Some(self.tree_open);
        ui.save_later();
        cx.notify();
    }

    /// How much of the tree is beside the note, 0 to 1, eased.
    fn tree_t(&self) -> f32 {
        match self.tree_anim {
            Some((coming, at)) if at.elapsed() < SLIDE => {
                let t = ease_out_quint()(at.elapsed().as_secs_f32() / SLIDE.as_secs_f32());
                if coming { t } else { 1. - t }
            }
            _ if self.tree_open => 1.,
            _ => 0.,
        }
    }

    fn toggle_folder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.browse.naming = None;
        if !self.browse.open.remove(&path) {
            self.browse.open.insert(path);
        }
        self.with_window(|this, window, cx| this.focus_in(window, cx), cx);
        cx.notify();
    }

    /// A new note in `dir`: a row in its place in the tree, its name
    /// offered as the first free "Untitled" and selected to type over.
    fn start_naming(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.ask_name(dir, false, cx);
    }

    /// A new folder's name asked in the tree, as a note's is.
    fn start_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.ask_name(dir, true, cx);
    }

    /// The tree's folders all shut, as the sidebar's Collapse All Folders.
    fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.browse.open.clear();
        cx.notify();
    }

    fn ask_name(&mut self, dir: PathBuf, folder: bool, cx: &mut Context<Self>) {
        self.ask(dir, folder, false, cx);
    }

    /// New Vault…: its name asked where the vault's block stands, the
    /// first free "Notes" offered, as the main window's sheet offers it.
    fn start_vault(&mut self, cx: &mut Context<Self>) {
        let Some(container) = crate::shell::library::container_now() else { return };
        self.menus.close(cx);
        self.ask(container, true, true, cx);
    }

    fn ask(&mut self, dir: PathBuf, folder: bool, vault: bool, cx: &mut Context<Self>) {
        if let Some(root) = self.browse.vault.clone().filter(|_| !vault) {
            let mut up = Some(dir.as_path());
            while let Some(d) = up.filter(|d| d.starts_with(&root) && *d != root.as_path()) {
                self.browse.open.insert(d.to_path_buf());
                up = d.parent();
            }
        }
        let name = if vault { vault::free_stem(&dir, t("Notes"), true) } else if folder { vault::free_stem(&dir, t("New folder"), true) } else { vault::free_stem(&dir, t("Untitled"), false) };
        self.with_window(move |this, window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx));
            input.update(cx, |i, cx| {
                i.set_value(name, window, cx);
                i.select_all(window, cx);
            });
            let sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, _, cx| match ev {
                InputEvent::PressEnter { .. } => this.finish_naming(cx),
                InputEvent::Change => {
                    if let Some(n) = this.browse.naming.as_mut().filter(|n| n.why.is_some()) {
                        n.why = None;
                        cx.notify();
                    }
                }
                _ => {}
            });
            this.browse.naming = Some(Naming { dir, folder, vault, input, why: None, _sub: sub });
            this.focus_in(window, cx);
            cx.notify();
        }, cx);
    }

    /// The name given: the note made under it and held. A name refused
    /// keeps the row, saying why.
    fn finish_naming(&mut self, cx: &mut Context<Self>) {
        let Some(n) = self.browse.naming.as_mut() else { return };
        let name = n.input.read(cx).value().to_string();
        let folder = n.folder;
        if n.vault {
            crate::shell::library::forget_vaults();
            match vault::new_vault(&n.dir, &name) {
                Ok(root) => {
                    self.browse.naming = None;
                    self.set_vault(Some(root), cx);
                    self.with_window(|this, window, cx| this.focus_in(window, cx), cx);
                }
                Err(e) => {
                    n.why = Some(match e.kind() {
                        std::io::ErrorKind::InvalidInput => t("A name can't be empty or have a slash.").into(),
                        _ => tf("The vault was not made: {why}", &[("why", &e)]).into(),
                    });
                    cx.notify();
                }
            }
            return;
        }
        match vault::create(&n.dir, &name, folder) {
            Ok(path) if folder => {
                self.browse.naming = None;
                self.browse.open.insert(path);
                self.read_tree(cx);
                self.with_window(|this, window, cx| this.focus_in(window, cx), cx);
            }
            Ok(path) => {
                self.browse.naming = None;
                self.read_tree(cx);
                self.open_tab(path, cx);
            }
            Err(e) => {
                let named = name.trim().to_string();
                let why = match e.kind() {
                    std::io::ErrorKind::AlreadyExists if folder => tf("There is already a folder named {name}.", &[("name", &named)]),
                    std::io::ErrorKind::AlreadyExists => tf("There is already a note named {name}.", &[("name", &named)]),
                    std::io::ErrorKind::InvalidInput => t("A name can't be empty or have a slash.").to_string(),
                    _ if folder => tf("The folder could not be made: {why}", &[("why", &e)]),
                    _ => tf("The note could not be made: {why}", &[("why", &e)]),
                };
                n.why = Some(why.into());
                cx.notify();
            }
        }
    }

    fn cancel_naming(&mut self, cx: &mut Context<Self>) {
        if self.browse.naming.take().is_some() {
            self.with_window(|this, window, cx| this.focus_in(window, cx), cx);
            cx.notify();
        }
    }

    /// The vault's block at the tree's foot: the vaults Onyx knows, rising
    /// from the block, as wide as it, then New Vault… and Close Vault, as
    /// the sidebar's list has them. Onyx's window is the head's own button
    /// at the right.
    fn vault_menu(&mut self, cx: &mut Context<Self>) {
        let b = self.vault_at.get();
        let at = point(b.origin.x, b.origin.y - px(6.));
        let shown = self.browse.vault.clone();
        let mut entries: Vec<Entry> = known_vaults()
            .into_iter()
            .map(|v| Entry::Choice { id: v.to_string_lossy().to_string().into(), icon: "icons/book-open.svg", label: vault_name(&v).into(), picture: false, chosen: shown.as_ref() == Some(&v) })
            .collect();
        if !entries.is_empty() {
            entries.push(Entry::Rule);
        }
        entries.push(Entry::choice("new", "icons/plus.svg", t("New Vault…")));
        if shown.is_some() {
            entries.push(Entry::choice("close", "icons/close.svg", t("Close Vault")));
        }
        self.menus.show_above("vaults", at, Some(b.size.width), entries);
        cx.notify();
    }

    /// A folder's icon and color, chosen on a sheet over the card.
    fn open_folder_look(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.browse.vault.clone() else { return };
        let picker = cx.new(|cx| crate::folder_look::Picker::new(root, folder, 14., ARROW_H, window, cx));
        let sub = cx.subscribe(&picker, |this, _, _: &crate::folder_look::Dismissed, cx| {
            this.folder_look = None;
            this.with_window(|this, window, cx| this.focus_in(window, cx), cx);
            cx.notify();
        });
        self.folder_look = Some((picker, sub));
        cx.notify();
    }

    /// Onyx's main window, for a vault to be opened or made there.
    fn show_onyx(&mut self, cx: &mut Context<Self>) {
        self.close(cx);
        cx.defer(|cx| crate::show_in_main(None, None, cx));
    }

    // -- Onyx's main window -----------------------------------------------------

    /// Open in Onyx: the note in a tab of the main window, made if it is
    /// shut, in the vault that holds it (a vault Onyx has opened, else the
    /// note's own folder). The note is written first, and this window goes.
    /// The note showing in the main window, or with none the main window
    /// itself, and the note in the menu bar put away.
    fn open_in_onyx(&mut self, cx: &mut Context<Self>) {
        let path = self.path().map(Path::to_path_buf);
        if path.is_some() {
            self.save(cx);
        }
        self.close(cx);
        cx.defer(move |cx| crate::show_in_main(path, None, cx));
    }

    fn open_link(&mut self, url: String, cx: &mut Context<Self>) {
        if url.contains("://") || url.starts_with("mailto:") {
            crate::sys::open(&url);
            return;
        }
        self.open_link_in_onyx(Link::Path(url), cx);
    }

    /// A wikilink: the note it finds, in a tab here; one it does not find
    /// is the main window's to make or say.
    fn open_note_link(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(from) = self.path().map(Path::to_path_buf) else { return };
        let root = vault_of(&from).unwrap_or_else(|| from.parent().map(Path::to_path_buf).unwrap_or_default());
        match vault::resolve_link(&root, &from, &name).filter(|p| p.is_file() && vault::is_note(p)) {
            Some(path) => self.open_tab(path, cx),
            None => self.open_link_in_onyx(Link::Note(name), cx),
        }
    }

    /// A link to a note or a file of the vault: Onyx's main window opens on
    /// the note's vault and follows it there.
    fn open_link_in_onyx(&mut self, link: Link, cx: &mut Context<Self>) {
        let Some(path) = self.path().map(Path::to_path_buf) else { return };
        self.save(cx);
        self.close(cx);
        cx.defer(move |cx| crate::show_in_main(Some(path), Some(link), cx));
    }

    // -- the note's menu ------------------------------------------------------

    /// A right click in the note: the main window's menu for it
    /// (`note_menu.rs`).
    fn canvas_menu(&mut self, editor: Entity<Editor>, at: Point<Pixels>, what: crate::editor::MenuAt, cx: &mut Context<Self>) {
        let (items, file) = self.canvas.entries(editor, what, true, cx);
        self.pressed = file;
        self.menus.show("canvas", at, items);
        cx.notify();
    }

    fn menu_pick(&mut self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        use crate::note_menu::Asked;
        self.menus.close(cx);
        if what == "folder" {
            if let Some(folder) = self.pressed.take() {
                match id.as_ref() {
                    "new-note" => self.start_naming(folder, cx),
                    "new-folder" => self.start_folder(folder, cx),
                    "folder-look" => self.open_folder_look(folder, window, cx),
                    _ => {}
                }
            }
            return;
        }
        if what == "vaults" {
            match id.as_ref() {
                "new" => self.start_vault(cx),
                "close" => self.close_vault(cx),
                path => self.set_vault(Some(PathBuf::from(path)), cx),
            }
            return;
        }
        let pressed = self.pressed.take();
        match self.canvas.pick(&id, window, cx) {
            Asked::Done => {}
            Asked::Link(link) => match (link.url, link.note) {
                (Some(url), _) => self.open_link(url, cx),
                (None, Some(note)) => self.open_note_link(note, cx),
                _ => {}
            },
            // What the system does is done here; a rename, or the file in
            // a tab, is the main window's.
            Asked::File(what) => {
                if let Some(file) = pressed {
                    match what.as_str() {
                        "open-app" => crate::sys::open(&file.to_string_lossy()),
                        "reveal" => crate::sys::reveal(&file),
                        "copy-path" => cx.write_to_clipboard(ClipboardItem::new_string(file.to_string_lossy().to_string())),
                        _ => self.open_link_in_onyx(Link::File(what, file), cx),
                    }
                }
            }
            Asked::Export => self.open_link_in_onyx(Link::Export, cx),
            Asked::Print => self.open_link_in_onyx(Link::Print, cx),
            Asked::Learn(issue) => self.settle(issue, true, cx),
            Asked::Ignore(issue) => self.settle(issue, false, cx),
        }
        cx.notify();
    }

    /// A marked word settled: by the main window when it is open, which
    /// tells every note, this one with them; else here.
    fn settle(&mut self, issue: crate::editor::MenuIssue, learn: bool, cx: &mut Context<Self>) {
        if let Some(shell) = main_shell(cx) {
            shell.update(cx, |s, cx| s.settle_issue(issue, learn, cx));
            return;
        }
        let mut words = (*self.words).clone();
        if learn {
            if let Err(e) = onyx_core::check::learn(&issue.words) {
                eprintln!("onyx: the word was not kept: {e}");
            }
            words.learned = onyx_core::check::learned();
        } else if issue.spelling {
            words.passed.insert(issue.words.to_lowercase());
        } else {
            if let Err(e) = onyx_core::check::ignore(&issue.rule, &issue.words) {
                eprintln!("onyx: the word was not kept: {e}");
            }
            words.ignored = onyx_core::check::ignored();
        }
        self.set_writing(writing_language(), Rc::new(words), cx);
    }

    // -- drawing --------------------------------------------------------------

    /// The row over the card: the tree's button, the tabs, Open in Onyx,
    /// and the pin at the right edge.
    fn render_head(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let accent = theme.primary;
        let hover = theme.muted;
        let pinned = self.pinned;
        let tree = self.tree_open;
        let icon_button = |id: &'static str, icon: &'static str, ink: Hsla, label: SharedString| {
            div()
                .id(id)
                .size(px(28.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .managed_tooltip(move |window, cx| tip(label.clone(), window, cx))
                .child(Icon::default().path(icon).with_size(px(14.)).text_color(ink))
        };
        let tabs = self.render_tabs(window, cx);
        h_flex()
            .h(px(HEAD_H))
            .flex_shrink_0()
            .px(px(8.))
            .gap(px(HEAD_GAP))
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            // On while the tree shows, as the pin is while it holds.
            .child(icon_button("menu-note-tree", "icons/panel-left.svg", if tree { accent } else { theme.muted_foreground }, t("Sidebar (⌘⇧B)").into()).when(tree, |d| d.bg(accent.opacity(0.12))).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.toggle_tree(cx);
            })))
            .child(tabs)
            // The note in the main window, or with none the window itself.
            .child(icon_button("menu-note-onyx", "icons/app-window.svg", theme.muted_foreground, t("Open in Onyx (⌘⇧O)").into()).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.open_in_onyx(cx);
            })))
            .child(
                icon_button("menu-note-pin", if pinned { "icons/pin-filled.svg" } else { "icons/pin.svg" }, if pinned { accent } else { theme.muted_foreground }, if pinned { t("Auto-Hide Window (⌘⇧P)").into() } else { t("Pin the Window (⌘⇧P)").into() })
                    .when(pinned, |d| d.bg(accent.opacity(0.12)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.toggle_pin(cx);
                    })),
            )
            .into_any_element()
    }

    /// The tabs, the main window's row (`tabs.rs`): a pill for each
    /// folder, the plus after them. Its width is figured from the row's
    /// own, as the main window's is, never measured off a frame.
    fn render_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // No tab, no row: the landing page's New Note and the tree's plus
        // make a note, and a third did not help.
        if self.tabs.is_empty() {
            return div().flex_1().min_w_0().into_any_element();
        }
        let items: Vec<Item> = self.tabs.iter().map(|h| Item { path: h.path.clone(), title: vault::title(&h.path) }).collect();
        // The card less its rim and the row's ends, less the three buttons,
        // the tree's, Open in Onyx and the pin, with the gaps between.
        let row = NOTE_W - 2. - 2. * 8. - 3. * (28. + HEAD_GAP);
        let shown = self.path().map(Path::to_path_buf);
        let root = self.browse.vault.clone();
        // From the left the row has no margins: the head's gap is all.
        self.tabs_row.sync(row, &items, shown.as_deref(), window, cx);
        if self.tabs_row.moving() {
            window.request_animation_frame();
        }
        self.tabs_row.render(&items, shown.as_deref(), root.as_deref(), cx)
    }

    /// No tab open: the landing page, the vault's page of the main window
    /// made small: the drawing and the greeting, then what can be done
    /// next, each on a pill with its keys.
    fn render_landing(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.browse.vault.is_none() && crate::shell::library::container_now().is_some() {
            return self.render_vault_page(cx);
        }
        let choice = |id: &'static str, icon: &'static str, label: &str, keys: &str, theme: &gpui_component::Theme| choice_pill(id, icon, label.to_string(), Press::Quiet, false, theme).min_w(px(220.)).h(px(34.)).px(px(14.)).gap(px(8.)).text_size(px(13.)).child(div().flex_1()).child(kbd_hint(keys, theme));
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .items_center()
            .justify_center()
            .pb(px(24.))
            .gap(px(22.))
            .child(
                v_flex()
                    .items_center()
                    .gap(px(10.))
                    .child(Shell::home_figure(&self.home_fig, &theme, cx.entity_id()))
                    .child(div().text_size(px(26.)).font_family(self.look.text_family.clone()).child(crate::shell::greeting(&self.user_name))),
            )
            .child(
                // As wide as the widest of them, in any language.
                v_flex()
                    .items_stretch()
                    .gap(px(8.))
                    .when(self.browse.vault.is_some(), |d| {
                        d.child(choice("menu-home-new", "icons/file-plus.svg", t("New Note"), "⌘N", &theme).on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.new_note(cx);
                        })))
                    })
                    .when(self.can_reopen(), |d| {
                        d.child(choice("menu-home-reopen", "icons/history.svg", t("Reopen Closed Tab"), "⌘⇧T", &theme).on_click(cx.listener(|this, _, window, cx| {
                            swallow_click(window, cx);
                            this.reopen_tab(cx);
                        })))
                    }),
            )
            .into_any_element()
    }

    /// The vault's block (`library::vault_block`) at the tree's foot, as
    /// the sidebar has it under its card: its press lists the vaults Onyx
    /// knows over it, as wide as it, and a press while they show puts them
    /// away. With no vault, nothing.
    fn render_tree_foot(&self, ground: Hsla, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::shell::library::{vault_block, BlockSize};
        let theme = cx.theme().clone();
        let root = self.browse.vault.clone()?;
        if self.browse.naming.as_ref().is_some_and(|n| n.vault) {
            return Some(self.vault_naming(ground, &theme).mx(px(6.)).mt(px(4.)).mb(px(6.)).into_any_element());
        }
        let name = vault_name(&root);
        let listed = self.menus.open.as_ref().is_some_and(|m| m.what == "vaults");
        let place = self.vault_at.clone();
        Some(
            vault_block("menu-note-vault", &root, &name, BlockSize::MENU, ground, listed, &theme, cx)
                .mx(px(6.))
                .mt(px(4.))
                .mb(px(6.))
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    if this.menus.open.as_ref().is_some_and(|m| m.what == "vaults") {
                        this.menus.close(cx);
                        cx.notify();
                    } else {
                        this.vault_menu(cx);
                    }
                }))
                .child(canvas(move |b, _, _| place.set(b), |_, _, _, _| ()).absolute().top_0().left_0().size_full())
                .into_any_element(),
        )
    }

    /// No tab and no vault, with a container: the main window's page before
    /// a vault made small (`Shell::render_place_page`): the notebooks'
    /// drawing, the plain instruction, and a tile for each vault, two to a
    /// row, then New Vault…, whose name is asked atop the tree's list. No
    /// Choose Container…: the container is the main window's.
    fn render_vault_page(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::shell::places::{new_vault_tile, tile_rows, vault_tile};
        let theme = cx.theme().clone();
        let vaults = known_vaults();
        let words = if vaults.is_empty() { t("Create a vault to start.") } else { t("Pick a vault to start.") };
        let mut tiles: Vec<AnyElement> = Vec::new();
        for (i, v) in vaults.into_iter().enumerate() {
            tiles.push(
                vault_tile(ElementId::NamedInteger("menu-page-vault".into(), i as u64), &v, &theme, cx)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.set_vault(Some(v.clone()), cx);
                        this.with_window(|this, window, cx| this.focus_in(window, cx), cx);
                    }))
                    .into_any_element(),
            );
        }
        tiles.push(
            new_vault_tile("menu-page-new-vault", &theme)
                .on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    if !this.tree_open {
                        this.toggle_tree(cx);
                    }
                    this.start_vault(cx);
                }))
                .into_any_element(),
        );
        let scroll = self.landing_scroll.clone();
        self.scrollers.name(&scroll);
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .child(
                div().id("menu-page").size_full().overflow_y_scroll().track_scroll(&scroll).child(
                    v_flex()
                        .min_h_full()
                        .items_center()
                        .justify_center()
                        .px(px(20.))
                        .pt(px(20.))
                        .pb(px(32.))
                        .gap(px(22.))
                        .child(v_flex().items_center().gap(px(10.)).child(Shell::shelf_figure(&self.home_fig, cx.entity_id())).child(div().text_size(px(26.)).font_family(self.look.text_family.clone()).child(words)))
                        .child(tile_rows(2).children(tiles)),
                ),
            )
            .vertical_scrollbar(&scroll)
            .into_any_element()
    }

    /// A new vault's name being given, in the shape of the block it will
    /// be (`library::vault_block` at `BlockSize::MENU`): an empty mark's
    /// tile and the field, its edge the accent, or the danger's while a
    /// name is refused.
    fn vault_naming(&self, ground: Hsla, theme: &gpui_component::Theme) -> Div {
        use crate::shell::library::BlockSize;
        let size = BlockSize::MENU;
        let Some(n) = &self.browse.naming else { return div() };
        let rim = if n.why.is_some() { theme.danger } else { theme.primary };
        let tile = div().size(px(size.mark)).rounded(px(size.mark * 0.3)).bg(theme.muted_foreground.opacity(0.18));
        h_flex()
            .flex_shrink_0()
            .h(px(size.h))
            .pl(px(size.h * 0.2))
            .pr(px(8.))
            .gap(px(size.h * 0.2))
            .items_center()
            .rounded(px(10.))
            .border_1()
            .border_color(rim.opacity(0.6))
            .bg(ground.blend(theme.foreground.opacity(0.05)))
            .text_size(px(size.text))
            .font_weight(FontWeight::SEMIBOLD)
            .child(tile)
            .child(div().flex_1().min_w_0().child(Input::new(&n.input).appearance(false).bordered(false).xsmall().px_0()))
    }

    /// No vault shown, with a container: the vaults to choose from, as the
    /// sidebar's card has them before a vault (`Shell::render_place_card`):
    /// a head with the vaults' title and New Vault…, then a block for each
    /// (`library::vault_block`), a new one's name atop them while it is
    /// asked.
    fn render_vault_list(&self, ground: Hsla, cx: &mut Context<Self>) -> AnyElement {
        use crate::shell::library::{row_tool, vault_block, BlockSize};
        let theme = cx.theme().clone();
        let tool = row_tool("menu-note-new-vault", "icons/plus.svg", t("New Vault…").into(), &theme).on_click(cx.listener(|this, _, window, cx| {
            swallow_click(window, cx);
            this.start_vault(cx);
        }));
        let head = h_flex()
            .flex_shrink_0()
            .mx(px(6.))
            .mt(px(4.))
            .h(px(32.))
            .pl(px(8.))
            .gap(px(6.))
            .items_center()
            .text_size(px(13.))
            .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().child(Icon::default().path("icons/books.svg").with_size(px(15.)).text_color(theme.muted_foreground)))
            .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(t("Vaults")))
            .child(tool);
        let mut rows: Vec<AnyElement> = Vec::new();
        if self.browse.naming.as_ref().is_some_and(|n| n.vault) {
            rows.push(self.vault_naming(ground, &theme).into_any_element());
        }
        for (i, v) in known_vaults().into_iter().enumerate() {
            let name = vault_name(&v);
            rows.push(
                vault_block(ElementId::NamedInteger("menu-note-vaults".into(), i as u64), &v, &name, BlockSize::MENU, ground, false, &theme, cx)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        this.set_vault(Some(v.clone()), cx);
                        this.with_window(|this, window, cx| this.focus_in(window, cx), cx);
                    }))
                    .into_any_element(),
            );
        }
        let scroll = self.browse.scroll.clone();
        self.scrollers.name(&scroll);
        v_flex()
            .flex_1()
            .min_h_0()
            .child(head)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(div().id("menu-note-vaults").size_full().overflow_y_scroll().track_scroll(&scroll).child(v_flex().px(px(6.)).pt(px(4.)).pb(px(10.)).gap(px(6.)).children(rows)))
                    .child(crate::shell::edge_fade(&scroll, ground, true, px(0.)))
                    .child(crate::shell::edge_fade_at(&scroll, ground, false, Corners { bottom_left: px(RADIUS - 1.), ..Default::default() }))
                    .vertical_scrollbar(&scroll),
            )
            .into_any_element()
    }

    /// Why a name was refused, over the tree's foot.
    fn render_why(&self, foot: bool, theme: &gpui_component::Theme) -> Option<AnyElement> {
        let why = self.browse.naming.as_ref().and_then(|n| n.why.clone())?;
        Some(
            div()
                .absolute()
                .bottom(px(if foot { 38. + 4. + 6. + 8. } else { 10. }))
                .left(px(8.))
                .right(px(8.))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(8.))
                .bg(theme.popover)
                .border_1()
                .border_color(theme.danger.opacity(0.4))
                .shadow_md()
                .text_size(px(12.5))
                .text_color(theme.danger)
                .child(why)
                .with_animation("menu-note-why", Animation::new(COME).with_easing(ease_out_quint()), |d, k| d.opacity(k).mb(px(-4. * (1. - k))))
                .into_any_element(),
        )
    }

    /// The vault at the top of its tree, the sidebar's own row
    /// (`library::vault_root_row`), with the Notes tools
    /// (`library::notes_tools`) at its right.
    fn render_tree_head(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::shell::library::{notes_tools, vault_root_row};
        let theme = cx.theme().clone();
        let Some(root) = self.browse.vault.clone() else { return div().h(px(8.)).into_any_element() };
        let name = vault_name(&root);
        let [collapse, note, folder] = notes_tools("menu-note", &theme);
        let tools = h_flex()
            .flex_shrink_0()
            .gap(px(2.))
            .child(collapse.on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.collapse_all(cx);
            })))
            .child(note.on_click(cx.listener({
                let root = root.clone();
                move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.start_naming(root.clone(), cx);
                }
            })))
            .child(folder.on_click(cx.listener({
                let root = root.clone();
                move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.start_folder(root.clone(), cx);
                }
            })));
        vault_root_row("menu-note-vault-root", &root, &name, &theme, cx).mx(px(6.)).mt(px(4.)).child(tools).into_any_element()
    }

    /// The tree: the vault's folders and notes, as the main window's
    /// sidebar has them, on `ground`; with no vault, the way to Onyx's
    /// window to open one.
    fn render_tree(&mut self, ground: Hsla, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        if self.browse.vault.is_none() && crate::shell::library::container_now().is_some() {
            return self.render_vault_list(ground, cx);
        }
        let Some(root) = self.browse.vault.clone() else {
            return v_flex()
                .flex_1()
                .px(px(16.))
                .items_center()
                .justify_center()
                .child(choice_pill("menu-note-onyx-vault", "icons/app-window.svg", t("Open Onyx"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.show_onyx(cx);
                })))
                .into_any_element();
        };
        let Some(tree) = self.browse.tree.clone() else { return div().flex_1().into_any_element() };
        let naming_root = self.browse.naming.as_ref().is_some_and(|n| n.dir == root);
        if tree.children.is_empty() && !naming_root {
            return div().flex_1().into_any_element();
        }
        let mut rows = Vec::new();
        if naming_root {
            rows.push(self.naming_row(0, &theme));
        }
        self.tree_rows(&tree.children, 0, &theme, window, cx, &mut rows);
        let scroll = self.browse.scroll.clone();
        self.scrollers.name(&scroll);
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .child(div().id("menu-note-tree").size_full().overflow_y_scroll().track_scroll(&scroll).child(v_flex().px(px(6.)).pt(px(4.)).pb(px(10.)).children(rows)))
            .child(crate::shell::edge_fade(&scroll, ground, true, px(0.)))
            // Its foot is over the vault's block, not at the card's corner.
            .child(crate::shell::edge_fade(&scroll, ground, false, px(0.)))
            .vertical_scrollbar(&scroll)
            .into_any_element()
    }

    /// The rows of a folder's entries, an open folder's after its row; a
    /// folder opening or shutting runs its rows' box up to their height or
    /// down, as the sidebar's do.
    fn tree_rows(&self, entries: &[vault::Entry], depth: usize, theme: &gpui_component::Theme, window: &Window, cx: &mut Context<Self>, out: &mut Vec<AnyElement>) {
        for e in entries {
            out.push(self.tree_row(e, depth, theme, window, cx));
            if !e.dir {
                continue;
            }
            let open = self.browse.open.contains(&e.path);
            let key = format!("menu-fold-{}", e.path.display());
            let moving = self.motion.presence(&key, open, cx);
            if !open && moving.is_none() {
                continue;
            }
            let mut inside = Vec::new();
            if self.browse.naming.as_ref().is_some_and(|n| n.dir == e.path) {
                inside.push(self.naming_row(depth + 1, theme));
            }
            self.tree_rows(&e.children, depth + 1, theme, window, cx, &mut inside);
            match moving {
                Some((opening, serial)) => {
                    let full = self.rows_in(e) as f32 * TREE_ROW_H;
                    out.push(
                        v_flex()
                            .flex_shrink_0()
                            .overflow_hidden()
                            .child(v_flex().flex_shrink_0().children(inside))
                            .with_animation(ElementId::NamedInteger(key.into(), serial as u64), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                                let t = if opening { t } else { 1. - t };
                                d.h(px(full * t)).opacity(t)
                            })
                            .into_any_element(),
                    );
                }
                None => out.extend(inside),
            }
        }
    }

    /// How many rows stand under a folder: its entries, an open folder's
    /// own, and a name being given in it.
    fn rows_in(&self, folder: &vault::Entry) -> usize {
        let naming = usize::from(self.browse.naming.as_ref().is_some_and(|n| n.dir == folder.path));
        naming + folder.children.iter().map(|c| 1 + if c.dir && self.browse.open.contains(&c.path) { self.rows_in(c) } else { 0 }).sum::<usize>()
    }

    /// A row of the tree: the chevron (a folder), Catppuccin's icon, the
    /// name, lit while it is the note held; a folder offers a new note in
    /// it under the pointer.
    fn tree_row(&self, e: &vault::Entry, depth: usize, theme: &gpui_component::Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let open = e.dir && self.browse.open.contains(&e.path);
        let lit = !e.dir && self.chosen() == Some(e.path.as_path());
        let file = e.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| e.name.clone());
        // A folder's icon; a note has none, as in the main window's tree.
        let icon = e.dir.then(|| crate::folder_look::icon(&e.path, &file, open, theme.mode.is_dark(), theme, cx));
        let hover_bg = theme.foreground.opacity(0.07);
        let path = e.path.clone();
        let dir = e.dir;
        // A name the tree's width cuts short says itself whole under the
        // pointer: the room the row leaves it (the list's padding, the
        // row's, the chevron, a folder's icon and plus, and their gaps)
        // against its words' width.
        let room = TREE_W - (2. * 6. + 8. + 14. * depth as f32 + 12. + 6. + 4.) - if e.dir { 16. + 6. } else { 0. };
        let run = TextRun { len: e.name.len(), font: font(theme.font_family.clone()), color: gpui::black(), background_color: None, underline: None, strikethrough: None };
        let wide = f32::from(window.text_system().shape_line(SharedString::from(e.name.clone()), px(13.), &[run], None).width());
        let whole: Option<SharedString> = (wide > room + 0.5).then(|| e.name.clone().into());
        // The pointer on a folder's plate shows its tools; the row's own
        // hover is its whole name's tip.
        let plate = h_flex()
            .id(ElementId::Name(format!("menu-plate-{}", e.path.display()).into()))
            .size_full()
            .pl(px(8. + 14. * depth as f32))
            .pr(px(4.))
            .gap(px(6.))
            .items_center()
            .rounded(px(7.))
            .when(lit, |d| d.bg(theme.foreground.opacity(0.13)))
            .when(!lit, |d| d.group_hover("menu-row", move |s| s.bg(hover_bg)))
            .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().when(e.dir, |d| d.child(Icon::default().path(if open { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" }).with_size(px(12.)).text_color(theme.muted_foreground))))
            .children(icon)
            .child(div().flex_1().min_w_0().truncate().child(e.name.clone()))
            // A folder's New Note and New Folder while the pointer is on
            // it, as the sidebar's (`Shell::hover_row`): no room is kept
            // for them else.
            .when(e.dir && self.browse.hover_row.as_ref() == Some(&e.path), |d| {
                let (note_in, folder_in) = (path.clone(), path.clone());
                d.child(
                    h_flex()
                        .flex_shrink_0()
                        .mr(px(-4.))
                        .gap(px(2.))
                        .child(crate::shell::library::row_tool(ElementId::Name(format!("menu-row-note-{}", e.path.display()).into()), "icons/file-plus.svg", t("New Note").into(), theme).on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            cx.stop_propagation();
                            this.start_naming(note_in.clone(), cx);
                        })))
                        .child(crate::shell::library::row_tool(ElementId::Name(format!("menu-row-folder-{}", e.path.display()).into()), "icons/folder-plus.svg", t("New Folder").into(), theme).on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            cx.stop_propagation();
                            this.start_folder(folder_in.clone(), cx);
                        }))),
                )
            })
            .when(dir, |d| {
                let hovered = path.clone();
                d.on_hover(cx.listener(move |this, on: &bool, _, cx| {
                    if *on {
                        this.browse.hover_row = Some(hovered.clone());
                    } else if this.browse.hover_row.as_ref() == Some(&hovered) {
                        this.browse.hover_row = None;
                    }
                    cx.notify();
                }))
            });
        div()
            .id(ElementId::Name(format!("menu-entry-{}", e.path.display()).into()))
            .group("menu-row")
            .h(px(TREE_ROW_H))
            .flex_shrink_0()
            .py(px(ROW_BREATH))
            .text_size(px(13.))
            .text_color(theme.foreground.opacity(if lit { 1. } else { 0.88 }))
            .cursor_pointer()
            .when_some(whole, |d, whole| d.managed_tooltip(move |window, cx| tip(whole.clone(), window, cx)))
            .child(plate)
            .on_click(cx.listener({
                let path = path.clone();
                move |this, _, window, cx| {
                    swallow_click(window, cx);
                    if dir {
                        this.toggle_folder(path.clone(), cx);
                    } else {
                        this.pick_note(path.clone(), cx);
                    }
                }
            }))
            // A folder's menu: a new note in it, and its icon and color.
            .when(dir, |d| {
                d.on_mouse_down(MouseButton::Right, cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    swallow_click(window, cx);
                    this.pressed = Some(path.clone());
                    let items = vec![Entry::choice("new-note", "icons/file-plus.svg", t("New Note")), Entry::choice("new-folder", "icons/folder-plus.svg", t("New Folder")), Entry::choice("folder-look", "tabler/palette.svg", t("Icon and Color…"))];
                    this.menus.show("folder", ev.position, items);
                    cx.notify();
                }))
            })
            .into_any_element()
    }

    /// The new note's row, where it will stand: the note's icon and the
    /// field its name is typed in.
    fn naming_row(&self, depth: usize, theme: &gpui_component::Theme) -> AnyElement {
        let Some(n) = &self.browse.naming else { return div().into_any_element() };
        let rim = if n.why.is_some() { theme.danger } else { theme.primary };
        div()
            .h(px(TREE_ROW_H))
            .flex_shrink_0()
            .py(px(ROW_BREATH))
            .text_size(px(13.))
            .child(
                h_flex()
                    .size_full()
                    .pl(px(7. + 14. * depth as f32))
                    .pr(px(8.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(7.))
                    // In by its rim, so its icon stands where its siblings' do.
                    .bg(theme.foreground.opacity(0.06))
                    .border_1()
                    .border_color(rim.opacity(0.6))
                    .child(div().w(px(12.)).flex_shrink_0())
                    .when(n.folder, |d| d.child(Icon::default().path("icons/folder.svg").with_size(px(15.)).text_color(theme.muted_foreground)))
                    .child(div().flex_1().min_w_0().child(Input::new(&n.input).appearance(false).bordered(false).xsmall().px_0())),
            )
            .into_any_element()
    }
}

impl TabHost for MenuNote {
    fn tabs_row(&mut self) -> &mut Tabs {
        &mut self.tabs_row
    }

    /// The tabs in the order a drag gave them, the same one showing.
    fn tabs_reorder(&mut self, order: &[PathBuf], _: &mut Window, cx: &mut Context<Self>) {
        let showing = self.path().map(Path::to_path_buf);
        let mut tabs = std::mem::take(&mut self.tabs);
        for path in order {
            if let Some(i) = tabs.iter().position(|h| h.path == *path) {
                self.tabs.push(tabs.remove(i));
            }
        }
        self.tabs.append(&mut tabs);
        self.shown = showing.and_then(|p| self.tab_of(&p));
        self.remember();
        cx.notify();
    }

    fn tab_show(&mut self, path: &Path, _: &mut Window, cx: &mut Context<Self>) {
        self.caret = false;
        if let Some(i) = self.tab_of(path) {
            self.show_tab(i, cx);
        }
    }

    fn tab_close(&mut self, path: &Path, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.tab_of(path) {
            self.close_tab(i, cx);
        }
    }

    /// The plus: a new note in the folder of the one showing, its name
    /// asked first, as the main window's.
    fn tab_plus(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.new_note(cx);
    }
}

impl Focusable for MenuNote {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The arrow's outline from its tip to `foot`, the card's edge: each side
/// bends out into the edge, and the tip is round. Filled, it closes under
/// the edge so the ground covers the card's edge there; drawn, it is the
/// two sides alone.
fn arrow_path(tip: Point<Pixels>, foot: Pixels, mut p: PathBuilder, filled: bool) -> Option<gpui::Path<Pixels>> {
    let at = |dx: f32, y: Pixels| point(tip.x + px(dx), y);
    let up = |dy: f32| tip.y + px(dy);
    let half = ARROW_W / 2.;
    p.move_to(at(-half, foot));
    p.cubic_bezier_to(at(-3., up(1.8)), at(-half * 0.56, foot), at(-5.2, up(3.6)));
    p.curve_to(at(3., up(1.8)), at(0., up(-0.4)));
    p.cubic_bezier_to(at(half, foot), at(5.2, up(3.6)), at(half * 0.56, foot));
    if filled {
        p.line_to(at(half, foot + px(1.5)));
        p.line_to(at(-half, foot + px(1.5)));
        p.close();
    }
    p.build().ok()
}

impl Render for MenuNote {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The router is this window's while it draws (`scroll.rs`).
        self.scrollers.begin();
        cx.set_global(crate::scroll::Router(self.scrollers.clone()));
        // Under the folder's sheet nothing scrolls but its grid.
        if let Some((picker, _)) = &self.folder_look {
            self.scrollers.name_over(&picker.read(cx).scroll);
        }
        let theme = cx.theme().clone();
        let (ground, rim) = (theme.background, theme.border);
        // The card's edge, a hairline: the system's shadow darkens the
        // outside of it already, and the theme's border beside that read
        // as a double line.
        let edge = theme.foreground.opacity(if theme.mode.is_dark() { 0.13 } else { 0.07 });
        // The tree's ground, a shade toward the main window's frame, which
        // its sidebar stands on: darker than the note's in a light
        // appearance, nowhere near the frame itself.
        // A shade toward the main window's sidebar, lighter than it.
        let side = ground.blend(theme.title_bar.opacity(0.16));
        let arrow_x = self.arrow_x;
        // The tree beside the note, run open or shut on the slide's clock.
        let shown = self.tree_t();
        let tree_w = TREE_W;
        if self.tree_anim.is_some_and(|(_, at)| at.elapsed() < SLIDE) {
            window.request_animation_frame();
        }
        // The card's foot is rounded; a fade over it rounds the corners
        // that are the card's: the note's left one only with no tree.
        let corner = px(RADIUS - 1.);
        let title = self.title_field(window, cx);
        let note = match self.held().map(|h| (h.editor.clone(), h.path.clone())) {
            Some((editor, path)) => {
                let scroll = editor.read(cx).scroll.clone();
                self.scrollers.name(&scroll);
                let line = (self.look.text_px * onyx_core::rhythm::LINE).round();
                // The note's name over its words, as the main window draws
                // it (`Shell::render_note`): in the words' face, bold, a
                // soft line under it that says it is the name.
                let title_font = Font { family: self.look.text_family.clone(), features: FontFeatures::default(), fallbacks: self.look.text_fallbacks.clone(), weight: FontWeight::BOLD, style: FontStyle::Normal };
                let title_px = px(self.look.text_px * onyx_core::rhythm::TITLE);
                let title_el = match title.filter(|_| !editor.read(cx).locked()) {
                    Some(field) => div()
                        .id("menu-note-title")
                        .key_context(crate::shell::TITLE_CONTEXT)
                        .on_action(cx.listener(|this, _: &crate::shell::Escape, window, cx| {
                            cx.stop_propagation();
                            this.drop_title(window, cx)
                        }))
                        // Not a press beside the words.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(Textarea::new(&field).appearance(false).bordered(false).flush(true).font(title_font).text_size(title_px).line_height(relative(onyx_core::rhythm::TITLE_LINE)).text_color(theme.foreground).p_0())
                        .into_any_element(),
                    None => div().font(title_font).text_size(title_px).line_height(relative(onyx_core::rhythm::TITLE_LINE)).text_color(theme.foreground).child(vault::title(&path)).into_any_element(),
                };
                // The words with room round them. The editor fills the
                // card from its first line down (`Editor::fill_page`), so a
                // press under the words is its own and puts the caret at
                // their end, as a page of paper is written on; one beside
                // them keeps the keyboard in the note and moves nothing.
                editor.update(cx, |e, _| e.fill_page((line * 3. - GUTTER).max(0.)));
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .relative()
                    .child(
                        div()
                            .id("menu-note-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                                this.browse.naming = None;
                                if let Some(h) = this.held() {
                                    let focus = h.editor.read(cx).focus_handle(cx);
                                    window.focus(&focus, cx);
                                }
                            }))
                            .child(
                                v_flex()
                                    .min_h_full()
                                    .px(px(GUTTER))
                                    .pt(px((line * 0.6).round()))
                                    .pb(px(GUTTER))
                                    .gap(px((line * 0.4).round()))
                                    .child(title_el)
                                    .child(div().w_full().h(px(1.)).flex_shrink_0().mb(px((line * 0.25).round())).bg(theme.border.opacity(0.7)))
                                    .child(editor),
                            ),
                    )
                    .child(crate::shell::edge_fade(&scroll, ground, true, px(0.)))
                    .child(crate::shell::edge_fade_at(&scroll, ground, false, Corners { bottom_right: corner, bottom_left: if shown > 0. { px(0.) } else { corner }, ..Default::default() }))
                    .vertical_scrollbar(&scroll)
                    .into_any_element()
            }
            None => self.render_landing(cx),
        };
        // The tree drawn at its full width in a box that grows, so its
        // rows keep their place as it comes. Its foot rounded as the
        // card's is: a clip in gpui is a rectangle, and the tree's ground
        // stood past the card's corner.
        let tree = (shown > 0.).then(|| {
            let head = self.render_tree_head(cx);
            let rows = self.render_tree(side, window, cx);
            let foot = self.render_tree_foot(side, cx);
            let why = self.render_why(foot.is_some(), &cx.theme().clone());
            div()
                .h_full()
                .flex_shrink_0()
                .w(px(tree_w * shown))
                .overflow_hidden()
                .rounded_bl(corner)
                .border_r_1()
                .border_color(rim.opacity(shown))
                .child(v_flex().w(px(tree_w)).h_full().bg(side).rounded_bl(corner).opacity(shown).relative().child(head).child(rows).children(foot).children(why))
        });
        let body = h_flex().flex_1().min_h_0().children(tree).child(note);
        let pick: Rc<dyn Fn(&mut Self, &'static str, SharedString, &mut Window, &mut Context<Self>)> = Rc::new(|this, what, id, window, cx| this.menu_pick(what, id, window, cx));
        let close: Rc<dyn Fn(&mut Self, &mut Context<Self>)> = Rc::new(|this, cx| this.menus.close(cx));
        let menu = self.menus.render(pick, close, cx);
        let head = self.render_head(window, cx);
        let router = cx.entity().downgrade();
        // The card's own shadow, a popover's: a soft wide one a little
        // under it and a close one that holds its edge, both light; deeper
        // in the dark, where a light one is lost.
        let dark = theme.mode.is_dark();
        let shade = |a: f32, y: f32, blur: f32| BoxShadow { color: gpui::black().opacity(a), offset: point(px(0.), px(y)), blur_radius: px(blur), spread_radius: px(0.), inset: false };
        let shadow = vec![shade(if dark { 0.34 } else { 0.10 }, 8., 22.), shade(if dark { 0.24 } else { 0.05 }, 1., 3.)];
        let card = v_flex()
            .absolute()
            .top(px(ARROW_H))
            .left(px(SHADE))
            .w(px(NOTE_W))
            .h(px(NOTE_H))
            .rounded(px(RADIUS))
            .shadow(shadow)
            .bg(ground)
            .border_1()
            .border_color(edge)
            .overflow_hidden()
            .key_context(crate::shell::SHEET_CONTEXT)
            .track_focus(&self.focus)
            .on_modifiers_changed(cx.listener(|this, e: &ModifiersChangedEvent, _, cx| this.tabs_row.modifiers_changed(&e.modifiers, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::CloseTab, _, cx| this.close_shown(cx)))
            .on_action(cx.listener(|this, _: &crate::shell::ReopenTab, _, cx| this.reopen_tab(cx)))
            .on_action(cx.listener(|this, _: &crate::shell::NewNote, _, cx| this.new_note(cx)))
            .on_action(cx.listener(|this, _: &crate::shell::Tab1, _, cx| this.go_tab(1, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::Tab2, _, cx| this.go_tab(2, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::Tab3, _, cx| this.go_tab(3, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::Tab4, _, cx| this.go_tab(4, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::Tab5, _, cx| this.go_tab(5, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::Tab6, _, cx| this.go_tab(6, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::Tab7, _, cx| this.go_tab(7, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::Tab8, _, cx| this.go_tab(8, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::Tab9, _, cx| this.go_tab(9, cx)))
            .on_action(cx.listener(|this, _: &crate::shell::ToggleSidebar, _, cx| this.toggle_tree(cx)))
            // The files' keys, the main window's, while the keyboard is
            // the window's own, in no words.
            .on_action(cx.listener(|this, _: &TrashNote, window, cx| if this.focus.is_focused(window) { this.trash_chosen(cx) } else { cx.propagate() }))
            .on_action(cx.listener(|this, _: &gpui_component::input::Undo, window, cx| if this.focus.is_focused(window) { this.files_step(false, cx) } else { cx.propagate() }))
            .on_action(cx.listener(|this, _: &gpui_component::input::Redo, window, cx| if this.focus.is_focused(window) { this.files_step(true, cx) } else { cx.propagate() }))
            // Escape: the menu goes, then a name being given.
            .on_action(cx.listener(|this, _: &crate::shell::Escape, _, cx| {
                if this.menus.open.is_some() {
                    this.menus.close(cx);
                } else if this.browse.naming.is_some() {
                    this.cancel_naming(cx);
                } else {
                    cx.propagate();
                }
            }))
            .child(head)
            .child(body)
            .children(self.folder_look.as_ref().map(|(p, _)| p.clone()));
        // The arrow, as a popover's: the ground filled over the card's
        // edge where the two meet, then its sides drawn in the edge's color,
        // running on into the card's own.
        let arrow = canvas(
            |_, _, _| (),
            move |b, _, window, _| {
                let tip = point(b.origin.x + px(arrow_x), b.origin.y);
                // The card's edge is a point wide from its top.
                let foot = b.origin.y + px(ARROW_H + 0.5);
                if let Some(p) = arrow_path(tip, foot, PathBuilder::fill(), true) {
                    window.paint_path(p, ground);
                }
                if let Some(p) = arrow_path(tip, foot, PathBuilder::stroke(px(1.)), false) {
                    window.paint_path(p, edge);
                }
            },
        )
        .absolute()
        .top_0()
        .left(px(SHADE))
        .w(px(NOTE_W))
        .h(px(ARROW_H + 3.));
        // The window fades as a whole, as the system composes it, its
        // shadow with it: drawn once at full ink, nothing is drawn again
        // for the fade. Faded by its words' opacity, every frame cast the
        // shadow anew from what was half drawn, and the window blinked.
        if self.fade.since.is_none() {
            self.fade.since = Some(Instant::now());
        }
        // The system's window told only of a change: at every draw it was a
        // call into the system at every blink and key.
        let alpha = self.fade.now();
        if self.alpha_set.replace(Some(alpha)) != Some(alpha) {
            crate::sys::set_window_alpha(window, alpha);
        }
        if !self.fade.done() {
            window.request_animation_frame();
        }
        div()
            .size_full()
            .relative()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &TogglePin, _, cx| this.toggle_pin(cx)))
            .on_action(cx.listener(|this, _: &OpenInOnyx, _, cx| this.open_in_onyx(cx)))
            .font_family(theme.font_family.clone())
            .text_color(theme.foreground)
            .child(
                div()
                    .size_full()
                    .relative()
                    .child(card)
                    .child(arrow)
                    .children(menu),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                match router.update(cx, |s, _| s.scrollers.route(e, false)) {
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
                    },
                )
                .absolute()
                .size_0(),
            )
    }
}

/// What the menu bar note asks of the main window, once the note is in a
/// tab there: a link followed, a file's choice (`rename`, `open`), Export
/// or Print.
pub(crate) enum Link {
    Path(String),
    Note(String),
    File(String, PathBuf),
    Export,
    Print,
}

/// The main window's view, while it is open.
fn main_shell(cx: &App) -> Option<Entity<Shell>> {
    cx.try_global::<crate::MainShell>().and_then(|m| m.0.upgrade())
}

/// Something asked of the main window, with its window to hand, a moment
/// on, outside whatever update asks. False when it is shut.
fn main_window(f: impl FnOnce(&mut Shell, &mut Window, &mut Context<Shell>) + 'static, cx: &mut App) -> bool {
    let Some((shell, handle)) = cx.try_global::<crate::MainShell>().and_then(|m| Some((m.0.upgrade()?, m.1))).filter(|(_, h)| cx.windows().contains(h)) else { return false };
    cx.defer(move |cx| {
        let _ = cx.update_window(handle, |_, window, cx| shell.update(cx, |s, cx| f(s, window, cx)));
    });
    true
}

fn main_has(path: &Path, cx: &App) -> bool {
    main_shell(cx).is_some_and(|s| s.read(cx).has_note(path))
}

/// What the main window's tab of the note holds, typed or not.
fn main_text(path: &Path, cx: &App) -> Option<String> {
    main_shell(cx).and_then(|s| s.read(cx).text_of_tab(path, cx))
}

/// The vault a note is in: the folder at the top of the container
/// that holds it; else the open one or one opened before that holds it,
/// the deepest, else the note's own folder.
pub(crate) fn vault_of(path: &Path) -> Option<PathBuf> {
    if let Some(folder) = crate::shell::library::container_now() {
        if let Ok(rest) = path.strip_prefix(&folder) {
            if let Some(top) = rest.components().next().filter(|_| rest.components().count() > 1) {
                return Some(folder.join(top));
            }
        }
    }
    let ui = UiState::load();
    ui.vault.iter().chain(ui.recent_vaults.iter()).filter(|v| path.starts_with(v)).max_by_key(|v| v.components().count()).cloned().or_else(|| path.parent().map(Path::to_path_buf))
}

/// The language a note is checked in, while the check is on.
fn writing_language() -> Option<String> {
    let cfg = Config::load();
    cfg.editor.check_writing.then(|| cfg.editor.writing_language.clone())
}

/// The vaults Onyx knows, as the main window's list has them: the main
/// window's, then the container's, the last opened first
/// (`library::vault_list`).
fn known_vaults() -> Vec<PathBuf> {
    let ui = UiState::load();
    crate::shell::library::vault_list(crate::shell::library::container_now().as_deref(), ui.vault.as_deref())
}

fn vault_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string())
}

// -- reached from the main window ------------------------------------------------

/// The main window's tab of a note changed: the menu bar note follows,
/// when it is the same. A moment on, since the main window may have been
/// asked by this note, which is busy then.
pub(crate) fn follow(path: &Path, text: String, cx: &mut App) {
    if !holds(path, cx) {
        return;
    }
    let Some(note) = cx.try_global::<MenuBarNote>().map(|n| n.0.clone()) else { return };
    let path = path.to_path_buf();
    cx.defer(move |cx| note.update(cx, |n, cx| n.follow(&path, &text, cx)));
}

/// The main window wrote a note: the menu bar note, when it is the same,
/// knows what the disk has.
pub(crate) fn written(path: &Path, text: String, cx: &mut App) {
    if !holds(path, cx) {
        return;
    }
    let Some(note) = cx.try_global::<MenuBarNote>().map(|n| n.0.clone()) else { return };
    let path = path.to_path_buf();
    cx.defer(move |cx| note.update(cx, |n, cx| n.written(&path, &text, cx)));
}

/// Whether the menu bar note is this one.
/// Notes renamed or moved in the main window: the menu bar note's tabs
/// follow, a moment on, outside whatever update moved them.
pub(crate) fn moved(moves: &[(PathBuf, PathBuf)], cx: &mut App) {
    let Some(note) = cx.try_global::<MenuBarNote>().map(|g| g.0.clone()) else { return };
    let moves = moves.to_vec();
    cx.defer(move |cx| note.update(cx, |n, cx| n.follow_moves(&moves, cx)));
}

/// Files on their way out of the vault: the menu bar note's tabs of them
/// are written and closed now, before they go.
pub(crate) fn let_go(paths: &[PathBuf], cx: &mut App) {
    let Some(note) = cx.try_global::<MenuBarNote>().map(|g| g.0.clone()) else { return };
    note.update(cx, |n, cx| n.let_go(paths, cx));
}

/// Files ⌘Z brought back in the main window: the menu bar note's tabs of
/// them open again.
pub(crate) fn came_back(here: &[PathBuf], cx: &mut App) {
    let Some(note) = cx.try_global::<MenuBarNote>().map(|g| g.0.clone()) else { return };
    let here = here.to_vec();
    cx.defer(move |cx| note.update(cx, |n, cx| n.came_back(&here, cx)));
}

/// The vault's files changed in the main window: the tree is read again.
pub(crate) fn files_changed(cx: &mut App) {
    let Some(note) = cx.try_global::<MenuBarNote>().map(|g| g.0.clone()) else { return };
    cx.defer(move |cx| {
        note.update(cx, |n, cx| {
            n.read_tree(cx);
            cx.notify();
        })
    });
}

pub(crate) fn holds(path: &Path, cx: &App) -> bool {
    cx.try_global::<MenuBarNote>().is_some_and(|n| n.1.borrow().iter().any(|p| p == path))
}

pub(crate) fn push_look(look: Look, cx: &mut App) {
    let Some(note) = cx.try_global::<MenuBarNote>().map(|n| n.0.clone()) else { return };
    cx.defer(move |cx| note.update(cx, |n, cx| n.set_look(look, cx)));
}

pub(crate) fn push_writing(writing: Option<String>, words: Rc<Words>, cx: &mut App) {
    let Some(note) = cx.try_global::<MenuBarNote>().map(|n| n.0.clone()) else { return };
    cx.defer(move |cx| note.update(cx, |n, cx| n.set_writing(writing, words, cx)));
}

/// A drawing of the window for a probe (`ONYX_GO=menunote`), written as
/// the main window's shot is.
pub(crate) fn shoot(path: &Path, cx: &mut App) -> Result<(), String> {
    let note = cx.try_global::<MenuBarNote>().map(|n| n.0.clone()).ok_or("no menu bar note")?;
    let handle = note.read(cx).window.ok_or("the menu bar note is shut")?;
    handle.update(cx, |_, window, _| crate::sys::shoot_window(window, path)).map_err(|e| e.to_string())?
}

/// For a probe (`ONYX_GO`): `menunote` opens or closes it as the item
/// does; `menunote:hold=<file>` holds that note; `menunote:type=<words>`
/// types in it through the editor, as keys do; `menunote:pin`,
/// `menunote:drag=<x>,<y>,<to x>,<to y>` (a left drag in its window),
/// `menunote:look=<folder>` (its icon and color's sheet),
/// `menunote:looksearch=<words>`, `menunote:lookcolor` (a new color's panel),
/// `menunote:tree` (the tree shut or opened), `menunote:vaults` (the
/// vault's menu),
/// `menunote:vault=<folder>`, `menunote:fold=<folder>` (open or shut),
/// `menunote:pick=<file>`, `menunote:new=<folder>` (a name asked there),
/// `menunote:name=<words>` (typed over it and given),
/// `menunote:onyx` (Open in Onyx), `menunote:shut` (another app taking the
/// keyboard, which closes it unless pinned); `menunote:say` prints its
/// state on stderr.
pub(crate) fn probe(step: &str, cx: &mut App) {
    let Some(note) = cx.try_global::<MenuBarNote>().map(|n| n.0.clone()) else { return };
    let rest = step.strip_prefix("menunote").unwrap_or("");
    match rest {
        "" => note.update(cx, |n, cx| n.toggle(false, cx)),
        ":say" => probe_say(cx),
        // The files as the note's window has them: its tabs, the file
        // chosen, where the keyboard is, and the vault's notes on the disk.
        ":files" => {
            let handle = note.read(cx).window;
            let keys = handle.and_then(|h| {
                let (own, editor) = (note.read(cx).focus.clone(), note.read(cx).held().map(|h| h.editor.clone()));
                h.update(cx, |_, w, cx| if own.is_focused(w) { "window" } else if editor.is_some_and(|e| e.read(cx).focus_handle(cx).is_focused(w)) { "words" } else { "elsewhere" }).ok()
            });
            let n = note.read(cx);
            let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let disk: Vec<String> = n.browse.vault.as_ref().map(|r| vault::notes(r).iter().map(|p| p.strip_prefix(r).unwrap_or(p).to_string_lossy().to_string()).collect()).unwrap_or_default();
            eprintln!("menufiles shown {:?} tabs {:?} chosen {:?} keys {keys:?} disk {disk:?}", n.path().map(name), n.tabs.iter().map(|h| name(&h.path)).collect::<Vec<_>>(), n.chosen().map(name));
        }
        // What the main window shows: its vault and its note.
        ":main" => {
            let shown = main_shell(cx).map(|s| {
                let s = s.read(cx);
                (s.vault_root().map(Path::to_path_buf), s.shown_editor().map(|e| e.read(cx).text()))
            });
            eprintln!("main {shown:?}");
        }
        ":pin" => note.update(cx, |n, cx| n.toggle_pin(cx)),
        ":tree" => note.update(cx, |n, cx| n.toggle_tree(cx)),
        // The folder's sheet of icon and color, opened in the window.
        r if r.starts_with(":look=") => {
            let folder = PathBuf::from(&r[":look=".len()..]);
            let Some(handle) = note.read(cx).window else { return };
            let _ = cx.update_window(handle.into(), |_, window, cx| note.update(cx, |n, cx| n.open_folder_look(folder, window, cx)));
        }
        // That sheet's search, and a new color's panel.
        r if r.starts_with(":looksearch=") => {
            let words = r[":looksearch=".len()..].to_string();
            let (Some(handle), Some(picker)) = (note.read(cx).window, note.read(cx).folder_look.as_ref().map(|(p, _)| p.clone())) else { return };
            let _ = cx.update_window(handle.into(), |_, window, cx| picker.update(cx, |p, cx| p.probe_search(&words, window, cx)));
        }
        ":lookcolor" => {
            let (Some(handle), Some(picker)) = (note.read(cx).window, note.read(cx).folder_look.as_ref().map(|(p, _)| p.clone())) else { return };
            let _ = cx.update_window(handle.into(), |_, window, cx| picker.update(cx, |p, cx| p.probe_making(window, cx)));
        }
        // A left drag in the note's window, in its points, as the main
        // window's `drag=`: into this window alone, moved in ten steps so
        // a drag starts and follows.
        r if r.starts_with(":drag=") => {
            let n: Vec<f32> = r[":drag=".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
            let Some(handle) = note.read(cx).window else { return };
            if let [x, y, tx, ty] = n[..] {
                // Through the window, not its root view, which the drag's
                // drawing reads while the events are handled.
                let _ = cx.update_window(handle.into(), |_, window, cx| {
                    let (button, modifiers) = (MouseButton::Left, Modifiers::default());
                    let at = |k: f32| point(px(x + (tx - x) * k), px(y + (ty - y) * k));
                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: at(0.), pressed_button: None, modifiers }), cx);
                    window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button, position: at(0.), modifiers, click_count: 1, first_mouse: false }), cx);
                    for i in 1..=10 {
                        window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: at(i as f32 / 10.), pressed_button: Some(button), modifiers }), cx);
                    }
                    window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button, position: at(1.), modifiers, click_count: 1 }), cx);
                });
            }
        }
        // A left press and release in the note's window, in its points,
        // and how many clicks it counts as: two for a double click.
        r if r.starts_with(":press=") => {
            let n: Vec<f32> = r[":press=".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
            let Some(handle) = note.read(cx).window else { return };
            if let [x, y, ref rest @ ..] = n[..] {
                let clicks = rest.first().map_or(1, |c| *c as usize);
                let _ = cx.update_window(handle.into(), |_, window, cx| {
                    let (button, modifiers, position) = (MouseButton::Left, Modifiers::default(), point(px(x), px(y)));
                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers }), cx);
                    window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button, position, modifiers, click_count: clicks, first_mouse: false }), cx);
                    window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button, position, modifiers, click_count: clicks }), cx);
                });
            }
        }
        // The note's name typed in its field and kept, as ↩ does.
        r if r.starts_with(":title=") => {
            let name = r[":title=".len()..].to_string();
            let Some(handle) = note.read(cx).window else { return };
            let _ = cx.update_window(handle.into(), |_, window, cx| {
                note.update(cx, |n, cx| {
                    if let Some(field) = n.title_field(window, cx) {
                        field.update(cx, |f, cx| f.set_value(name, window, cx));
                        n.keep_title(&field, window, cx);
                    }
                })
            });
        }
        // The pointer moved there in the note's window, nothing pressed.
        r if r.starts_with(":point=") => {
            let n: Vec<f32> = r[":point=".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
            let Some(handle) = note.read(cx).window else { return };
            if let [x, y] = n[..] {
                let _ = cx.update_window(handle.into(), |_, window, cx| {
                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: point(px(x), px(y)), pressed_button: None, modifiers: Modifiers::default() }), cx);
                });
            }
        }
        // A key pressed in the note's window, as the main window's `key=`.
        r if r.starts_with(":key=") => {
            let Some(handle) = note.read(cx).window else { return };
            if let Ok(keystroke) = Keystroke::parse(&r[":key=".len()..]) {
                let _ = cx.update_window(handle.into(), |_, window, cx| {
                    window.dispatch_event(PlatformInput::KeyDown(KeyDownEvent { keystroke, is_held: false, prefer_character_input: false }), cx);
                });
            }
        }
        ":vaults" => note.update(cx, |n, cx| n.vault_menu(cx)),
        r if r.starts_with(":vault=") => note.update(cx, |n, cx| n.set_vault(Some(PathBuf::from(&r[":vault=".len()..])), cx)),
        r if r.starts_with(":fold=") => note.update(cx, |n, cx| n.toggle_folder(PathBuf::from(&r[":fold=".len()..]), cx)),
        r if r.starts_with(":pick=") => note.update(cx, |n, cx| n.pick_note(PathBuf::from(&r[":pick=".len()..]), cx)),
        r if r.starts_with(":new=") => note.update(cx, |n, cx| n.start_naming(PathBuf::from(&r[":new=".len()..]), cx)),
        r if r.starts_with(":name=") => {
            let words = r[":name=".len()..].to_string();
            let handle = note.read(cx).window;
            if let (Some(handle), Some(input)) = (handle, note.read(cx).browse.naming.as_ref().map(|n| n.input.clone())) {
                let _ = handle.update(cx, |_, window, cx| input.update(cx, |i, cx| i.set_value(words, window, cx)));
            }
            note.update(cx, |n, cx| n.finish_naming(cx));
        }
        ":onyx" => note.update(cx, |n, cx| n.open_in_onyx(cx)),
        ":shut" => note.update(cx, |n, cx| {
            n.save(cx);
            if !n.pinned {
                n.shut = Some(Instant::now());
                n.close(cx);
            }
        }),
        r if r.starts_with(":hold=") => note.update(cx, |n, cx| n.open_tab(PathBuf::from(&r[":hold=".len()..]), cx)),
        ":close" => note.update(cx, |n, cx| n.close_shown(cx)),
        ":reopen" => note.update(cx, |n, cx| n.reopen_tab(cx)),
        r if r.starts_with(":tab=") => note.update(cx, |n, cx| n.go_tab(r[":tab=".len()..].parse().unwrap_or(1), cx)),
        r if r.starts_with(":type=") => {
            let words = r[":type=".len()..].to_string();
            if let Some(editor) = note.read(cx).held().map(|h| h.editor.clone()) {
                editor.update(cx, |e, cx| e.type_text(&words, cx));
            }
        }
        _ => eprintln!("onyx: no such step: {step}"),
    }
}

/// For a probe: the window's state on stderr.
fn probe_say(cx: &mut App) {
    let Some(note) = cx.try_global::<MenuBarNote>().map(|n| n.0.clone()) else { return };
    let handle = note.read(cx).window;
    let bounds = handle.and_then(|h| h.update(cx, |_, w, _| w.bounds()).ok());
    // Whether the window has the keyboard, and whether the note's editor does.
    let key = handle.and_then(|h| h.update(cx, |_, w, _| w.is_window_active()).ok());
    let focused = handle.and_then(|h| {
        let editor = note.read(cx).held().map(|h| h.editor.clone());
        h.update(cx, |_, w, cx| editor.is_some_and(|e| e.read(cx).focus_handle(cx).is_focused(w))).ok()
    });
    let n = note.read(cx);
    let text = n.held().map(|h| h.editor.read(cx).text()).unwrap_or_default();
    let disk = n.path().and_then(|p| vault::read(p).ok()).unwrap_or_default();
    let naming = n.browse.naming.as_ref().map(|m| (m.dir.clone(), m.input.read(cx).value().to_string(), m.why.clone()));
    eprintln!("menunote tabs={:?} shown={:?} key={key:?} editor_focused={focused:?} open={} pinned={} note={:?} dirty={} text={:?} disk={:?} bounds={:?} arrow={} tree={} vault={:?} open={:?} naming={naming:?}", n.tabs.iter().map(|h| vault::title(&h.path)).collect::<Vec<_>>(), n.shown, n.is_open(), n.pinned, n.path(), n.held().is_some_and(|h| h.dirty), text, disk, bounds, n.arrow_x, n.tree_open, n.browse.vault, n.browse.open);
}
