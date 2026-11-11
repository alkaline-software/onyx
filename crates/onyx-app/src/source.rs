//! The note's source, in a pane beside it: the file's text as it is
//! written, in a code editor as Emaki's file editor is (the toolkit's
//! `Editor` in markdown, its colors, its line numbers, its find, Sublime
//! Text's keys in `sublime.rs`). The pane and the note are two views of
//! one text: a change in either is in the other as it is made. The note's
//! editor holds the text that is written to the disk; the pane follows it
//! by the editor's count of changes (`Editor::version`), and what is typed
//! in the pane goes to the editor as one change of its own
//! (`Editor::apply_edits`), so the note is written as when it is typed in.
//! `docs/editor.md`, The source.

use std::ops::Range;
use std::rc::Rc;

use gpui::{div, px, App, AppContext as _, Context, InteractiveElement as _, ParentElement as _, Styled as _, Entity, FocusHandle, Focusable, HighlightStyle, IntoElement, Pixels, Point, Render, Subscription, UnderlineStyle, WeakEntity, Window};
use gpui_component::input::{AutoPairs, EditorState, InputEvent, TabSize, TextDecoration, TextDecorationCollection};
use gpui_component::ActiveTheme as _;
use onyx_core::check::Kind;

use crate::editor::Editor;
use crate::editor::Look;

/// What a right click in the pane does: the window's menu, at a point.
pub(crate) type MenuAt = Rc<dyn Fn(Point<Pixels>, &mut Window, &mut App)>;

pub(crate) struct SourcePane {
    pub(crate) state: Entity<EditorState>,
    editor: WeakEntity<Editor>,
    look: Look,
    /// The editor's count of changes the pane last took.
    synced: u64,
    /// The editor's spelling marks under the pane's words.
    marks: Option<TextDecorationCollection>,
    menu: MenuAt,
    /// Where the pane was last laid out, for the scroll router.
    bounds: Rc<std::cell::Cell<gpui::Bounds<Pixels>>>,
    _subs: Vec<Subscription>,
}

impl SourcePane {
    pub(crate) fn new(editor: &Entity<Editor>, look: Look, menu: impl Fn(&Entity<EditorState>) -> MenuAt, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (text, synced) = {
            let e = editor.read(cx);
            (e.text(), e.version())
        };
        let (columns, tabs) = indent_unit(&text);
        let step = if tabs { "\t".to_string() } else { " ".repeat(columns) };
        let state = cx.new(|cx| {
            let mut state = EditorState::new(window, cx).language("markdown").line_number(true).soft_wrap(true).tab_size(TabSize { tab_size: columns, hard_tabs: tabs });
            state.set_next_line_indent(move |before, _| Some(next_indent(before, &step)));
            state.set_auto_pairs(AutoPairs { closing: pairs("{}[]()<>"), surround: pairs("()[]``__**{}''\"\"<>~~$$") });
            state.set_value(text, window, cx);
            state
        });
        let menu = menu(&state);
        let mut subs = Vec::new();
        // What is typed here goes to the note.
        subs.push(cx.subscribe(&state, |me: &mut Self, _, ev: &InputEvent, cx| {
            if let InputEvent::Change = ev {
                me.to_note(cx);
            }
        }));
        // What the note's editor changed comes here, and its marks.
        subs.push(cx.observe_in(editor, window, |me: &mut Self, _, window, cx| me.from_note(window, cx)));
        let mut me = Self { state, editor: editor.downgrade(), look, synced, marks: None, menu, bounds: Rc::default(), _subs: subs };
        me.set_marks(cx);
        me
    }

    pub(crate) fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        self.look = look;
        cx.notify();
    }

    /// The pane's text taken into the note, as one change of the editor's.
    fn to_note(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.upgrade() else { return };
        let now = self.state.read(cx).value().to_string();
        let was = editor.read(cx).text();
        if let Some((range, with)) = diff(&was, &now) {
            editor.update(cx, |e, cx| e.apply_edits(vec![(range, with)], cx));
        }
        self.synced = editor.read(cx).version();
        self.set_marks(cx);
    }

    /// The note's text taken into the pane, where it changed there, the
    /// pane's selection kept on the words it was on.
    fn from_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.upgrade() else { return };
        let version = editor.read(cx).version();
        if version != self.synced {
            self.synced = version;
            let now = editor.read(cx).text();
            let (was, sel) = {
                let s = self.state.read(cx);
                (s.value().to_string(), s.selected_range())
            };
            if let Some((range, with)) = diff(&was, &now) {
                let keep = carry(&sel, &range, with.len());
                // The view stays where it is: the change is the note's,
                // and the pane's caret is not where it was made.
                self.state.update(cx, |s, cx| {
                    let view = s.scroll_offset();
                    s.replace_bytes(range, &with, keep.end, window, cx);
                    s.set_selected_range(keep, cx);
                    s.set_scroll_offset(view, cx);
                });
            }
        }
        self.set_marks(cx);
    }

    /// The note's spelling and grammar marks, while the two hold the same
    /// text, under the same words here: a wavy line, red for a spelling,
    /// blue for the grammar, as the note draws them.
    fn set_marks(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.upgrade() else { return };
        let (danger, info) = (cx.theme().danger, cx.theme().info);
        let marks: Vec<TextDecoration> = {
            let e = editor.read(cx);
            if e.version() != self.synced {
                Vec::new()
            } else {
                e.shown_issues()
                    .iter()
                    .filter(|i| i.kind == Kind::Spelling || !i.fixes.is_empty())
                    .map(|i| {
                        let color = if i.kind == Kind::Spelling { danger } else { info };
                        TextDecoration::new(i.range.clone(), HighlightStyle { underline: Some(UnderlineStyle { thickness: px(1.), color: Some(color), wavy: true }), ..Default::default() })
                    })
                    .collect()
            }
        };
        match &self.marks {
            Some(set) => {
                set.set(marks, cx);
            }
            None if marks.is_empty() => {}
            None => self.marks = Some(self.state.update(cx, |s, cx| s.create_decorations_collection(marks, cx))),
        }
    }

    /// The byte the caret is at, for the note to put its caret there.
    pub(crate) fn caret(&self, cx: &App) -> usize {
        self.state.read(cx).cursor()
    }

    /// The caret put at a byte of the text, and the view brought to it.
    pub(crate) fn put_caret(&mut self, at: usize, cx: &mut Context<Self>) {
        self.state.update(cx, |s, cx| {
            let at = at.min(s.value().len());
            // A selection set scrolls the view to it.
            s.set_selected_range(at..at, cx);
        });
    }
}

impl Focusable for SourcePane {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }
}

impl Render for SourcePane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = self.menu.clone();
        let editor = gpui_component::input::Editor::new(&self.state)
            .appearance(false)
            .bordered(false)
            .h_full()
            .font_family(self.look.mono_family.clone())
            .text_size(px(self.look.mono_px))
            .on_secondary_click(move |at, window, cx| menu(at, window, cx))
            .into_any_element();
        // A gesture begun over the pane is the pane's, momentum included,
        // wherever the pointer is carried (`scroll.rs`): the editor scrolls
        // by its own offset, so the router hands it the wheel.
        if let Some(router) = cx.try_global::<crate::scroll::Router>() {
            let state = self.state.downgrade();
            router.0.name_own(self.bounds.clone(), Rc::new(move |e, window, cx| {
                if let Some(state) = state.upgrade() {
                    state.update(cx, |s, cx| s.scroll_wheel(e, window, cx));
                }
            }));
        }
        let bounds = self.bounds.clone();
        // Escape with something selected leaves the caret where it is, as
        // in Emaki's editor; with nothing, it is the window's.
        div()
            .size_full()
            .child(gpui::canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {}).absolute().size_full())
            .on_action(cx.listener(|this, _: &gpui_component::input::Escape, _, cx| {
                let had = this.state.update(cx, |s, cx| {
                    let caret = s.cursor();
                    let had = !s.selected_range().is_empty();
                    if had {
                        s.set_selected_range(caret..caret, cx);
                    }
                    had
                });
                if !had {
                    cx.propagate();
                }
            }))
            .child(self.sublime_wrap(editor, cx))
    }
}

/// What changed from `was` to `now`, as one stretch of `was` and what
/// stands there in `now`, on characters' starts; none when they are the
/// same. A keystroke is a stretch of its own, a paste or a list put right
/// one stretch round all it touched.
pub(crate) fn diff(was: &str, now: &str) -> Option<(Range<usize>, String)> {
    if was == now {
        return None;
    }
    let (a, b) = (was.as_bytes(), now.as_bytes());
    let mut head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !was.is_char_boundary(head) || !now.is_char_boundary(head) {
        head -= 1;
    }
    let most = a.len().min(b.len()) - head;
    let mut tail = a.iter().rev().zip(b.iter().rev()).take(most).take_while(|(x, y)| x == y).count();
    while !was.is_char_boundary(a.len() - tail) || !now.is_char_boundary(b.len() - tail) {
        tail -= 1;
    }
    Some((head..a.len() - tail, now[head..b.len() - tail].to_string()))
}

/// A selection carried over a change of `range` to `len` bytes: before it,
/// where it was; after it, moved by as much; inside it, to its end.
fn carry(sel: &Range<usize>, range: &Range<usize>, len: usize) -> Range<usize> {
    let at = |o: usize| {
        if o <= range.start {
            o
        } else if o >= range.end {
            o - range.end + range.start + len
        } else {
            range.start + len
        }
    };
    at(sel.start)..at(sel.end)
}

fn pairs(s: &str) -> Vec<(char, char)> {
    s.chars().collect::<Vec<_>>().chunks(2).map(|p| (p[0], p[1])).collect()
}

/// The indent of the line Enter starts at the end of `before`: the line's
/// own, a step deeper after an opening bracket, as Emaki's editor has it.
fn next_indent(before: &str, unit: &str) -> String {
    let base: String = before.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    if before.trim_end().ends_with(['{', '[', '(']) { format!("{base}{unit}") } else { base }
}

/// One step of indent for a note: the smallest indent of two or more any
/// of its lines starts with, a tab if a line starts with one, else two
/// spaces, as a markdown list nests.
fn indent_unit(text: &str) -> (usize, bool) {
    let mut least: Option<usize> = None;
    for line in text.lines().take(2000) {
        if line.starts_with('\t') {
            return (4, true);
        }
        let spaces = line.len() - line.trim_start_matches(' ').len();
        if spaces >= 2 && !line.trim().is_empty() {
            least = Some(least.map_or(spaces, |n| n.min(spaces)));
        }
    }
    match least {
        Some(n) if n <= 8 => (n, false),
        _ => (2, false),
    }
}

#[cfg(test)]
mod tests {
    use super::{carry, diff, indent_unit, next_indent};

    fn apply(was: &str, (r, with): (std::ops::Range<usize>, String)) -> String {
        format!("{}{with}{}", &was[..r.start], &was[r.end..])
    }

    #[test]
    fn a_change_is_one_stretch_on_characters() {
        for (was, now) in [("abc", "abXc"), ("abc", "ac"), ("", "x"), ("x", ""), ("aaa", "aaaa"), ("日本語", "日本人語"), ("café", "cafe"), ("e\u{301}", "e"), ("一二", "一三二")] {
            let d = diff(was, now).unwrap();
            assert!(was.is_char_boundary(d.0.start) && was.is_char_boundary(d.0.end));
            assert_eq!(apply(was, d), now, "{was} to {now}");
        }
        assert!(diff("same", "same").is_none());
        assert_eq!(diff("abc", "abXc").unwrap(), (2..2, "X".to_string()));
    }

    #[test]
    fn a_selection_stays_on_its_words() {
        // Typed before it, it moves on; after it, it stays; over it, it
        // ends where the change does.
        assert_eq!(carry(&(5..7), &(1..1), 3), 8..10);
        assert_eq!(carry(&(5..7), &(9..9), 3), 5..7);
        assert_eq!(carry(&(5..7), &(4..6), 0), 4..5);
    }

    #[test]
    fn a_new_line_starts_at_the_note_s_indent() {
        assert_eq!(next_indent("  - item", "  "), "  ");
        assert_eq!(next_indent("```js function() {", "  "), "  ");
        assert_eq!(indent_unit("- a\n    - b\n"), (4, false));
        assert_eq!(indent_unit("text\n"), (2, false));
        assert_eq!(indent_unit("\tx\n"), (4, true));
    }
}
