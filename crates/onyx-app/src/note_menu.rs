//! A note's menu, the right click in its words, for whichever window
//! holds the note: the main window's tabs and source pane (`shell.rs`) and
//! the menu bar note (`menubar.rs`). What it offers goes by what was
//! pressed (`MenuAt`): a piece's link and how it shows, a picture's size
//! and place, a file's choices, a link's, a marked word's corrections, the
//! words' Cut, Copy and Paste, a table's rows and columns, Delete for a
//! piece. What only the note's editor does is done here; what the window
//! does (a link followed, a file opened, Export, a word learned for every
//! note) is handed back as `Asked`.

use std::path::PathBuf;

use gpui::*;

use crate::editor::{CellAt, Editor, LayoutAt, MenuAt, MenuIssue, MenuLink, Scope, TableFacts};
use crate::i18n::t;
use crate::menu::{Entry, Tool};

/// What the menu was opened on, kept until a choice is made.
#[derive(Default)]
pub(crate) struct NoteMenu {
    editor: Option<Entity<Editor>>,
    /// Where the keyboard goes for Cut, Copy and Paste: the note's
    /// editor, a frame's words, the source pane's field.
    focus: Option<FocusHandle>,
    picture: Option<usize>,
    link: Option<MenuLink>,
    issue: Option<MenuIssue>,
    cell: Option<CellAt>,
    layout: Option<LayoutAt>,
    remove: Option<usize>,
}

/// A choice the window carries out.
pub(crate) enum Asked {
    Done,
    /// A link followed: an address or a note's name.
    Link(MenuLink),
    /// A file's choice (`rename`, `open`, `open-app`, `reveal`,
    /// `copy-path`), for the file the menu was opened on.
    File(String),
    Export,
    Print,
    /// A marked word learned, or let be.
    Learn(MenuIssue),
    Ignore(MenuIssue),
}

impl NoteMenu {
    /// A frame's words: copied, never changed, and only the frame's.
    pub(crate) fn for_frame(&mut self, editor: Entity<Editor>, focus: FocusHandle, selection: bool) -> Vec<Entry> {
        *self = Self { editor: Some(editor), focus: Some(focus), ..Self::default() };
        crate::menu::edit_entries(selection, false, true)
    }

    /// The source pane's field: on a marked word the corrections are the
    /// menu, and nothing else.
    pub(crate) fn for_source(&mut self, editor: Entity<Editor>, focus: FocusHandle, issue: Option<MenuIssue>, selection: bool, clipboard: bool) -> Vec<Entry> {
        let items = match &issue {
            Some(issue) => issue_entries(issue),
            None => crate::menu::edit_entries(selection, clipboard, false),
        };
        *self = Self { editor: Some(editor), focus: Some(focus), issue, ..Self::default() };
        items
    }

    /// The menu for what was pressed in a note, and the file it was on,
    /// if any, which the window keeps for a file's choices. `note` is
    /// whether the note can be exported and printed from here.
    pub(crate) fn entries(&mut self, editor: Entity<Editor>, what: MenuAt, note: bool, cx: &App) -> (Vec<Entry>, Option<PathBuf>) {
        if let Some(frame) = what.frame {
            return (self.for_frame(editor, frame.focus, frame.selection), None);
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
            rows.extend(layout_entries(l));
        }
        // A picture's size and place, as its bar has them, and the viewer.
        if let Some((_, across, asked)) = what.picture {
            use onyx_core::html::{Across, Width};
            let full = asked == Some(Width::Share(100));
            // The viewer is a picture's; a file has its frame.
            if !what.file.as_deref().is_some_and(crate::layout::is_framed) {
                items.push(Entry::choice("pic-view", "icons/maximize.svg", t("View")));
                items.push(Entry::Rule);
            }
            // On a phone what a note shows is the page's width whatever the line asks (`widgets::full_width`): nothing to size or place.
            let phone = crate::widgets::full_width();
            if !phone {
                rows.push(Entry::Icons { label: t("Size").into(), items: vec![Tool::new("pic-auto", "icons/image.svg", t("Original Size"), asked.is_none()), Tool::new("pic-full", "icons/picture-full.svg", t("Full Width"), full)] });
            }
            if !full && !phone {
                rows.push(Entry::Icons { label: t("Align").into(), items: vec![Tool::new("pic-left", "icons/align-left.svg", t("Align Left"), across == Across::Start), Tool::new("pic-center", "icons/align-center.svg", t("Align Center"), across == Across::Center), Tool::new("pic-right", "icons/align-right.svg", t("Align Right"), across == Across::End)] });
            }
        }
        if !rows.is_empty() {
            items.extend(rows);
            items.push(Entry::Rule);
        }
        if what.file.is_some() {
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
                items.extend(layout_entries(l));
            }
        } else if what.file.is_none() {
            if let Some(issue) = &what.issue {
                items.extend(issue_entries(issue));
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
                items.extend(table_entries(*scope, facts, at, col));
            } else if what.blank && !locked {
                items.push(Entry::Rule);
                items.push(Entry::choice("table-new", "icons/table.svg", t("Insert Table")));
            }
            if note && what.table.as_ref().is_none_or(|(_, s, _)| *s == Scope::Cell) {
                items.push(Entry::Rule);
                items.push(Entry::choice("canvas-export", "icons/file-export.svg", t("Export…")));
                items.push(Entry::choice("canvas-print", "icons/printer.svg", t("Print…")));
            }
        }
        // Delete, last, for a picture or a frame: out of the note, the
        // file left where it is.
        let remove = what.remove.filter(|_| !locked);
        if remove.is_some() {
            if !items.is_empty() && !matches!(items.last(), Some(Entry::Rule)) {
                items.push(Entry::Rule);
            }
            items.push(Entry::danger("remove", "icons/trash.svg", t("Delete")));
        }
        let file = what.file.clone();
        *self = Self {
            editor: Some(editor),
            focus: Some(focus),
            picture: what.picture.map(|(line, _, _)| line),
            link: what.link,
            issue: what.issue,
            cell: what.table.map(|(cell, _, _)| cell),
            layout: what.layout.filter(|_| !locked),
            remove,
        };
        (items, file)
    }

    /// A choice made: done here when it is the note's own, else handed
    /// back.
    pub(crate) fn pick(&mut self, id: &str, window: &mut Window, cx: &mut App) -> Asked {
        let editor = self.editor.clone();
        match id {
            "remove" => {
                if let (Some(line), Some(editor)) = (self.remove.take(), editor) {
                    editor.update(cx, |e, cx| e.remove_object(line, cx));
                }
            }
            id if id.starts_with("layout-") => {
                if let (Some(at), Some(editor)) = (self.layout.take(), editor) {
                    let what = id["layout-".len()..].to_string();
                    editor.update(cx, |e, cx| e.layout_choice(at.line, at.at, &what, cx));
                }
            }
            id if id.starts_with("pic-") => {
                if let (Some(line), Some(editor)) = (self.picture.take(), editor) {
                    let what = id["pic-".len()..].to_string();
                    editor.update(cx, |e, cx| e.picture_choice(line, &what, cx));
                }
            }
            id if id.starts_with("file-") => return Asked::File(id["file-".len()..].to_string()),
            "open-link" => {
                if let Some(link) = self.link.take() {
                    return Asked::Link(link);
                }
            }
            "copy-link" => {
                if let Some(url) = self.link.take().and_then(|l| l.url) {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                }
            }
            "copy-text" => {
                if let Some(link) = self.link.take() {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.words));
                }
            }
            "copy-markdown" => {
                if let Some(link) = self.link.take() {
                    cx.write_to_clipboard(ClipboardItem::new_string(link.raw));
                }
            }
            "canvas-export" => return Asked::Export,
            "canvas-print" => return Asked::Print,
            "table-new" => {
                if let Some(editor) = editor {
                    editor.update(cx, |e, cx| e.insert_table(cx));
                }
            }
            "no-fix" => {}
            "learn-word" => {
                if let Some(issue) = self.issue.take() {
                    return Asked::Learn(issue);
                }
            }
            "ignore-word" => {
                if let Some(issue) = self.issue.take() {
                    return Asked::Ignore(issue);
                }
            }
            id if id.starts_with("fix-") => {
                let fix = id["fix-".len()..].parse::<usize>().ok();
                if let (Some(issue), Some(editor), Some(fix)) = (self.issue.take(), editor, fix) {
                    if let Some(with) = issue.fixes.get(fix) {
                        editor.update(cx, |e, cx| e.fix_issue(issue.range.clone(), with, cx));
                    }
                }
            }
            id if self.cell.is_some() && !matches!(id, "cut" | "copy" | "paste" | "select-all") => {
                if let (Some(cell), Some(editor)) = (self.cell.take(), editor) {
                    let id = id.to_string();
                    editor.update(cx, |e, cx| e.table_do(&id, cell, cx));
                }
            }
            id => {
                if let Some(focus) = self.focus.take() {
                    crate::menu::do_edit(id, &focus, window, cx);
                }
            }
        }
        Asked::Done
    }
}

/// How a link can show: among words, as a card of a size, or for a file a
/// frame shows, whole; the way it shows now ticked.
pub(crate) fn layout_entries(l: &LayoutAt) -> Vec<Entry> {
    use crate::editor::Layout;
    use onyx_core::html::CardSize;
    let mut tools = Vec::new();
    if l.preview {
        tools.push(Tool::new("layout-preview", "icons/card-preview.svg", t("Preview"), l.now == Layout::Preview));
    }
    // A table of data shows as its table, a small card or a link: a large
    // card's and a medium one's picture of it said nothing.
    if !l.sheet {
        tools.push(Tool::new("layout-large", "icons/card-large.svg", t("Large Card"), l.now == Layout::Card(CardSize::Large)));
        tools.push(Tool::new("layout-medium", "icons/card-medium.svg", t("Medium Card"), l.now == Layout::Card(CardSize::Medium)));
    }
    tools.push(Tool::new("layout-small", "icons/card-small.svg", t("Small Card"), l.now == Layout::Card(CardSize::Small)));
    tools.push(Tool::new("layout-link", "icons/link.svg", t("Link"), l.now == Layout::Link));
    let mut items = vec![Entry::Icons { label: t("Show As").into(), items: tools }];
    // A large card, narrower than the words, stands where it is placed.
    if let Some(across) = l.across.filter(|_| !crate::widgets::full_width()) {
        use onyx_core::html::Across;
        items.push(Entry::Icons { label: t("Align").into(), items: vec![Tool::new("layout-left", "icons/align-left.svg", t("Align Left"), across == Across::Start), Tool::new("layout-center", "icons/align-center.svg", t("Align Center"), across == Across::Center), Tool::new("layout-right", "icons/align-right.svg", t("Align Right"), across == Across::End)] });
    }
    items
}

/// A mark's menu: what could stand there, and what settles the word.
pub(crate) fn issue_entries(issue: &MenuIssue) -> Vec<Entry> {
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

/// A table's menu. On a cell, what it most often wants; on a row's or a
/// column's handle, all that row or column can do. What takes the table
/// apart comes last, after a line of its own.
pub(crate) fn table_entries(scope: Scope, facts: &TableFacts, row: usize, col: usize) -> Vec<Entry> {
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
            // Widths: this column as its words ask, every column so (only
            // once a person set them), one width each, across the page.
            items.push(Entry::Rule);
            if facts.sized {
                items.push(Entry::choice("col-fit", "tabler/arrow-autofit-content.svg", t("Fit Column to Contents")));
                items.push(Entry::choice("table-fit", "tabler/arrow-autofit-width.svg", t("Fit All Columns to Contents")));
            }
            if facts.columns > 1 {
                items.push(Entry::choice("table-even", "tabler/layout-distribute-vertical.svg", t("Distribute Columns Evenly")));
            }
            items.push(Entry::choice("table-full", "tabler/arrows-horizontal.svg", t("Full Width")));
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
