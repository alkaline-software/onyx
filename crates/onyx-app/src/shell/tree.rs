//! The notes' tree as a list to work in, after Emaki's files panel
//! (`emaki/docs/panels.md`, Choosing rows, Moving, and the tree's keys):
//! rows chosen by a click, ⌘ and a click, ⇧ and a click, or a rectangle
//! dragged over them from the room around them; dragged onto a folder,
//! or cut and pasted, to move them; and keys to go from row to row.
//!
//! A move keeps every link going where it went (`vault::move_into`,
//! `vault::relink_moves`): the notes moved keep their own links, the
//! others' links to them follow, and an open note is changed through its
//! editor, from what it holds, as one step of undo.

use std::path::{Path, PathBuf};

use gpui::*;
use gpui_component::ActiveTheme as _;

use onyx_core::vault;

use super::{Shell, TREE_ROW_H};
use crate::i18n::{t, tf};
use crate::menu::Entry;

actions!(onyx_tree, [TreeUp, TreeDown, TreeExtendUp, TreeExtendDown, TreeLeft, TreeRight, TreeOpen, TreeTrash, TreeSelectAll, TreeCut, TreePaste, TreeLetGo]);

/// The keys' context, the tree's while the keyboard is in it.
pub const TREE_CONTEXT: &str = "SideTree";

/// The tree's keys, Finder's and Emaki's.
pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(TREE_CONTEXT);
    vec![
        KeyBinding::new("up", TreeUp, c),
        KeyBinding::new("down", TreeDown, c),
        KeyBinding::new("shift-up", TreeExtendUp, c),
        KeyBinding::new("shift-down", TreeExtendDown, c),
        KeyBinding::new("left", TreeLeft, c),
        KeyBinding::new("right", TreeRight, c),
        KeyBinding::new("enter", TreeOpen, c),
        KeyBinding::new("secondary-backspace", TreeTrash, c),
        KeyBinding::new("secondary-a", TreeSelectAll, c),
        KeyBinding::new("secondary-x", TreeCut, c),
        KeyBinding::new("secondary-v", TreePaste, c),
        KeyBinding::new("escape", TreeLetGo, c),
    ]
}

/// Rows on their way to a folder: what moves, and how it is named while
/// it moves.
#[derive(Clone)]
pub struct TreeDrag(pub Vec<PathBuf>, pub SharedString);

impl Render for TreeDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px(px(10.))
            .h(px(26.))
            .flex()
            .items_center()
            .rounded(px(7.))
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow_md()
            .text_size(px(12.5))
            .text_color(theme.foreground)
            .child(self.1.clone())
    }
}

/// A rectangle being dragged over the rows: where it began and where the
/// pointer is, in the list's own height from its top, and whether the
/// press has moved far enough to be a drag and not a click.
#[derive(Clone, Copy)]
pub(crate) struct Band {
    pub from: f32,
    pub to: f32,
    pub moved: bool,
}

/// What the tree has chosen and is doing.
#[derive(Default)]
pub(crate) struct Choice {
    /// The rows chosen together.
    pub marked: Vec<PathBuf>,
    /// The row a ⇧ reaches from, and the row the keys stand on.
    pub anchor: Option<PathBuf>,
    pub cursor: Option<PathBuf>,
    /// The rows cut, drawn faint until a paste moves them.
    pub cut: Vec<PathBuf>,
    pub band: Option<Band>,
}

impl Shell {
    /// The rows the tree shows, top to bottom, a folder before what it
    /// holds and a folded one without it.
    pub(crate) fn visible_rows(&self) -> Vec<(PathBuf, bool)> {
        fn walk(entries: &[vault::Entry], folded: &std::collections::HashSet<PathBuf>, out: &mut Vec<(PathBuf, bool)>) {
            for e in entries {
                out.push((e.path.clone(), e.dir));
                if e.dir && !folded.contains(&e.path) {
                    walk(&e.children, folded, out);
                }
            }
        }
        let mut out = Vec::new();
        if let Some(v) = &self.vault {
            walk(&v.tree.children, &v.folded, &mut out);
        }
        out
    }

    /// Whether a row is lit: chosen, or with none chosen the note showing.
    pub(crate) fn row_lit(&self, path: &Path) -> bool {
        if self.choice.marked.is_empty() { self.shown.as_deref() == Some(path) } else { self.choice.marked.iter().any(|m| m == path) }
    }

    /// A press on a row: alone, it chooses the row and opens it (a note)
    /// or folds it (a folder); with ⌘ it adds the row to those chosen or
    /// takes it out; with ⇧ it chooses from the row last clicked to this.
    /// The keyboard stays in the tree.
    pub(crate) fn tree_click(&mut self, path: PathBuf, dir: bool, modifiers: Modifiers, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        if modifiers.shift {
            let from = self.choice.anchor.as_ref().and_then(|a| rows.iter().position(|(p, _)| p == a)).unwrap_or(0);
            if let Some(to) = rows.iter().position(|(p, _)| *p == path) {
                self.choice.marked = rows[from.min(to)..=from.max(to)].iter().map(|(p, _)| p.clone()).collect();
            }
            self.choice.cursor = Some(path);
        } else if modifiers.secondary() {
            match self.choice.marked.iter().position(|p| *p == path) {
                Some(ix) => {
                    self.choice.marked.remove(ix);
                }
                None => {
                    // A note showing that was lit alone counts as chosen.
                    if self.choice.marked.is_empty() {
                        if let Some(shown) = self.shown.clone().filter(|s| rows.iter().any(|(p, _)| p == s) && *s != path) {
                            self.choice.marked.push(shown);
                        }
                    }
                    self.choice.marked.push(path.clone());
                }
            }
            self.choice.anchor = Some(path.clone());
            self.choice.cursor = Some(path);
        } else {
            self.choice.marked = vec![path.clone()];
            self.choice.anchor = Some(path.clone());
            self.choice.cursor = Some(path.clone());
            if dir {
                self.toggle_fold(&path, window);
            } else {
                self.open_note(path, window, cx);
            }
        }
        window.focus(&self.tree_focus, cx);
        cx.notify();
    }

    fn toggle_fold(&mut self, path: &Path, window: &mut Window) {
        if let Some(v) = self.vault.as_mut() {
            if !v.folded.remove(path) {
                v.folded.insert(path.to_path_buf());
            }
        }
        self.save_ui(window);
    }

    // -- the rectangle ------------------------------------------------------

    /// A press in the room around the rows: the start of a rectangle, and
    /// with no ⌘ or ⇧ the rows chosen let go.
    pub(crate) fn band_start(&mut self, y: f32, modifiers: Modifiers, window: &mut Window, cx: &mut Context<Self>) {
        if !modifiers.secondary() && !modifiers.shift {
            self.choice.marked.clear();
        }
        self.choice.band = Some(Band { from: y, to: y, moved: false });
        window.focus(&self.tree_focus, cx);
        cx.notify();
    }

    /// The rectangle drawn on: every row it reaches is chosen. Every row is
    /// one height and the list begins at the scroller's top, so the rows
    /// under it are found by number.
    pub(crate) fn band_to(&mut self, y: f32, cx: &mut Context<Self>) {
        let Some(band) = self.choice.band.as_mut() else { return };
        band.to = y;
        band.moved |= (band.to - band.from).abs() > 3.;
        if !band.moved {
            return;
        }
        let (top, bottom) = (band.from.min(band.to).max(0.), band.from.max(band.to).max(0.));
        let rows = self.visible_rows();
        let (first, last) = ((top / TREE_ROW_H).floor() as usize, (bottom / TREE_ROW_H).floor() as usize);
        self.choice.marked = rows.iter().skip(first).take(last.saturating_sub(first) + 1).map(|(p, _)| p.clone()).collect();
        self.choice.anchor = self.choice.marked.first().cloned();
        self.choice.cursor = self.choice.marked.last().cloned();
        cx.notify();
    }

    /// A point of the window as a height in the list, from its top.
    pub(crate) fn list_y(&self, at: Point<Pixels>) -> f32 {
        f32::from(at.y - self.side_scroll.bounds().origin.y - self.side_scroll.offset().y) - super::LIST_TOP
    }

    /// The rectangle while it is drawn, over the list where the list
    /// scrolls it.
    pub(crate) fn render_band(&self, theme: &gpui_component::Theme) -> Option<AnyElement> {
        let band = self.choice.band.filter(|b| b.moved)?;
        let shift = f32::from(self.side_scroll.offset().y) + super::LIST_TOP;
        let (top, bottom) = (band.from.min(band.to) + shift, band.from.max(band.to) + shift);
        Some(div().absolute().left(px(6.)).right(px(6.)).top(px(top)).h(px((bottom - top).max(1.))).rounded(px(4.)).bg(theme.primary.opacity(0.12)).border_1().border_color(theme.primary.opacity(0.5)).into_any_element())
    }

    pub(crate) fn band_end(&mut self, cx: &mut Context<Self>) {
        if self.choice.band.take().is_some() {
            cx.notify();
        }
    }

    // -- the keys -----------------------------------------------------------

    /// ↑ and ↓: the row above or below, chosen alone; with ⇧ the choice
    /// reaches to it from where it began.
    fn tree_step(&mut self, down: bool, extend: bool, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        if rows.is_empty() {
            return;
        }
        let at = self.choice.cursor.as_ref().or(self.shown.as_ref()).and_then(|c| rows.iter().position(|(p, _)| p == c));
        let next = match at {
            Some(i) if down => (i + 1).min(rows.len() - 1),
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        let path = rows[next].0.clone();
        if extend {
            let from = self.choice.anchor.as_ref().and_then(|a| rows.iter().position(|(p, _)| p == a)).unwrap_or(next);
            self.choice.marked = rows[from.min(next)..=from.max(next)].iter().map(|(p, _)| p.clone()).collect();
        } else {
            self.choice.marked = vec![path.clone()];
            self.choice.anchor = Some(path.clone());
        }
        self.choice.cursor = Some(path);
        self.scroll_to_cursor(next);
        cx.notify();
    }

    /// The row the keys stand on kept in sight.
    fn scroll_to_cursor(&self, ix: usize) {
        let view = f32::from(self.side_scroll.bounds().size.height);
        if view <= 0. {
            return;
        }
        let top = -f32::from(self.side_scroll.offset().y);
        let (y, h) = (ix as f32 * TREE_ROW_H + super::LIST_TOP, TREE_ROW_H);
        let to = if y < top { y } else if y + h > top + view - 12. { y + h - view + 12. } else { return };
        self.side_scroll.set_offset(point(px(0.), px(-to.max(0.))));
    }

    /// → opens a folder, or goes into one open; ← closes it, or goes up to
    /// the folder the row is in.
    fn tree_side(&mut self, right: bool, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.visible_rows();
        let Some(ix) = self.choice.cursor.as_ref().and_then(|c| rows.iter().position(|(p, _)| p == c)) else { return };
        let (path, dir) = rows[ix].clone();
        let open = dir && !self.vault.as_ref().is_some_and(|v| v.folded.contains(&path));
        let go = |s: &mut Self, to: PathBuf| {
            s.choice.marked = vec![to.clone()];
            s.choice.anchor = Some(to.clone());
            s.choice.cursor = Some(to);
        };
        match (right, dir, open) {
            (true, true, false) | (false, true, true) => self.toggle_fold(&path, window),
            (true, true, true) => {
                if let Some((next, _)) = rows.get(ix + 1).filter(|(p, _)| p.parent() == Some(path.as_path())) {
                    go(self, next.clone());
                }
            }
            (false, _, _) => {
                if let Some(up) = path.parent().filter(|p| rows.iter().any(|(r, _)| r == p)) {
                    go(self, up.to_path_buf());
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// ↩: the note the keys stand on opens; a folder folds or opens.
    fn tree_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.choice.cursor.clone().or_else(|| self.choice.marked.first().cloned()) else { return };
        if path.is_dir() {
            self.toggle_fold(&path, window);
        } else {
            self.open_note(path, window, cx);
            window.focus(&self.tree_focus, cx);
        }
        cx.notify();
    }

    /// What the tree's keys and menus act on: the rows chosen, or the note
    /// showing.
    fn tree_targets(&self) -> Vec<PathBuf> {
        if self.choice.marked.is_empty() { self.shown.iter().filter(|s| vault::is_note(s)).cloned().collect() } else { self.choice.marked.clone() }
    }

    /// The rows chosen to the trash; a folder chosen with what it holds is
    /// one.
    pub(crate) fn trash_chosen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let all = self.tree_targets();
        let tops: Vec<PathBuf> = all.iter().filter(|p| !all.iter().any(|o| o != *p && p.starts_with(o))).cloned().collect();
        for p in tops {
            self.trash(p, window, cx);
        }
        self.choice.marked.clear();
        self.choice.cut.clear();
        cx.notify();
    }

    /// ⌘V: what was cut moved into the folder the keys stand on, or the
    /// folder of the row they stand on, or the vault's top.
    fn tree_paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.choice.cut.is_empty() {
            return;
        }
        let Some(root) = self.vault_root().map(Path::to_path_buf) else { return };
        let into = match self.choice.cursor.clone() {
            Some(p) if p.is_dir() => p,
            Some(p) => p.parent().map(Path::to_path_buf).unwrap_or(root),
            None => root,
        };
        let items = std::mem::take(&mut self.choice.cut);
        self.move_items(items, into, window, cx);
    }

    pub(crate) fn register_tree_keys(&self, el: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        el.key_context(TREE_CONTEXT)
            .track_focus(&self.tree_focus)
            .on_action(cx.listener(|this, _: &TreeUp, _, cx| this.tree_step(false, false, cx)))
            .on_action(cx.listener(|this, _: &TreeDown, _, cx| this.tree_step(true, false, cx)))
            .on_action(cx.listener(|this, _: &TreeExtendUp, _, cx| this.tree_step(false, true, cx)))
            .on_action(cx.listener(|this, _: &TreeExtendDown, _, cx| this.tree_step(true, true, cx)))
            .on_action(cx.listener(|this, _: &TreeLeft, window, cx| this.tree_side(false, window, cx)))
            .on_action(cx.listener(|this, _: &TreeRight, window, cx| this.tree_side(true, window, cx)))
            .on_action(cx.listener(|this, _: &TreeOpen, window, cx| this.tree_open(window, cx)))
            .on_action(cx.listener(|this, _: &TreeTrash, window, cx| this.trash_chosen(window, cx)))
            .on_action(cx.listener(|this, _: &TreeSelectAll, _, cx| {
                this.choice.marked = this.visible_rows().into_iter().map(|(p, _)| p).collect();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TreeCut, _, cx| {
                this.choice.cut = this.tree_targets();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TreePaste, window, cx| this.tree_paste(window, cx)))
            .on_action(cx.listener(|this, _: &TreeLetGo, _, cx| {
                this.choice.marked.clear();
                this.choice.cut.clear();
                cx.notify();
            }))
    }

    // -- moving -------------------------------------------------------------

    /// What a drag of a row carries: the rows chosen, when it is one of
    /// them, else the row alone.
    pub(crate) fn drag_of(&self, path: &Path) -> TreeDrag {
        let items: Vec<PathBuf> = if self.choice.marked.iter().any(|m| m == path) { self.choice.marked.clone() } else { vec![path.to_path_buf()] };
        let name: SharedString = if items.len() == 1 { path.file_name().map(|n| vault::title(Path::new(n))).unwrap_or_default().into() } else { crate::i18n::count(items.len(), "{n} item", "{n} items").into() };
        TreeDrag(items, name)
    }

    /// Rows moved into a folder, and every link with them (`vault::move_into`):
    /// an open note's links changed through its editor first, from what it
    /// holds; what knows a moved thing by its path follows (`follow_moves`).
    pub(crate) fn move_items(&mut self, items: Vec<PathBuf>, into: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault_root().map(Path::to_path_buf) else { return };
        let planned = match vault::planned_moves(&items, &into) {
            Ok(m) if m.is_empty() => return,
            Ok(m) => m,
            Err(e) => {
                self.say(tf("Nothing was moved: {why}", &[("why", &e)]), cx);
                return;
            }
        };
        // Each open note's changes, from its editor, while the files are
        // still where they were.
        let open: Vec<(usize, PathBuf)> = self.tabs.iter().enumerate().filter(|(_, t)| t.asset.is_none()).map(|(i, t)| (i, t.path.clone())).collect();
        let edits: Vec<(usize, Vec<(std::ops::Range<usize>, String)>)> = open
            .iter()
            .map(|(i, note)| {
                let text = self.tabs[*i].editor.read(cx).text();
                (*i, vault::relink_moves(&root, note, &vault::moved_path(note, &planned), &text, &planned))
            })
            .filter(|(_, e)| !e.is_empty())
            .collect();
        let open_paths: Vec<PathBuf> = open.into_iter().map(|(_, p)| p).collect();
        match vault::move_into(&root, &items, &into, &open_paths) {
            Ok(moves) => {
                for (i, e) in edits {
                    self.tabs[i].editor.update(cx, |ed, cx| ed.apply_edits(e, cx));
                }
                // The folder moved into opens, so what moved is in sight.
                if let Some(v) = self.vault.as_mut() {
                    v.folded.remove(&into);
                }
                self.choice.marked = moves.iter().map(|(_, to)| to.clone()).collect();
                self.follow_moves(&moves, window, cx);
            }
            Err(e) => self.say(tf("Nothing was moved: {why}", &[("why", &e)]), cx),
        }
        cx.notify();
    }

    /// Everything that knows an entry by its path follows it to where it
    /// went, a rename or a move: an open note's tab (its editor made to
    /// listen by the new path, its pictures looked for from there), the
    /// note shown, the tabs closed, the folders folded, the rows chosen.
    pub(crate) fn follow_moves(&mut self, moves: &[(PathBuf, PathBuf)], window: &mut Window, cx: &mut Context<Self>) {
        let moved = |p: &Path| {
            let to = vault::moved_path(p, moves);
            (to != p).then_some(to)
        };
        let root = self.vault_root().map(Path::to_path_buf);
        for ix in 0..self.tabs.len() {
            let Some(to) = moved(&self.tabs[ix].path) else { continue };
            let sub = Self::listen(&self.tabs[ix].editor.clone(), to.clone(), cx);
            if self.shown.as_ref() == Some(&self.tabs[ix].path) {
                self.shown = Some(to.clone());
            }
            let name = Self::tab_title(&to);
            self.tabs[ix].title.update(cx, |f, cx| {
                if f.value() != name {
                    f.set_value(name, window, cx);
                }
            });
            let (place, root) = (to.clone(), root.clone());
            self.tabs[ix].editor.update(cx, |e, cx| e.set_place(place, root, cx));
            self.tabs[ix].path = to;
            self.tabs[ix]._sub = sub;
        }
        if let Some(to) = self.last_shown.as_deref().and_then(moved) {
            self.last_shown = Some(to);
        }
        for p in &mut self.closed_tabs {
            if let Some(to) = moved(p) {
                *p = to;
            }
        }
        for p in self.choice.marked.iter_mut().chain(self.choice.anchor.iter_mut()).chain(self.choice.cursor.iter_mut()) {
            if let Some(to) = moved(p) {
                *p = to;
            }
        }
        if let Some(v) = self.vault.as_mut() {
            v.folded = v.folded.iter().map(|f| vault::moved_path(f, moves)).collect();
        }
        self.sort_tabs();
        self.reread_tree(cx);
        self.save_ui(window);
    }

    /// The menu of several rows chosen together: what can be asked of
    /// them all.
    pub(crate) fn show_marked_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let n = self.choice.marked.len();
        let entries = vec![
            Entry::choice("cut", "icons/scissors.svg", t("Cut")),
            Entry::choice("copy-paths", "icons/link.svg", t("Copy Paths")),
            Entry::Rule,
            Entry::choice("trash", "icons/trash.svg", crate::i18n::count(n, "Move {n} Item to Trash", "Move {n} Items to Trash")),
        ];
        self.menus.show("marked", at, entries);
        cx.notify();
    }

    pub(crate) fn marked_pick(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "cut" => self.choice.cut = self.choice.marked.clone(),
            "copy-paths" => crate::widgets::to_clipboard(self.choice.marked.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>().join("\n"), cx),
            "trash" => self.trash_chosen(window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// A drag let go over a folder (or a note, which means its folder, or
    /// the room around the rows, which means the vault's top).
    pub(crate) fn tree_drop(&mut self, drag: &TreeDrag, into: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.move_items(drag.0.clone(), into, window, cx);
    }
}
