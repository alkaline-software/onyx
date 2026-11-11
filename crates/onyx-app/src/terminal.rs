//! The terminal beside a note, at the page's right: the person's shell
//! in the vault's folder, a tab each, taken from Emaki's shell terminal
//! with its look (Kaku's), its keys, its selection, its links and its
//! pointer. A shell is a `Pty` (`onyx_core::pty`): a child on a pty this
//! process owns and a screen model that draws nothing. Here the screen is
//! drawn, a row a line in the code face, the keyboard is written to the
//! child, and the pty is kept the size of its room. `docs/window.md`,
//! The terminal.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};

use onyx_core::config::TerminalConfig;
use onyx_core::pty::{self, Pty};

use crate::i18n::{count, t, tf};
use crate::menu::Entry;
use crate::widgets::{choice_pill, swallow_click, tip, Press};

/// The keys the terminal takes before the window's: Tab and ⇧Tab (the
/// window's would move the keyboard on), ⌘K, ⌘T and ⌘N.
pub(crate) const TERMINAL_CONTEXT: &str = "Terminal";
actions!(onyx_terminal, [TermTab, TermBackTab, TermClear, TermNewTab, TermFind, TermFindNext, TermFindPrev]);

/// The find card over the screen: its keys are its own (Esc, ⌘F, ⌘G),
/// and nothing typed in it reaches the shell.
pub(crate) const TERM_FIND_CONTEXT: &str = "TermFind";

/// The terminal's own keys, bound in its context, after the window's so a
/// tie of depth is theirs: off a Mac every Ctrl and a letter is the
/// shell's.
pub(crate) fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(TERMINAL_CONTEXT);
    let mut keys = vec![
        KeyBinding::new("tab", TermTab, c),
        KeyBinding::new("shift-tab", TermBackTab, c),
        KeyBinding::new("secondary-k", TermClear, c),
        KeyBinding::new("secondary-t", TermNewTab, c),
        KeyBinding::new("secondary-n", TermNewTab, c),
        KeyBinding::new("secondary-f", TermFind, c),
        KeyBinding::new("secondary-g", TermFindNext, c),
        KeyBinding::new("secondary-shift-g", TermFindPrev, c),
        KeyBinding::new("secondary-f", TermFind, Some(TERM_FIND_CONTEXT)),
        KeyBinding::new("secondary-g", TermFindNext, Some(TERM_FIND_CONTEXT)),
        KeyBinding::new("secondary-shift-g", TermFindPrev, Some(TERM_FIND_CONTEXT)),
        KeyBinding::new("escape", crate::shell::Escape, Some(TERM_FIND_CONTEXT)),
    ];
    if !cfg!(target_os = "macos") {
        keys.extend(('a'..='z').map(|ch| KeyBinding::new(&format!("ctrl-{ch}"), NoAction, c)));
        keys.extend([
            KeyBinding::new("ctrl-enter", NoAction, c),
            KeyBinding::new("ctrl-shift-k", TermClear, c),
            KeyBinding::new("ctrl-shift-t", TermNewTab, c),
            KeyBinding::new("ctrl-shift-w", crate::shell::CloseTab, c),
        ]);
    }
    keys
}

/// How wide the terminal is to begin with, and the least it is drawn.
pub(crate) const TERM_W: f32 = 520.;
pub(crate) const TERM_LEAST: f32 = 300.;
/// A font's own line over the letters' size, as JetBrains Mono has it.
const LINE_RATIO: f32 = 1.32;
/// The wheel's pixels to a row, as Kaku counts a trackpad's.
const WHEEL_ROW: f32 = 15.;
/// The cursor's half blink.
const BLINK: Duration = Duration::from_millis(500);
/// How often a terminal that shows looks for something new on its screen.
const POLL: Duration = Duration::from_millis(33);
/// A drag held past the screen's edge scrolls it this often, by so many
/// rows a second at the edge, so many more for each pixel further out,
/// and no more than so many.
const EDGE_TICK: Duration = Duration::from_millis(16);
const EDGE_ROWS: f32 = 10.;
const EDGE_MORE: f32 = 1.2;
const EDGE_MOST: f32 = 120.;
/// How long the panel says a selection was copied.
const COPIED_FOR: Duration = Duration::from_millis(2500);
/// The row of tabs over the screen.
const HEAD_H: f32 = 31.;
/// The radius of the sheet the terminal stands on (`Shell::render_terminal`).
pub(crate) const SHEET_R: f32 = 10.;
/// A shell's tab in the head: its height, the most and the least it is
/// wide, and the room between two.
const TAB_H: f32 = 22.;
const TAB_MAX: f32 = 132.;
const TAB_MIN: f32 = 92.;
const TAB_GAP: f32 = 0.;
/// How long a new tab takes to grow in, and one screen to take another's place.
const TAB_ANIM: Duration = Duration::from_millis(180);

/// One shell: a tab in the head and the pty behind it.
struct ShellTab {
    /// Which tab it is, never given twice.
    id: u64,
    /// The number in its name: one more than the largest there was when
    /// it was made, so a closed tab's number is given again.
    n: u64,
    pty: Arc<Pty>,
    born: Instant,
}

/// The shells, in the order of their tabs.
#[derive(Default)]
struct Shells {
    tabs: Vec<ShellTab>,
    /// The tab that shows.
    on: u64,
    /// The last `id` given.
    last: u64,
    /// The first tab has been made: an empty row after that is the
    /// person's doing and stays empty while the side shows. Brought up
    /// again empty, the side makes one as it did at first (`term_go`).
    begun: bool,
    /// Why the last shell did not start.
    failed: Option<String>,
}

impl Shells {
    fn current(&self) -> Option<&ShellTab> {
        self.tabs.iter().find(|t| t.id == self.on)
    }
}

/// What is selected: where it began and where it runs to, each (line,
/// column). A line is counted from the screen's top row with nothing
/// scrolled back, and is less than 0 for what has left the screen, so a
/// selection keeps its place while the screen scrolls under it. By the
/// letter, the word (a double click) or the row (a triple), each end a
/// cell; or from the keyboard, each end a place between two cells, as a
/// caret is (`unit` 0). A press that never moved selects nothing.
#[derive(Clone, Copy)]
struct Sel {
    anchor: (i64, u16),
    head: (i64, u16),
    unit: usize,
    /// The same columns of every row (a drag with ⌥), not a run of text.
    block: bool,
}

/// Kaku's `selection_word_boundary`: what a word is made of.
fn word_char(c: char) -> bool {
    !c.is_whitespace() && c != '\0' && !"{}[]()\"'-".contains(c)
}

impl Sel {
    /// The two ends, the earlier first.
    fn ends(&self) -> ((i64, u16), (i64, u16)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// The columns selected in each of `cells`, the lines from `top` on,
    /// from and up to.
    fn spans(&self, top: i64, cells: &[Vec<char>]) -> Vec<Option<(usize, usize)>> {
        let (a, b) = self.ends();
        let mut out = vec![None; cells.len()];
        if self.unit <= 1 && a == b {
            return out;
        }
        let at = |l: i64| usize::try_from(l - top).ok().filter(|i| *i < cells.len());
        let last = top + cells.len() as i64 - 1;
        if self.block {
            let (from, to) = (self.anchor.1.min(self.head.1) as usize, self.anchor.1.max(self.head.1) as usize + 1);
            for l in a.0.max(top)..=b.0.min(last) {
                out[at(l).unwrap()] = Some((from, to));
            }
            return out;
        }
        let (mut from, mut to) = (a.1 as usize, b.1 as usize + (self.unit > 0) as usize);
        match self.unit {
            2 => {
                if let Some(row) = at(a.0).map(|i| &cells[i]) {
                    while from > 0 && from <= row.len() && row.get(from).is_some_and(|c| word_char(*c)) && word_char(row[from - 1]) {
                        from -= 1;
                    }
                }
                if let Some(row) = at(b.0).map(|i| &cells[i]) {
                    while to < row.len() && word_char(row[to - 1]) && word_char(row[to]) {
                        to += 1;
                    }
                }
            }
            3 => (from, to) = (0, usize::MAX),
            _ => {}
        }
        for l in a.0.max(top)..=b.0.min(last) {
            let i = at(l).unwrap();
            let (f, t) = (if l == a.0 { from } else { 0 }, if l == b.0 { to } else { usize::MAX });
            // A row with nothing on it still shows that it is selected.
            let t = t.min(cells[i].len().max(1));
            if f < t {
                out[i] = Some((f, t));
            }
        }
        out
    }
}

/// What is being typed, as a field's text is to its caret: the
/// rows from where it begins to where it ends, every line of it, where
/// the typing begins on the first (after the prompt), and which rows run
/// on into the next as one paragraph. Rows are lines counted from the
/// screen's top row. Claude Code's input is one paragraph: its rows
/// carry no sign of which were wrapped.
struct Typing {
    first: i64,
    rows: Vec<Vec<char>>,
    start: (i64, u16),
    runs: Vec<bool>,
    boxed: bool,
}

impl Typing {
    /// Whether a place of the screen is in what is being typed.
    fn holds(&self, at: (i64, u16)) -> bool {
        at.0 >= self.first && at.0 < self.first + self.rows.len() as i64 && at >= self.start
    }

    /// The paragraph a line is in, from its start to its end.
    fn para(&self, line: i64) -> ((i64, u16), (i64, u16)) {
        let at = (line - self.first).clamp(0, self.rows.len() as i64 - 1).max(0) as usize;
        let (mut i, mut j) = (at, at);
        while i > 0 && self.runs[i - 1] {
            i -= 1;
        }
        while j + 1 < self.rows.len() && self.runs[j] {
            j += 1;
        }
        let ends = |k: usize| (self.first + k as i64, self.rows.get(k).map_or(0, Vec::len) as u16);
        ((ends(i).0, 0).max(self.start), ends(j))
    }
}

/// Where the cursor is once the program has drawn it after what was
/// sent: looked at until it has moved and holds still, or for long enough
/// that it will not move.
fn settled(pty: &Pty, was: (u16, u16)) -> (u16, u16) {
    let began = Instant::now();
    let (mut last, mut still) = (was, 0);
    while began.elapsed() < Duration::from_millis(250) {
        std::thread::sleep(Duration::from_millis(6));
        let now = pty.cursor().unwrap_or(last);
        if now != last {
            (last, still) = (now, 0);
        } else if now != was || began.elapsed() > Duration::from_millis(80) {
            still += 1;
            if still >= 3 {
                break;
            }
        }
    }
    last
}

/// The cursor steered to a place with arrow keys, as a field's caret
/// goes to a click: sent, looked at, and sent again until it is there or
/// goes no further. A letter is an arrow, but the program says where its
/// letters are (a new line, a wrapped row, an indented one), so where the
/// cursor goes is read, not worked out. Left and Right only: Up at the
/// first line is the shell's history. Where it ended.
fn steer(pty: &Pty, to: (u16, u16)) -> (u16, u16) {
    let Some(mut at) = pty.cursor() else { return to };
    let row_len = |r: u16| pty.rows_back(0).0.get(r as usize).map(|row| row_cells(row).len() as u16).unwrap_or(0);
    for _ in 0..64 {
        if at == to {
            break;
        }
        let on = at < to;
        // Along a row by its cells; to another row, past the end of this
        // one, or back past its start.
        let n = if at.0 == to.0 { at.1.abs_diff(to.1) } else if on { row_len(at.0).saturating_sub(at.1) + 1 } else { at.1 + 1 };
        let key: &[u8] = match (on, pty.app_cursor()) {
            (true, false) => b"\x1b[C",
            (true, true) => b"\x1bOC",
            (false, false) => b"\x1b[D",
            (false, true) => b"\x1bOD",
        };
        pty.write(&key.repeat(n.max(1) as usize));
        let now = settled(pty, at);
        if now == at {
            break;
        }
        at = now;
    }
    at
}

/// How many letters lie from the cursor to a place after it, counted
/// by walking there with Right, in steps that stay inside the text: half
/// the cells to it on its row (a letter takes one or two), else half of
/// what is left of the cursor's row, and one at a time near it. Each step
/// that moved is that many letters, a new line or a wrapped row's end
/// one. The cursor ends there, or where the text ends before it.
fn letters_to(pty: &Pty, to: (u16, u16)) -> usize {
    let Some(mut at) = pty.cursor() else { return 0 };
    let row_len = |r: u16| pty.rows_back(0).0.get(r as usize).map(|row| row_cells(row).len() as u16).unwrap_or(0);
    let mut letters = 0;
    for _ in 0..2000 {
        if at >= to {
            break;
        }
        let n = if at.0 == to.0 { (to.1 - at.1) / 2 } else { row_len(at.0).saturating_sub(at.1) / 2 }.max(1);
        let key: &[u8] = if pty.app_cursor() { b"\x1bOC" } else { b"\x1b[C" };
        pty.write(&key.repeat(n as usize));
        let now = settled(pty, at);
        if now == at || now > to && n > 1 {
            break;
        }
        letters += n as usize;
        at = now;
    }
    letters
}

/// What the screen keeps of its own: focus, selection, scrolling back,
/// size, cursor.
struct TermView {
    focus: FocusHandle,
    /// Where its screen was last drawn, and the rows and columns that fit.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    size: Rc<Cell<(u16, u16)>>,
    /// How many rows back it is scrolled, and what of the wheel is left
    /// over from the last whole row.
    back: usize,
    wheel: f32,
    /// A cell's width, as last drawn.
    cell_w: f32,
    /// What was typed while the cursor was being steered (`term_edit`),
    /// sent once it is there; None while nothing is.
    edit: Arc<Mutex<Option<Vec<u8>>>>,
    /// The button held on a program that hears the mouse, and the cell
    /// last told to it.
    mouse: Option<u8>,
    mouse_cell: (u16, u16),
    /// What is selected, whether the drag that selects is still held,
    /// and when a selection was last copied.
    sel: Option<Sel>,
    selecting: bool,
    copied: Option<Instant>,
    /// The room around the screen as last drawn: at the sides and the
    /// top, and at the foot.
    pad: (f32, f32),
    /// Where the cursor was at the last draw and since when, which is
    /// when its blinking began again; whether it was drawn; and whether
    /// it had the keyboard.
    blink: ((u16, u16), Instant),
    blink_on: bool,
    focused: bool,
    /// The link under the pointer.
    link: Option<Link>,
    /// The cell the pointer was last over, column and row.
    hover: (u16, u16),
    /// A drag that selects, held past the screen's top (less than 0) or
    /// foot, by so many pixels, and the column it is over: the screen
    /// scrolls on under it while it is there. What of a row is left over
    /// from the last step, and whether the steps are running.
    edge: f32,
    edge_col: u16,
    edge_rows: f32,
    edge_run: bool,
    /// What its bar reads and moves (`TermScroll`).
    scroll: TermScroll,
}

/// A terminal's scrolling back, as the toolkit's bar takes a scroller:
/// the kept lines over the screen are the content, and how far back it
/// is, the offset. The terminal has no scroller of its own (it draws
/// the rows for where it is back to), so this is what the bar every
/// scroller has reads and moves.
///
/// At its foot it says the kept lines it last saw: what a program prints
/// there adds lines, and the bar, which shows whenever the offset
/// changes, came up at every one. It takes up the count again whenever it
/// is scrolled.
#[derive(Clone, Default)]
struct TermScroll(Rc<std::cell::RefCell<TermScrollAt>>);

#[derive(Default)]
struct TermScrollAt {
    bounds: Bounds<Pixels>,
    line_h: f32,
    rows: usize,
    kept: usize,
    back: usize,
    /// How far back a drag of the bar's thumb asks for, until it is drawn.
    asked: Option<usize>,
}

impl TermScroll {
    /// As the screen is drawn: where, how tall a line is, how many rows,
    /// how far back, and the kept lines, read only once it has moved.
    fn drawn(&self, bounds: Bounds<Pixels>, line_h: f32, rows: usize, back: usize, kept: impl FnOnce() -> usize) {
        let mut at = self.0.borrow_mut();
        if at.back != back {
            at.kept = kept();
        }
        (at.bounds, at.line_h, at.rows, at.back) = (bounds, line_h.max(1.), rows, back);
    }

    /// How far back a drag of the thumb asked for, once.
    fn asked(&self) -> Option<usize> {
        self.0.borrow_mut().asked.take()
    }
}

impl gpui_component::scroll::ScrollbarHandle for TermScroll {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().bounds
    }

    fn offset(&self) -> Point<Pixels> {
        let at = self.0.borrow();
        point(px(0.), px(-((at.kept.saturating_sub(at.back)) as f32 * at.line_h)))
    }

    fn set_offset(&self, offset: Point<Pixels>) {
        let mut at = self.0.borrow_mut();
        let from_top = (-f32::from(offset.y) / at.line_h).round().max(0.) as usize;
        let back = at.kept.saturating_sub(from_top);
        at.back = back;
        at.asked = Some(back);
    }

    fn content_size(&self) -> Size<Pixels> {
        let at = self.0.borrow();

        size(at.bounds.size.width, px((at.kept + at.rows) as f32 * at.line_h).max(at.bounds.size.height))
    }
}

impl TermView {
    fn new(cx: &mut App) -> Self {
        TermView {
            focus: cx.focus_handle(),
            bounds: Rc::new(Cell::new(Bounds::default())),
            size: Rc::new(Cell::new((0, 0))),
            back: 0,
            wheel: 0.,
            cell_w: 0.,
            scroll: TermScroll::default(),
            edit: Arc::new(Mutex::new(None)),
            mouse: None,
            mouse_cell: (0, 0),
            sel: None,
            selecting: false,
            copied: None,
            pad: (0., 0.),
            blink: ((0, 0), Instant::now()),
            blink_on: true,
            focused: false,
            link: None,
            hover: (u16::MAX, u16::MAX),
            edge: 0.,
            edge_col: 0,
            edge_rows: 0.,
            edge_run: false,
        }
    }
}

/// A link on the screen: its row, the cells it takes, and what it opens.
#[derive(Clone, PartialEq)]
struct Link {
    row: u16,
    from: usize,
    to: usize,
    target: Target,
}

#[derive(Clone, PartialEq)]
enum Target {
    Url(String),
    File(std::path::PathBuf),
}

/// How many columns of a row stand before its `chars`-th character, by
/// the screen model's own cells.
fn cells_before(row: &[pty::Span], chars: usize) -> usize {
    let (mut seen, mut cols) = (0usize, 0usize);
    for (cell, w) in row.iter().flat_map(|s| s.each_cell()) {
        seen += cell.chars().count();
        if seen > chars {
            break;
        }
        cols += w as usize;
    }
    cols
}

/// A row as its cells hold it: a cell's first character, and a wide
/// one's second column a NUL.
fn row_cells(row: &[pty::Span]) -> Vec<char> {
    let mut out = Vec::new();
    for (cell, w) in row.iter().flat_map(|s| s.each_cell()) {
        out.push(cell.chars().next().unwrap_or(' '));
        if w > 1 {
            out.push('\0');
        }
    }
    out
}

/// Paths as a shell reads them, a space between: each character a shell
/// would read as its own after a backslash, as a Mac's terminals type a
/// dropped file; in double quotes for PowerShell where one needs them.
fn path_words(paths: &[std::path::PathBuf]) -> String {
    let word = |p: &std::path::PathBuf| {
        let p = p.to_string_lossy();
        if cfg!(windows) {
            return if p.contains([' ', '\'', '&', '(', ')', ';', ',']) { format!("\"{p}\"") } else { p.into_owned() };
        }
        p.chars().fold(String::new(), |mut out, c| {
            if c.is_whitespace() || "\\'\"()[]{}&;|<>*?$`!#".contains(c) {
                out.push('\\');
            }
            out.push(c);
            out
        })
    };
    paths.iter().map(word).collect::<Vec<_>>().join(" ")
}

/// A shell's tab being dragged along the row.

/// A shell's tab being dragged along the row.
#[derive(Clone)]
struct DragShell(u64, String);

impl Render for DragShell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div().px(px(10.)).py(px(4.)).rounded(px(6.)).bg(theme.popover).border_1().border_color(theme.border).text_size(px(11.5)).text_color(theme.foreground).child(self.1.clone())
    }
}

/// What a right click on the screen does: the window's menu, at a point,
/// with these choices; a choice comes back by `menu_do`.
pub(crate) type TermMenu = Rc<dyn Fn(Point<Pixels>, Vec<Entry>, &mut Window, &mut App)>;

/// What the terminal asks of the window: to be put away (⌘W with no
/// tab left).
pub(crate) enum TermEvent {
    Away,
}

/// The terminal beside a note: the shells, a tab each, and the screen of
/// the one that shows. `docs/window.md`, The terminal.
pub(crate) struct TermPanel {
    shells: Shells,
    view: TermView,
    /// Where a new shell starts: the vault's folder.
    cwd: PathBuf,
    cfg: TerminalConfig,
    /// The window's code face.
    mono: SharedString,
    menu: TermMenu,
    /// Whether the window shows it; put away, it is drawn as it was while
    /// it goes, with nothing started, sized or measured for it.
    shown: bool,
    /// The watch that draws the screen again when it changed.
    poll: Option<Task<()>>,
    /// The tab left, when, and the swap's number: its screen fades out.
    swap: Option<(u64, Instant, u32)>,
    /// Until when the tab that shows is brought into view in its row.
    reveal: Option<Instant>,
    tabs_scroll: ScrollHandle,
    drag: Option<u64>,
    /// The find card, made the first time it opens.
    find: Option<Find>,
    /// What an input method is composing, drawn at the cursor until it is
    /// typed.
    marked: Option<String>,
}

/// The find card's state: its field, whether it shows and since when
/// (for its coming and going), what it found and which one stands, and
/// when the screen was last looked through.
struct Find {
    input: Entity<InputState>,
    open: bool,
    since: Instant,
    hits: Vec<Hit>,
    at: usize,
    looked: Instant,
    _sub: Subscription,
}

/// A match: its line (counted as a selection's is, less than 0 for what
/// has left the screen) and the columns it takes.
#[derive(Clone, Copy, PartialEq)]
struct Hit {
    line: i64,
    from: usize,
    to: usize,
}

impl EventEmitter<TermEvent> for TermPanel {}

impl Focusable for TermPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.view.focus.clone()
    }
}

/// The letters' size, a row's height over the face's own line, and the
/// room round the screen at its sides and top and at its foot: the
/// middle steps of Emaki's.
const TEXT: f32 = 13.;
const LINE_SCALE: f32 = 1.15;
const PAD: (f32, f32) = (15., 8.);

impl TermPanel {
    pub(crate) fn new(cwd: PathBuf, cfg: TerminalConfig, mono: SharedString, menu: TermMenu, cx: &mut Context<Self>) -> Self {
        Self { shells: Shells::default(), view: TermView::new(cx), cwd, cfg, mono, menu, shown: false, poll: None, swap: None, reveal: None, tabs_scroll: ScrollHandle::new(), drag: None, find: None, marked: None }
    }

    /// Where the next shell starts: a vault opened since.
    pub(crate) fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    pub(crate) fn set_look(&mut self, cfg: TerminalConfig, mono: SharedString, cx: &mut Context<Self>) {
        self.cfg = cfg;
        self.mono = mono;
        cx.notify();
    }

    /// The window shows it, or puts it away. Brought up with no tab in
    /// it, it opens on a new one, as it did the first time.
    pub(crate) fn set_shown(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.shown == on {
            return;
        }
        self.shown = on;
        self.view.back = 0;
        self.view.sel = None;
        if on {
            if self.shells.tabs.is_empty() {
                self.shells.begun = false;
            }
            self.watch(cx);
        } else {
            self.poll = None;
        }
        cx.notify();
    }

    /// While it shows, the screen is drawn again whenever it changed: a
    /// pty writes from a thread of its own and says nothing to the
    /// window. The cursor's blink is a draw too, while it has the keyboard.
    fn watch(&mut self, cx: &mut Context<Self>) {
        self.poll = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let on = this.update(cx, |this, cx| {
                let v = &this.view;
                let blink = v.focused && this.cfg.cursor_blink && this.blink_on() != v.blink_on;
                if blink || this.pty().is_some_and(|p| p.quiet_for() < POLL * 4 || !p.alive()) {
                    cx.notify();
                }
                this.shown
            });
            if !on.unwrap_or(false) {
                break;
            }
        }));
    }

    /// Whether the cursor is in the lit half of its blink: on and off by
    /// turns, with no fade, from when it last moved.
    fn blink_on(&self) -> bool {
        (self.view.blink.1.elapsed().as_millis() / BLINK.as_millis()) % 2 == 0
    }

    /// A row's height.
    fn line_h(&self) -> f32 {
        (TEXT * LINE_RATIO * LINE_SCALE).round()
    }

    /// The scheme the settings ask for: the window's own appearance, or
    /// one of the two kept whatever the window is.
    fn scheme(&self, cx: &App) -> &'static Scheme {
        scheme(match self.cfg.theme.as_str() {
            "dark" => false,
            "light" => true,
            _ => !cx.theme().mode.is_dark(),
        })
    }

    /// The terminal's ground, for the sheet it stands on.
    pub(crate) fn ground(&self, cx: &App) -> Hsla {
        hsla(self.scheme(cx).bg)
    }

    /// The window's code face in the scheme's plain weight, with Kaku's
    /// list of faces to fall back on.
    fn font(&self, k: &Scheme) -> Font {
        Font {
            family: self.mono.clone(),
            features: FontFeatures(Arc::new(if self.cfg.ligatures { Vec::new() } else { vec![("calt".into(), 0), ("clig".into(), 0), ("liga".into(), 0)] })),
            fallbacks: Some(FontFallbacks::from_fonts(TERM_FALLBACKS.iter().map(|f| f.to_string()).collect())),
            weight: k.plain,
            style: FontStyle::Normal,
        }
    }

    /// The shell that shows, when there is one.
    fn pty(&self) -> Option<Arc<Pty>> {
        self.shells.current().map(|t| t.pty.clone())
    }

    /// A new shell, in a tab of its own that then shows: the person's
    /// login shell in the vault's folder (their home when the folder is
    /// gone), its prompt marking where what is typed begins. `at` is where
    /// in the row, the end when none.
    fn shell_new(&mut self, at: Option<usize>, cx: &mut Context<Self>) {
        let cwd = if self.cwd.is_dir() { self.cwd.clone() } else { onyx_core::paths::home() };
        let (argv, env) = onyx_core::shell_marks::shell_command();
        let made = Pty::spawn_with(&argv, &env, &cwd.to_string_lossy(), Arc::new(|| {})).inspect(|p| p.reflow_on_resize());
        let set = &mut self.shells;
        set.begun = true;
        match made {
            Ok(pty) => {
                set.failed = None;
                set.last += 1;
                let (id, from) = (set.last, set.on);
                let n = set.tabs.iter().map(|t| t.n).max().unwrap_or(0) + 1;
                let at = at.unwrap_or(set.tabs.len()).min(set.tabs.len());
                set.tabs.insert(at, ShellTab { id, n, pty, born: Instant::now() });
                set.on = id;
                self.swapped(from, cx);
            }
            Err(e) => set.failed = Some(e),
        }
        cx.notify();
    }

    /// Another tab shows.
    fn shell_pick(&mut self, id: u64, cx: &mut Context<Self>) {
        let set = &mut self.shells;
        if set.on == id || !set.tabs.iter().any(|t| t.id == id) {
            return;
        }
        let from = std::mem::replace(&mut set.on, id);
        self.swapped(from, cx);
        cx.notify();
    }

    /// A tab is closed and its shell let go. The one after it shows in
    /// its place, the one before when it was the last.
    fn shell_close(&mut self, id: u64, cx: &mut Context<Self>) {
        let set = &mut self.shells;
        let Some(ix) = set.tabs.iter().position(|t| t.id == id) else { return };
        set.tabs.remove(ix).pty.kill();
        if set.on == id {
            set.on = set.tabs.get(ix).or(set.tabs.last()).map(|t| t.id).unwrap_or(0);
            self.swapped(id, cx);
        }
        cx.notify();
    }

    /// The screen that shows changed: the one before goes as the new one
    /// comes. The tab that shows is brought into view in its row, and kept
    /// there while it grows in.
    fn swapped(&mut self, from: u64, cx: &mut Context<Self>) {
        self.swap = Some((from, Instant::now(), self.swap.map(|(_, _, n)| n + 1).unwrap_or(0)));
        let until = TAB_ANIM + Duration::from_millis(80);
        self.reveal = Some(Instant::now() + until);
        self.view.sel = None;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(until).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        self.view.back = 0;
        self.view.wheel = 0.;
    }

    /// A tab dragged along the row takes the place the pointer is over.
    fn shell_drag_to(&mut self, id: u64, x: Pixels, row: Bounds<Pixels>, tab_w: f32, cx: &mut Context<Self>) {
        if self.drag != Some(id) {
            self.drag = Some(id);
            cx.notify();
        }
        let set = &mut self.shells;
        let Some(from) = set.tabs.iter().position(|t| t.id == id) else { return };
        let to = ((f32::from(x - row.left() - self.tabs_scroll.offset().x) / (tab_w + TAB_GAP)).floor().max(0.) as usize).min(set.tabs.len() - 1);
        if to != from {
            let tab = set.tabs.remove(from);
            set.tabs.insert(to, tab);
            cx.notify();
        }
    }

    /// The shell tab beside the one that shows, round the row's end.
    fn shell_step(&mut self, by: isize, cx: &mut Context<Self>) {
        let set = &self.shells;
        let Some(ix) = set.tabs.iter().position(|t| t.id == set.on) else { return };
        let n = set.tabs.len() as isize;
        let id = set.tabs[(ix as isize + by).rem_euclid(n) as usize].id;
        self.shell_pick(id, cx);
    }

    /// The head: a tab a shell, then the button that makes one. A tab is
    /// pressed to show it, dragged to move it, and closed with its own
    /// button. `room` is what the row may take: tabs share it down to
    /// `TAB_MIN` each, and past that the row scrolls with the button held
    /// at its right end.
    fn render_tabs(&mut self, room: f32, cx: &mut Context<Self>) -> AnyElement {
        let k = self.scheme(cx);
        let face = self.font(k);
        if !cx.has_active_drag() {
            self.drag = None;
        }
        let name = pty::shell_argv().first().and_then(|s| std::path::Path::new(s).file_stem().map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| "shell".into());
        let set = &self.shells;
        let n = set.tabs.len().max(1) as f32;
        let tab_w = ((room - TAB_H - 6. - TAB_GAP * n) / n).clamp(TAB_MIN, TAB_MAX);
        if self.reveal.is_some_and(|until| Instant::now() < until) {
            if let Some(ix) = set.tabs.iter().position(|t| t.id == set.on) {
                self.tabs_scroll.scroll_to_item(ix);
            }
        } else {
            self.reveal = None;
        }
        let mut row = h_flex().id("shell-tabs").min_w_0().h_full().items_center().gap(px(TAB_GAP)).overflow_x_scroll().track_scroll(&self.tabs_scroll).on_drag_move::<DragShell>(cx.listener(move |this, e: &DragMoveEvent<DragShell>, _, cx| {
            let id = e.drag(cx).0;
            this.shell_drag_to(id, e.event.position.x, e.bounds, tab_w, cx);
        }));
        for (ix, tab) in set.tabs.iter().enumerate() {
            let id = tab.id;
            // The sheet the terminal stands on rounds its corner, and a clip
            // is square: the first tab rounds its own, by the sheet's radius
            // less its border, or its ground stood square past the curve.
            let corner = ix == 0;
            let active = set.on == id;
            let label: SharedString = format!("{name} {}", tab.n).into();
            let ghost = label.to_string();
            let group: SharedString = format!("shell-tab-{id}").into();
            let fresh = tab.born.elapsed() < TAB_ANIM;
            // Kaku's tabs: flat, the one showing on a raised ground in the
            // bold weight, the others gray until the pointer is over them.
            let (hover_bg, press_bg, close_bg, lit) = (hsla(k.tab_hover), hsla(k.tab_on), hsla(k.split), hsla(k.fg));
            let el = h_flex()
                .id(("shell-tab", id as usize))
                .group(group.clone())
                .w(px(tab_w))
                .h(px(HEAD_H - 2.))
                .pl(px(10.))
                .pr(px(5.))
                .gap(px(6.))
                .items_center()
                .cursor_pointer()
                .overflow_hidden()
                .font(face.clone())
                .text_size(px(11.5))
                .map(|d| if active { d.bg(hsla(k.tab_on)).text_color(lit).font_weight(k.bold) } else { d.text_color(hsla(k.tab_off)).hover(move |s| s.bg(hover_bg).text_color(lit)).active(move |s| s.bg(press_bg)) })
                .when(corner, |d| d.rounded_tl(px(SHEET_R - 1.)))
                .when(self.drag == Some(id), |d| d.opacity(0.45))
                .on_drag(DragShell(id, ghost), |d, _, _, cx| cx.new(|_| d.clone()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.shell_pick(id, cx);
                    window.focus(&this.view.focus, cx);
                }))
                .child(Icon::default().path("icons/terminal.svg").with_size(px(12.)).flex_shrink_0())
                .child(div().flex_1().min_w_0().truncate().child(label))
                .child(
                    div()
                        .id(("shell-tab-close", id as usize))
                        .size(px(16.))
                        .flex_shrink_0()
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(move |s| s.bg(close_bg))
                        // On the tab that shows, and on any under the pointer.
                        .when(!active, |d| d.invisible().group_hover(group, |s| s.visible()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            this.shell_close(id, cx);
                        }))
                        .child(Icon::default().path("icons/close.svg").with_size(px(10.)).text_color(hsla(k.tab_off))),
                );
            // A new tab grows in from nothing. The wrapper is there at
            // rest too, under the same name.
            row = row.child(div().flex_shrink_0().overflow_hidden().child(el).with_animation(ElementId::Name(format!("shell-tab-w-{id}").into()), Animation::new(TAB_ANIM).with_easing(ease_out_quint()), move |d, t| if fresh { d.w(px(tab_w * t)).opacity(t) } else { d }));
        }
        let (hover_bg, press_bg, lit) = (hsla(k.tab_hover), hsla(k.tab_on), hsla(k.fg));
        let plus = div()
            .id("shell-tab-new")
            .w(px(TAB_H + 6.))
            .h(px(HEAD_H - 2.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(hsla(k.tab_off))
            .hover(move |s| s.bg(hover_bg).text_color(lit))
            .active(move |s| s.bg(press_bg))
            .managed_tooltip(|window, cx| tip(t("New Tab (⌘T)"), window, cx))
            .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.shell_new(None, cx);
                window.focus(&this.view.focus, cx);
            }))
            .child(Icon::default().path("icons/plus.svg").with_size(px(12.)));
        h_flex().min_w_0().h_full().items_center().gap(px(TAB_GAP)).child(row).child(plus).into_any_element()
    }

    /// The wheel over the screen: back through what has left it, or, where
    /// a program has the screen to itself, its arrow keys, or the program's
    /// own where it hears the mouse.
    fn wheel(&mut self, delta_y: Pixels, at: Point<Pixels>, mods: &Modifiers) {
        let Some(pty) = self.pty() else { return };
        self.view.wheel += f32::from(delta_y) / WHEEL_ROW;
        let lines = self.view.wheel.trunc() as i64;
        if lines == 0 {
            return;
        }
        self.view.wheel -= lines as f32;
        if let Some(want) = pty.mouse().filter(|_| self.view.back == 0) {
            let (col, row) = self.cell(at, &pty);
            let code = if lines > 0 { 64 } else { 65 } | mod_bits(mods);
            pty.write(&pty::mouse_bytes(code, col, row, true, want.form).repeat(lines.unsigned_abs().min(12) as usize));
            return;
        }
        if pty.alt_screen() {
            let key: &[u8] = match (lines > 0, pty.app_cursor()) {
                (true, false) => b"\x1b[A",
                (true, true) => b"\x1bOA",
                (false, false) => b"\x1b[B",
                (false, true) => b"\x1bOB",
            };
            pty.write(&key.repeat(lines.unsigned_abs() as usize));
        } else {
            self.view.back = (self.view.back as i64 + lines).max(0) as usize;
            self.unhover();
        }
    }

    /// The cell of the screen a point of the window is over: column, row,
    /// held inside the screen.
    fn cell(&self, at: Point<Pixels>, pty: &Pty) -> (u16, u16) {
        let b = self.view.bounds.get();
        let (rows, cols) = pty.size();
        let col = (f32::from(at.x - b.left()) - self.view.pad.0) / self.view.cell_w.max(1.);
        let row = (f32::from(at.y - b.top()) - self.view.pad.0) / self.line_h();
        (col.floor().clamp(0., cols.saturating_sub(1) as f32) as u16, row.floor().clamp(0., rows.saturating_sub(1) as f32) as u16)
    }

    /// A button pressed on the screen. A program that asked to hear the
    /// mouse is told, unless Shift is held, which keeps the press for the
    /// terminal as terminals do. Otherwise the left button begins a
    /// selection (by the letter, and by the word or the row on a double or
    /// triple click; a click that selects nothing puts the cursor there,
    /// `mouse_up`), and opens the link under it with ⌘; the right one
    /// opens the menu, the middle one pastes.
    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pty) = self.pty() else {
            if e.button == MouseButton::Right {
                self.open_menu(e.position, window, cx);
            }
            return;
        };
        let (col, row) = self.cell(e.position, &pty);
        // ⌘ and a click opens the link under the pointer, in a program
        // that hears the mouse too; Ctrl off a Mac.
        if e.button == MouseButton::Left && e.modifiers.secondary() {
            if let Some(link) = self.link_at(&pty, row, col) {
                match link.target {
                    Target::Url(url) => cx.open_url(&url),
                    Target::File(path) => crate::sys::open(&path.to_string_lossy()),
                }
            }
            return;
        }
        let want = pty.mouse().filter(|_| pty.alive() && self.view.back == 0);
        if let (Some(want), false, Some(button)) = (want, e.modifiers.shift, button_code(e.button)) {
            self.view.sel = None;
            self.view.mouse = Some(button);
            self.view.mouse_cell = (col, row);
            pty.write(&pty::mouse_bytes(button | mod_bits(&e.modifiers), col, row, true, want.form));
            return;
        }
        match e.button {
            MouseButton::Left => {}
            MouseButton::Right => return self.open_menu(e.position, window, cx),
            MouseButton::Middle => return self.paste(cx),
            _ => return,
        }
        let line = self.line(row);
        // With Shift the selection there is runs on to the press; one
        // made from the keyboard is by the letter from then on.
        if let Some(sel) = self.view.sel.as_mut().filter(|_| e.modifiers.shift) {
            if sel.unit == 0 {
                sel.unit = 1;
                if sel.anchor > sel.head {
                    sel.anchor.1 = sel.anchor.1.saturating_sub(1);
                }
            }
            sel.head = (line, col);
            self.view.selecting = true;
            return;
        }
        // ⌥ and a drag takes the same columns of every row.
        self.view.sel = Some(Sel { anchor: (line, col), head: (line, col), unit: e.click_count.clamp(1, 3), block: e.modifiers.alt });
        self.view.selecting = true;
    }

    /// The menu of a right click: Copy, Paste, Select All, New Tab, Close
    /// Tab, and Clear apart.
    fn open_menu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let mut items = Vec::new();
        if !self.sel_text().is_empty() {
            items.push(Entry::choice("copy", "icons/copy.svg", t("Copy")));
        }
        if cx.read_from_clipboard().is_some() {
            items.push(Entry::choice("paste", "icons/clipboard.svg", t("Paste")));
        }
        items.push(Entry::choice("select-all", "icons/select-all.svg", t("Select All")));
        items.push(Entry::Rule);
        items.push(Entry::choice("new-tab", "icons/plus.svg", t("New Tab")));
        if self.pty().is_some() {
            items.push(Entry::choice("close-tab", "icons/close.svg", t("Close Tab")));
        }
        // What empties the screen, apart.
        items.push(Entry::Rule);
        items.push(Entry::choice("clear", "icons/eraser.svg", t("Clear")));
        let menu = self.menu.clone();
        window.defer(cx, move |window, cx| menu(at, items, window, cx));
    }

    /// A choice of the menu.
    pub(crate) fn menu_do(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "copy" => {
                self.copy(cx);
            }
            "paste" => self.paste(cx),
            "select-all" => self.select_all(),
            "clear" => self.clear(),
            "new-tab" => self.shell_new(None, cx),
            "close-tab" => {
                if let Some(id) = self.shells.current().map(|t| t.id) {
                    self.shell_close(id, cx);
                }
            }
            _ => {}
        }
        window.focus(&self.view.focus, cx);
        cx.notify();
    }

    /// What is being typed, when the program reading the keys edits its
    /// own line (`Pty::edits_its_line`): not where a program has the
    /// screen to itself, a command takes whole lines or none, or the
    /// screen is scrolled back. It begins where the prompt marked it
    /// (`shell_marks`) and runs to the last row written after it, every
    /// line of it; where nothing marked it, it is what stands between the
    /// rules over and under the cursor (Claude Code's input), else the
    /// cursor's line and the rows it wraps on.
    fn typing(&self, pty: &Pty) -> Option<Typing> {
        if !pty.alive() || self.view.back != 0 || !pty.edits_its_line() {
            return None;
        }
        // A program with the screen to itself (an editor, a pager) has its
        // own keys for all of this, but for an input between two rules.
        let own_screen = pty.alt_screen();
        let (row, _) = pty.cursor()?;
        let (rows, cols) = pty.size();
        let cur = row as i64;
        let cells = |l: i64| pty.lines(l, l).1.first().map(|(r, _)| row_cells(r)).unwrap_or_default();
        let blank = |l: i64| cells(l).iter().all(|c| c.is_whitespace() || *c == '\0');
        let rule = |l: i64| cells(l).iter().filter(|c| **c == '─').count() >= 8;
        // A row runs on into the next when its text did, not when it
        // wraps for the blanks it was filled with (zsh's mark after output
        // with no newline): as the screen model reflows it.
        let wraps = |l: i64| pty.lines(l, l).1.first().is_some_and(|(r, w)| *w && row_cells(r).len() + 1 >= cols as usize);
        let foot = |mut l: i64| {
            while l + 1 < rows as i64 && !blank(l + 1) && !rule(l + 1) {
                l += 1;
            }
            l
        };
        let mut boxed = false;
        let (first, last, start) = match pty.typing_start().filter(|(l, _)| *l <= cur && !own_screen) {
            Some((l, c)) => (l, foot(cur), (l, c)),
            None => {
                let mut top = cur;
                while top > -(pty.kept_back() as i64) && !blank(top - 1) && !rule(top - 1) {
                    top -= 1;
                }
                if top > -(pty.kept_back() as i64) && rule(top - 1) && rule(foot(cur) + 1) {
                    boxed = true;
                    (top, foot(cur), (top, 0))
                } else if own_screen {
                    return None;
                } else {
                    let (mut first, mut last) = (cur, cur);
                    while first > 0 && wraps(first - 1) {
                        first -= 1;
                    }
                    while last < rows as i64 - 1 && wraps(last) {
                        last += 1;
                    }
                    (first, last, (first, 0))
                }
            }
        };
        let (first, lines) = pty.lines(first, last);
        let rows: Vec<Vec<char>> = lines.iter().map(|(r, _)| row_cells(r)).collect();
        let runs = (0..rows.len() as i64).map(|i| boxed || wraps(first + i)).collect();
        Some(Typing { first, rows, start, runs, boxed })
    }

    /// Something done to what is being typed by steering the cursor, off
    /// the window's thread: what is typed meanwhile waits and is sent once
    /// it is done, after `then`.
    fn edit(&mut self, pty: Arc<Pty>, job: impl FnOnce(&Pty) + Send + 'static, then: Vec<u8>) {
        let queue = Arc::clone(&self.view.edit);
        {
            let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
            if q.is_some() {
                // One already under way: this one follows it.
                q.get_or_insert_with(Vec::new).extend(then);
                return;
            }
            *q = Some(Vec::new());
        }
        std::thread::spawn(move || {
            job(&pty);
            pty.write(&then);
            let waiting = queue.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
            pty.write(&waiting);
        });
    }

    /// Bytes for the terminal, or for after the cursor is where it is
    /// being steered to.
    fn send(&self, pty: &Pty, bytes: &[u8]) {
        let mut q = self.view.edit.lock().unwrap_or_else(|e| e.into_inner());
        match q.as_mut() {
            Some(waiting) => waiting.extend_from_slice(bytes),
            None => pty.write(bytes),
        }
    }

    /// The cursor taken to a place in what is being typed, as a field's
    /// caret goes to a click.
    fn caret_to(&mut self, pty: Arc<Pty>, to: (u16, u16)) {
        self.edit(
            pty,
            move |p| {
                steer(p, to);
            },
            Vec::new(),
        );
    }

    /// What is selected of what is being typed goes, as Backspace takes a
    /// selection in a field, and `then` is typed in its place: a whole
    /// paragraph by the program's own keys for one (^E, ^U), as Kaku's ⌘A
    /// and Backspace, and part of one by steering the cursor to its start
    /// and counting the letters to its end. What the program will not
    /// delete (its prompt) stays. A selection over more than one paragraph
    /// is only copied: a terminal's line editors know a line at a time.
    /// Whether it went.
    fn delete_sel(&mut self, then: Vec<u8>) -> bool {
        let (Some(sel), Some(pty)) = (self.view.sel, self.pty()) else { return false };
        let Some(typing) = self.typing(&pty) else { return false };
        let spans = sel.spans(typing.first, &typing.rows);
        let (Some(i), Some(j)) = (spans.iter().position(Option::is_some), spans.iter().rposition(Option::is_some)) else { return false };
        let a = (typing.first + i as i64, spans[i].unwrap().0 as u16).max(typing.start);
        let b = (typing.first + j as i64, (spans[j].unwrap().1.min(typing.rows[j].len().max(1))) as u16);
        let (from, to) = typing.para(a.0);
        if a >= b || !typing.holds(b) || typing.para(b.0) != (from, to) {
            return false;
        }
        let on_screen = |(l, c): (i64, u16)| (l.max(0) as u16, c);
        let here = pty.cursor().is_some_and(|(r, _)| (from.0..=to.0).contains(&(r as i64)));
        let (a, b) = (on_screen(a), on_screen(b));
        self.view.sel = None;
        if a <= on_screen(from) && b >= on_screen(to) {
            // An input between rules (Claude Code's) takes a row with ^U:
            // each, from the input's end, and the Backspace that joins it
            // to the one before.
            let rows = typing.rows.len();
            let boxed = typing.boxed;
            self.edit(
                pty,
                move |p| {
                    if boxed {
                        steer(p, b);
                        p.write(&b"\x15\x7f".repeat(rows));
                    } else {
                        if !here {
                            steer(p, a);
                        }
                        p.write(b"\x05\x15");
                    }
                },
                then,
            );
            return true;
        }
        // Counted from its start to its end, then taken back by that many:
        // a program may move what it shows as the text changes, and a place
        // on the screen aimed at while deleting was another letter by then.
        self.edit(
            pty,
            move |p| {
                steer(p, a);
                let n = letters_to(p, b);
                p.write(&b"\x7f".repeat(n));
            },
            then,
        );
        true
    }

    /// The link the cell is part of, when it is part of one.
    fn link_at(&self, pty: &Pty, row: u16, col: u16) -> Option<Link> {
        let (rows, _) = pty.rows_back(self.view.back);
        // A web address cut into rows at the screen's edge opens whole.
        let line = rows.get(row as usize)?;
        let at = (0..=line.iter().map(|s| s.text.chars().count()).sum::<usize>()).find(|i| cells_before(line, *i) > col as usize).map(|i| i.saturating_sub(1)).unwrap_or(usize::MAX);
        if let Some((link, (_, start, end))) = pty::links(&rows).into_iter().find_map(|l| l.parts.iter().copied().find(|(r, s, e)| *r == row as usize && (*s..*e).contains(&at)).map(|p| (l, p))) {
            return Some(Link { row, from: cells_before(line, start), to: cells_before(line, end), target: Target::Url(link.url) });
        }
        let (from, to, target) = link_at(&row_cells(rows.get(row as usize)?), col as usize, &self.cwd)?;
        Some(Link { row, from, to, target })
    }

    /// What is on the clipboard is pasted at the prompt: its words, or a
    /// picture as the path of a file it is kept in, which a program takes
    /// for a picture, or files copied in the Finder as their paths. A
    /// terminal takes words alone, and a screenshot pasted there did
    /// nothing.
    fn paste(&mut self, cx: &mut Context<Self>) {
        use gpui::ClipboardEntry;
        let Some(pty) = self.pty().filter(|p| p.alive()) else { return };
        let Some(item) = cx.read_from_clipboard() else { return };
        let mut said: Vec<String> = Vec::new();
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(img) => {
                    if let Some(path) = keep_image(img.format.mime_type(), &img.bytes) {
                        said.push(path_words(&[path]));
                    }
                }
                ClipboardEntry::ExternalPaths(paths) => said.push(path_words(paths.paths())),
                ClipboardEntry::String(s) => said.push(s.text().to_string()),
            }
        }
        if said.is_empty() {
            return cx.notify();
        }
        // A selection in what is being typed is what the paste replaces.
        let words = pty.paste_bytes(&said.join(" "));
        if !self.delete_sel(words.clone()) {
            self.send(&pty, &words);
        }
        self.view.back = 0;
        self.view.sel = None;
        cx.notify();
    }

    /// The button let go, on the screen or outside it. A selection just
    /// made is copied, as it is in a terminal.
    fn mouse_up(&mut self, e: &MouseUpEvent, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.view.selecting) {
            // With copying on selection off, what is selected stays for ⌘C.
            let kept = !self.cfg.copy_on_select && !self.sel_text().is_empty();
            if !kept && !self.copy(cx) {
                // A click that selected nothing puts the cursor where it
                // was, in the line being typed, as it does in a field. A
                // double click's word is a selection.
                let at = self.view.sel.take().filter(|s| s.unit == 1).map(|s| s.anchor.0);
                if let (Some(line), Some(pty)) = (at, self.pty()) {
                    // To the gap between two letters nearest the pointer,
                    // as a caret goes.
                    let b = self.view.bounds.get();
                    let x = (f32::from(e.position.x - b.left()) - self.view.pad.0) / self.view.cell_w.max(1.);
                    let col = x.round().clamp(0., pty.size().1 as f32) as u16;
                    if let Some(typing) = self.typing(&pty) {
                        let to = (line, col).max(typing.start);
                        if typing.holds(to) {
                            self.caret_to(pty, (to.0.max(0) as u16, to.1));
                            self.view.blink.1 = Instant::now();
                        }
                    }
                }
            }
            cx.notify();
            return;
        }
        let Some(button) = self.view.mouse.take() else { return };
        let Some(pty) = self.pty() else { return };
        if let Some(want) = pty.mouse().filter(|w| w.release) {
            let (col, row) = self.cell(e.position, &pty);
            pty.write(&pty::mouse_bytes(button | mod_bits(&e.modifiers), col, row, false, want.form));
        }
    }

    /// The pointer moved over the screen: the link under it is underlined,
    /// or it is told to the program a cell at a time, with a button held,
    /// or without one where the program asked for that too.
    fn mouse_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        if e.pressed_button.is_none() {
            self.view.mouse = None;
            self.view.selecting = false;
        }
        let Some(pty) = self.pty() else { return };
        // A drag that selects follows the pointer anywhere in the window
        // (`select_to`).
        if self.view.selecting {
            return;
        }
        let cell = self.cell(e.position, &pty);
        if cell != self.view.hover {
            self.view.hover = cell;
            let link = self.link_at(&pty, cell.1, cell.0);
            if link != self.view.link {
                self.view.link = link;
                cx.notify();
            }
        }
        let Some(want) = pty.mouse().filter(|_| pty.alive() && self.view.back == 0) else { return };
        let held = self.view.mouse;
        if !(want.motion || want.drag && held.is_some()) {
            return;
        }
        if cell == self.view.mouse_cell {
            return;
        }
        self.view.mouse_cell = cell;
        // No button held is button 3 on the move.
        pty.write(&pty::mouse_bytes(held.unwrap_or(3) | 32 | mod_bits(&e.modifiers), cell.0, cell.1, true, want.form));
    }

    /// The line of the screen's `row` as it is drawn now, scrolled back or
    /// not (`Sel`).
    fn line(&self, row: u16) -> i64 {
        row as i64 - self.view.back as i64
    }

    /// A drag that selects, wherever the pointer is in the window: the
    /// selection runs to the cell under the pointer, and held past the
    /// screen's top or foot the screen scrolls on under it, the faster the
    /// further out (`edge_step`).
    fn select_to(&mut self, at: Point<Pixels>, held: bool, cx: &mut Context<Self>) {
        if !held {
            self.view.selecting = false;
            self.view.edge = 0.;
            return;
        }
        let Some(pty) = self.pty() else { return };
        let (col, row) = self.cell(at, &pty);
        let b = self.view.bounds.get();
        let (y, top, foot) = (f32::from(at.y), f32::from(b.top()), f32::from(b.bottom()));
        let edge = if y < top { y - top } else if y > foot { y - foot } else { 0. };
        let line = self.line(row);
        let view = &mut self.view;
        (view.edge, view.edge_col) = (edge, col);
        if let Some(sel) = view.sel.as_mut().filter(|s| s.head != (line, col)) {
            sel.head = (line, col);
            cx.notify();
        }
        if edge != 0. && !view.edge_run {
            view.edge_run = true;
            view.edge_rows = 0.;
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(EDGE_TICK).await;
                if !this.update(cx, |this, cx| this.edge_step(cx)).unwrap_or(false) {
                    break;
                }
            })
            .detach();
        }
    }

    /// One step of the screen scrolling under a drag held past its edge: a
    /// row at a time at the edge, quicker further out, and the selection's
    /// end on the row that came in. Whether to go on.
    fn edge_step(&mut self, cx: &mut Context<Self>) -> bool {
        let pty = self.pty();
        let view = &mut self.view;
        let Some(pty) = pty.filter(|_| view.selecting && view.edge != 0.) else {
            view.edge_run = false;
            return false;
        };
        view.edge_rows += (EDGE_ROWS + view.edge.abs() * EDGE_MORE).min(EDGE_MOST) * EDGE_TICK.as_secs_f32();
        let n = view.edge_rows.trunc() as usize;
        if n == 0 {
            return true;
        }
        view.edge_rows -= n as f32;
        let rows = pty.size().0 as i64;
        let back = if view.edge < 0. { (view.back + n).min(pty.kept_back()) } else { view.back.saturating_sub(n) };
        view.back = back;
        let line = if view.edge < 0. { -(back as i64) } else { rows - 1 - back as i64 };
        let col = view.edge_col;
        if let Some(sel) = view.sel.as_mut() {
            sel.head = (line, col);
        }
        self.unhover();
        cx.notify();
        true
    }

    /// ⌘A: at a prompt, the paragraph the cursor is in, as Kaku has it;
    /// else, or with nothing typed at all, all the terminal holds, what
    /// has left the screen with it, up to the end of the last line with
    /// anything on it.
    fn select_all(&mut self) {
        let Some(pty) = self.pty() else { return };
        if let (Some(typing), Some((row, _))) = (self.typing(&pty), pty.cursor()) {
            let (from, to) = typing.para(row as i64);
            if from < to {
                self.view.sel = Some(Sel { anchor: from, head: to, unit: 0, block: false });
            }
            // An empty line among others has nothing to select.
            let typed = typing.rows.iter().zip(typing.first..).any(|(r, l)| r.iter().zip(0..).any(|(c, i)| (l, i) >= typing.start && !c.is_whitespace() && *c != '\0'));
            if from < to || typed {
                return;
            }
        }
        let (rows, _) = pty.rows_back(0);
        let last = rows.iter().rposition(|r| r.iter().any(|s| !s.text.trim().is_empty())).unwrap_or(0);
        let end = rows.get(last).map(|r| row_cells(r).len()).unwrap_or(0) as u16;
        self.view.sel = Some(Sel { anchor: (-(pty.kept_back() as i64), 0), head: (last as i64, end), unit: 0, block: false });
    }

    /// A selection by the letter, the word or the row made one between
    /// places, as the keyboard moves it, with its ends where it shows them.
    fn caret_sel(sel: Sel, pty: &Pty) -> Sel {
        if sel.unit == 0 {
            return sel;
        }
        let (a, b) = sel.ends();
        let (top, lines) = pty.lines(a.0, b.0);
        let cells: Vec<Vec<char>> = lines.iter().map(|(r, _)| row_cells(r)).collect();
        let spans = Sel { block: false, ..sel }.spans(top, &cells);
        let (start, end) = match (spans.iter().position(Option::is_some), spans.iter().rposition(Option::is_some)) {
            (Some(f), Some(l)) => ((top + f as i64, spans[f].unwrap().0 as u16), (top + l as i64, spans[l].unwrap().1 as u16)),
            _ => (a, a),
        };
        let on = sel.head >= sel.anchor;
        Sel { anchor: if on { start } else { end }, head: if on { end } else { start }, unit: 0, block: false }
    }

    /// Shift with the keys that move a caret runs a selection on, from the
    /// selection there is or from the cursor: an arrow a letter or a row,
    /// with ⌥ a word, with ⌘ the line's start or end, as Home and End are.
    /// Off a Mac Ctrl is the word's, and the keys select only where
    /// something is selected already: with nothing, Shift and an arrow are
    /// the shell's (PowerShell selects with them). Where a program has the
    /// screen to itself the keys are its own. Whether the key was one of
    /// them.
    fn select_key(&mut self, k: &Keystroke) -> bool {
        enum By {
            Letter,
            Word,
            Line,
            Row,
        }
        let m = &k.modifiers;
        let mac = cfg!(target_os = "macos");
        let word = if mac { m.alt && !m.control } else { m.control && !m.alt };
        let by = match (k.key.as_str(), m.platform && mac, word, m.alt || m.control) {
            _ if !m.shift => return false,
            ("left" | "right", false, false, false) => By::Letter,
            ("left" | "right", false, true, _) => By::Word,
            ("left" | "right", true, false, false) | ("home" | "end", false, false, false) => By::Line,
            ("up" | "down", false, false, false) => By::Row,
            _ => return false,
        };
        let on = matches!(k.key.as_str(), "right" | "end" | "down");
        let Some(pty) = self.pty() else { return false };
        let sel = match self.view.sel {
            Some(sel) => Self::caret_sel(sel, &pty),
            None if mac && !pty.alt_screen() => match pty.cursor() {
                Some((row, col)) => Sel { anchor: (row as i64, col), head: (row as i64, col), unit: 0, block: false },
                None => return false,
            },
            None => return false,
        };
        let (rows, top) = (pty.size().0 as i64, -(pty.kept_back() as i64));
        let cells = |l: i64| pty.lines(l, l).1.first().map(|(r, _)| row_cells(r)).unwrap_or_default();
        let (mut l, mut c) = (sel.head.0, sel.head.1 as usize);
        let mut row = cells(l);
        match by {
            By::Letter if on => {
                if c < row.len() {
                    c += 1;
                    // Past a wide letter's second cell too.
                    if row.get(c) == Some(&'\0') {
                        c += 1;
                    }
                } else if l < rows - 1 {
                    (l, c) = (l + 1, 0);
                }
            }
            By::Letter => {
                if c > 0 {
                    c -= 1;
                    if row.get(c) == Some(&'\0') {
                        c = c.saturating_sub(1);
                    }
                } else if l > top {
                    l -= 1;
                    c = cells(l).len();
                }
            }
            By::Word if on => {
                if c >= row.len() && l < rows - 1 {
                    (l, c) = (l + 1, 0);
                    row = cells(l);
                }
                while c < row.len() && !word_char(row[c]) {
                    c += 1;
                }
                while c < row.len() && word_char(row[c]) {
                    c += 1;
                }
            }
            By::Word => {
                if c == 0 && l > top {
                    l -= 1;
                    row = cells(l);
                    c = row.len();
                }
                c = c.min(row.len());
                while c > 0 && !word_char(row[c - 1]) {
                    c -= 1;
                }
                while c > 0 && word_char(row[c - 1]) {
                    c -= 1;
                }
            }
            By::Line => c = if on { row.len() } else { 0 },
            By::Row => l = (l + if on { 1 } else { -1 }).clamp(top, rows - 1),
        }
        let view = &mut self.view;
        view.sel = Some(Sel { head: (l, c as u16), ..sel });
        // The end that moved stays in sight.
        let back = view.back as i64;
        view.back = if l < -back { -l } else if l > rows - 1 - back { rows - 1 - l } else { back }.max(0) as usize;
        true
    }

    /// The keys that scroll back through what has left the screen, as a
    /// Mac's terminals have them: ⌘↑ and ⌘↓ a row, ⇧ or ⌘ with Page Up and
    /// Page Down a screen, ⌘Home and ⌘End to the top and the foot. Off a
    /// Mac Ctrl+Shift takes ⌘'s place, and Shift with a page key is a
    /// screen too. Not where a program has the screen to itself, which
    /// scrolls its own. Whether the key was one of them.
    fn scroll_key(&mut self, k: &Keystroke) -> bool {
        let m = &k.modifiers;
        let held = if cfg!(target_os = "macos") { m.platform && !m.shift } else { m.control && m.shift };
        if m.alt || !(held || m.shift && !m.control && !m.platform) {
            return false;
        }
        let Some(pty) = self.pty().filter(|p| !p.alt_screen()) else { return false };
        let page = pty.size().0.saturating_sub(1).max(1) as i64;
        let by: i64 = match (k.key.as_str(), held) {
            ("up", true) => 1,
            ("down", true) => -1,
            ("pageup", _) => page,
            ("pagedown", _) => -page,
            ("home", true) => i64::MAX / 2,
            ("end", true) => i64::MIN / 2,
            _ => return false,
        };
        let back = (self.view.back as i64).saturating_add(by).clamp(0, pty.kept_back() as i64);
        self.view.back = back as usize;
        true
    }

    /// Files dropped on the terminal: their paths, as a shell reads them
    /// and a space after, as a Mac's terminals type them.
    fn drop_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Some(pty) = self.pty().filter(|p| p.alive()) else { return };
        if paths.is_empty() {
            return;
        }
        pty.paste(&format!("{} ", path_words(paths)));
        self.view.back = 0;
        self.view.sel = None;
        cx.notify();
    }

    /// The words selected: each row's stretch without the blank at its
    /// end, a new line between rows unless one runs on into the next.
    fn sel_text(&self) -> String {
        let (Some(sel), Some(pty)) = (self.view.sel, self.pty()) else { return String::new() };
        let (a, b) = sel.ends();
        let (top, lines) = pty.lines(a.0, b.0);
        let wraps: Vec<bool> = lines.iter().map(|(_, w)| *w).collect();
        let cells: Vec<Vec<char>> = lines.iter().map(|(r, _)| row_cells(r)).collect();
        let spans = sel.spans(top, &cells);
        let last = spans.iter().rposition(|s| s.is_some());
        let mut out = String::new();
        for (r, span) in spans.iter().enumerate() {
            let Some((from, to)) = *span else { continue };
            let piece: String = cells[r].iter().skip(from).take(to.saturating_sub(from)).filter(|c| **c != '\0').collect();
            let runs_on = !sel.block && wraps.get(r).copied().unwrap_or(false) && Some(r) != last;
            out.push_str(if runs_on { &piece } else { piece.trim_end() });
            if Some(r) != last && !runs_on {
                out.push('\n');
            }
        }
        out
    }

    /// The selection goes to the clipboard, and the terminal says so for a
    /// moment. Whether there was anything to copy.
    fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let text = self.sel_text();
        if text.is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.view.copied = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR + Duration::from_millis(30)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        true
    }

    /// What was under the pointer is looked for again when it next moves:
    /// the rows have changed under it.
    fn unhover(&mut self) {
        self.view.link = None;
        self.view.hover = (u16::MAX, u16::MAX);
    }

    /// ⌘K: the screen and what has left it are forgotten, and the program
    /// is asked to draw again (^L), which a shell answers with a fresh
    /// prompt.
    fn clear(&mut self) {
        let Some(pty) = self.pty() else { return };
        if pty.clear_all() {
            pty.write(b"\x0c");
            self.view.back = 0;
            self.view.sel = None;
        }
    }

    /// The terminal's own keys off a Mac, as the system's terminal has
    /// them: Ctrl+Shift+C and V copy and paste, and so do Ctrl+Insert and
    /// Shift+Insert; on Windows Ctrl+V pastes too, and Ctrl+C copies what
    /// is selected and is the shell's Ctrl+C when nothing is. Ctrl+Tab and
    /// Ctrl+Page Down are the next tab. Whether it was one of them.
    fn pc_key(&mut self, k: &Keystroke, cx: &mut Context<Self>) -> bool {
        let m = &k.modifiers;
        if cfg!(target_os = "macos") || m.alt || m.platform {
            return false;
        }
        let windows = cfg!(windows);
        match (m.control, m.shift, k.key.to_ascii_lowercase().as_str()) {
            (true, true, "c") | (true, false, "insert") => {
                self.copy(cx);
            }
            (true, false, "c") if windows && !self.sel_text().is_empty() => {
                self.copy(cx);
                self.view.sel = None;
            }
            (true, true, "v") | (false, true, "insert") => self.paste(cx),
            (true, false, "v") if windows => self.paste(cx),
            (true, false, "tab") | (true, false, "pagedown") => self.shell_step(1, cx),
            (true, true, "tab") | (true, false, "pageup") => self.shell_step(-1, cx),
            _ => return false,
        }
        true
    }

    /// A key on the screen, as the bytes a terminal sends for it. ⌘V is a
    /// paste, ⌘A selects all, Shift with the keys that move a caret
    /// selects (`select_key`), and some keys scroll back (`scroll_key`);
    /// any other key with ⌘ is the window's. Off a Mac, `pc_key` has the
    /// terminal's own.
    fn key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(pty) = self.pty().filter(|p| p.alive()) else { return };
        let k = &e.keystroke;
        if self.pc_key(k, cx) || self.select_key(k) || self.scroll_key(k) {
            self.unhover();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // A letter, a mark or a space is text: it is not taken here, so gpui
        // hands it to the input method, which composes it or types it
        // through `replace_text_in_range`. Off a Mac, Alt with a key is the
        // shell's meta and stays a key.
        let typed = k.key_char.clone().filter(|c| !k.modifiers.platform && !k.modifiers.control && !c.is_empty() && !c.chars().any(char::is_control));
        if typed.is_some() && (cfg!(target_os = "macos") || !k.modifiers.alt) {
            return;
        }
        // With part of the line being typed selected, Backspace and Delete,
        // with ⌥ or ⌘ too, take the selection, as they do in a field.
        if self.view.sel.is_some() && matches!(k.key.as_str(), "backspace" | "delete") && self.delete_sel(Vec::new()) {
            self.unhover();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if k.modifiers.platform {
            // ⌘ with the keys of a line is the line's start, its end, all
            // of it before the cursor or after it, and a new line that
            // sends nothing, as a Mac's terminals have them; ⌘⇧[ and ⌘⇧]
            // are the tab before and after.
            let line: Option<&[u8]> = match k.key.as_str() {
                "left" => Some(b"\x01"),
                "right" => Some(b"\x05"),
                "backspace" => Some(b"\x15"),
                "delete" => Some(b"\x0b"),
                "enter" => Some(b"\x1b\r"),
                _ => None,
            };
            let handled = match k.key.as_str() {
                "v" => {
                    self.paste(cx);
                    true
                }
                "c" => self.copy(cx),
                // ⌘X cuts what is selected of the line being typed.
                "x" => {
                    let cut = self.copy(cx);
                    self.delete_sel(Vec::new()) || cut
                }
                // ⌘Z takes back the last change to the line, as the
                // shell's own undo (Ctrl-_).
                "z" if !k.modifiers.shift => {
                    pty.write(b"\x1f");
                    true
                }
                "a" if !k.modifiers.shift => {
                    self.select_all();
                    true
                }
                "[" | "{" if k.modifiers.shift => {
                    self.shell_step(-1, cx);
                    true
                }
                "]" | "}" if k.modifiers.shift => {
                    self.shell_step(1, cx);
                    true
                }
                _ => match line {
                    Some(bytes) => {
                        pty.write(bytes);
                        self.view.back = 0;
                        self.view.sel = None;
                        true
                    }
                    None => false,
                },
            };
            if handled {
                self.unhover();
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }
        // ⌥ with an arrow, Backspace or Delete is a word, as a Mac's
        // terminals have it.
        let word: Option<&[u8]> = match (k.modifiers.alt, k.key.as_str()) {
            (true, "left") => Some(b"\x1bb"),
            (true, "right") => Some(b"\x1bf"),
            (true, "backspace") => Some(b"\x1b\x7f"),
            (true, "delete") => Some(b"\x1bd"),
            // ⇧↩ is a new line that sends nothing.
            (false, "enter") if k.modifiers.shift => Some(b"\x1b\r"),
            _ => None,
        };
        let Some(mut bytes) = word.map(<[u8]>::to_vec).or_else(|| term_bytes(k)) else { return };
        if pty.app_cursor() && bytes.len() == 3 && bytes.starts_with(b"\x1b[") && matches!(bytes[2], b'A'..=b'D' | b'H' | b'F') {
            bytes[1] = b'O';
        }
        self.send(&pty, &bytes);
        self.view.back = 0;
        self.view.sel = None;
        self.unhover();
        // A key begins the cursor's blink again, lit.
        self.view.blink.1 = Instant::now();
        cx.stop_propagation();
        cx.notify();
    }

    /// ⌘F: the find card over the screen, with the keyboard in it and
    /// what it held selected; again in the card, it goes.
    fn find_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.as_ref().is_some_and(|f| f.open && f.input.read(cx).focus_handle(cx).is_focused(window)) {
            return self.find_close(window, cx);
        }
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(t("Find in this terminal")));
            let sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, _, cx| match ev {
                InputEvent::Change => {
                    this.find_run(true, cx);
                    cx.notify();
                }
                InputEvent::PressEnter { shift, .. } => this.find_step(if *shift { -1 } else { 1 }, cx),
                _ => {}
            });
            self.find = Some(Find { input, open: false, since: Instant::now(), hits: Vec::new(), at: 0, looked: Instant::now(), _sub: sub });
        }
        // A selection on the screen is what is looked for.
        let picked = self.sel_text();
        let Some(find) = self.find.as_mut() else { return };
        if !find.open {
            find.open = true;
            find.since = Instant::now();
        }
        let input = find.input.clone();
        input.update(cx, |s, cx| {
            if !picked.is_empty() && !picked.contains('\n') {
                s.set_value(picked.clone(), window, cx);
            }
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        self.find_run(true, cx);
        cx.notify();
    }

    /// The card goes, the matches with it, and the keyboard back to the
    /// screen.
    fn find_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = self.find.as_mut().filter(|f| f.open) {
            find.open = false;
            find.since = Instant::now();
            find.hits.clear();
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(crate::widgets::MENU_OUT + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
        window.focus(&self.view.focus, cx);
        cx.notify();
    }

    /// The screen and what has left it looked through for what the card
    /// holds: each line's letters as its cells hold them, matched without
    /// regard to case unless what is looked for has a capital, as a
    /// terminal's find does. `fresh` is a new phrase: the newest match
    /// stands, the one nearest the prompt; else the one standing is kept.
    fn find_run(&mut self, fresh: bool, cx: &App) {
        let Some(pty) = self.pty() else { return };
        let Some(find) = self.find.as_mut().filter(|f| f.open) else { return };
        find.looked = Instant::now();
        let was = find.hits.get(find.at).copied();
        find.hits = Vec::new();
        let q: Vec<char> = find.input.read(cx).value().chars().collect();
        if q.is_empty() {
            return;
        }
        let exact = q.iter().any(|c| c.is_uppercase());
        let fold = |c: char| if exact { c } else { c.to_lowercase().next().unwrap_or(c) };
        let q: Vec<char> = q.into_iter().map(fold).collect();
        let rows = pty.size().0 as i64;
        let (first, lines) = pty.lines(-(pty.kept_back() as i64), rows - 1);
        for (i, (row, _)) in lines.iter().enumerate() {
            let cells = row_cells(row);
            // The letters, each with the column it starts at.
            let letters: Vec<(char, usize)> = cells.iter().enumerate().filter(|(_, c)| **c != '\0').map(|(col, c)| (fold(*c), col)).collect();
            if letters.len() < q.len() {
                continue;
            }
            let mut at = 0;
            while at + q.len() <= letters.len() {
                if letters[at..at + q.len()].iter().map(|(c, _)| *c).eq(q.iter().copied()) {
                    let end = letters.get(at + q.len()).map(|(_, col)| *col).unwrap_or(cells.len());
                    find.hits.push(Hit { line: first + i as i64, from: letters[at].1, to: end });
                    at += q.len();
                } else {
                    at += 1;
                }
            }
        }
        find.at = match was.filter(|_| !fresh).and_then(|w| find.hits.iter().position(|h| *h == w)) {
            Some(i) => i,
            None => find.hits.len().saturating_sub(1),
        };
        if fresh {
            self.find_show();
        }
    }

    /// The match that stands is brought into sight, a third down the
    /// screen when it was out of it.
    fn find_show(&mut self) {
        let (Some(pty), Some(find)) = (self.pty(), self.find.as_ref()) else { return };
        let Some(hit) = find.hits.get(find.at) else { return };
        let rows = pty.size().0 as i64;
        let back = self.view.back as i64;
        if hit.line < -back || hit.line > rows - 1 - back {
            self.view.back = (rows / 3 - hit.line).clamp(0, pty.kept_back() as i64) as usize;
            self.unhover();
        }
    }

    /// The next match (↩, ⌘G) or the one before (⇧↩, ⇧⌘G), round the ends.
    fn find_step(&mut self, by: i64, cx: &mut Context<Self>) {
        self.find_run(false, cx);
        let Some(find) = self.find.as_mut().filter(|f| f.open && !f.hits.is_empty()) else { return };
        let n = find.hits.len() as i64;
        find.at = (find.at as i64 + by).rem_euclid(n) as usize;
        self.find_show();
        cx.notify();
    }

    /// The find card, at the screen's top right: the phrase, how many it
    /// found and which stands, the way to the one before and the next, and
    /// its close. It comes in as the note's find bar does and goes over a
    /// moment.
    fn render_find(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let find = self.find.as_ref()?;
        let gone = !find.open && find.since.elapsed() >= crate::widgets::MENU_OUT;
        if gone {
            return None;
        }
        let theme = cx.theme().clone();
        let empty = find.input.read(cx).value().trim().is_empty();
        let n = find.hits.len();
        let count = if empty {
            String::new()
        } else if n == 0 {
            t("no matches").to_string()
        } else {
            tf("{n} of {total}", &[("n", &(find.at + 1)), ("total", &n)])
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
        let open = find.open;
        let card = h_flex()
            .id("term-find")
            .key_context(TERM_FIND_CONTEXT)
            .on_action(cx.listener(|this, _: &crate::shell::Escape, window, cx| {
                cx.stop_propagation();
                this.find_close(window, cx)
            }))
            .on_action(cx.listener(|this, _: &TermFind, window, cx| this.find_open(window, cx)))
            .on_action(cx.listener(|this, _: &TermFindNext, _, cx| this.find_step(1, cx)))
            .on_action(cx.listener(|this, _: &TermFindPrev, _, cx| this.find_step(-1, cx)))
            // A press here is the card's, not the screen's.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(300.))
            .max_w_full()
            .h(px(34.))
            .px(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded(px(10.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_sm()
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .child(Icon::default().path("icons/search.svg").with_size(px(14.)).text_color(theme.muted_foreground))
            .child(div().flex_1().min_w_0().text_size(px(13.)).child(Input::new(&find.input).appearance(false).bordered(false)))
            .child(div().text_size(px(11.5)).flex_shrink_0().text_color(if n == 0 && !empty { theme.danger } else { theme.muted_foreground }).child(count))
            .child(button("term-find-prev", "icons/chevron-up.svg", t("Previous (⇧↩)").into()).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.find_step(-1, cx)
            })))
            .child(button("term-find-next", "icons/chevron-down.svg", t("Next (↩)").into()).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.find_step(1, cx)
            })))
            .child(button("term-find-close", "icons/close.svg", t("Close (Esc)").into()).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.find_close(window, cx)
            })));
        let length = if open { crate::widgets::MENU_IN } else { crate::widgets::MENU_OUT };
        Some(
            div()
                .absolute()
                .top(px(HEAD_H + 8.))
                .right(px(14.))
                .left(px(14.))
                .flex()
                .justify_end()
                .child(card)
                .with_animation(ElementId::Name(format!("term-find-{open}").into()), Animation::new(length).with_easing(ease_out_quint()), move |d, t| {
                    let t = if open { t } else { 1. - t };
                    d.opacity(t).mt(px(-6. * (1. - t)))
                })
                .into_any_element(),
        )
    }

    /// A probe's step (`ONYX_GO`): `termtype:<words>` types them and
    /// Return, `termput:<words>` without it, `term:tab+` and `term:tab-`
    /// make and close a tab, `term:copied` shows the toast,
    /// `term:sel:<unit>,<line>,<col>,<line>,<col>` selects.
    pub(crate) fn probe(&mut self, step: &str, window: &mut Window, cx: &mut Context<Self>) {
        match step {
            "term:tab+" => self.shell_new(None, cx),
            "term:tab-" => {
                if let Some(id) = self.shells.current().map(|t| t.id) {
                    self.shell_close(id, cx);
                }
            }
            "term:copied" => self.view.copied = Some(Instant::now()),
            _ if step.starts_with("term:find:") => {
                if self.find.is_none() || !self.find.as_ref().is_some_and(|f| f.open) {
                    eprintln!("onyx: term:find needs the card open (term:findopen)");
                }
                if let Some(find) = &self.find {
                    let q = step["term:find:".len()..].to_string();
                    find.input.update(cx, |s, cx| s.set_value(q, window, cx));
                }
                self.find_run(true, cx);
                if let Some(f) = &self.find {
                    eprintln!("onyx: term find {} of {}", f.at + 1, f.hits.len());
                }
            }
            "term:findopen" => self.find_open(window, cx),
            // As an input method gives text: composing, then typed.
            _ if step.starts_with("term:mark:") => EntityInputHandler::replace_and_mark_text_in_range(self, None, &step["term:mark:".len()..], None, window, cx),
            _ if step.starts_with("term:commit:") => EntityInputHandler::replace_text_in_range(self, None, &step["term:commit:".len()..], window, cx),
            "term:findnext" => self.find_step(1, cx),
            "term:clear" => self.clear(),
            _ if step.starts_with("term:sel:") => {
                let n: Vec<i64> = step["term:sel:".len()..].split(',').filter_map(|v| v.parse().ok()).collect();
                if let [unit, l1, c1, l2, c2] = n[..] {
                    self.view.sel = Some(Sel { anchor: (l1, c1 as u16), head: (l2, c2 as u16), unit: unit as usize % 4, block: unit >= 4 });
                    eprintln!("onyx: selected {:?}", self.sel_text());
                }
            }
            _ if step.starts_with("termput:") => {
                if let Some(pty) = self.pty() {
                    pty.write(step["termput:".len()..].as_bytes());
                }
            }
            _ if step.starts_with("termtype:") => {
                if let Some(pty) = self.pty() {
                    pty.write(step["termtype:".len()..].as_bytes());
                    cx.spawn(async move |_, cx| {
                        cx.background_executor().timer(Duration::from_millis(400)).await;
                        pty.write(b"\r");
                    })
                    .detach();
                }
            }
            "termcursor" => {
                if let Some(pty) = self.pty() {
                    eprintln!("onyx: term cursor {:?} size {:?} typing {:?} back {} text {:?}", pty.cursor(), pty.size(), pty.typing_start(), self.view.back, pty::plain(&pty.rows_back(0).0).trim_end());
                }
            }
            _ => {}
        }
        cx.notify();
    }
}

impl Render for TermPanel {
    /// The head, a row of tabs over the screen, and the screen of the tab
    /// that shows. Put away, it is drawn as it was while it goes: nothing
    /// is started, sized or measured for it.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let leaving = !self.shown;
        let theme = cx.theme().clone();
        // The screen's size in cells, from the room it was last given.
        let k = self.scheme(cx);
        let face = self.font(k);
        let (text, lh) = (TEXT, self.line_h());
        let scale = window.scale_factor().max(1.);
        let (pad, pad_foot) = PAD;
        let cell_w = {
            let probe = "MMMMMMMMMM";
            let run = TextRun { len: probe.len(), font: face.clone(), color: gpui::black(), background_color: None, underline: None, strikethrough: None };
            f32::from(window.text_system().shape_line(probe.into(), px(text), &[run], None).width) / probe.len() as f32
        };
        let room = self.view.bounds.get().size;
        let size = (((f32::from(room.height) - pad - pad_foot) / lh).floor().max(4.) as u16, ((f32::from(room.width) - 2. * pad) / cell_w.max(1.)).floor().max(20.) as u16);
        if room.height > px(0.) && !leaving {
            self.view.size.set(size);
            self.view.cell_w = cell_w;
            self.view.pad = (pad, pad_foot);
        }
        // The first tab is made when the terminal is first looked at.
        if !leaving && !self.shells.begun {
            self.shell_new(None, cx);
            self.swap = None;
        }
        let shown: Result<Arc<Pty>, AnyElement> = self.pty().ok_or_else(|| {
            let failed = self.shells.failed.clone();
            v_flex()
                .absolute()
                .inset_0()
                .items_center()
                .justify_center()
                .px(px(28.))
                .text_size(px(12.5))
                .text_center()
                .map(|d| match failed {
                    Some(e) => d.text_color(hsla(k.ansi[1])).child(tf("Could not start a shell: {error}", &[("error", &e)])),
                    None => d.text_color(hsla(k.tab_off)).child(t("No shell open. Press + for a new one.")),
                })
                .into_any_element()
        });
        // The tab just left, drawn once more as it goes.
        let swap = self.swap.filter(|(_, at, _)| at.elapsed() < TAB_ANIM);
        let swap_serial = self.swap.map(|(_, _, n)| n + 1).unwrap_or(0);
        let was = swap.filter(|_| !leaving).and_then(|(from, _, _)| self.shells.tabs.iter().find(|t| t.id == from).map(|t| t.pty.clone()));
        if let Ok(pty) = &shown {
            if !leaving && room.height > px(0.) && pty.size() != size {
                pty.resize(size.0, size.1);
                // The rows reflow under a selection, which no longer names
                // what it did.
                self.view.sel = None;
            }
        }
        let (ground, ink) = (k.bg, k.fg);
        let focused = self.view.focus.is_focused(window);
        if !leaving {
            self.view.focused = focused;
        }
        // A program that hears the mouse has the arrow, a link the hand.
        let grabbed = shown.as_ref().is_ok_and(|p| p.alive() && p.mouse().is_some()) && self.view.back == 0;
        let pointer = if self.view.link.is_some() { CursorStyle::PointingHand } else if grabbed { CursorStyle::Arrow } else { CursorStyle::IBeam };
        let body: AnyElement = match shown {
            Err(el) => el,
            Ok(pty) => {
                let (rows, back) = pty.rows_back(self.view.back);
                self.view.back = back;
                let picked = match self.view.sel {
                    Some(sel) if !leaving => sel.spans(-(back as i64), &rows.iter().map(|r| row_cells(r)).collect::<Vec<_>>()),
                    _ => Vec::new(),
                };
                // The find card's matches on the rows in sight, looked for
                // again while the screen changes.
                if self.find.as_ref().is_some_and(|f| f.open && f.looked.elapsed() > Duration::from_millis(300)) && pty.quiet_for() < POLL * 4 {
                    self.find_run(false, cx);
                }
                let mut found: Vec<Vec<(usize, usize, bool)>> = vec![Vec::new(); rows.len()];
                if let Some(find) = self.find.as_ref().filter(|f| f.open && !leaving) {
                    for (i, hit) in find.hits.iter().enumerate() {
                        if let Some(row) = usize::try_from(hit.line + back as i64).ok().and_then(|r| found.get_mut(r)) {
                            row.push((hit.from, hit.to, i == find.at));
                        }
                    }
                }
                let lines = screen_lines(&rows, cell_w, lh, k, &picked, &found, self.view.link.as_ref().filter(|_| !leaving));
                let copied = self.view.copied.filter(|at| at.elapsed() < COPIED_FOR && !leaving).map(|_| {
                    // Kaku's toast: two cells in from the corner, a cell and
                    // a half tall, gone over its last half second.
                    div()
                        .absolute()
                        .right(px(2. * cell_w))
                        .bottom(px(2. * lh))
                        .h(px(1.5 * lh))
                        .px(px(0.75 * cell_w * 2.))
                        .flex()
                        .items_center()
                        .rounded(px(8.))
                        .bg(hsla(k.toast.0).opacity(0.9))
                        .text_color(hsla(k.toast.1))
                        .font(face.clone())
                        .text_size(px(text))
                        .child(t("Copied"))
                        .with_animation("term-copied", Animation::new(COPIED_FOR), |d, t| d.opacity(if t > 0.8 { (1. - t) / 0.2 } else { 1. }))
                });
                // The cursor, where the next letter goes: Kaku's bar, two
                // of the screen's pixels wide and the row's height, on and
                // off by turns from when it last moved while the terminal
                // has the keyboard, and still otherwise.
                let at = pty.cursor().filter(|_| back == 0 && pty.alive());
                if let Some(at) = at.filter(|at| !leaving && *at != self.view.blink.0) {
                    self.view.blink = (at, Instant::now());
                }
                if !leaving {
                    self.view.blink_on = self.blink_on();
                }
                let (shape, blinks) = (self.cfg.cursor.clone(), self.cfg.cursor_blink);
                // What an input method is composing stands at the cursor,
                // underlined, on the screen's ground, in place of the cursor.
                let composing = self.marked.clone().filter(|m| !m.is_empty() && !leaving).zip(at).map(|(m, (row, col))| {
                    div()
                        .absolute()
                        .top(px(pad + row as f32 * lh))
                        .left(px(pad + col as f32 * cell_w))
                        .h(px(lh))
                        .flex()
                        .items_center()
                        .bg(hsla(ground))
                        .text_color(hsla(ink))
                        .underline()
                        .whitespace_nowrap()
                        .child(m)
                });
                let at = at.filter(|_| composing.is_none());
                let cursor = at.filter(|_| !focused || !blinks || self.view.blink_on).map(|(row, col)| {
                    let (thin, ink) = (px((2. / scale).max(1.)), hsla(k.cursor));
                    let d = div().absolute().top(px(pad + row as f32 * lh)).left(px(pad + col as f32 * cell_w)).h(px(lh));
                    match shape.as_str() {
                        // The letter under a block shows through it; without
                        // the keyboard the block is its outline.
                        "block" if focused => d.w(px(cell_w)).bg(ink.opacity(0.55)),
                        "block" => d.w(px(cell_w)).border_1().border_color(ink),
                        "underline" => d.w(px(cell_w)).border_b(thin * 2.).border_color(ink),
                        _ => d.w(thin).bg(ink),
                    }
                });
                let ended = (!pty.alive()).then(|| {
                    h_flex()
                        .absolute()
                        .bottom(px(12.))
                        .left_0()
                        .right_0()
                        .justify_center()
                        .gap(px(10.))
                        .items_center()
                        .text_size(px(12.))
                        .text_color(hsla(mix(ink, ground, 0.4)))
                        .child(t("The shell has ended."))
                        .child(choice_pill("term-again", "icons/terminal.svg", t("New Shell"), Press::Quiet, false, &theme).on_click(cx.listener(move |this, _, window, cx| {
                            swallow_click(window, cx);
                            // A new one in the ended one's tab's place.
                            if let Some((ix, id)) = this.shells.tabs.iter().position(|t| t.id == this.shells.on).map(|ix| (ix, this.shells.on)) {
                                this.shell_close(id, cx);
                                this.shell_new(Some(ix), cx);
                            }
                            window.focus(&this.view.focus, cx);
                        })))
                });
                let screen_face = face.clone();
                let screen = move |lines: Vec<AnyElement>| div().absolute().inset_0().overflow_hidden().px(px(pad)).pt(px(pad)).font(screen_face.clone()).text_size(px(text)).text_color(hsla(ink)).children(lines);
                let live = swap.is_some();
                div()
                    .absolute()
                    .inset_0()
                    .overflow_hidden()
                    // One tab in another's place: it fades in while the
                    // other fades out over it.
                    .child(screen(lines).children(cursor).children(composing).with_animation(ElementId::Name(format!("term-screen-{swap_serial}").into()), Animation::new(TAB_ANIM).with_easing(ease_out_quint()), move |d, t| if live { d.opacity(t) } else { d }))
                    .children(was.map(|old| screen(screen_lines(&old.rows_back(0).0, cell_w, lh, k, &[], &[], None)).with_animation(ElementId::Name(format!("term-screen-was-{swap_serial}").into()), Animation::new(TAB_ANIM).with_easing(ease_out_quint()), |d, t| d.opacity(1. - t))))
                    .children(ended)
                    .children(copied)
                    .into_any_element()
            }
        };
        // The bar's thumb dragged: that far back. Then the bar is told
        // where the screen now is.
        if let Some(back) = self.view.scroll.asked() {
            let most = self.pty().map(|p| p.kept_back()).unwrap_or(0);
            self.view.back = back.min(most);
        }
        {
            let view = &self.view;
            let rows = view.size.get().0 as usize;
            let pty = self.pty();
            view.scroll.drawn(view.bounds.get(), self.line_h(), rows, view.back, || pty.map(|p| p.kept_back()).unwrap_or(0));
        }
        let lines_back = self.view.back;
        let back = (lines_back > 0).then(|| div().flex_shrink_0().pr(px(10.)).font(face.clone()).text_size(px(11.)).text_color(hsla(k.tab_off)).child(count(lines_back, "{n} line back", "{n} lines back")));
        // The screen scrolls by its own count of rows back: it names itself
        // to the router with what moves it, so a gesture begun on it stays
        // on it, momentum included, wherever the pointer is carried.
        if !leaving {
            if let Some(router) = cx.try_global::<crate::scroll::Router>() {
                let me = cx.entity().downgrade();
                router.0.name_own(self.view.bounds.clone(), Rc::new(move |e, _, cx| {
                    if let Some(me) = me.upgrade() {
                        me.update(cx, |this, cx| {
                            let lh = this.line_h();
                            this.wheel(e.delta.pixel_delta(px(lh)).y, e.position, &e.modifiers);
                            cx.notify();
                        });
                    }
                }));
            }
        }
        let bounds = self.view.bounds.clone();
        let term_scroll = self.view.scroll.clone();
        let entity = cx.entity().downgrade();
        let (me, input_focus) = (cx.entity(), self.view.focus.clone());
        // A drag that selects is followed anywhere in the window: an
        // element hears the pointer only while it is over it.
        let selecting = (self.view.selecting && !leaving).then(|| {
            let entity = entity.clone();
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    let entity = entity.clone();
                    window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            if let Some(this) = entity.upgrade() {
                                this.update(cx, |this, cx| this.select_to(e.position, e.pressed_button == Some(MouseButton::Left), cx));
                            }
                        }
                    });
                },
            )
            .absolute()
            .size_0()
        });
        let screen = div()
            .id("term-screen")
            .key_context(TERMINAL_CONTEXT)
            .when(!leaving, |d| d.track_focus(&self.view.focus))
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(hsla(ground))
            .cursor(pointer)
            // Files dragged in from the Finder are typed as their paths,
            // the edge in the accent while they are over it.
            .border_1()
            .border_color(transparent_black())
            .drag_over::<ExternalPaths>({
                let accent = theme.primary;
                move |s, _, _, _| s.border_color(accent)
            })
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.drop_paths(paths.paths(), cx);
                window.focus(&this.view.focus, cx);
            }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| this.key(e, cx)))
            .on_action(cx.listener(|this, _: &TermTab, _, cx| {
                if let Some(pty) = this.pty() {
                    pty.write(b"\t");
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TermBackTab, _, cx| {
                if let Some(pty) = this.pty() {
                    pty.write(b"\x1b[Z");
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TermFind, window, cx| this.find_open(window, cx)))
            .on_action(cx.listener(|this, _: &TermFindNext, _, cx| this.find_step(1, cx)))
            .on_action(cx.listener(|this, _: &TermFindPrev, _, cx| this.find_step(-1, cx)))
            .on_action(cx.listener(|this, _: &TermClear, _, cx| {
                this.clear();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TermNewTab, window, cx| {
                this.shell_new(None, cx);
                window.focus(&this.view.focus, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::shell::CloseTab, _, cx| {
                // ⌘W, the window's own action, heard here first while the
                // terminal has the keyboard: it closes the tab showing,
                // and with none left puts the terminal away, as its button
                // does, so it never reaches the note's tab.
                match this.shells.current().map(|t| t.id) {
                    Some(id) => this.shell_close(id, cx),
                    None => cx.emit(TermEvent::Away),
                }
            }))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let lh = this.line_h();
                this.wheel(e.delta.pixel_delta(px(lh)).y, e.position, &e.modifiers);
                cx.notify();
            }))
            .on_any_mouse_down(cx.listener(|this, e: &MouseDownEvent, window, cx| {
                window.focus(&this.view.focus, cx);
                this.mouse_down(e, window, cx);
                swallow_click(window, cx);
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.mouse_move(e, cx)))
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if !over && this.view.link.is_some() {
                    this.unhover();
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, _, cx| this.mouse_up(e, cx)))
            .on_mouse_up(MouseButton::Right, cx.listener(|this, e: &MouseUpEvent, _, cx| this.mouse_up(e, cx)))
            .on_mouse_up(MouseButton::Middle, cx.listener(|this, e: &MouseUpEvent, _, cx| this.mouse_up(e, cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, _, cx| this.mouse_up(e, cx)))
            // Where the screen is and how large, for the next draw.
            .when(!leaving, |d| {
                d.child(
                    canvas(
                        move |b, _, cx| {
                            if bounds.get() != b {
                                bounds.set(b);
                                if let Some(e) = entity.upgrade() {
                                    cx.defer(move |cx| e.update(cx, |_, cx| cx.notify()));
                                }
                            }
                            b
                        },
                        // Typed text comes through the input handler, so an
                        // input method composes in place.
                        move |_, b, window, cx| {
                            if input_focus.is_focused(window) {
                                window.handle_input(&input_focus, ElementInputHandler::new(b, me.clone()), cx);
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                )
            })
            .child(body)
            .children(selecting)
            // The bar every scroller has, over the kept lines: last, over
            // the screen's layers.
            .vertical_scrollbar(&term_scroll);
        let w = f32::from(self.view.bounds.get().size.width).max(TERM_LEAST);
        let head_row = self.render_tabs(w - if back.is_some() { 110. } else { 0. }, cx);
        let find = self.render_find(cx);
        // No ground of its own: the sheet it stands on lays the same one,
        // rounded (`shell.rs`, `render_terminal`), and a ground here would
        // stand square past the sheet's curve, as a clip is square.
        v_flex()
            .relative()
            .size_full()
            .child(h_flex().h(px(HEAD_H)).flex_shrink_0().items_center().overflow_hidden().border_b_1().border_color(hsla(k.split)).child(head_row).child(div().flex_1()).children(back))
            .child(screen)
            .children(find)
    }
}

/// Text from the keyboard, as gpui's input handler gives it: an input
/// method's composing is drawn at the cursor (`marked`), and what is
/// typed goes to the shell, in place of what is selected of the line
/// being typed. The terminal keeps no text of its own: the only text it
/// tells of is what is being composed.
impl EntityInputHandler for TermPanel {
    fn text_for_range(&mut self, range: std::ops::Range<usize>, adjusted: &mut Option<std::ops::Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let marked: Vec<u16> = self.marked.as_deref().unwrap_or("").encode_utf16().collect();
        let range = range.start.min(marked.len())..range.end.min(marked.len());
        *adjusted = Some(range.clone());
        Some(String::from_utf16_lossy(&marked[range]))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let n = self.marked.as_deref().unwrap_or("").encode_utf16().count();
        Some(UTF16Selection { range: n..n, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<std::ops::Range<usize>> {
        self.marked.as_ref().map(|m| 0..m.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        let Some(pty) = self.pty().filter(|p| p.alive()) else { return cx.notify() };
        if !text.is_empty() {
            let bytes = text.as_bytes().to_vec();
            // A letter typed takes the place of what is selected of the
            // line being typed, as it does in a field.
            if self.view.sel.is_none() || !self.delete_sel(bytes.clone()) {
                self.send(&pty, &bytes);
            }
        }
        self.view.back = 0;
        self.view.sel = None;
        self.unhover();
        // A key begins the cursor's blink again, lit.
        self.view.blink.1 = Instant::now();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(&mut self, _: Option<std::ops::Range<usize>>, text: &str, _: Option<std::ops::Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = Some(text.to_string()).filter(|t| !t.is_empty());
        self.view.back = 0;
        cx.notify();
    }

    /// Where the input method's candidates stand: under the cursor, where
    /// what is composed begins.
    fn bounds_for_range(&mut self, _: std::ops::Range<usize>, element: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let (row, col) = self.pty()?.cursor()?;
        let lh = self.line_h();
        let (pad, cell_w) = (self.view.pad.0, self.view.cell_w);
        Some(Bounds::new(element.origin + point(px(pad + col as f32 * cell_w), px(pad + row as f32 * lh)), size(px(cell_w), px(lh))))
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        true
    }
}

/// A picture on the clipboard, kept as a file for its path to be pasted:
/// in the cache's folder, by when it was pasted.
fn keep_image(mime: &str, bytes: &[u8]) -> Option<PathBuf> {
    let ext = mime.rsplit('/').next().filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("png");
    let dir = onyx_core::paths::cache_dir().join("pasted");
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = dir.join(format!("{stamp}.{ext}"));
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

/// Faces to fall back on, Kaku's list: Chinese, symbols, emoji, and the
/// icons a prompt's theme draws.
const TERM_FALLBACKS: &[&str] = &["PingFang SC", "Apple Symbols", "Apple Color Emoji", "Symbols Nerd Font Mono"];

/// A key pressed in the terminal, as a terminal sends it. None for
/// a key that is the app's own (anything with ⌘) or sends nothing.
///
/// Off a Mac the keys follow the system's own terminal. Ctrl with an
/// arrow, Backspace or Delete is a word: on Windows Windows Terminal's
/// sequences, which the console turns into the keys PowerShell and cmd
/// know; on Linux readline's word keys, which every shell takes. Alt with a key is that key after Escape, and a modifier on an
/// arrow, Home, End or a page key is told as xterm tells it.
fn term_bytes(k: &Keystroke) -> Option<Vec<u8>> {
    let m = &k.modifiers;
    if m.platform {
        return None;
    }
    let pc = !cfg!(target_os = "macos");
    // AltGr is Ctrl and Alt to the window, and types a character of its own.
    if pc && m.control && m.alt {
        if let Some(c) = k.key_char.as_ref().filter(|c| !c.is_empty() && **c != k.key && !c.chars().any(char::is_control)) {
            return Some(c.clone().into_bytes());
        }
    }
    if pc && m.control && !m.alt && !m.shift {
        let word: Option<&[u8]> = match (cfg!(windows), k.key.as_str()) {
            (true, "left") => Some(b"\x1b[1;5D"),
            (true, "right") => Some(b"\x1b[1;5C"),
            (true, "backspace") => Some(b"\x08"),
            (true, "delete") => Some(b"\x1b[3;5~"),
            (false, "left") => Some(b"\x1bb"),
            (false, "right") => Some(b"\x1bf"),
            (false, "backspace") => Some(b"\x17"),
            (false, "delete") => Some(b"\x1bd"),
            _ => None,
        };
        if let Some(bytes) = word {
            return Some(bytes.to_vec());
        }
    }
    // xterm's modifier number: 1, and Shift 1, Alt 2, Ctrl 4 added.
    let held = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.control as u8;
    if pc && held > 1 {
        let final_letter = match k.key.as_str() {
            "up" => Some('A'),
            "down" => Some('B'),
            "right" => Some('C'),
            "left" => Some('D'),
            "home" => Some('H'),
            "end" => Some('F'),
            _ => None,
        };
        if let Some(c) = final_letter {
            return Some(format!("\x1b[1;{held}{c}").into_bytes());
        }
        let tilde = match k.key.as_str() {
            "insert" => Some(2),
            "delete" => Some(3),
            "pageup" => Some(5),
            "pagedown" => Some(6),
            _ => None,
        };
        if let Some(n) = tilde {
            return Some(format!("\x1b[{n};{held}~").into_bytes());
        }
    }
    if m.control && k.key == "space" {
        return Some(vec![0]);
    }
    let named: Option<&[u8]> = match k.key.as_str() {
        "enter" => Some(b"\r"),
        "escape" => Some(b"\x1b"),
        "backspace" => Some(b"\x7f"),
        "tab" if m.shift => Some(b"\x1b[Z"),
        "tab" => Some(b"\t"),
        "space" => Some(b" "),
        "up" => Some(b"\x1b[A"),
        "down" => Some(b"\x1b[B"),
        "right" => Some(b"\x1b[C"),
        "left" => Some(b"\x1b[D"),
        "home" => Some(b"\x1b[H"),
        "end" => Some(b"\x1b[F"),
        "insert" => Some(b"\x1b[2~"),
        "delete" => Some(b"\x1b[3~"),
        "pageup" => Some(b"\x1b[5~"),
        "pagedown" => Some(b"\x1b[6~"),
        _ => None,
    };
    // Off a Mac, Alt is a meta key: Escape, then what the key sends.
    let meta = |bytes: Vec<u8>| if pc && m.alt { [b"\x1b".as_slice(), &bytes].concat() } else { bytes };
    if let Some(bytes) = named {
        return Some(meta(bytes.to_vec()));
    }
    if m.control {
        let mut chars = k.key.chars();
        let c = chars.next().filter(|_| chars.next().is_none())?.to_ascii_lowercase();
        let byte = match c {
            'a'..='z' => c as u8 & 0x1f,
            '@' => 0,
            '[' => 0x1b,
            '\\' => 0x1c,
            ']' => 0x1d,
            '/' | '_' | '-' => 0x1f,
            _ => return None,
        };
        return Some(meta(vec![byte]));
    }
    let typed = k.key_char.clone().or_else(|| (k.key.chars().count() == 1).then(|| if m.shift { k.key.to_uppercase() } else { k.key.clone() }))?;
    Some(meta(typed.into_bytes()))
}


/// The link a cell is part of, by Kaku's rules: the cells it takes and
/// what it opens. An address with a scheme, one that begins "www.", a
/// mail address, a bare domain under a well-known ending, and a path to
/// a file that is there (`cwd` is where a relative one starts; a line and
/// column after it are let go).
fn link_at(cells: &[char], col: usize, cwd: &std::path::Path) -> Option<(usize, usize, Target)> {
    const ENDINGS: &[&str] = &[
        "com", "net", "org", "edu", "gov", "io", "dev", "ai", "fun", "xyz", "me", "im", "tv", "to", "co", "info", "biz", "tech", "site", "online", "cloud", "blog", "store", "link", "live", "news", "cn", "jp", "kr", "uk", "de", "fr",
        "us", "ca", "au", "br", "ru", "nl", "se", "ch", "hk", "tw", "sg",
    ];
    let edge = |c: char| c.is_whitespace() || c == '\0' || "\"'<>`".contains(c);
    if cells.get(col).is_none_or(|c| edge(*c)) {
        return None;
    }
    let mut from = cells[..col].iter().rposition(|c| edge(*c)).map(|i| i + 1).unwrap_or(0);
    let mut to = cells[col..].iter().position(|c| edge(*c)).map(|i| col + i).unwrap_or(cells.len());
    // The brackets around it and the stop after it are not part of it.
    while from < to && "([{“‘".contains(cells[from]) {
        from += 1;
    }
    while to > from && ".,;:!?)]}”’".contains(cells[to - 1]) && !(cells[to - 1] == ')' && cells[from..to].contains(&'(')) {
        to -= 1;
    }
    let word: String = cells[from..to].iter().collect();
    let target = if let Some(at) = word.find("://") {
        // From where the scheme's name begins.
        // Past the character that is not one of its letters, which may be
        // more than a byte (“, →).
        let start = word[..at].char_indices().rfind(|(_, c)| !c.is_alphanumeric() && *c != '_').map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
        if start == at {
            return None;
        }
        from += word[..start].chars().count();
        Target::Url(word[start..].to_string())
    } else if word.to_lowercase().starts_with("www.") && word.len() > 4 {
        Target::Url(format!("https://{word}"))
    } else if let Some((name, host)) = word.split_once('@').filter(|(name, host)| {
        let plain = |s: &str, more: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || more.contains(c));
        plain(name, "_.+-") && host.contains('.') && host.split('.').all(|part| plain(part, "_-"))
    }) {
        Target::Url(format!("mailto:{name}@{host}"))
    } else {
        let host = word.split(['/', ':']).next().unwrap_or("").to_lowercase();
        let parts: Vec<&str> = host.split('.').collect();
        let domain = parts.len() > 1 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) && ENDINGS.contains(parts.last()?);
        if domain {
            Target::Url(format!("https://{word}"))
        } else if word.contains('/') {
            // A path, with the line and the column a compiler adds let go.
            let mut path = word.as_str();
            for _ in 0..2 {
                if let Some((head, tail)) = path.rsplit_once(':').filter(|(_, tail)| !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit())) {
                    let _ = tail;
                    path = head;
                }
            }
            let path = match path.strip_prefix("~/") {
                Some(rest) => onyx_core::paths::home().join(rest),
                None => cwd.join(path),
            };
            if !path.exists() {
                return None;
            }
            Target::File(path)
        } else {
            return None;
        }
    };
    (from..to).contains(&col).then_some((from, to, target))
}

/// A button as a mouse report names it.
fn button_code(b: MouseButton) -> Option<u8> {
    match b {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

/// The keys held with the mouse, as a report adds them.
fn mod_bits(m: &Modifiers) -> u8 {
    (if m.shift { 4 } else { 0 }) | (if m.alt { 8 } else { 0 }) | (if m.control { 16 } else { 0 })
}

/// One of Kaku's two color schemes (tw93/kaku, MIT): the screen's
/// ground and ink, the cursor, the selection's ground (with how strong)
/// and ink, the rule under the head, a tab's grounds and the ink of one
/// not showing, the toast's ground and ink, the sixteen named colors
/// (the window's own inks for code, `look.rs`), and the grounds and inks
/// it draws as others because a program's screen made for the other
/// kind of terminal reads badly with them as they are.
struct Scheme {
    bg: u32,
    fg: u32,
    cursor: u32,
    sel: (u32, f32),
    sel_fg: u32,
    split: u32,
    tab_on: u32,
    tab_hover: u32,
    tab_off: u32,
    toast: (u32, u32),
    ansi: [u32; 16],
    grounds: &'static [(u32, u32)],
    inks: &'static [(u32, u32)],
    /// The ground a gray from the other side of the scale is drawn as: a
    /// program's quiet band on a dark terminal is a quiet band here.
    wash: u32,
    /// The weights of plain and of bold letters: one step heavier on a
    /// light ground, where thin strokes fade.
    plain: FontWeight,
    bold: FontWeight,
}

static KAKU_DARK: Scheme = Scheme {
    bg: 0x2a2826,
    fg: 0xd5d4d6,
    cursor: 0x8e6ad9,
    sel: (0x8e6ad9, 0.55),
    sel_fg: 0xd5d4d6,
    split: 0x3f3c38,
    tab_on: 0x3f3c38,
    tab_hover: 0x33312e,
    tab_off: 0x6d6d6d,
    toast: (0x8e6ad9, 0xffffff),
    ansi: {
        let k = &crate::look::INKS_DARK;
        [0xc8c6cc, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0xd5d4d6, 0x6d6d6d, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0xd5d4d6]
    },
    grounds: &[(0xc8c6cc, 0x2a2826), (0x6d6d6d, 0x413f3b), (0x6e6e6e, 0x413f3b), (0x8ec3ff, 0x413f3b), (0xd5d4d6, 0x504d49)],
    inks: &[(0x000000, 0xd5d4d6), (0x110f18, 0xd5d4d6), (0x15141b, 0xd5d4d6), (0x1a1a1a, 0xd5d4d6), (0x1c1c1c, 0xd5d4d6)],
    wash: 0x413f3b,
    plain: FontWeight::NORMAL,
    bold: FontWeight::MEDIUM,
};

static KAKU_LIGHT: Scheme = Scheme {
    bg: 0xfffcf0,
    fg: 0x100f0f,
    cursor: 0x343331,
    sel: (0xe8e6db, 1.),
    sel_fg: 0x100f0f,
    split: 0xdddbcf,
    tab_on: 0xe8e6db,
    tab_hover: 0xe8e6db,
    tab_off: 0x4a4946,
    // The window's graphite and paper, as its frame is: Kaku's dark
    // gold read as a stain on the cream.
    toast: (0x3d3a36, 0xf3f1ea),
    ansi: {
        let k = &crate::look::INKS_LIGHT;
        [0x100f0f, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0x575653, 0x6f6e69, k.red, k.green, k.gold, k.blue, k.violet, k.teal, 0x403e3c]
    },
    grounds: &[(0x575653, 0xf2f0eb), (0x585754, 0xf2f0eb), (0x225fa6, 0xf2f0eb), (crate::look::INKS_LIGHT.teal, 0xf2f0eb), (crate::look::INKS_LIGHT.green, 0xf2f0eb), (crate::look::INKS_LIGHT.gold, 0xf2f0eb), (crate::look::INKS_LIGHT.blue, 0xd3e3ea), (0x403e3c, 0xe8e6db)],
    inks: &[(0xffffdb, 0x575653), (0xffffdc, 0x575653)],
    wash: 0xf2f0eb,
    plain: FontWeight::MEDIUM,
    bold: FontWeight::SEMIBOLD,
};

fn scheme(light: bool) -> &'static Scheme {
    if light { &KAKU_LIGHT } else { &KAKU_DARK }
}

fn hsla(c: u32) -> Hsla {
    rgb(c).into()
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |sh: u32| ((a >> sh & 0xff) as f32 * (1.0 - t) + (b >> sh & 0xff) as f32 * t).round() as u32;
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// The color a scheme draws in place of `c`, when it has one.
fn swapped(table: &[(u32, u32)], c: u32) -> u32 {
    table.iter().find(|(from, _)| *from == c).map(|(_, to)| *to).unwrap_or(c)
}

/// Whether a color is a gray, or near enough to one.
fn gray(c: u32) -> bool {
    let ch = [c >> 16 & 0xff, c >> 8 & 0xff, c & 0xff];
    ch.iter().max().unwrap() - ch.iter().min().unwrap() < 28
}

/// `c`'s hue, brought to the scheme's side of the scale: its brightest
/// channel full, then a little of it over the scheme's ground. A brighter
/// ground to begin with (a word marked within a marked line) keeps more,
/// so what was stronger stays stronger.
fn tint(c: u32, ground: u32, light: bool) -> u32 {
    let top = (c >> 16 & 0xff).max(c >> 8 & 0xff).max(c & 0xff).max(1) as f32;
    let ch = |sh: u32| (((c >> sh & 0xff) as f32 * 255. / top).round() as u32).min(255);
    let vivid = ch(16) << 16 | ch(8) << 8 | ch(0);
    let b = brightness(c);
    mix(ground, vivid, if light { 0.14 + 0.5 * b } else { 0.2 + 0.2 * (1. - b) })
}

/// How bright a color is to the eye, 0 to 1.
fn brightness(c: u32) -> f32 {
    let ch = |sh: u32| {
        let v = (c >> sh & 0xff) as f32 / 255.;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
}

/// An ink that can be read on its ground: Kaku's `text_min_contrast_ratio`
/// of 3. An ink too close to the ground is taken toward black or white,
/// whichever is the farther from it, as far as it takes. This is what
/// makes colors a program chose for a dark terminal readable on the
/// light scheme, and the other way round.
fn legible(fg: u32, bg: u32) -> u32 {
    let ratio = |a: u32, b: u32| {
        let (x, y) = (brightness(a), brightness(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    };
    if ratio(fg, bg) >= 3. {
        return fg;
    }
    let to = if brightness(bg) > 0.18 { 0x000000 } else { 0xffffff };
    (1..=10).map(|t| mix(fg, to, t as f32 / 10.)).find(|c| ratio(*c, bg) >= 3.).unwrap_or(to)
}

/// A screen's rows as they are drawn, a row a line. `found` is what the
/// find card matched on each row, from and up to, and whether it is the
/// match that stands.
fn screen_lines(rows: &[Vec<pty::Span>], cell_w: f32, lh: f32, k: &Scheme, picked: &[Option<(usize, usize)>], found: &[Vec<(usize, usize, bool)>], link: Option<&Link>) -> Vec<AnyElement> {
    let mut lines: Vec<AnyElement> = Vec::with_capacity(rows.len());
    for (ix, row) in rows.iter().enumerate() {
        let sel = picked.get(ix).copied().flatten();
        let linked = link.filter(|l| l.row as usize == ix).map(|l| (l.from, l.to));
        // Whether a cell is selected, and whether it is part of the link
        // under the pointer: either changes how its letter is drawn.
        let marks = |col: usize| (sel.is_some_and(|(f, t)| (f..t).contains(&col)), linked.is_some_and(|(f, t)| (f..t).contains(&col)));
        // The letters, as runs of plain ones each set at the column it
        // starts at. A cell the face may draw wider or narrower than its
        // columns (an emoji, a symbol from a fallback face) stands by
        // itself at its own: laid out by the face's advances, one row of
        // text drifted off the columns the cursor, the grounds and the
        // selection are drawn at.
        let mut pieces: Vec<(usize, String, Vec<(std::ops::Range<usize>, HighlightStyle)>, bool)> = Vec::new();
        let mut piece_col = 0usize;
        let mut text = String::new();
        let mut looks = Vec::new();
        // What is drawn under the letters, a cell at a time: each
        // stretch's ground, the whole height of the row, then what is
        // selected, then the block and line characters as the shapes
        // they are.
        let mut grounds: Vec<AnyElement> = Vec::new();
        let mut under: Vec<AnyElement> = Vec::new();
        let mut col = 0usize;
        for s in row {
            // The sixteen named colors are the scheme's own.
            let mut fg = match s.fg_ix {
                Some(i) => k.ansi[i as usize],
                None => s.fg.map(|c| swapped(k.inks, c)).unwrap_or(k.fg),
            };
            let mut bg = match s.bg_ix {
                Some(i) => Some(swapped(k.grounds, k.ansi[i as usize])),
                None => s.bg.map(|c| swapped(k.grounds, c)),
            };
            // A gray ground a program chose for the other kind of terminal
            // (a program's band behind a prompt, near black for a dark
            // one) is the scheme's own quiet ground, and a gray ink that
            // was to stand out on it is the scheme's ink. Kaku swaps the
            // grays it knows by value; this is the same for any gray.
            let light = brightness(k.bg) > 0.5;
            if bg.is_some_and(|c| s.bg_ix.is_none() && gray(c) && if light { brightness(c) < 0.2 } else { brightness(c) > 0.5 }) {
                bg = Some(k.wash);
                if s.fg_ix.is_none() && gray(fg) && if light { brightness(fg) > 0.4 } else { brightness(fg) < 0.2 } {
                    fg = k.fg;
                }
            }
            // A colored ground from the other side of the scale (a
            // program's diff, green and red, set for a dark terminal) is a
            // soft tint of its own hue on this one, and a light ink on it
            // the scheme's: as bars of deep green and red on the light
            // scheme they were the loudest thing on the screen.
            else if let Some(c) = bg.filter(|c| s.bg_ix.is_none() && !gray(*c) && if light { brightness(*c) < 0.35 } else { brightness(*c) > 0.45 }) {
                bg = Some(tint(c, k.bg, light));
                if s.fg_ix.is_none() && if light { brightness(fg) > 0.4 } else { brightness(fg) < 0.2 } {
                    fg = k.fg;
                }
            }
            if s.inverse {
                (fg, bg) = (bg.unwrap_or(k.bg), Some(fg));
            }
            if s.dim {
                fg = mix(fg, bg.unwrap_or(k.bg), 0.45);
            }
            // Shapes keep the color asked for; letters must be readable.
            let shape = hsla(fg);
            let fg = legible(fg, bg.unwrap_or(k.bg));
            let look = |(picked, linked): (bool, bool)| HighlightStyle {
                color: Some(hsla(if picked { k.sel_fg } else { fg })),
                // Kaku has no italic: slanted text is drawn upright.
                font_weight: s.bold.then_some(k.bold),
                underline: (s.underline || linked).then(|| UnderlineStyle { thickness: px(1.), ..Default::default() }),
                ..Default::default()
            };
            let from = col;
            let mut run = (text.len(), marks(col));
            for (cell, w) in s.each_cell() {
                let now = marks(col);
                if now != run.1 {
                    if run.0 < text.len() {
                        looks.push((run.0..text.len(), look(run.1)));
                    }
                    run = (text.len(), now);
                }
                if text.is_empty() {
                    piece_col = col;
                }
                let x0 = col as f32 * cell_w;
                let c = cell.chars().next().unwrap_or(' ');
                if let Some((shapes, strength)) = block_shape(c) {
                    for (x, y, bw, bh) in shapes {
                        under.push(div().absolute().left(px(x0 + x * cell_w)).top(px(y * lh)).w(px(bw * cell_w + 0.5)).h(px(bh * lh)).bg(shape.opacity(strength)).into_any_element());
                    }
                    text.push(' ');
                } else if let Some(parts) = line_shape(c) {
                    under.extend(line_parts(parts, x0, cell_w, lh, shape));
                    text.push(' ');
                } else if cell.len() == 1 {
                    text.push_str(cell);
                } else {
                    if run.0 < text.len() {
                        looks.push((run.0..text.len(), look(run.1)));
                    }
                    if !text.is_empty() {
                        pieces.push((piece_col, std::mem::take(&mut text), std::mem::take(&mut looks), false));
                    }
                    let shown = cell_face(cell, w);
                    pieces.push((col, shown.to_string(), vec![(0..shown.len(), look(now))], narrow_emoji(cell, w)));
                    run = (0, now);
                }
                col += w as usize;
            }
            if run.0 < text.len() {
                looks.push((run.0..text.len(), look(run.1)));
            }
            if let Some(bg) = bg {
                grounds.push(div().absolute().left(px(from as f32 * cell_w)).top_0().w(px((col - from) as f32 * cell_w + 0.5)).h(px(lh)).bg(hsla(bg)).into_any_element());
            }
        }
        // What the find card matched, under the selection: the one that
        // stands in the cursor's ink, the others fainter.
        for &(from, to, current) in found.get(ix).map(Vec::as_slice).unwrap_or(&[]) {
            grounds.push(div().absolute().left(px(from as f32 * cell_w)).top_0().w(px(to.saturating_sub(from) as f32 * cell_w)).h(px(lh)).bg(hsla(k.cursor).opacity(if current { 0.5 } else { 0.2 })).into_any_element());
        }
        if let Some((from, to)) = sel {
            grounds.push(div().absolute().left(px(from as f32 * cell_w)).top_0().w(px((to - from) as f32 * cell_w)).h(px(lh)).bg(hsla(k.sel.0).opacity(k.sel.1)).into_any_element());
        }
        if !text.is_empty() {
            pieces.push((piece_col, text, looks, false));
        }
        let letters = pieces.into_iter().map(|(at, text, looks, narrow)| {
            let piece = div().absolute().top_0().left(px(at as f32 * cell_w));
            if narrow {
                // Set small enough to stand in its one column.
                piece.w(px(cell_w)).h(px(lh)).flex().items_center().justify_center().text_size(px(cell_w * 0.8)).line_height(px(lh)).child(StyledText::new(text).with_highlights(looks))
            } else {
                piece.child(StyledText::new(text).with_highlights(looks))
            }
        });
        lines.push(div().h(px(lh)).relative().flex_shrink_0().whitespace_nowrap().children(grounds).children(under).children(letters).into_any_element());
    }
    lines
}

/// What a cell draws: a character the emoji selector (U+FE0F) follows
/// in a cell of one column is drawn in its text form. The screen model
/// and the shell count it as one column, and its emoji is two wide:
/// drawn so, ☁️ covered the space after it.
fn cell_face(cell: &str, w: u8) -> std::borrow::Cow<'_, str> {
    if w == 1 && cell.contains('\u{fe0f}') {
        cell.replace('\u{fe0f}', "").into()
    } else {
        cell.into()
    }
}

/// Whether a cell holds a picture character with no text form (🖥,
/// U+1F5A5), with or without the emoji selector, that the screen model
/// and the program count as one column: it is drawn only as an emoji, two
/// columns wide, and drawn so it covered the character after it. It is
/// set small enough to fit.
fn narrow_emoji(cell: &str, w: u8) -> bool {
    w == 1 && cell.chars().next().is_some_and(|c| (0x1f000..=0x1faff).contains(&(c as u32)))
}

/// A line-drawing character (U+2500 on) as the arms it has from the
/// middle of its cell, left, right, up and down; how thick; and whether
/// its corner is round. A font draws these to its own line height, which
/// is less than a row's here, and the frames Claude Code draws came out
/// with a gap between every two rows. The ones not here are the font's.
fn line_shape(c: char) -> Option<(bool, bool, bool, bool, f32, bool)> {
    let (l, r, u, d, thick, round) = match c as u32 {
        0x2500 => (true, true, false, false, 1., false),
        0x2501 => (true, true, false, false, 2., false),
        0x2502 => (false, false, true, true, 1., false),
        0x2503 => (false, false, true, true, 2., false),
        0x250c => (false, true, false, true, 1., false),
        0x2510 => (true, false, false, true, 1., false),
        0x2514 => (false, true, true, false, 1., false),
        0x2518 => (true, false, true, false, 1., false),
        0x251c => (false, true, true, true, 1., false),
        0x2524 => (true, false, true, true, 1., false),
        0x252c => (true, true, false, true, 1., false),
        0x2534 => (true, true, true, false, 1., false),
        0x253c => (true, true, true, true, 1., false),
        0x256d => (false, true, false, true, 1., true),
        0x256e => (true, false, false, true, 1., true),
        0x256f => (true, false, true, false, 1., true),
        0x2570 => (false, true, true, false, 1., true),
        0x2574 => (true, false, false, false, 1., false),
        0x2575 => (false, false, true, false, 1., false),
        0x2576 => (false, true, false, false, 1., false),
        0x2577 => (false, false, false, true, 1., false),
        _ => return None,
    };
    Some((l, r, u, d, thick, round))
}

/// The arms of a line-drawing character, drawn in the cell that begins
/// at `x0`: each from the cell's middle to its edge.
fn line_parts((l, r, u, d, thick, round): (bool, bool, bool, bool, f32, bool), x0: f32, cell_w: f32, lh: f32, ink: Hsla) -> Vec<AnyElement> {
    let (cx, cy) = ((x0 + cell_w / 2. - thick / 2.).round(), (lh / 2. - thick / 2.).round());
    let (left, right) = (cx - x0, x0 + cell_w - cx);
    let at = |x: f32, y: f32, w: f32, h: f32| div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h));
    if round {
        // A quarter of a ring: two edges of a box with one round corner.
        let radius = px((cell_w / 2.).floor());
        let (x, w) = if r { (cx, right) } else { (x0, left + thick) };
        let (y, h) = if d { (cy, lh - cy) } else { (0., cy + thick) };
        let b = at(x, y, w, h).border_color(ink);
        let b = match (r, d) {
            (true, true) => b.border_l(px(thick)).border_t(px(thick)).rounded_tl(radius),
            (false, true) => b.border_r(px(thick)).border_t(px(thick)).rounded_tr(radius),
            (false, false) => b.border_r(px(thick)).border_b(px(thick)).rounded_br(radius),
            (true, false) => b.border_l(px(thick)).border_b(px(thick)).rounded_bl(radius),
        };
        return vec![b.into_any_element()];
    }
    let mut out = Vec::new();
    if l || r {
        let (x, w) = (if l { x0 } else { cx }, if l && r { cell_w + 0.5 } else if l { left + thick } else { right + 0.5 });
        out.push(at(x, cy, w, thick).bg(ink).into_any_element());
    }
    if u || d {
        let (y, h) = (if u { 0. } else { cy }, if u && d { lh } else if u { cy + thick } else { lh - cy });
        out.push(at(cx, y, thick, h).bg(ink).into_any_element());
    }
    out
}

/// A block character (U+2580 to U+259F) as the parts of its cell it
/// fills, each (left, top, width, height) in cells, and how strong the
/// ink is. A font draws these to its own letter height, which is less
/// than a row: pictures made of them (a program's own mark at the top
/// of its screen) came out in stripes with gaps between the rows.
fn block_shape(c: char) -> Option<(Vec<(f32, f32, f32, f32)>, f32)> {
    let code = c as u32;
    if !(0x2580..=0x259f).contains(&code) {
        return None;
    }
    let (ul, ur, ll, lr) = ((0., 0., 0.5, 0.5), (0.5, 0., 0.5, 0.5), (0., 0.5, 0.5, 0.5), (0.5, 0.5, 0.5, 0.5));
    let full = vec![(0., 0., 1., 1.)];
    Some(match code {
        0x2580 => (vec![(0., 0., 1., 0.5)], 1.),
        0x2581..=0x2587 => {
            let h = (code - 0x2580) as f32 / 8.;
            (vec![(0., 1. - h, 1., h)], 1.)
        }
        0x2588 => (full, 1.),
        0x2589..=0x258f => (vec![(0., 0., (0x2590 - code) as f32 / 8., 1.)], 1.),
        0x2590 => (vec![(0.5, 0., 0.5, 1.)], 1.),
        0x2591 => (full, 0.25),
        0x2592 => (full, 0.5),
        0x2593 => (full, 0.75),
        0x2594 => (vec![(0., 0., 1., 0.125)], 1.),
        0x2595 => (vec![(0.875, 0., 0.125, 1.)], 1.),
        0x2596 => (vec![ll], 1.),
        0x2597 => (vec![lr], 1.),
        0x2598 => (vec![ul], 1.),
        0x2599 => (vec![ul, ll, lr], 1.),
        0x259a => (vec![ul, lr], 1.),
        0x259b => (vec![ul, ur, ll], 1.),
        0x259c => (vec![ul, ur, lr], 1.),
        0x259d => (vec![ur], 1.),
        0x259e => (vec![ur, ll], 1.),
        _ => (vec![ur, ll, lr], 1.),
    })
}


#[cfg(test)]
mod tests {
    use super::{cell_face, link_at, narrow_emoji, Target};

    #[test]
    fn an_emoji_in_one_column_is_drawn_as_text() {
        assert_eq!(cell_face("\u{2601}\u{fe0f}", 1), "\u{2601}");
        assert_eq!(cell_face("1\u{fe0f}\u{20e3}", 1), "1\u{20e3}");
        // Two columns have room for the emoji.
        assert_eq!(cell_face("\u{2764}\u{fe0f}", 2), "\u{2764}\u{fe0f}");
        assert_eq!(cell_face("\u{1f680}", 2), "\u{1f680}");
    }

    #[test]
    fn a_picture_with_no_text_form_in_one_column_is_set_small() {
        assert!(narrow_emoji("\u{1f5a5}", 1));
        assert!(narrow_emoji("\u{1f5a5}\u{fe0f}", 1));
        // Two columns have room for it, and a symbol with a text form is drawn as text.
        assert!(!narrow_emoji("\u{1f5a5}", 2));
        assert!(!narrow_emoji("\u{2601}\u{fe0f}", 1));
    }

    #[test]
    fn a_link_after_a_quote_mark_of_more_than_a_byte() {
        for (line, url) in [("“https://example.com”", "https://example.com"), ("→https://a.io", "https://a.io"), ("see (https://b.dev).", "https://b.dev")] {
            let cells: Vec<char> = line.chars().collect();
            let col = cells.iter().position(|c| *c == ':').unwrap();
            let (from, _, target) = link_at(&cells, col, std::path::Path::new("/")).unwrap();
            assert!(target == Target::Url(url.to_string()), "{line}");
            assert_eq!(cells[from], 'h', "{line}");
        }
    }
}
