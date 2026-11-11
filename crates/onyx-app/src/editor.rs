//! The live-preview editor: a note's markdown drawn as it reads, with the
//! marks of whatever the caret is in shown as written, as Obsidian's
//! editor does. One element draws every line itself (`EditorElement`):
//! the text shaped by gpui, the bullets, boxes, bars, plates and rules
//! as quads, and the caret and selection over them. The toolkit's input
//! was not used: it lays every line out in one face at one size, and a
//! heading is larger than the words under it.
//!
//! The text is `lines::Doc`; what each line is comes from
//! `markdown::analyze`, read again whole after every change. A line is
//! laid out when its text, its hidden marks or the width change
//! (`Cache`), so a keystroke lays out the lines it touched and not the
//! note. Typing arrives through gpui's input handler, so an input method
//! composes in place; keys that move or change arrive as actions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::*;
use gpui_component::{input, ActiveTheme as _};

use crate::layout::{Cache, Pictures, Reader, Rhythm, Row, Target, TextRow, Widget};
use crate::lines::{list_line, next_marker, quote_line, Doc, ListKind, Pos, Sel};
use crate::markdown::{self, Analysis, LineKind, SpanKind};

mod cards;
mod cells;
mod pages;
mod picture;
mod writing;

pub use cards::{Layout, LayoutAt};
pub use cells::{CellAt, Scope, TableFacts};
pub use picture::{Seen, Viewing};
pub use writing::{MenuIssue, Words};

actions!(
    editor,
    [Enter, NewLine, Tab, Outdent, Backspace, Delete, DeleteWordBack, DeleteLineBack, Left, Right, Up, Down, SelectLeft, SelectRight, SelectUp, SelectDown, WordLeft, WordRight, SelectWordLeft, SelectWordRight, LineStart, LineEnd, SelectLineStart, SelectLineEnd, DocStart, DocEnd, SelectDocStart, SelectDocEnd, PageUp, PageDown, Bold, Italic, ToggleTask]
);

/// The key context of a note being written in.
pub const EDITOR_CONTEXT: &str = "Editor";

/// Each key as the Mac has it, bound as `secondary-<key>` where ⌘ is
/// meant (Ctrl off a Mac). Copy, Cut, Paste, Select All, Undo and Redo
/// are the toolkit's own actions, so the Edit menu serves a field and
/// the editor alike.
pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(EDITOR_CONTEXT);
    vec![
        KeyBinding::new("enter", Enter, c),
        KeyBinding::new("shift-enter", NewLine, c),
        KeyBinding::new("tab", Tab, c),
        KeyBinding::new("shift-tab", Outdent, c),
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("shift-backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("alt-backspace", DeleteWordBack, c),
        KeyBinding::new("secondary-backspace", DeleteLineBack, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("secondary-left", LineStart, c),
        KeyBinding::new("secondary-right", LineEnd, c),
        KeyBinding::new("home", LineStart, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("secondary-shift-left", SelectLineStart, c),
        KeyBinding::new("secondary-shift-right", SelectLineEnd, c),
        KeyBinding::new("shift-home", SelectLineStart, c),
        KeyBinding::new("shift-end", SelectLineEnd, c),
        KeyBinding::new("secondary-up", DocStart, c),
        KeyBinding::new("secondary-down", DocEnd, c),
        KeyBinding::new("secondary-shift-up", SelectDocStart, c),
        KeyBinding::new("secondary-shift-down", SelectDocEnd, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("secondary-b", Bold, c),
        KeyBinding::new("secondary-i", Italic, c),
        KeyBinding::new("secondary-enter", ToggleTask, c),
        KeyBinding::new("secondary-a", input::SelectAll, c),
        KeyBinding::new("secondary-c", input::Copy, c),
        KeyBinding::new("secondary-x", input::Cut, c),
        KeyBinding::new("secondary-v", input::Paste, c),
        KeyBinding::new("secondary-z", input::Undo, c),
        KeyBinding::new("secondary-shift-z", input::Redo, c),
    ]
}

/// How a note is shown: written in, or read only, as an asset's tab and
/// a note in a frame are.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// As it reads, with the marks of what the caret is in shown.
    #[default]
    Live,
    /// As it reads only, with nothing to type in.
    Reading,
}

/// A link a right click was on: what it goes to, its words, and the
/// whole of it as written.
#[derive(Clone, Debug)]
pub struct MenuLink {
    pub url: Option<String>,
    pub note: Option<String>,
    pub words: String,
    pub raw: String,
}

/// What a right click was on, as the window needs it to make the menu.
#[derive(Clone, Debug, Default)]
pub struct MenuAt {
    pub link: Option<MenuLink>,
    pub issue: Option<MenuIssue>,
    pub table: Option<(CellAt, Scope, TableFacts)>,
    /// On a line with nothing on it, where a table can stand.
    pub blank: bool,
    /// A file of the vault other than a note: a picture drawn there, or
    /// what a link leads to.
    pub file: Option<PathBuf>,
    /// A picture drawn there: its line, where it stands, and the width it
    /// asks for, for the menu's place and size.
    pub picture: Option<(usize, onyx_core::html::Across, Option<onyx_core::html::Width>)>,
    /// In a file's frame (`embed.rs`): the menu acts on what the frame
    /// holds, not the note.
    pub frame: Option<FrameMenu>,
    /// A picture's or a frame's line, which Delete takes out of the note.
    pub remove: Option<usize>,
    /// A link that can show another way: among words, as a card, whole.
    pub layout: Option<cards::LayoutAt>,
}

/// A right click in a frame: where its keyboard goes, which takes the
/// menu's choice, and whether something is selected there.
#[derive(Clone, Debug)]
pub struct FrameMenu {
    pub focus: FocusHandle,
    pub selection: bool,
}

/// A file of the vault's assets on its way from the sidebar: dropped on
/// the page, the note links to it there.
#[derive(Clone)]
pub struct AssetDrag(pub PathBuf, pub SharedString);

impl Render for AssetDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px(px(10.))
            .h(px(28.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow_md()
            .text_size(px(12.5))
            .text_color(theme.foreground)
            .child(self.1.clone())
    }
}

/// A picture's corners on the canvas, as round as Craft's, and a frame's.
/// A line's own color with nothing of it showing, the fill under a line
/// drawn alone: gpui blends a border into its fill in plain color as it
/// nears the fill, so a fill of clear black darkens the soft inside of a
/// round corner into a second, darker line.
pub(crate) fn clear_of(line: Hsla) -> Hsla {
    line.opacity(0.)
}

pub const PICTURE_ROUND: f32 = crate::embed::CONTROL_ROUND;

/// What the editor tells the view around it.
pub enum EditorEvent {
    /// The text changed.
    Changed,
    /// A link was clicked: an address, or a path relative to the note.
    OpenLink(String),
    /// A `[[note]]` link was clicked: the name it gives.
    OpenNote(String),
    /// A right click: where, and what it was on. The window makes the
    /// menu, as it does every menu.
    Menu(Point<Pixels>, MenuAt),
    /// A picture opened in the viewer, over the window.
    View(picture::Viewing),
    /// The find's places changed with nothing typed: a PDF's words read.
    Found,
    /// A file a bar renamed in place: the file, and its new name.
    Rename(PathBuf, String),
}

/// The faces and size a note is set in, from the settings.
#[derive(Clone, PartialEq)]
pub struct Look {
    pub text_family: SharedString,
    pub mono_family: SharedString,
    pub text_px: f32,
    /// Code's size: a block, a span, a table, a note as written.
    pub mono_px: f32,
    /// The faces behind each, for the characters it lacks (`fonts::cjk_fallbacks`).
    pub text_fallbacks: Option<FontFallbacks>,
    pub mono_fallbacks: Option<FontFallbacks>,
}

impl Default for Look {
    fn default() -> Self {
        Self { text_family: "Inter".into(), mono_family: "JetBrains Mono".into(), text_px: 16., mono_px: 14., text_fallbacks: None, mono_fallbacks: None }
    }
}

/// A table's corners, as a code plate's.
const TABLE_R: f32 = 8.;
const BLINK: Duration = Duration::from_millis(530);
/// The caret is kept this far from the top and foot of what shows: out
/// of the page's edge fade, which counts as out of sight (`FADE_H` in
/// the shell). The two change together.
const SCROLL_MARGIN: f32 = 28.;

/// `ONYX_TRACE=1` prints what each layout and paint costs, for a probe.
fn tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ONYX_TRACE").is_some())
}

fn trace(what: &str, since: Instant) {
    if tracing() {
        eprintln!("trace {what} {:.2}ms", since.elapsed().as_secs_f64() * 1000.);
    }
}

/// A stamp on the app's own clock, for the gap between an event and the paint after it.
pub(crate) fn stamp(what: &str) {
    if tracing() {
        static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let t = EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.;
        eprintln!("stamp {t:.1} {what}");
    }
}

pub struct Editor {
    doc: Doc,
    sel: Sel,
    analysis: Analysis,
    mode: Mode,
    look: Look,
    focus: FocusHandle,
    /// Counts the text's readings (`reanalyze`), for `version`.
    version: u64,
    /// The scroller the view around the editor puts it in.
    pub scroll: ScrollHandle,
    cache: Rc<RefCell<Cache>>,
    /// An input method's composition: what it has put in so far.
    marked: Option<(Pos, Pos)>,
    blink_at: Instant,
    blinking: bool,
    focused: bool,
    /// A drag under way: how the press began (a caret, a word, a line),
    /// and the range that press selected.
    drag: Option<(Grain, Pos, Pos)>,
    /// A link pressed as the note reads, followed at the release unless
    /// the press became a selection, so a selection can begin on a link.
    pressed_link: Option<Target>,
    want_scroll: bool,
    /// The column ↑ and ↓ keep to, in pixels from the words' left.
    preferred_x: Option<f32>,
    /// Where the element was last painted, for an input method's windows.
    origin: Point<Pixels>,
    /// The pointer's shape over the page, and where it last was.
    pointer: CursorStyle,
    pointer_at: (f32, f32),
    read_only: bool,
    /// The phrase the find bar looks for, every place it is in the note,
    /// and which of them the bar stands on. The one stood on is the
    /// selection; the rest are washed.
    find_query: String,
    find: Rc<Vec<(Pos, Pos)>>,
    find_at: usize,
    /// The places on the note's PDFs (`pages.rs`), and every place of
    /// both in the note's order: a place in the text, or one on a page.
    find_pages: Rc<Vec<pages::PageHit>>,
    find_order: Vec<(bool, usize)>,
    /// A place on a line to bring into view in place of the caret: its
    /// line, and its top and height in the line's row.
    want_spot: Option<(usize, f32, f32)>,
    /// The pictures a note holds, and where the note is, to find them.
    pictures: Rc<RefCell<Pictures>>,
    writing: writing::Writing,
    /// Where the pointer is in a table, and a drag of its edges or handles.
    hover: Option<cells::Hover>,
    table_drag: Option<cells::TableDrag>,
    /// A table's columns as a drag has them: its first line, and each width.
    hold: Option<(usize, Vec<f32>)>,
    /// The picture the pointer is over, a drag of its side, and its width
    /// as the drag has it (`picture.rs`).
    pic_hover: Option<picture::PicHover>,
    pic_drag: Option<picture::PicDrag>,
    pic_hold: Option<(usize, f32)>,
    /// The line of a picture or a frame the bar's pencil opened, the
    /// caret in it rather than on the whole (`picture.rs`).
    editing: Option<usize>,
    /// The frames of the files the note embeds, by file and which of its
    /// frames (`pages.rs`, `embed.rs`).
    embeds: HashMap<(PathBuf, usize), Entity<crate::embed::Embed>>,
    /// The note's cards (`card.rs`), by their link and which of its they are.
    cards: HashMap<(String, usize), Entity<crate::card::Card>>,
    /// A large card's bar, by its card.
    card_bars: HashMap<(String, usize), Entity<crate::embed::Embed>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Grain {
    Char,
    Word,
    Line,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A caret's place on the page, from the element's top left.
#[derive(Clone, Copy)]
struct CaretAt {
    x: f32,
    /// The line it stands on: what ↑ and ↓ step from and the view keeps in sight.
    y: f32,
    h: f32,
    /// The caret itself, in the middle of that line.
    bar_y: f32,
    bar_h: f32,
}

impl CaretAt {
    fn new(x: f32, y: f32, h: f32, bar_h: f32) -> Self {
        Self { x, y, h, bar_y: y + ((h - bar_h) / 2.).floor(), bar_h }
    }
}

// -- the editor -------------------------------------------------------------

impl Editor {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            doc: Doc::default(),
            sel: Sel::default(),
            analysis: markdown::analyze(""),
            mode: Mode::Live,
            look: Look::default(),
            focus: cx.focus_handle(),
            version: 0,
            scroll: ScrollHandle::new(),
            cache: Rc::new(RefCell::new(Cache::default())),
            marked: None,
            blink_at: Instant::now(),
            blinking: false,
            focused: false,
            drag: None,
            pressed_link: None,
            want_scroll: false,
            preferred_x: None,
            origin: Point::default(),
            pointer: CursorStyle::IBeam,
            pointer_at: (0., 0.),
            read_only: false,
            find_query: String::new(),
            find: Rc::new(Vec::new()),
            find_pages: Rc::new(Vec::new()),
            find_order: Vec::new(),
            want_spot: None,
            find_at: 0,
            pictures: Rc::default(),
            writing: writing::Writing::default(),
            hover: None,
            table_drag: None,
            hold: None,
            pic_hover: None,
            pic_drag: None,
            pic_hold: None,
            editing: None,
            embeds: HashMap::new(),
            cards: HashMap::new(),
            card_bars: HashMap::new(),
        }
    }

    /// Where the note is, which a picture's path goes by.
    /// What the note knows of the files it shows, shared with its frames;
    /// a tab's frame takes it too (`shell/library.rs`).
    pub(crate) fn store(&self) -> Rc<RefCell<Pictures>> {
        self.pictures.clone()
    }

    pub fn set_place(&mut self, note: PathBuf, vault: Option<PathBuf>, cx: &mut Context<Self>) {
        let mut pictures = self.pictures.borrow_mut();
        if pictures.note != note || pictures.vault != vault {
            pictures.note = note;
            pictures.vault = vault;
            pictures.found.clear();
            pictures.found_any.clear();
            cx.notify();
        }
    }

    /// What a layout goes by, as things stand.
    fn reader(&self) -> Reader<'_> {
        let revealing = self.revealing();
        Reader {
            doc: &self.doc,
            analysis: &self.analysis,
            look: &self.look,
            rhythm: Rhythm::of(&self.look),
            mode: self.mode,
            sel: revealing.then(|| self.sel.range()),
            issues: if self.locked() { &[] } else { self.writing.shown(&self.doc) },
            quiet: self.writing.quiet(&self.doc, self.sel),
            pictures: &self.pictures,
            hold: self.hold.as_ref(),
            across: Default::default(),
            nested: false,
            resize: self.pic_hold,
            editing: self.editing,
        }
    }

    /// Another note's text: the history, the selection and the scroll start over.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.doc = Doc::from_text(text);
        self.sel = Sel::default();
        self.marked = None;
        self.drag = None;
        self.preferred_x = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.reanalyze();
        self.check_afresh(cx);
        cx.notify();
    }

    /// The text as the disk has it, when nothing was typed: the caret and
    /// the history are kept where the text still has them.
    pub fn replace_text_quietly(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.doc.text() == text {
            return;
        }
        let keep = self.sel;
        self.doc = Doc::from_text(text);
        self.sel = Sel { anchor: self.doc.clamp(keep.anchor), head: self.doc.clamp(keep.head) };
        self.marked = None;
        self.reanalyze();
        self.check_afresh(cx);
        cx.notify();
    }

    pub fn text(&self) -> String {
        self.doc.text()
    }


    /// A change of how the note is shown. The keyboard stays where it
    /// is: a caret placed stays placed, and a note left by a click in the
    /// margin keeps reading whole, whichever mode follows.
    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.mode != mode {
            let was_reading = self.mode == Mode::Reading;
            self.mode = mode;
            self.marked = None;
            self.editing = None;
            self.hold_objects();
            self.want_scroll = true;
            if was_reading && self.focused {
                self.kick_blink(cx);
            }
            cx.notify();
        }
    }

    // -- find ---------------------------------------------------------------

    /// Every place the phrase is in the note, and the bar on the first
    /// at or after the caret, which becomes the selection. The count.
    pub fn set_find(&mut self, query: &str, cx: &mut Context<Self>) -> usize {
        self.find_query = query.trim().to_string();
        let was = self.sel.range().0;
        self.refind();
        self.find_at = (0..self.find_order.len()).position(|i| self.found_at(i) >= was).unwrap_or(0);
        if self.find_order.is_empty() {
            cx.notify();
        } else {
            self.find_go(cx);
        }
        self.find_order.len()
    }

    /// Where the find's `i`th place stands in the note: a place on a
    /// PDF's page after any in its line's words.
    fn found_at(&self, i: usize) -> Pos {
        match self.find_order.get(i) {
            Some(&(false, k)) => self.find[k].0,
            Some(&(true, k)) => Pos::new(self.find_pages[k].line, usize::MAX),
            None => Pos::new(usize::MAX, 0),
        }
    }

    pub fn clear_find(&mut self, cx: &mut Context<Self>) {
        self.find_query.clear();
        self.find = Rc::new(Vec::new());
        self.find_pages = Rc::new(Vec::new());
        self.find_order.clear();
        self.find_at = 0;
        cx.notify();
    }

    /// The matches found again, as the text is now.
    pub(super) fn refind(&mut self) {
        let found: Vec<(Pos, Pos)> = if self.find_query.is_empty() {
            Vec::new()
        } else {
            let text = self.doc.text();
            onyx_core::search::find_all(&text, &self.find_query).into_iter().map(|r| (self.doc.pos_of(r.start), self.doc.pos_of(r.end))).collect()
        };
        let pages = if self.find_query.is_empty() { Vec::new() } else { self.find_on_pages(&self.find_query) };
        // Both in the note's order: a page's places after its line's words.
        let mut order: Vec<(bool, usize)> = (0..found.len()).map(|k| (false, k)).chain((0..pages.len()).map(|k| (true, k))).collect();
        let key = |&(page, k): &(bool, usize)| if page { (pages[k].line, usize::MAX, pages[k].page, pages[k].from) } else { (found[k].0.line, found[k].0.col, 0, 0) };
        order.sort_by_key(key);
        self.find = Rc::new(found);
        self.find_pages = Rc::new(pages);
        self.find_order = order;
        if self.find_at >= self.find_order.len() {
            self.find_at = 0;
        }
    }

    pub(super) fn find_go(&mut self, cx: &mut Context<Self>) {
        let Some(&(page, k)) = self.find_order.get(self.find_at) else { return };
        if page {
            let hit = self.find_pages[k].clone();
            self.find_go_page(&hit, cx);
            return;
        }
        let (a, b) = self.find[k];
        self.sel = Sel { anchor: a, head: b };
        self.preferred_x = None;
        self.want_scroll = true;
        self.kick_blink(cx);
        cx.notify();
    }

    /// The next match, or the one before, round the ends.
    pub fn find_step(&mut self, delta: i64, cx: &mut Context<Self>) {
        let n = self.find_order.len() as i64;
        if n == 0 {
            return;
        }
        self.find_at = (self.find_at as i64 + delta).rem_euclid(n) as usize;
        self.find_go(cx);
    }

    pub fn find_count(&self) -> usize {
        self.find_order.len()
    }

    pub fn find_at(&self) -> usize {
        self.find_at
    }

    /// Whether the note takes no change: a note that is read only, or
    /// one shown as it reads. Nothing else about it is less: it selects,
    /// copies and finds as any note does, so the lock is on what changes
    /// the text, each change checking it, and never on the pointer or
    /// the keyboard. A task's box is the one thing ticked through it
    /// (`toggle_task_on`).
    /// The note is read, not written: a press on a card opens it.
    pub(crate) fn reads(&self) -> bool {
        self.mode == Mode::Reading
    }

    pub(crate) fn locked(&self) -> bool {
        self.read_only || self.mode == Mode::Reading
    }

    /// Whether the marks around the caret are shown: only while the
    /// keyboard is in the editor, and never as the note reads, which is
    /// Edit's own drawing with every mark away and nothing to type in.
    fn revealing(&self) -> bool {
        self.focused && self.mode != Mode::Reading
    }

    pub fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        if self.look != look {
            for frame in self.embeds.values() {
                frame.update(cx, |m, cx| m.set_look(look.clone(), cx));
            }
            self.look = look;
            cx.notify();
        }
    }

    /// For a probe: words typed as the input method would type them.
    pub fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        for ch in text.chars() {
            let (a, b) = self.sel.range();
            let typed = self.cell_words(&ch.to_string());
            let end = self.doc.replace(a, b, &typed, self.sel);
            self.sel = Sel::caret(end);
            self.after_edit(cx);
        }
    }

    /// For a probe: where each line stands.
    pub fn line_tops(&self) -> Vec<f32> {
        self.cache.borrow().tops.clone()
    }

    /// For a probe: the caret at the end, and the view with it.
    pub fn move_to_end(&mut self, cx: &mut Context<Self>) {
        let end = self.doc.end();
        self.move_to(end, false, cx);
    }

    pub fn has_selection(&self) -> bool {
        let (a, b) = self.sel.range();
        a != b
    }

    /// The link at a point of the page, as its menu needs it.
    fn link_at(&self, x: f32, y: f32) -> Option<MenuLink> {
        let Some(Hit::Link(target)) = self.hit(x, y) else { return None };
        let p = self.pos_at(x, y, &self.cache.borrow())?;
        let at = self.doc.offset_of(p);
        let (url, note) = match target {
            Target::Link(url) => (Some(url), None),
            Target::Note(name) => (None, Some(name)),
        };
        let span = self.analysis.line_spans[p.line].iter().map(|&ix| &self.analysis.spans[ix]).find(|s| matches!(s.kind, SpanKind::Link(_) | SpanKind::WikiLink(_) | SpanKind::Image(_)) && s.range.start <= at && at <= s.range.end);
        let slice = |a: usize, b: usize| self.doc.slice(self.doc.pos_of(a), self.doc.pos_of(b));
        let (words, raw) = span.map(|s| (slice(s.open.end, s.close.start), slice(s.range.start, s.range.end))).unwrap_or_default();
        Some(MenuLink { url, note, words, raw })
    }

    /// A right click off a link: a press inside the selection leaves it,
    /// one on a word selects the word, as a Mac's text does, and one on
    /// nothing puts the caret there, for a paste.
    fn select_for_menu(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let Some(p) = self.pos_at(x, y, &self.cache.borrow()) else { return };
        let (a, b) = self.sel.range();
        if a != b && a <= p && p <= b {
            return;
        }
        let (from, to) = self.grain_range(p, Grain::Word);
        self.sel = if from != to && !self.doc.slice(from, to).trim().is_empty() { Sel { anchor: from, head: to } } else { Sel::caret(p) };
        self.preferred_x = None;
        self.kick_blink(cx);
    }

    fn follow(&mut self, target: Target, cx: &mut Context<Self>) {
        match target {
            Target::Link(url) => cx.emit(EditorEvent::OpenLink(url)),
            Target::Note(name) => cx.emit(EditorEvent::OpenNote(name)),
        }
    }

    /// For a script: the caret to the start of a line, as a key moves it.
    pub fn move_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        self.move_to(Pos { line, col: 0 }, false, cx);
    }

    /// For a probe: the selection's anchor and head, line and byte.
    pub fn selection_ends(&self) -> ((usize, usize), (usize, usize)) {
        ((self.sel.anchor.line, self.sel.anchor.col), (self.sel.head.line, self.sel.head.col))
    }

    pub fn selection_text(&self) -> String {
        let (a, b) = self.sel.range();
        self.doc.slice(a, b)
    }

    /// What ⌘C takes: the file's own text, or as the note reads the words
    /// that show and nothing that does not.
    pub fn copied_text(&self) -> String {
        if self.mode == Mode::Reading {
            self.shown_text()
        } else {
            self.selection_text()
        }
    }

    /// The selection as it reads: each line by the rule that hides its
    /// marks in the drawing (`hidden_on`), with the lines drawn as no
    /// words (a fence, a rule, a heading's underline, a table's bar row)
    /// left out, and a table's rows as their cells, a tab between them,
    /// whole, since a table is selected a line at a time.
    fn shown_text(&self) -> String {
        let (a, b) = self.sel.range();
        if a == b {
            return String::new();
        }
        let mut lines = Vec::new();
        for line in a.line..=b.line {
            let text = self.doc.line(line);
            let from = if line == a.line { a.col } else { 0 };
            let to = if line == b.line { b.col } else { text.len() };
            match &self.analysis.lines[line].kind {
                LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::SetextUnderline | LineKind::Rule => {}
                LineKind::TableFirst(t) | LineKind::TableRow(t) if a.line != b.line || self.cell_of(a).is_none() => {
                    let table = &self.analysis.tables[*t];
                    if let Some(cells) = table.row_of(line).and_then(|r| table.rows.get(r)) {
                        lines.push(cells.join("\t"));
                    }
                }
                _ => lines.push(markdown::visible_part(text, from, to, &self.hidden_on(line))),
            }
        }
        lines.join("\n")
    }

    fn reanalyze(&mut self) {
        self.analysis = markdown::analyze(&self.doc.text());
        self.version += 1;
    }

    /// A count that moves whenever the text may have changed, by any way,
    /// for a view of the same text to follow it (`source.rs`).
    pub(crate) fn version(&self) -> u64 {
        self.version
    }

    /// The byte of the text the caret is at.
    pub(crate) fn caret_byte(&self) -> usize {
        self.doc.offset_of(self.sel.head)
    }

    /// The caret put at a byte of the text, the keyboard here and the view
    /// brought to it: from the source pane as it goes (`source.rs`).
    pub(crate) fn put_caret_at(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.sel = Sel::caret(self.doc.pos_of(at.min(self.doc.text().len())));
        self.editing = None;
        self.hold_objects();
        self.want_scroll = true;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The spelling and grammar marks as the note shows them, in bytes of
    /// its text.
    pub(crate) fn shown_issues(&self) -> &[onyx_core::check::Issue] {
        if self.locked() { &[] } else { self.writing.shown(&self.doc) }
    }

    /// After every change: the text read again, numbered lists put
    /// right, the caret shown, the view around told.
    fn after_edit(&mut self, cx: &mut Context<Self>) {
        stamp("after_edit");
        let started = Instant::now();
        self.reanalyze();
        self.renumber();
        trace("after_edit(analyze+renumber)", started);
        if !self.find_query.is_empty() {
            self.refind();
        }
        self.text_changed(cx);
        // A caret a change leaves on a picture's line (a picture just
        // typed) stays in the line, which shows as written; a paste
        // selects what it brought whole after (`hold_brought`).
        if self.editing.is_some_and(|l| l != self.sel.head.line) {
            self.editing = None;
        }
        if self.sel.is_caret() && self.object_line(self.sel.head.line) {
            self.editing = Some(self.sel.head.line);
        }
        self.want_scroll = true;
        self.preferred_x = None;
        self.kick_blink(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn after_move(&mut self, cx: &mut Context<Self>) {
        self.hold_objects();
        self.want_scroll = true;
        self.kick_blink(cx);
        cx.notify();
    }

    /// Numbered lists count from their first item, as Obsidian keeps
    /// them, whatever was typed; one step of undo with the change before it.
    fn renumber(&mut self) {
        let mut changes = Vec::new();
        for list in self.analysis.lists.iter().filter(|l| l.ordered) {
            let mut expected: Option<u64> = None;
            for &line in &list.items {
                let Some(mark) = self.analysis.lines[line].list.as_ref() else { continue };
                let ListKind::Ordered(n) = mark.kind else { continue };
                let want = expected.unwrap_or(n);
                if n != want {
                    let text = self.doc.line(line);
                    let digits = text[mark.marker.start..].chars().take_while(|c| c.is_ascii_digit()).count();
                    changes.push((Pos::new(line, mark.marker.start), Pos::new(line, mark.marker.start + digits), want.to_string()));
                }
                expected = Some(want + 1);
            }
        }
        if changes.is_empty() {
            return;
        }
        // The caret stays with its line, moved by what its own number grew.
        let mut sel = self.sel;
        for (from, to, text) in &changes {
            for p in [&mut sel.anchor, &mut sel.head] {
                if p.line == from.line && p.col >= to.col {
                    p.col = p.col + text.len() - (to.col - from.col);
                }
            }
        }
        let before = self.sel;
        self.doc.replace_many(changes, before, sel);
        self.sel = sel;
        self.reanalyze();
    }

    fn kick_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_at = Instant::now();
        if self.blinking {
            return;
        }
        self.blinking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let go_on = this
                    .update(cx, |e, cx| {
                        cx.notify();
                        e.focused && e.mode != Mode::Reading
                    })
                    .unwrap_or(false);
                if !go_on {
                    let _ = this.update(cx, |e, _| e.blinking = false);
                    break;
                }
            }
        })
        .detach();
    }

    fn caret_shown(&self) -> bool {
        self.revealing() && ((self.blink_at.elapsed().as_millis() / BLINK.as_millis()) % 2 == 0)
    }

    // -- edits --------------------------------------------------------------

    /// `text` in place of the selection.
    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let (a, b) = self.sel.range();
        let text = self.cell_words(text);
        let end = self.doc.replace(a, b, &text, self.sel);
        self.sel = Sel::caret(end);
        self.after_edit(cx);
    }

    fn replace_range(&mut self, a: Pos, b: Pos, text: &str, cx: &mut Context<Self>) -> Pos {
        let end = self.doc.replace(a, b, text, self.sel);
        self.sel = Sel::caret(end);
        self.after_edit(cx);
        end
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if self.in_cell() {
            self.cell_down(cx);
            return;
        }
        if self.object_enter(cx) {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("\n", cx);
            return;
        }
        let at = self.sel.head;
        let text = self.doc.line(at.line).to_string();
        let (quote_depth, body_start) = quote_line(&text).unwrap_or((0, 0));
        let quote_prefix = "> ".repeat(quote_depth);
        if let Some(item) = list_line(&text[body_start..]) {
            let marker_end = body_start + item.indent.len() + item.marker.len();
            if at.col >= marker_end {
                if item.body.trim().is_empty() && at.col == text.len() {
                    // An empty item ends the list: its marker goes.
                    self.replace_range(Pos::new(at.line, 0), Pos::new(at.line, text.len()), &quote_prefix, cx);
                } else {
                    let line = format!("\n{quote_prefix}{}{}", item.indent, next_marker(&item));
                    self.insert(&line, cx);
                }
                self.doc.end_step();
                return;
            }
        } else if quote_depth > 0 && at.col >= body_start {
            if text[body_start..].trim().is_empty() && at.col == text.len() {
                self.replace_range(Pos::new(at.line, 0), Pos::new(at.line, text.len()), "", cx);
            } else {
                self.insert(&format!("\n{quote_prefix}"), cx);
            }
            self.doc.end_step();
            return;
        }
        self.insert("\n", cx);
        self.doc.end_step();
    }

    fn new_line(&mut self, _: &NewLine, _: &mut Window, cx: &mut Context<Self>) {
        self.insert("\n", cx);
        self.doc.end_step();
    }

    /// The lines the selection touches.
    fn selected_lines(&self) -> Range<usize> {
        let (a, b) = self.sel.range();
        a.line..b.line + 1
    }

    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if self.in_cell() {
            self.cell_next(cx);
            return;
        }
        let lines: Vec<usize> = self.selected_lines().filter(|l| list_line(self.doc.line(*l)).is_some()).collect();
        if lines.is_empty() {
            self.insert("\t", cx);
            return;
        }
        let before = self.sel;
        let mut sel = self.sel;
        for p in [&mut sel.anchor, &mut sel.head] {
            if lines.contains(&p.line) {
                p.col += 1;
            }
        }
        let changes = lines.iter().map(|&l| (Pos::new(l, 0), Pos::new(l, 0), "\t".to_string())).collect();
        self.doc.replace_many(changes, before, sel);
        self.sel = sel;
        self.after_edit(cx);
    }

    fn outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if self.in_cell() {
            self.cell_prev(cx);
            return;
        }
        let mut changes = Vec::new();
        let mut sel = self.sel;
        for l in self.selected_lines() {
            let text = self.doc.line(l);
            if list_line(text).is_none() {
                continue;
            }
            let cut = if text.starts_with('\t') { 1 } else { text.len() - text.trim_start_matches(' ').len().max(text.len().saturating_sub(4)).min(text.len()) };
            let cut = if text.starts_with('\t') { cut } else { (text.len() - text.trim_start_matches(' ').len()).min(4) };
            if cut == 0 {
                continue;
            }
            changes.push((Pos::new(l, 0), Pos::new(l, cut), String::new()));
            for p in [&mut sel.anchor, &mut sel.head] {
                if p.line == l {
                    p.col = p.col.saturating_sub(cut);
                }
            }
        }
        if changes.is_empty() {
            return;
        }
        let before = self.sel;
        self.doc.replace_many(changes, before, sel);
        self.sel = sel;
        self.after_edit(cx);
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if self.object_back(cx) {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            return;
        }
        let at = self.sel.head;
        if let Some(from) = self.back_stop(at) {
            if from != at {
                self.replace_range(from, at, "", cx);
            }
            return;
        }
        let text = self.doc.line(at.line);
        // Right after a list marker with nothing typed yet: the marker goes.
        if let Some(item) = list_line(text) {
            let marker_end = item.indent.len() + item.marker.len();
            if at.col == marker_end && item.body.is_empty() {
                self.replace_range(Pos::new(at.line, item.indent.len()), Pos::new(at.line, marker_end), "", cx);
                self.doc.end_step();
                return;
            }
        }
        let from = self.doc.prev_grapheme(at);
        if from != at {
            self.replace_range(from, at, "", cx);
        }
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if self.object_forward(cx) {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            return;
        }
        let at = self.sel.head;
        let to = self.forward_stop(at).unwrap_or_else(|| self.doc.next_grapheme(at));
        if to != at {
            self.replace_range(at, to, "", cx);
        }
    }

    fn delete_word_back(&mut self, _: &DeleteWordBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            return;
        }
        let at = self.sel.head;
        let from = self.in_this_cell(at, self.doc.word_left(at));
        if from != at {
            self.replace_range(from, at, "", cx);
        }
        self.doc.end_step();
    }

    fn delete_line_back(&mut self, _: &DeleteLineBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let at = self.sel.head;
        let from = if at.col > 0 { Pos::new(at.line, 0) } else { self.doc.prev_grapheme(at) };
        let from = self.in_this_cell(at, from);
        if from != at {
            self.replace_range(from, at, "", cx);
            self.doc.end_step();
        }
    }

    /// ⌘A takes a cell first, its words being a text of their own; again,
    /// the note.
    fn select_all(&mut self, _: &input::SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let cell = self.cell_of(self.sel.head).map(|at| self.cell_span(at));
        self.sel = match cell {
            Some((a, b)) if self.sel.range() != (a, b) => Sel { anchor: a, head: b },
            _ => Sel { anchor: Pos::default(), head: self.doc.end() },
        };
        cx.notify();
    }

    fn copy(&mut self, _: &input::Copy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.copied_text();
        if !text.is_empty() {
            self.to_clipboard(text, cx);
        }
    }

    /// What is copied, on the clipboard: a picture selected whole goes as
    /// its line, which a paste here takes as the same picture, and as the
    /// picture itself for any other app.
    fn to_clipboard(&self, text: String, cx: &mut Context<Self>) {
        let picture = self.object_selected().and_then(|line| self.picture_at_line(line));
        // A probe's copy leaves the clipboard alone and says it.
        if let Some(path) = picture.as_ref().filter(|_| std::env::var_os("ONYX_GO").is_some()) {
            eprintln!("copy {text:?} with {}", path.display());
            return;
        }
        if !picture.is_some_and(|path| crate::sys::copy_with_picture(&text, &path)) {
            crate::widgets::to_clipboard(text, cx);
        }
    }

    fn cut(&mut self, _: &input::Cut, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.copied_text();
        if !text.is_empty() {
            self.to_clipboard(text, cx);
            if !self.locked() {
                self.insert("", cx);
                self.doc.end_step();
            }
        }
    }

    fn paste(&mut self, _: &input::Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else { return };
        // Files copied in the Finder, or a picture copied alone, come in
        // as assets; anything else as its text.
        match item.entries().first() {
            Some(ClipboardEntry::ExternalPaths(paths)) => return self.attach(paths.paths().to_vec(), None, cx),
            Some(ClipboardEntry::Image(picture)) => return self.attach(Vec::new(), Some(picture.clone()), cx),
            _ => {}
        }
        if let Some(text) = item.text() {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            let open = self.editing;
            self.insert(&text, cx);
            self.doc.end_step();
            if open.is_none() {
                self.hold_brought();
            }
        }
    }

    /// Changes made from outside, at byte ranges of the text (a file's
    /// links following its rename, `vault::relink`): one step of undo,
    /// the caret kept where it was, the view not moved.
    pub fn apply_edits(&mut self, edits: Vec<(std::ops::Range<usize>, String)>, cx: &mut Context<Self>) {
        if edits.is_empty() {
            return;
        }
        let changes = edits.into_iter().map(|(r, t)| (self.doc.pos_of(r.start), self.doc.pos_of(r.end), t)).collect();
        let before = self.sel;
        self.doc.replace_many(changes, before, before);
        self.doc.end_step();
        self.sel = Sel { anchor: self.doc.clamp(before.anchor), head: self.doc.clamp(before.head) };
        self.reanalyze();
        self.text_changed(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    /// For a probe: files brought in as a paste of them would.
    pub fn attach_files(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.attach(files, None, cx);
    }

    /// Files dropped on the page: the caret goes where they fell, and they
    /// come in there as assets.
    fn dropped(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        self.drop_files(paths.paths().to_vec(), window, cx);
    }

    /// An asset dragged from the sidebar: its link where it fell.
    fn dropped_asset(&mut self, asset: &AssetDrag, window: &mut Window, cx: &mut Context<Self>) {
        self.drop_files(vec![asset.0.clone()], window, cx);
    }

    fn drop_files(&mut self, files: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let at = window.mouse_position() - self.origin;
        if let Some(p) = self.pos_at(f32::from(at.x), f32::from(at.y), &self.cache.borrow()) {
            self.sel = Sel::caret(p);
        }
        window.focus(&self.focus, cx);
        self.attach(files, None, cx);
    }

    /// Files and a picture brought into the note: each kept in the vault's
    /// assets folder, off the window's thread (a file is copied, one
    /// already in the vault stays where it is), then referred to at the
    /// caret in standard markdown, each a block of its own between blank
    /// lines, so a picture is drawn here and in any other app.
    fn attach(&mut self, files: Vec<PathBuf>, picture: Option<Image>, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let (note, vault) = {
            let p = self.pictures.borrow();
            (p.note.clone(), p.vault.clone())
        };
        let Some(root) = vault.or_else(|| note.parent().map(std::path::Path::to_path_buf)) else { return };
                let kept = cx.background_spawn(async move {
            let mut kept: Vec<PathBuf> = files.iter().filter(|f| f.is_file()).filter_map(|f| onyx_core::vault::copy_asset(&root, f).ok()).collect();
            if let Some(picture) = picture {
                let name = format!("Pasted image {}.{}", chrono::Local::now().format("%Y%m%d%H%M%S"), picture.format.extension());
                kept.extend(onyx_core::vault::add_asset(&root, &name, &picture.bytes).ok());
            }
            kept
        });
        cx.spawn(async move |this, cx| {
            let kept = kept.await;
            let _ = this.update(cx, |e, cx| {
                if kept.is_empty() || e.pictures.borrow().note != note || e.locked() {
                    return;
                }
                let mut text = kept.iter().map(|f| onyx_core::vault::reference(&note, f)).collect::<Vec<_>>().join("\n\n");
                // A blank line between it and the words on either side,
                // whether they share its line or stand on the next.
                let (a, b) = e.sel.range();
                let filled = |line: usize| !e.doc.line(line).trim().is_empty();
                if !e.doc.line(a.line).get(..a.col).unwrap_or("").trim().is_empty() {
                    text.insert_str(0, "\n\n");
                } else if a.line > 0 && filled(a.line - 1) {
                    text.insert(0, '\n');
                }
                if !e.doc.line(b.line).get(b.col..).unwrap_or("").trim().is_empty() {
                    text.push_str("\n\n");
                } else if b.line + 1 < e.doc.line_count() && filled(b.line + 1) {
                    text.push('\n');
                }
                let open = e.editing;
                e.insert(&text, cx);
                e.doc.end_step();
                if open.is_none() {
                    e.hold_brought();
                }
            });
        })
        .detach();
    }

    fn undo(&mut self, _: &input::Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if let Some(sel) = self.doc.undo() {
            self.sel = Sel { anchor: self.doc.clamp(sel.anchor), head: self.doc.clamp(sel.head) };
            self.reanalyze();
            self.sel = self.settled_sel();
            self.text_changed(cx);
            self.want_scroll = true;
            self.kick_blink(cx);
            cx.emit(EditorEvent::Changed);
            cx.notify();
        }
    }

    fn redo(&mut self, _: &input::Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if let Some(sel) = self.doc.redo() {
            self.sel = Sel { anchor: self.doc.clamp(sel.anchor), head: self.doc.clamp(sel.head) };
            self.reanalyze();
            self.sel = self.settled_sel();
            self.text_changed(cx);
            self.want_scroll = true;
            self.kick_blink(cx);
            cx.emit(EditorEvent::Changed);
            cx.notify();
        }
    }

    /// ⌘B and ⌘I: a span of the kind around or under the caret is
    /// unwrapped; a selection is wrapped; a caret on a word wraps the
    /// word; a caret on nothing opens an empty pair to type into, and in
    /// such a pair closes it again.
    fn toggle_wrap(&mut self, marker: &str, kind: SpanKind, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let (a, b) = self.sel.range();
        let (ga, gb) = (self.doc.offset_of(a), self.doc.offset_of(b));
        let around = self.analysis.spans.iter().filter(|s| s.kind == kind && s.range.start <= ga && gb <= s.range.end).min_by_key(|s| s.range.len()).cloned();
        if let Some(s) = around {
            let (open, close) = (self.doc.pos_of(s.open.start), self.doc.pos_of(s.close.start));
            let (open_end, close_end) = (self.doc.pos_of(s.open.end), self.doc.pos_of(s.close.end));
            let before = self.sel;
            let shift = |p: Pos| -> Pos {
                let mut p = p;
                if p.line == close.line && p.col >= close_end.col {
                    p.col -= close_end.col - close.col;
                }
                if p.line == open.line && p.col >= open_end.col {
                    p.col -= open_end.col - open.col;
                } else if p.line == open.line && p.col > open.col {
                    p.col = open.col;
                }
                p
            };
            let after = Sel { anchor: shift(before.anchor), head: shift(before.head) };
            self.doc.replace_many(vec![(open, open_end, String::new()), (close, close_end, String::new())], before, after);
            self.sel = after;
            self.after_edit(cx);
            return;
        }
        if a != b {
            let text = self.doc.slice(a, b);
            let lead = text.len() - text.trim_start().len();
            let trail = text.len() - text.trim_end().len();
            let (ta, tb) = (self.doc.pos_of(ga + lead), self.doc.pos_of(gb - trail));
            if ta >= tb {
                return;
            }
            let before = self.sel;
            self.doc.replace_many(vec![(ta, ta, marker.to_string()), (tb, tb, marker.to_string())], before, before);
            let inner_a = Pos::new(ta.line, ta.col + marker.len());
            let inner_b = if tb.line == ta.line { Pos::new(tb.line, tb.col + marker.len()) } else { tb };
            self.sel = Sel { anchor: inner_a, head: inner_b };
            self.after_edit(cx);
            return;
        }
        let word = self.doc.word_at(a);
        if word.start < word.end {
            let (wa, wb) = (Pos::new(a.line, word.start), Pos::new(a.line, word.end));
            let before = self.sel;
            let caret = Pos::new(a.line, a.col + marker.len());
            self.doc.replace_many(vec![(wa, wa, marker.to_string()), (wb, wb, marker.to_string())], before, Sel::caret(caret));
            self.sel = Sel::caret(caret);
            self.after_edit(cx);
            return;
        }
        let line = self.doc.line(a.line);
        if line[..a.col].ends_with(marker) && line[a.col..].starts_with(marker) {
            let from = Pos::new(a.line, a.col - marker.len());
            let to = Pos::new(a.line, a.col + marker.len());
            self.replace_range(from, to, "", cx);
            return;
        }
        let caret = Pos::new(a.line, a.col + marker.len());
        self.doc.replace(a, a, &format!("{marker}{marker}"), self.sel);
        self.sel = Sel::caret(caret);
        self.after_edit(cx);
    }

    fn bold(&mut self, _: &Bold, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_wrap("**", SpanKind::Strong, cx);
    }

    fn italic(&mut self, _: &Italic, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_wrap("*", SpanKind::Em, cx);
    }

    /// ⌘↩: a task's box ticked or cleared; a bullet made a task; a plain
    /// line made a task.
    fn toggle_task(&mut self, _: &ToggleTask, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let line = self.sel.head.line;
        self.toggle_task_on(line, cx);
    }

    /// A task's box ticked. It is the one change Read mode lets in, by a
    /// press on the box alone, as a box on a page is there to be ticked;
    /// ⌘↩ is the keyboard's and is locked (`toggle_task`).
    fn toggle_task_on(&mut self, line: usize, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let text = self.doc.line(line).to_string();
        let (_, body_start) = quote_line(&text).unwrap_or((0, 0));
        let before = self.sel;
        match list_line(&text[body_start..]) {
            Some(item) => {
                let start = body_start + item.indent.len();
                match item.kind {
                    ListKind::Task(done) => {
                        let bx = start + 2;
                        let new = if done { "[ ]" } else { "[x]" };
                        self.doc.replace_many(vec![(Pos::new(line, bx), Pos::new(line, bx + 3), new.to_string())], before, before);
                    }
                    ListKind::Bullet => {
                        let at = Pos::new(line, start + item.marker.len());
                        let mut after = before;
                        for p in [&mut after.anchor, &mut after.head] {
                            if p.line == line && p.col >= at.col {
                                p.col += 4;
                            }
                        }
                        self.doc.replace_many(vec![(at, at, "[ ] ".to_string())], before, after);
                        self.sel = after;
                    }
                    ListKind::Ordered(_) => return,
                }
            }
            None => {
                let at = Pos::new(line, body_start);
                let mut after = before;
                for p in [&mut after.anchor, &mut after.head] {
                    if p.line == line && p.col >= at.col {
                        p.col += 6;
                    }
                }
                self.doc.replace_many(vec![(at, at, "- [ ] ".to_string())], before, after);
                self.sel = after;
            }
        }
        self.after_edit(cx);
    }

    // -- moves ----------------------------------------------------------------

    fn move_to(&mut self, p: Pos, select: bool, cx: &mut Context<Self>) {
        let p = self.settled(self.doc.clamp(p), select);
        self.sel = if select { Sel { anchor: self.sel.anchor, head: p } } else { Sel::caret(p) };
        self.after_move(cx);
    }

    fn left(&mut self, select: bool, cx: &mut Context<Self>) {
        stamp("key left");
        let leave = if select { None } else { self.leave_object(true) };
        let p = if let Some(p) = leave { p } else if !select && !self.sel.is_caret() { self.sel.start() } else { self.step_back(self.sel.head) };
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    fn right(&mut self, select: bool, cx: &mut Context<Self>) {
        let leave = if select { None } else { self.leave_object(false) };
        let p = if let Some(p) = leave { p } else if !select && !self.sel.is_caret() { self.sel.end() } else { self.step_on(self.sel.head) };
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    fn word_left(&mut self, select: bool, cx: &mut Context<Self>) {
        let p = self.leave_object(true).filter(|_| !select).unwrap_or_else(|| self.doc.word_left(self.sel.head));
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    fn word_right(&mut self, select: bool, cx: &mut Context<Self>) {
        let p = self.leave_object(false).filter(|_| !select).unwrap_or_else(|| self.doc.word_right(self.sel.head));
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    /// Home: the start of the words, before the line's marks; pressed
    /// there, the line's very start.
    fn line_start(&mut self, select: bool, cx: &mut Context<Self>) {
        let at = self.sel.head;
        let text = self.doc.line(at.line);
        let (_, qs) = quote_line(text).unwrap_or((0, 0));
        let content = list_line(&text[qs..]).map_or(qs, |i| qs + i.indent.len() + i.marker.len());
        let p = if at.col > content { content } else { 0 };
        if let Some(cell) = self.cell_of(at) {
            let (from, _) = self.cell_span(cell);
            self.preferred_x = None;
            self.move_to(from, select, cx);
            return;
        }
        self.preferred_x = None;
        self.move_to(Pos::new(at.line, p), select, cx);
    }

    fn line_end(&mut self, select: bool, cx: &mut Context<Self>) {
        let at = self.sel.head;
        if let Some(cell) = self.cell_of(at) {
            let (_, to) = self.cell_span(cell);
            self.preferred_x = None;
            self.move_to(to, select, cx);
            return;
        }
        self.preferred_x = None;
        self.move_to(Pos::new(at.line, self.doc.line(at.line).len()), select, cx);
    }

    /// ↑ and ↓: a wrapped row up or down in the same line, or the same
    /// cell, else the line before or after, at the column the first press
    /// began in.
    fn vertical(&mut self, down: bool, select: bool, cx: &mut Context<Self>) {
        let head = self.sel.head;
        let Some(at) = self.caret_at(head) else { return };
        let x = *self.preferred_x.get_or_insert(at.x);
        let cache = self.cache.borrow();
        let Some((top, bottom)) = self.flow_span(head, &cache) else { return };
        let target_y = if down { at.y + at.h + 1. } else { at.y - 1. };
        let target = if target_y >= top && target_y < bottom {
            self.pos_at(x, target_y, &cache)
        } else {
            // The next line that has height: a table's separator has none.
            let step = |l: usize| if down { Some(l + 1).filter(|n| *n < self.doc.line_count()) } else { l.checked_sub(1) };
            let mut next = step(head.line);
            while let Some(l) = next.filter(|l| cache.entries.get(*l).is_some_and(|e| e.height <= 0.)) {
                next = step(l);
            }
            match next {
                Some(line) => {
                    let y = if down { cache.tops[line] + 1. } else { cache.tops[line] + cache.entries[line].height - 1. };
                    self.pos_at(x, y.max(0.), &cache)
                }
                None if down => Some(self.doc.end()),
                None => Some(Pos::default()),
            }
        };
        drop(cache);
        if let Some(p) = target {
            self.move_to(p, select, cx);
        }
    }

    fn page(&mut self, down: bool, cx: &mut Context<Self>) {
        let head = self.sel.head;
        let Some(at) = self.caret_at(head) else { return };
        let view = f32::from(self.scroll.bounds().size.height).max(100.);
        let x = *self.preferred_x.get_or_insert(at.x);
        let y = if down { at.y + view } else { at.y - view };
        let target = { self.pos_at(x, y.max(0.), &self.cache.borrow()) };
        if let Some(p) = target {
            self.move_to(p, false, cx);
        }
    }

    // -- the layout ---------------------------------------------------------

    fn hidden_on(&self, line: usize) -> Vec<std::ops::Range<usize>> {
        self.reader().hidden_on(line)
    }

    /// The whole note laid out at `width`.
    pub(crate) fn relayout(&mut self, width: f32, window: &mut Window, cx: &mut App) {
        let theme = cx.theme().clone();
        let mut cache = self.cache.borrow_mut();
        self.reader().lay_out_note(&mut cache, width, &theme, window);
    }

    // -- places on the page -------------------------------------------------

    /// Where the caret at `p` stands, from the element's top left.
    fn caret_at(&self, p: Pos) -> Option<CaretAt> {
        let cache = self.cache.borrow();
        let entry = cache.entries.get(p.line)?;
        let top = cache.tops[p.line];
        let on_text = |r: &TextRow, top: f32| {
            let (x, y) = r.flow.caret(p.col);
            CaretAt::new(r.inset + x, top + r.top_pad + y, r.flow.line_h, r.flow.caret_h)
        };
        Some(match &*entry.row {
            Row::Text(r) => on_text(r, top),
            Row::Picture(pic) if pic.source.is_some() => on_text(pic.source.as_ref()?, top),
            Row::Table(t) => {
                let c = t.column_of(p.col);
                let (x, y) = t.cells.get(c)?.caret(p.col);
                CaretAt::new(t.inset + t.grid.xs[c] + crate::layout::CELL_X + x, top + t.cell_y + y, t.cells[c].line_h, t.cells[c].caret_h)
            }
            Row::Rule { inset, line_h } => CaretAt::new(*inset, top, *line_h, Rhythm::caret(self.look.text_px, *line_h)),
            Row::Pad { h, .. } => CaretAt::new(0., top, *h, Rhythm::caret(self.look.text_px, *h)),
            _ => {
                let line = entry.height.max(Rhythm::of(&self.look).line);
                CaretAt::new(0., top, line, Rhythm::caret(self.look.text_px, line))
            }
        })
    }

    /// The top and the foot of the words a place stands in, from the
    /// element's top: its row, or its cell.
    fn flow_span(&self, p: Pos, cache: &Cache) -> Option<(f32, f32)> {
        let entry = cache.entries.get(p.line)?;
        let top = cache.tops[p.line];
        Some(match &*entry.row {
            Row::Table(t) => {
                let flow = t.cells.get(t.column_of(p.col))?;
                (top + t.cell_y, top + t.cell_y + flow.height)
            }
            // A caption's rows, so ↑ from its first leaves the line.
            Row::Picture(pic) => match &pic.source {
                Some(r) => (top + r.top_pad, top + r.top_pad + r.flow.height),
                None => (top, top + entry.height),
            },
            _ => (top, top + entry.height),
        })
    }

    /// The line under `y`: a line of no height is passed by for the one
    /// that has it.
    fn line_at(&self, y: f32, cache: &Cache) -> usize {
        let line = cache.tops.partition_point(|t| *t <= y).saturating_sub(1).min(cache.entries.len().saturating_sub(1));
        let mut line = line;
        while line > 0 && cache.entries[line].height <= 0. {
            line -= 1;
        }
        line
    }

    /// The place in the text at a point of the page, from the element's
    /// top left: the row under `y`, the character nearest `x` in it.
    fn pos_at(&self, x: f32, y: f32, cache: &Cache) -> Option<Pos> {
        if cache.entries.is_empty() {
            return None;
        }
        let line = self.line_at(y, cache);
        let entry = &cache.entries[line];
        let text_len = self.doc.line(line).len();
        let top = cache.tops[line];
        let on_text = |r: &TextRow| {
            let mut col = r.flow.col_at(x - r.inset, y - top - r.top_pad);
            // A click at a hidden span's right edge stands past the span.
            let start = self.analysis.line_starts[line];
            let hidden_col = r.flow.paras.iter().find(|p| p.from <= col && col <= p.to).is_some_and(|p| p.shown.is_hidden(col));
            for &six in &self.analysis.line_spans[line] {
                let s = &self.analysis.spans[six];
                if s.close.start >= start && s.close.start - start == col && s.close.end > s.close.start && hidden_col {
                    col = s.close.end - start;
                }
            }
            Pos::new(line, col.min(text_len))
        };
        Some(match &*entry.row {
            Row::Text(r) => on_text(r),
            // On the picture itself, the end of its line; on the line shown under it,
            // the place in it.
            Row::Picture(pic) => match &pic.source {
                Some(r) if y >= top + r.top_pad => on_text(r),
                _ => Pos::new(line, text_len),
            },
            Row::Table(t) => {
                let c = t.column_at(x - t.inset);
                let flow = t.cells.get(c)?;
                Pos::new(line, flow.col_at(x - t.inset - t.grid.xs[c] - crate::layout::CELL_X, y - top - t.cell_y).min(text_len))
            }
            _ => Pos::new(line, 0),
        })
    }

    /// What a press lands on besides words: a task's box, a link.
    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let cache = self.cache.borrow();
        if cache.entries.is_empty() {
            return None;
        }
        let line = self.line_at(y, &cache);
        let entry = cache.entries.get(line)?;
        let top = cache.tops[line];
        match &*entry.row {
            Row::Text(r) => {
                for w in &r.widgets {
                    if let Widget::Task { bounds, .. } = w {
                        let b = Bounds::new(point(bounds.origin.x - px(3.), bounds.origin.y + px(top) - px(3.)), bounds.size + size(px(6.), px(6.)));
                        if b.contains(&point(px(x), px(y))) {
                            return Some(Hit::Task(line));
                        }
                    }
                }
                r.flow.link_at(x - r.inset, y - top - r.top_pad).map(Hit::Link)
            }
            Row::Table(t) => {
                let c = t.column_at(x - t.inset);
                t.cells.get(c)?.link_at(x - t.inset - t.grid.xs[c] - crate::layout::CELL_X, y - top - t.cell_y).map(Hit::Link)
            }
            _ => None,
        }
    }

    /// Whether the scroller was moved.
    fn scroll_to_caret(&mut self, bounds: Bounds<Pixels>, window: &Window) -> bool {
        self.want_scroll = false;
        let spot = self.want_spot.take().and_then(|(line, y, h)| self.cache.borrow().tops.get(line).map(|top| (top + y, h)));
        let Some((y, h)) = spot.or_else(|| self.caret_at(self.sel.head).map(|at| (at.y, at.h))) else { return false };
        let view = window.content_mask().bounds;
        let caret_top = f32::from(bounds.top()) + y;
        let caret_bottom = caret_top + h;
        let (vt, vb) = (f32::from(view.top()), f32::from(view.bottom()));
        let offset = self.scroll.offset();
        let mut dy = 0.;
        if caret_top < vt + SCROLL_MARGIN {
            dy = vt + SCROLL_MARGIN - caret_top;
        } else if caret_bottom > vb - SCROLL_MARGIN {
            dy = (vb - SCROLL_MARGIN) - caret_bottom;
        }
        if dy.abs() > 0.5 {
            let max = f32::from(self.scroll.max_offset().y);
            let y = (f32::from(offset.y) + dy).clamp(-max.max(0.), 0.);
            self.scroll.set_offset(point(offset.x, px(y)));
            return true;
        }
        false
    }
}

enum Hit {
    Task(usize),
    Link(Target),
}

// -- the input method -------------------------------------------------------

impl EntityInputHandler for Editor {
    /// False, so a key goes to the bindings before an input method. gpui
    /// on a Mac hands every printable key, ⌥ chords among them, to an
    /// active Chinese, Japanese or Korean input method first while the
    /// handler says it takes text, and the method swallowed the ⌃ and ⌥ chords of the modes and the widths.
    /// Nothing of typing changes: a key no binding takes still reaches the
    /// method and composes, and this is read nowhere else on a Mac.
    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        false
    }

    fn text_for_range(&mut self, range_utf16: Range<usize>, adjusted_range: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let a = self.doc.pos_of_utf16(range_utf16.start);
        let b = self.doc.pos_of_utf16(range_utf16.end);
        *adjusted_range = Some(self.doc.utf16_of(a)..self.doc.utf16_of(b));
        Some(self.doc.slice(a, b))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let (a, b) = self.sel.range();
        Some(UTF16Selection { range: self.doc.utf16_of(a)..self.doc.utf16_of(b), reversed: self.sel.head < self.sel.anchor })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.map(|(a, b)| self.doc.utf16_of(a)..self.doc.utf16_of(b))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        stamp("key typed");
        if self.locked() {
            return;
        }
        let (a, b) = match range_utf16 {
            Some(r) => (self.doc.pos_of_utf16(r.start), self.doc.pos_of_utf16(r.end)),
            None => match self.marked {
                Some((a, b)) => (a, b),
                None => self.sel.range(),
            },
        };
        self.marked = None;
        let text = self.cell_words(text);
        let end = self.doc.replace(a, b, &text, self.sel);
        self.sel = Sel::caret(end);
        self.after_edit(cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, new_selected_range: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let (a, b) = match range_utf16 {
            Some(r) => (self.doc.pos_of_utf16(r.start), self.doc.pos_of_utf16(r.end)),
            None => match self.marked {
                Some((a, b)) => (a, b),
                None => self.sel.range(),
            },
        };
        let end = self.doc.replace(a, b, new_text, self.sel);
        let start_u = self.doc.utf16_of(a);
        self.marked = (a != end).then_some((a, end));
        self.sel = match new_selected_range {
            Some(r) => Sel { anchor: self.doc.pos_of_utf16(start_u + r.start), head: self.doc.pos_of_utf16(start_u + r.end) },
            None => Sel::caret(end),
        };
        self.reanalyze();
        self.text_changed(cx);
        self.want_scroll = true;
        self.kick_blink(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(&mut self, range_utf16: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let at = self.caret_at(self.doc.pos_of_utf16(range_utf16.start))?;
        Some(Bounds::new(point(self.origin.x + px(at.x), self.origin.y + px(at.y)), size(px(2.), px(at.h))))
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let local = (f32::from(point.x - self.origin.x), f32::from(point.y - self.origin.y));
        let p = self.pos_at(local.0, local.1, &self.cache.borrow())?;
        Some(self.doc.utf16_of(p))
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.doc.utf16_len())
    }
}

// -- the view ---------------------------------------------------------------

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("editor")
            .key_context(EDITOR_CONTEXT)
            .track_focus(&self.focus)
            .w_full()
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::new_line))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_line_back))
            .on_action(cx.listener(|this, _: &Left, _, cx| this.left(false, cx)))
            .on_action(cx.listener(|this, _: &Right, _, cx| this.right(false, cx)))
            .on_action(cx.listener(|this, _: &Up, _, cx| this.vertical(false, false, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.vertical(true, false, cx)))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.left(true, cx)))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.right(true, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.vertical(false, true, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.vertical(true, true, cx)))
            .on_action(cx.listener(|this, _: &WordLeft, _, cx| this.word_left(false, cx)))
            .on_action(cx.listener(|this, _: &WordRight, _, cx| this.word_right(false, cx)))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| this.word_left(true, cx)))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| this.word_right(true, cx)))
            .on_action(cx.listener(|this, _: &LineStart, _, cx| this.line_start(false, cx)))
            .on_action(cx.listener(|this, _: &LineEnd, _, cx| this.line_end(false, cx)))
            .on_action(cx.listener(|this, _: &SelectLineStart, _, cx| this.line_start(true, cx)))
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| this.line_end(true, cx)))
            .on_action(cx.listener(|this, _: &DocStart, _, cx| this.move_to(Pos::default(), false, cx)))
            .on_action(cx.listener(|this, _: &DocEnd, _, cx| {
                let end = this.doc.end();
                this.move_to(end, false, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectDocStart, _, cx| this.move_to(Pos::default(), true, cx)))
            .on_action(cx.listener(|this, _: &SelectDocEnd, _, cx| {
                let end = this.doc.end();
                this.move_to(end, true, cx)
            }))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(false, cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(true, cx)))
            .on_action(cx.listener(Self::bold))
            .on_action(cx.listener(Self::italic))
            .on_action(cx.listener(Self::toggle_task))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_drop(cx.listener(Self::dropped))
            .on_drop(cx.listener(Self::dropped_asset))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .child(EditorElement { editor: cx.entity().clone() })
            .into_any_element()
    }
}

// -- the element ------------------------------------------------------------

/// The element that lays the note out and paints it, one child of the
/// scroller the view around it draws.
struct EditorElement {
    editor: Entity<Editor>,
}

struct Prepainted {
    hitbox: Hitbox,
    /// The hitbox with room each side for a table's handles, which stand
    /// out in the page's margin.
    wide: Hitbox,
    /// The frames of the files the note embeds that show, laid out over
    /// their rows (`embed.rs`).
    frames: Vec<AnyElement>,
}

/// How much of a table's handles to paint, and where.
struct Controls {
    table: cells::TableBox,
    row: (f32, f32),
    col: usize,
    /// Which handle or edge the pointer is on.
    on: Option<cells::Control>,
    opacity: f32,
}

/// What paint takes out of the editor before it draws, since drawing
/// needs the app the editor is read through.
struct PaintInfo {
    focus: FocusHandle,
    pointer: CursorStyle,
    cache: Rc<RefCell<Cache>>,
    /// Each code block's first and last line, and where its plate begins.
    blocks: Vec<(usize, usize, f32)>,
    /// Each table's first and last line.
    tables: Vec<(usize, usize)>,
    sel: Sel,
    marked: Option<(Pos, Pos)>,
    caret: Option<Bounds<Pixels>>,
    find: Rc<Vec<(Pos, Pos)>>,
    pictures: Rc<RefCell<Pictures>>,
    /// The cell the caret is in, from the element's top left.
    cell: Option<Bounds<Pixels>>,
    controls: Option<Controls>,
    /// The bar and the handles of the picture the pointer is over.
    picture: Option<picture::PicPaint>,
    /// Where a dragged row or column would go: a line, from the element's top left.
    drop_line: Option<Bounds<Pixels>>,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// Words, with what stands behind and over them: chips, what the find
/// bar found, the selection, and an input method's underline. `line` is
/// the text's line, or none for words that are no part of the text.
fn paint_flow(flow: &crate::layout::Flow, origin: Point<Pixels>, line: Option<usize>, past_end: bool, info: &PaintInfo, theme: &gpui_component::Theme, window: &mut Window, cx: &mut App) {
    let lh = flow.line_h;
    for (b, color) in flow.chips(origin) {
        let b = Bounds::new(point(b.origin.x - px(2.), b.origin.y + px(lh * 0.08)), size(b.size.width + px(4.), px(lh * 0.84)));
        window.paint_quad(quad(b, px(4.), color, px(0.), theme.transparent, BorderStyle::default()));
    }
    let mut marked_boxes = Vec::new();
    if let Some(line) = line {
        // The find bar's matches on this line, washed; the one the bar
        // stands on is the selection, painted after.
        if !info.find.is_empty() {
            let wash = Hsla { s: 0.1, ..theme.selection };
            for &(fa, fb) in info.find.iter().filter(|(a, b)| a.line <= line && line <= b.line) {
                let from = if fa.line == line { fa.col } else { 0 };
                let to = if fb.line == line { fb.col } else { usize::MAX };
                for b in flow.boxes(from, to, origin) {
                    window.paint_quad(quad(b, px(2.), wash, px(0.), theme.transparent, BorderStyle::default()));
                }
            }
        }
        // The selection, as far as it lies on this line.
        let (sa, sb) = info.sel.range();
        if sa != sb && sa.line <= line && line <= sb.line {
            let from = if sa.line == line { sa.col } else { 0 };
            let to = if sb.line == line { sb.col } else { usize::MAX };
            let mut boxes = flow.boxes(from, to, origin);
            if past_end && sb.line > line {
                // The line's end is selected too: a little past the last word.
                if let Some(last) = boxes.last_mut() {
                    last.size.width += px(6.);
                } else {
                    boxes.push(Bounds::new(origin, size(px(6.), px(lh))));
                }
            }
            for b in boxes {
                window.paint_quad(fill(b, theme.selection));
            }
        }
        if let Some((ma, mb)) = info.marked.filter(|(ma, _)| ma.line == line) {
            let to = if mb.line == line { mb.col } else { usize::MAX };
            marked_boxes = flow.boxes(ma.col, to, origin);
        }
    }
    flow.paint(origin, window, cx);
    // An input method's composition, underlined.
    for b in marked_boxes {
        window.paint_quad(fill(Bounds::new(point(b.origin.x, b.origin.y + px(lh) - px(2.)), size(b.size.width, px(1.))), theme.foreground));
    }
}

/// A code block's language mark: how far in from the plate's top and
/// right it sits, its icon's size, and its name's (`layout.rs` shapes the
/// name at `MARK_PX`).
const MARK_IN: f32 = 9.;
const MARK_ICON: f32 = 13.;
/// The room the icons leave inside their box, and a letter after its ink.
const MARK_ICON_PAD: f32 = 1.5;
const MARK_BEARING: f32 = 0.5;
pub(crate) const MARK_PX: f32 = 11.5;

/// The ground and the outline of a table, the head's tint inside it. The
/// lines between its cells are each row's.
fn paint_plate(left: f32, top: f32, width: f32, height: f32, head: f32, theme: &gpui_component::Theme, window: &mut Window) {
    let r = px(TABLE_R);
    let corners = Corners { top_left: r, top_right: r, bottom_left: px(0.), bottom_right: px(0.) };
    window.paint_quad(quad(Bounds::new(point(px(left), px(top)), size(px(width), px(head))), corners, theme.muted.opacity(0.7), px(0.), theme.transparent, BorderStyle::default()));
    window.paint_quad(quad(Bounds::new(point(px(left), px(top)), size(px(width), px(height))), r, clear_of(theme.border), px(1.), theme.border, BorderStyle::default()));
}

/// One row at a place on the page. `line` is the text's line, or none
/// inside a block of HTML.
#[allow(clippy::too_many_arguments)]
fn paint_row(row: &Row, ox: f32, oy: f32, height: f32, full_w: Pixels, line: Option<usize>, info: &PaintInfo, theme: &gpui_component::Theme, window: &mut Window, cx: &mut App) {
    match row {
        Row::Text(r) => paint_text(r, ox, oy, height, line, info, theme, window, cx),
        Row::Pad { lang, .. } => {
            if let Some((l, icon, color)) = lang {
                // The mark sits in the plate's corner as far from its top as
                // from its right, measured to what shows: the icon's top and
                // the name's last letter. The name's capitals are centered on
                // the icon, and its descenders stay in the fence's row.
                let name_w = f32::from(l.width());
                let x = f32::from(full_w) - name_w - (MARK_IN - MARK_BEARING);
                let icon_top = oy + MARK_IN - MARK_ICON_PAD;
                let cap = 0.727 * MARK_PX;
                let baseline = icon_top + MARK_ICON / 2. + cap / 2.;
                let tall = l.ascent + l.descent;
                let _ = l.paint(point(px(ox + x), px(baseline) - l.ascent), tall, TextAlign::Left, None, window, cx);
                let ib = Bounds::new(point(px(ox + x - MARK_ICON - 5.), px(icon_top)), size(px(MARK_ICON), px(MARK_ICON)));
                let _ = window.paint_svg(ib, SharedString::from(*icon), None, TransformationMatrix::unit(), *color, cx);
            }
        }
        Row::Hidden => {}
        Row::Rule { inset, line_h } => {
            let b = Bounds::new(point(px(ox + inset), px(oy + line_h / 2. - 1.)), size(full_w - px(*inset), px(2.)));
            window.paint_quad(fill(b, theme.border));
        }
        Row::Table(t) => {
            let left = ox + t.inset;
            let total = t.grid.width;
            if !t.head {
                window.paint_quad(fill(Bounds::new(point(px(left), px(oy)), size(px(total), px(1.))), theme.border));
            }
            for (c, flow) in t.cells.iter().enumerate() {
                let x = left + t.grid.xs[c];
                if c > 0 {
                    window.paint_quad(fill(Bounds::new(point(px(x), px(oy)), size(px(1.), px(t.height))), theme.border));
                }
                paint_flow(flow, point(px(x + crate::layout::CELL_X), px(oy + t.cell_y)), line, false, info, theme, window, cx);
            }
        }
        Row::Nested(n) => {
            // A selection over the block is a wash over the whole of it.
            if let Some(line) = line {
                let (sa, sb) = info.sel.range();
                if sa != sb && sa.line <= line && line <= sb.line {
                    window.paint_quad(fill(Bounds::new(point(px(ox), px(oy)), size(full_w, px(n.height))), theme.selection));
                }
            }
            for &(first, last, inset) in &n.plates.code {
                let (Some((top, _)), Some((last_top, last_row))) = (n.rows.get(first), n.rows.get(last)) else { continue };
                let b = Bounds::new(point(px(ox + inset), px(oy + top)), size(full_w - px(inset), px(last_top + last_row.height() - top)));
                window.paint_quad(quad(b, px(8.), theme.muted, px(1.), theme.border, BorderStyle::default()));
            }
            for &(first, last) in &n.plates.tables {
                let (Some((top, Row::Table(head))), Some((last_top, last_row))) = (n.rows.get(first), n.rows.get(last)) else { continue };
                paint_plate(ox + head.inset, oy + top, head.grid.width, last_top + last_row.height() - top, head.height, theme, window);
            }
            for (top, inner) in &n.rows {
                paint_row(inner, ox, oy + top, inner.height(), full_w, None, info, theme, window, cx);
            }
        }
        Row::Picture(p) => {
            let control = Bounds::new(point(px(ox + p.x), px(oy + p.pad)), size(px(p.w), px(p.h)));
            let round = PICTURE_ROUND;
            // A frame is a view of its own, laid over the row (`embed.rs`);
            // so is a picture's bar, which comes over it on the pointer.
            if !p.embed {
                // Edged as a frame is, its bar standing inside the line. The
                // picture's own rim stops in the middle of that line, so the
                // soft edge a round corner gives it falls under the line and
                // never shows past it.
                let inner = Bounds::new(control.origin + point(px(0.5), px(0.5)), size(control.size.width - px(1.), control.size.height - px(1.)));
                let drawn = info.pictures.borrow().drawn.get(&p.path).cloned();
                match drawn {
                    Some(image) => {
                        let _ = window.paint_image(inner, inner, Corners::all(px(round - 0.5)), image, 0, false);
                    }
                    None => window.paint_quad(quad(inner, px(round - 0.5), theme.muted, px(0.), theme.transparent, BorderStyle::default())),
                }
                window.paint_quad(quad(control, px(round), clear_of(theme.border), px(1.), theme.border, BorderStyle::default()));
            }
            // The caption under it, on a quiet plate as wide as its words.
            if let Some(source) = &p.source {
                let len = source.flow.paras.last().map_or(0, |pa| pa.to);
                let words = source.flow.boxes(0, len, point(px(ox + source.inset), px(oy + source.top_pad)));
                let wide = words.iter().fold(None::<Bounds<Pixels>>, |all, b| Some(all.map_or(*b, |a| a.union(b))));
                if let Some(w) = wide.filter(|w| w.size.width > px(0.5)) {
                    let plate = Bounds::new(point(w.origin.x - px(9.), w.origin.y - px(2.)), size(w.size.width + px(18.), w.size.height + px(4.)));
                    window.paint_quad(quad(plate, px(8.), theme.muted.opacity(0.7), px(0.), theme.transparent, BorderStyle::default()));
                }
                paint_text(source, ox, oy, source.flow.height, line, info, theme, window, cx);
            }
            // Selected, whole or with what is around it: ringed in the
            // accent, out from its edge, so a frame over it shows it too.
            if let Some(line) = line {
                if !p.embed && p.source.is_none() && covers(&info.sel, line) {
                    window.with_content_mask(None, |window| picture::paint_ring(control, theme.primary, window));
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_text(r: &TextRow, ox: f32, oy: f32, height: f32, line: Option<usize>, info: &PaintInfo, theme: &gpui_component::Theme, window: &mut Window, cx: &mut App) {
    let text_origin = point(px(ox + r.inset), px(oy + r.top_pad));
    paint_flow(&r.flow, text_origin, line, true, info, theme, window, cx);
    let lh = px(r.flow.line_h);
    for w in &r.widgets {
        match w {
            Widget::Bullet { x, y, r: radius } => {
                let b = Bounds::new(point(px(ox + x - radius), px(oy + y - radius)), size(px(radius * 2.), px(radius * 2.)));
                window.paint_quad(quad(b, px(*radius), theme.muted_foreground, px(0.), theme.transparent, BorderStyle::default()));
            }
            Widget::Number { line: shaped, x, y } => {
                let _ = shaped.paint(point(px(ox + x), px(oy + y)), lh, TextAlign::Left, None, window, cx);
            }
            Widget::Task { bounds: b, done } => {
                let b = Bounds::new(point(px(ox) + b.origin.x, px(oy) + b.origin.y), b.size);
                if *done {
                    window.paint_quad(quad(b, px(4.), theme.primary, px(0.), theme.transparent, BorderStyle::default()));
                    let inner = Bounds::new(point(b.origin.x + b.size.width * 0.15, b.origin.y + b.size.height * 0.15), size(b.size.width * 0.7, b.size.height * 0.7));
                    let _ = window.paint_svg(inner, "icons/check.svg".into(), None, TransformationMatrix::unit(), theme.primary_foreground, cx);
                } else {
                    window.paint_quad(quad(b, px(4.), clear_of(theme.muted_foreground), px(1.5), theme.muted_foreground.opacity(0.7), BorderStyle::default()));
                }
            }
            Widget::Guide { x } => window.paint_quad(fill(Bounds::new(point(px(ox + x), px(oy)), size(px(1.), px(height))), theme.border)),
            Widget::Bar { x } => window.paint_quad(fill(Bounds::new(point(px(ox + x), px(oy)), size(px(2.), px(height))), theme.primary.opacity(0.7))),
        }
    }
}

/// A table's handles and the strips that add to it, over the page.
fn paint_controls(c: &Controls, ox: f32, oy: f32, theme: &gpui_component::Theme, window: &mut Window, cx: &mut App) {
    use cells::{Control, ADD, ADD_GAP, GRIP_LONG, GRIP_SHORT};
    let k = c.opacity;
    let b = &c.table;
    // A handle: its dots, two rows of three along its long side.
    let grip = |bounds: Bounds<Pixels>, upright: bool, on: bool, window: &mut Window| {
        let ground = if on { theme.primary } else { theme.background };
        let ink = if on { theme.primary_foreground } else { theme.muted_foreground };
        window.paint_quad(quad(bounds, px(4.), ground.opacity(k), px(1.), (if on { theme.primary } else { theme.border }).opacity(k), BorderStyle::default()));
        let (cx0, cy0) = (bounds.origin.x + bounds.size.width / 2., bounds.origin.y + bounds.size.height / 2.);
        for long in [-4.5, 0., 4.5] {
            for short in [-2., 2.] {
                let (dx, dy) = if upright { (short, long) } else { (long, short) };
                let dot = Bounds::new(point(cx0 + px(dx - 1.), cy0 + px(dy - 1.)), size(px(2.), px(2.)));
                window.paint_quad(quad(dot, px(1.), ink.opacity(k), px(0.), theme.transparent, BorderStyle::default()));
            }
        }
    };
    let strip = |bounds: Bounds<Pixels>, on: bool, window: &mut Window, cx: &mut App| {
        let ground = if on { theme.primary.opacity(0.14 * k) } else { theme.muted.opacity(0.6 * k) };
        window.paint_quad(quad(bounds, px(4.), ground, px(0.), theme.transparent, BorderStyle::default()));
        let side = px(12.);
        let at = point(bounds.origin.x + (bounds.size.width - side) / 2., bounds.origin.y + (bounds.size.height - side) / 2.);
        let ink = if on { theme.primary } else { theme.muted_foreground };
        let _ = window.paint_svg(Bounds::new(at, size(side, side)), "icons/plus.svg".into(), None, TransformationMatrix::unit(), ink.opacity(k), cx);
    };
    let (left, top, right, bottom) = (ox + b.left, oy + b.top, ox + b.right, oy + b.bottom);
    strip(Bounds::new(point(px(right + ADD_GAP), px(top)), size(px(ADD), px(bottom - top))), c.on == Some(Control::AddColumn), window, cx);
    strip(Bounds::new(point(px(left), px(bottom + ADD_GAP)), size(px(right - left), px(ADD))), c.on == Some(Control::AddRow), window, cx);
    let row_mid = oy + (c.row.0 + c.row.1) / 2.;
    grip(Bounds::new(point(px(left - GRIP_SHORT / 2.), px(row_mid - GRIP_LONG / 2.)), size(px(GRIP_SHORT), px(GRIP_LONG))), true, matches!(c.on, Some(Control::RowGrip(_))), window);
    if let (Some(x), Some(w)) = (b.grid.xs.get(c.col), b.grid.cols.get(c.col)) {
        let mid = left + x + w / 2.;
        grip(Bounds::new(point(px(mid - GRIP_LONG / 2.), px(top - GRIP_SHORT / 2.)), size(px(GRIP_LONG), px(GRIP_SHORT))), false, matches!(c.on, Some(Control::ColumnGrip(_))), window);
    }
    if let Some(Control::Edge(col)) = c.on {
        if let (Some(x), Some(w)) = (b.grid.xs.get(col), b.grid.cols.get(col)) {
            window.paint_quad(fill(Bounds::new(point(px(left + x + w - 1.), px(top)), size(px(2.), px(bottom - top))), theme.primary));
        }
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        // Whether the keyboard is in the editor, known before the layout,
        // since the marks around the caret show only while it is; and the
        // caret begins to blink as the keyboard arrives.
        self.editor.update(cx, |e, cx| {
            let now = e.focus.is_focused(window);
            if now != e.focused {
                stamp(if now { "editor focus gained" } else { "editor focus lost" });
            }
            if now && !e.focused {
                e.kick_blink(cx);
            }
            e.focused = now;
        });
        stamp("editor request_layout");
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let editor = self.editor.clone();
        // The height is the note's at the width it is given.
        let id = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            let width = f32::from(width.unwrap_or(px(600.))).max(80.);
            let started = Instant::now();
            let total = editor.update(cx, |e, cx| {
                e.relayout(width, window, cx);
                e.cache.borrow().total
            });
            if tracing() {
                eprintln!("trace measure width={width} known={:?} avail={:?}", known.width, available.width);
            }
            trace("relayout", started);
            size(px(width), px(total.max(f32::from(window.line_height()))))
        });
        (id, ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Prepainted {
        stamp("editor prepaint");
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let far = px(cells::ADD + cells::ADD_GAP + 4.);
        let wide = window.insert_hitbox(Bounds::new(point(bounds.origin.x - far, bounds.origin.y), size(bounds.size.width + far * 2., bounds.size.height + far)), HitboxBehavior::Normal);
        self.editor.update(cx, |e, cx| {
            e.origin = bounds.origin;
            e.read_pictures(cx);
            e.read_pdfs(cx);
            // The scroller reads its offset before its child is laid out,
            // so a move made here shows at the next frame, which is asked for.
            if e.want_scroll && e.scroll_to_caret(bounds, window) {
                cx.notify();
            }
        });
        // Each frame that shows, over its row: after the editor's own
        // hitboxes, so a press on a frame is the frame's.
        let shown = self.editor.update(cx, |e, cx| e.frames_in_view(bounds, window, cx));
        let cards = self.editor.update(cx, |e, cx| e.cards_in_view(bounds, window, cx));
        let mut frames = Vec::with_capacity(shown.len() + cards.len());
        let views = shown.into_iter().map(|(f, b)| (f.into_any_element(), b)).chain(cards.into_iter().map(|(c, b)| (c.into_any_element(), b)));
        for (mut el, b) in views {
            el.layout_as_root(b.size.map(AvailableSpace::Definite), window, cx);
            el.prepaint_at(b.origin, window, cx);
            frames.push(el);
        }
        Prepainted { hitbox, wide, frames }
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), prepaint: &mut Prepainted, window: &mut Window, cx: &mut App) {
        let theme = cx.theme().clone();
        let editor = self.editor.clone();
        stamp("paint begins");
        let paint_started = Instant::now();
        let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let info = editor.update(cx, |e, cx| {
            e.focused = e.focus.is_focused(window);
            e.paint_info(point(px(ox), px(oy)), window, cx)
        });
        window.handle_input(&info.focus, ElementInputHandler::new(bounds, editor.clone()), cx);
        window.set_cursor_style(info.pointer, if info.controls.is_some() || info.picture.is_some() { &prepaint.wide } else { &prepaint.hitbox });

        let cache = info.cache.borrow();
        let view = window.content_mask().bounds;
        let visible = |top: f32, h: f32| oy + top + h >= f32::from(view.top()) - 1. && oy + top <= f32::from(view.bottom()) + 1.;
        let first = cache.tops.partition_point(|t| *t <= f32::from(view.top()) - oy).saturating_sub(1);

        // The plates behind code blocks and tables, before anything on them.
        for &(first_line, last_line, inset) in &info.blocks {
            let (Some(top), Some(last)) = (cache.tops.get(first_line), cache.entries.get(last_line)) else { continue };
            let bottom = cache.tops[last_line] + last.height;
            if !visible(*top, bottom - top) {
                continue;
            }
            let b = Bounds::new(point(px(ox + inset), px(oy + top)), size(bounds.size.width - px(inset), px(bottom - top)));
            window.paint_quad(quad(b, px(8.), theme.muted, px(1.), theme.border, BorderStyle::default()));
        }
        for &(first_line, last_line) in &info.tables {
            let (Some(top), Some(last), Some(head)) = (cache.tops.get(first_line), cache.entries.get(last_line), cache.entries.get(first_line)) else { continue };
            let bottom = cache.tops[last_line] + last.height;
            if let (Row::Table(t), true) = (&*head.row, visible(*top, bottom - top)) {
                paint_plate(ox + t.inset, oy + top, t.grid.width, bottom - top, head.height, &theme, window);
            }
        }

        let mut rings: Vec<Bounds<Pixels>> = Vec::new();
        for line in first..cache.entries.len() {
            let entry = &cache.entries[line];
            let top = cache.tops[line];
            if oy + top > f32::from(view.bottom()) + 1. {
                break;
            }
            if entry.height <= 0. || !visible(top, entry.height) {
                continue;
            }
            paint_row(&entry.row, ox, oy + top, entry.height, bounds.size.width, Some(line), &info, &theme, window, cx);
            // A frame's ring goes on after the frame, which covers what
            // is past its round corners in the sheet's color.
            if let Row::Picture(p) = &*entry.row {
                if p.embed && p.source.is_none() && covers(&info.sel, line) {
                    rings.push(Bounds::new(point(px(ox + p.x), px(oy + top + p.pad)), size(px(p.w), px(p.h))));
                }
            }
        }
        drop(cache);
        if let Some(b) = info.cell {
            window.paint_quad(quad(Bounds::new(point(px(ox) + b.origin.x, px(oy) + b.origin.y), b.size), px(3.), clear_of(theme.primary), px(1.5), theme.primary, BorderStyle::default()));
        }
        if let Some(c) = &info.controls {
            window.with_content_mask(None, |window| paint_controls(c, ox, oy, &theme, window, cx));
            if c.opacity < 1. {
                window.request_animation_frame();
            }
        }
        if let Some(b) = info.drop_line {
            window.paint_quad(fill(Bounds::new(point(px(ox) + b.origin.x, px(oy) + b.origin.y), b.size), theme.primary));
        }
        if let Some(b) = info.caret {
            window.paint_quad(fill(b, theme.caret));
        }
        trace("paint", paint_started);

        // The pointer: a press places the caret or takes a widget, a drag
        // selects, a press on a link follows it.
        let (hitbox, wide) = (prepaint.hitbox.clone(), prepaint.wide.clone());
        let ed = editor.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || ev.button != MouseButton::Left || !wide.is_hovered(window) {
                return;
            }
            let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y));
            let taken = ed.update(cx, |e, cx| e.press_control(x, y, ev.position, cx) || e.press_picture(x, y, ev.click_count, ev.modifiers.shift, window, cx));
            if taken {
                cx.stop_propagation();
                crate::widgets::swallow_click(window, cx);
                return;
            }
            if !hitbox.is_hovered(window) {
                return;
            }
            // The press is the editor's: the scroller around it hears a
            // press in the margin only, which takes the keyboard back.
            cx.stop_propagation();
            stamp("mouse down");
            ed.update(cx, |e, cx| {
                // As the note reads a press selects as anywhere else, for
                // ⌘C and the find to work on, and a link waits for the
                // release, so a selection can begin on one.
                let reading = e.mode == Mode::Reading;
                if !e.focus.is_focused(window) {
                    window.focus(&e.focus, cx);
                }
                // A picture opens in the viewer at a double click, or at a
                // click while it cannot change.
                if let Some((_, line)) = e.picture_at(x, y) {
                    if ev.click_count == 2 || (ev.click_count == 1 && !e.pictures_change()) {
                        e.view_picture(line, bounds.origin, cx);
                        return;
                    }
                }
                match e.hit(x, y) {
                    Some(Hit::Task(line)) if !e.read_only => {
                        e.toggle_task_on(line, cx);
                        e.doc.end_step();
                    }
                    Some(Hit::Link(target)) if !reading && (ev.modifiers.secondary() || ev.click_count == 1) => e.follow(target, cx),
                    hit => {
                        e.pressed_link = match hit {
                            Some(Hit::Link(target)) if reading && (ev.modifiers.secondary() || ev.click_count == 1) => Some(target),
                            _ => None,
                        };
                        // Beside a picture is not on it: the line nearest.
                        let beside = e.beside_picture(x, y);
                        let p = match beside {
                            Some(Some(p)) => p,
                            Some(None) => return,
                            None => match e.pos_at(x, y, &e.cache.borrow()) {
                                Some(p) => p,
                                None => return,
                            },
                        };
                        let grain = match ev.click_count {
                            1 => Grain::Char,
                            2 => Grain::Word,
                            _ => Grain::Line,
                        };
                        let (a, b) = e.grain_range(p, grain);
                        if ev.modifiers.shift && grain == Grain::Char {
                            e.sel.head = e.held_to(e.sel.anchor, p);
                        } else {
                            e.sel = Sel { anchor: a, head: b };
                        }
                        e.drag = Some((grain, a, b));
                        e.preferred_x = None;
                        e.kick_blink(cx);
                        cx.notify();
                    }
                }
            });
            crate::widgets::swallow_click(window, cx);
        });
        let (hitbox, wide, ed) = (prepaint.hitbox.clone(), prepaint.wide.clone(), editor.clone());
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || ev.button != MouseButton::Right || !wide.is_hovered(window) {
                return;
            }
            let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y));
            let on_control = ed.read(cx).control_at(x, y).is_some_and(|c| matches!(c, cells::Control::RowGrip(_) | cells::Control::ColumnGrip(_)));
            if !on_control && !hitbox.is_hovered(window) {
                return;
            }
            cx.stop_propagation();
            ed.update(cx, |e, cx| {
                let at = e.right_press(x, y, window, cx);
                cx.emit(EditorEvent::Menu(ev.position, at));
                cx.notify();
            });
        });
        let ed = editor.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y));
            ed.update(cx, |e, cx| {
                if ev.pressed_button == Some(MouseButton::Left) && (e.drag_control(x, y, cx) || e.drag_picture(x, y, cx)) {
                    return;
                }
                if let (Some((grain, a, b)), Some(MouseButton::Left)) = (e.drag, ev.pressed_button) {
                    if let Some(p) = e.pos_at(x, y.max(0.), &e.cache.borrow()) {
                        let (pa, pb) = e.grain_range(p, grain);
                        let (anchor, head) = if pa < a { (b, pa) } else { (a, pb.max(b)) };
                        let head = e.held_to(anchor, head);
                        if e.sel != (Sel { anchor, head }) {
                            e.sel = Sel { anchor, head };
                            e.want_scroll = true;
                            if anchor != head {
                                e.pressed_link = None;
                            }
                            cx.notify();
                        }
                    }
                } else {
                    e.pointer_at = (x, y);
                    e.point_at_table(x, y, cx);
                    e.point_at_picture(x, y, cx);
                    let inside = window.mouse_position().x >= bounds.origin.x;
                    let pointer = match (e.control_at(x, y), e.picture_edge_at(x, y)) {
                        (Some(cells::Control::Edge(_)), _) => CursorStyle::ResizeColumn,
                        (_, Some(edge)) => edge.cursor(),
                        (Some(_), _) => CursorStyle::PointingHand,
                        (None, None) if e.pictures_change() && e.picture_at(x, y).is_some_and(|(_, l)| e.object_line(l)) => CursorStyle::Arrow,
                        (None, None) => match e.hit(x, y) {
                            Some(Hit::Link(_)) if inside => CursorStyle::PointingHand,
                            Some(Hit::Task(_)) if inside && !e.read_only => CursorStyle::PointingHand,
                            _ => CursorStyle::IBeam,
                        },
                    };
                    if pointer != e.pointer {
                        e.pointer = pointer;
                        cx.notify();
                    }
                }
            });
        });
        let ed = editor;
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                ed.update(cx, |e, cx| {
                    if e.drop_control(cx) || e.drop_picture(cx) {
                        return;
                    }
                    if let Some(target) = e.pressed_link.take() {
                        e.follow(target, cx);
                    }
                    // A line half in the page's edge fade takes a press on
                    // its half in sight. It is lifted clear here, not at
                    // the press, so the words do not move under a drag
                    // about to begin.
                    if e.drag.take().is_some() {
                        e.want_scroll = true;
                        cx.notify();
                    }
                });
            }
        });
        for frame in &mut prepaint.frames {
            frame.paint(window, cx);
        }
        // The rings around pieces, over the frames they stand around.
        window.with_content_mask(None, |window| {
            for b in rings {
                picture::paint_ring(b, theme.primary, window);
            }
            if let Some(c) = &info.picture {
                picture::paint_picture_edge(c, ox, oy, &theme, window);
            }
        });
        if info.picture.as_ref().is_some_and(|c| c.opacity < 1.) {
            window.request_animation_frame();
        }
    }
}

/// The selection takes something of `line` and is not a caret.
fn covers(sel: &Sel, line: usize) -> bool {
    let (a, b) = sel.range();
    a != b && a.line <= line && line <= b.line
}

impl Editor {
    /// What the drawing needs, read once.
    fn paint_info(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> PaintInfo {
        let (sa, _) = self.sel.range();
        let cache = self.cache.borrow();
        let caret = (self.sel.is_caret() && self.caret_shown()).then(|| self.caret_at(sa)).flatten().map(|c| Bounds::new(point(px(c.x), px(c.bar_y)), size(px(1.5), px(c.bar_h))));
        let cell = self.revealing().then(|| self.cell_of(self.sel.head)).flatten().and_then(|at| {
            let t = self.table(at.table)?;
            let b = cells::table_box(t, &cache)?;
            let line = t.line_of(at.row);
            let (x, w) = (*b.grid.xs.get(at.col)?, *b.grid.cols.get(at.col)?);
            Some(Bounds::new(point(px(b.left + x), px(cache.tops[line])), size(px(w), px(cache.entries.get(line)?.height + 1.))))
        });
        let (mut control_on, mut hover_row) = (None, None);
        if let Some(h) = self.hover {
            if let Some(t) = self.table(h.first) {
                let line = t.line_of(h.row);
                hover_row = cache.entries.get(line).map(|e| (cache.tops[line], cache.tops[line] + e.height));
            }
            let p = self.pointer_at;
            drop(cache);
            control_on = self.control_at(p.0, p.1);
            return self.finish_info(sa, caret, cell, control_on, hover_row);
        }
        drop(cache);
        self.finish_info(sa, caret, cell, control_on, hover_row)
    }

    fn finish_info(&self, _: Pos, caret: Option<Bounds<Pixels>>, cell: Option<Bounds<Pixels>>, on: Option<cells::Control>, hover_row: Option<(f32, f32)>) -> PaintInfo {
        let origin = point(self.origin.x, self.origin.y);
        let shift = |b: Bounds<Pixels>| Bounds::new(point(origin.x + b.origin.x, origin.y + b.origin.y), b.size);
        let cache = self.cache.borrow();
        let controls = self.hover.and_then(|h| {
            let table = cells::table_box(self.table(h.first)?, &cache)?;
            let opacity = (h.since.elapsed().as_secs_f32() / 0.12).min(1.);
            Some(Controls { table, row: hover_row?, col: h.col, on, opacity })
        });
        let drop_line = self.table_drag.as_ref().and_then(|d| match d {
            cells::TableDrag::Row { first, row, to, moved: true, .. } => {
                let t = self.table(*first)?;
                let b = cells::table_box(t, &cache)?;
                let line = t.line_of(*to);
                let y = if to > row { cache.tops[line] + cache.entries.get(line)?.height } else { cache.tops[line] };
                Some(Bounds::new(point(px(b.left), px(y - 1.)), size(px(b.right - b.left), px(2.))))
            }
            cells::TableDrag::Column { first, col, to, moved: true, .. } => {
                let b = cells::table_box(self.table(*first)?, &cache)?;
                let x = b.left + b.grid.xs.get(*to)? + if to > col { *b.grid.cols.get(*to)? } else { 0. };
                Some(Bounds::new(point(px(x - 1.), px(b.top)), size(px(2.), px(b.bottom - b.top))))
            }
            _ => None,
        });
        PaintInfo {
            focus: self.focus.clone(),
            pointer: self.pointer,
            cache: self.cache.clone(),
            blocks: self.analysis.code_blocks.iter().map(|b| (b.first, b.last, self.reader().base_inset(&self.analysis.lines[b.first]))).collect(),
            tables: self.analysis.tables.iter().map(|t| (t.first, t.last)).collect(),
            sel: self.sel,
            marked: self.marked,
            caret: caret.map(shift),
            find: self.find.clone(),
            pictures: self.pictures.clone(),
            cell,
            controls,
            picture: self.picture_paint(),
            drop_line,
        }
    }

    /// A right click: what it was on, with the selection put as a Mac's
    /// text puts it.
    fn right_press(&mut self, x: f32, y: f32, window: &mut Window, cx: &mut Context<Self>) -> MenuAt {
        if let Some(control) = self.control_at(x, y) {
            let hover = self.hover;
            let scope = match control {
                cells::Control::RowGrip(_) => Some(Scope::Row),
                cells::Control::ColumnGrip(_) => Some(Scope::Column),
                _ => None,
            };
            if let (Some(scope), Some(h)) = (scope, hover) {
                let cell = CellAt { table: h.first, row: h.row, col: h.col };
                return MenuAt { table: self.table_facts(cell).map(|f| (cell, scope, f)), ..Default::default() };
            }
        }
        // On a link nothing moves: the keyboard taken here would wake the
        // marks on the caret's line.
        let link = self.link_at(x, y);
        if link.is_some() {
            let file = link.as_ref().and_then(|l| l.url.as_deref()).and_then(|url| self.file_of(url));
            let layout = self.pos_at(x, y, &self.cache.borrow()).and_then(|p| self.layout_at(p.line, p.col));
            return MenuAt { link, file, layout, ..Default::default() };
        }
        // On a picture the picture is selected whole, and the menu is its
        // own and its file's, as its bar's is.
        if let Some((file, line)) = self.picture_at(x, y) {
            if self.object_line(line) {
                self.select_object(line, window, cx);
            }
            return self.piece_menu(line, Some(file).filter(|_| !self.is_web_card(line)));
        }
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        let place = self.pos_at(x, y, &self.cache.borrow());
        let issue = place.and_then(|p| self.issue_at(self.doc.offset_of(p)));
        if issue.is_some() {
            if let Some(p) = place {
                self.sel = Sel::caret(p);
                self.kick_blink(cx);
            }
        } else {
            self.select_for_menu(x, y, cx);
        }
        let table = place.and_then(|p| self.cell_of(p)).and_then(|c| Some((c, Scope::Cell, self.table_facts(c)?)));
        let blank = !self.locked() && self.doc.line(self.sel.head.line).trim().is_empty() && table.is_none();
        MenuAt { link: None, issue, table, blank, ..Default::default() }
    }

    /// A card of a page of the web stands on `line`: it has no file.
    fn is_web_card(&self, line: usize) -> bool {
        matches!(self.cache.borrow().entries.get(line).map(|e| &*e.row), Some(Row::Picture(p)) if p.web)
    }

    /// The file a link leads to, when it is one of the vault's and not a note.
    fn file_of(&self, url: &str) -> Option<PathBuf> {
        let pictures = self.pictures.borrow();
        onyx_core::vault::find_file(pictures.vault.as_deref(), &pictures.note, url).filter(|p| !onyx_core::vault::is_note(p))
    }

    /// The picture drawn under a point, when one is, and its line.
    pub(super) fn picture_at(&self, x: f32, y: f32) -> Option<(PathBuf, usize)> {
        let cache = self.cache.borrow();
        if cache.entries.is_empty() {
            return None;
        }
        let line = self.line_at(y, &cache);
        let (entry, top) = (cache.entries.get(line)?, cache.tops[line]);
        let Row::Picture(p) = &*entry.row else { return None };
        let top = top + p.pad;
        (x >= p.x && x <= p.x + p.w && y >= top && y <= top + p.h).then(|| (p.path.clone(), line))
    }

    /// The pictures a layout met, read off the window's thread.
    fn read_pictures(&mut self, cx: &mut Context<Self>) {
        let wanted = std::mem::take(&mut self.pictures.borrow_mut().wanted);
        for path in wanted {
            if !self.pictures.borrow_mut().reading.insert(path.clone()) {
                continue;
            }
            cx.spawn(async move |this, cx| {
                let file = path.clone();
                let image = cx.background_spawn(async move { crate::layout::read_picture(&file) }).await;
                let _ = this.update(cx, |e, cx| {
                    if let Some(image) = image {
                        e.pictures.borrow_mut().drawn.insert(path, image);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Lets the pictures go, as a note is closed.
    pub fn let_pictures_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut store = self.pictures.borrow_mut();
        for (_, image) in store.drawn.drain() {
            cx.drop_image(image, Some(window));
        }
        drop(store);
        for (_, frame) in self.embeds.drain() {
            frame.update(cx, |m, cx| m.let_go(window, cx));
        }
    }
}

impl Editor {
    /// A place, past the hidden marks it stands before.
    fn past_hidden(&self, p: Pos) -> Pos {
        let mut hidden = self.hidden_on(p.line);
        hidden.sort_by_key(|r| r.start);
        let col = hidden.iter().fold(p.col, |col, r| if r.start <= col && col < r.end { r.end } else { col });
        Pos { line: p.line, col }
    }

    /// What a press selects at `p`: the place, the word, the line.
    fn grain_range(&self, p: Pos, grain: Grain) -> (Pos, Pos) {
        // A press on a word's first letter maps to the end of what stands
        // before it, which may be a hidden mark. The word is what shows.
        let p = if grain == Grain::Word { self.past_hidden(p) } else { p };
        match grain {
            Grain::Char => (p, p),
            Grain::Word => {
                let w = self.doc.word_at(p);
                if w.start == w.end {
                    (p, self.doc.next_grapheme(p))
                } else {
                    (Pos::new(p.line, w.start), Pos::new(p.line, w.end))
                }
            }
            Grain::Line => match self.cell_of(p) {
                Some(cell) => self.cell_span(cell),
                None => (Pos::new(p.line, 0), if p.line + 1 < self.doc.line_count() { Pos::new(p.line + 1, 0) } else { self.doc.end() }),
            },
        }
    }
}
