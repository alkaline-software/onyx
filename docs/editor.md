# The editor

About `crates/onyx-app/src/editor.rs` with `editor/cells.rs` and `editor/writing.rs`, `layout.rs`, `lines.rs`, `markdown.rs` and `tables.rs`, and `onyx-core/src/rhythm.rs`, `html.rs` and `check.rs`: a note's text, what each line of it is, and the element that lays it out and paints it as it reads, with the marks of whatever the caret is in shown as written.

## Rules

- **A row's height goes by its kind and how its words wrap, and never by what stands in it or beside it.** Every measure comes from `Rhythm` (`layout.rs`), which reads `onyx_core::rhythm`. A blank line is a whole line; the room over a heading is the heading's own, whatever its neighbor is; a fence holds its row hidden or shown; a setext underline holds its line when it hides. So a word typed on a blank line moves nothing, the caret entering a block moves nothing, and a line stands at the same height in all three modes. A new kind of row keeps to this or it brings the jump back. Only a block of HTML changes height with the caret, since its source and what it reads as are different things.
- **A table's columns are worked out with no caret near** (`make_grids`, a `Reader` with no selection), so a cell's marks showing never moves the grid.
- **A cell's words are bytes of their row's line.** Nothing is kept about a table but the file. What a cell cannot hold is written so it can (`Editor::cell_words`): a bar as `\|`, a line break as `<br>`. A selection begun in a cell stays in it; one begun outside takes a table whole (`held_to`), so a delete cannot leave half a row. ⌫ at a cell's start does nothing, and no bar is ever typed over.
- **A change of a table's shape is one step of undo** (`table_do`, `write_sheet`), through `tables::Sheet`, which keeps each cell as written, spaces and all.
- **The canvas and the page read HTML one way**: `html::to_markdown` for a block, `html::read_tag` for a tag among the words. Add a tag in `html.rs` and both have it. Nothing runs: a script, a style sheet, a frame or a form is left out.
- **The text is `lines::Doc`, a vector of lines, and a place is a line and a byte offset on a character's boundary.** Everything that moves the caret goes through `next_grapheme`, `prev_grapheme`, `word_left`, `word_right` or `clamp`; nothing slices a line at a byte it computed. A slice at a byte inside a character panics, and a panic in a draw takes the app down.
- **Typed text arrives through `EntityInputHandler`** (`replace_text_in_range`, `replace_and_mark_text_in_range`), which gpui calls with the key's character when no binding took it, so an input method composes in place with its candidates window at the caret (`bounds_for_range`). The keys that move, select, delete or format are actions bound in `key_bindings`; Copy, Cut, Paste, Select All, Undo and Redo are the toolkit's own actions, so the Edit menu serves a field and the editor alike. Never bind a printable key.
- **The editor's input handler says it does not accept text input** (`accepts_text_input` is false), so a key goes to the bindings before an input method. On a Mac gpui hands every printable key, ⌥ chords among them, to an active Chinese, Japanese or Korean input method first while the handler says it takes text, and the pinyin method swallowed the ⌃ and ⌥ digits. A key no binding takes still reaches the method and composes, and on a Mac nothing else reads the flag; it also keeps a multi-stroke binding from waiting on a printable key, and the editor has none. Never bind a printable key, with or without ⌥.
- **Offsets an input method hands over are UTF-16** (`Doc::utf16_of`, `pos_of_utf16`); offsets the markdown reading hands over are bytes of the whole text (`offset_of`, `pos_of`). Mixing the two put the caret inside a character.
- **Several changes at once are one step of undo** (`Doc::replace_many`): a list renumbered, ⌘B around a span, Tab on a selection of lines. Typing within 900 ms of the last keystroke joins its step, erasing likewise; `end_step` parts them where a key should.
- **The whole text is read again after every change** (`markdown::analyze`), and a line is laid out again only when its key changed: its text, its hidden marks, its kind, the width, the faces. A keystroke lays out the lines it touched. A layout computed in `request_layout`'s measure closure is the one `prepaint` and `paint` read, so the three agree.
- **What is hidden is decided at layout, by the caret, never at paint.** The rules are in `Reader::hidden_on` (below). A change to them is a change to the cache key too, or a line keeps its old marks.
- **Paint reads a snapshot** (`PaintInfo`): the cache behind an `Rc`, the selection, the marked range. Painting needs the app the editor is read through, and a read held across a paint call does not compile.
- **A click's place comes from the shown words and is mapped back** (`Shown::to_source`), then snapped past a hidden closing mark, so a press at a rendered span's right edge lands after the whole span and the span stays rendered.
- **The caret is never in the page's edge fade**, which counts as out of sight (`docs/window.md`, A scroller's edges). The fade takes no press; a key, an edit or a drag scrolls the caret `SCROLL_MARGIN` clear, which is the fade's height (`FADE_H`), and the two change together; a press on a line half in the fade lifts it clear when the press ends, not at the press, so the words do not move under a drag about to begin. The wheel may carry a caret into the fade, where it dims with its line like any other word.
- **Read mode locks the text and nothing else** (`Editor::locked`, also a read-only note). The pointer and the keyboard work as in any note: a press takes the keyboard and places a selection with no caret drawn, a drag, a double and a triple click select, ⌘A, ⌘C and the find work. Every change checks the lock itself, in the function that makes it (`insert`, `enter`, `tab`, `backspace`, `paste`, `undo`, `toggle_wrap`, `toggle_task_on`, the input handler's two), never at the place it is called from, so a new way in cannot get round it. A press on a task's box is the one change let through (`toggle_task_on` checks `read_only`, not the lock), as a box on a page is there to be ticked; ⌘↩ stays locked, and so does undoing a tick. A link is followed at the release, not the press (`pressed_link`), so a selection can begin on one. What ⌘C copies is the words that show (`shown_text`, by the `hidden_on` the drawing uses, so the two cannot differ): no marks, a link's words without its address, a wikilink's alias, a list's items without their markers, a table's rows with a tab between cells, and nothing for a fence, a rule or a heading's underline. Edit and Source copy the file's own text.
- **A right click selects like a Mac's text** (`select_for_menu`): inside the selection it leaves it, on a word it selects the word, on nothing it puts the caret there, and on a link it moves nothing. A word is found past any hidden mark the place stands before (`past_hidden`): a press on a word's first letter maps to the end of what stands before it, which may be a `**` that does not show, and a double click there selected the mark.

## The text

`lines.rs`. A `Doc` is lines with no line ends; `text()` joins them with `\n`, and a file with Windows' line ends is read as plain ones and written back plain. A `Sel` is an anchor and a head; a caret is the two equal. `replace`, `replace_many`, `undo` and `redo` are the only ways the text changes. `list_line`, `next_marker` and `quote_line` read what a line is for Enter, Tab and ⌘↩.

## What each line is

`markdown.rs`. `analyze` runs pulldown-cmark over the whole text with offsets (tables, strikethrough, task lists, math, wikilinks, YAML headers, footnotes on) and keeps, by line: its kind (`LineKind`: a heading with its level, a fence's open and close, a line of code, a table's first line and its other rows, a rule, a YAML header's edges and lines, a setext underline, HTML, blank), its block marks (a heading's `#`s with the space after, a quote's `>`s), its list mark (the marker as written with its space, its kind, its depth), how many list levels it stands inside, and whether it is a done task. Every inline span is kept whole (`Span`: strong, emphasis, strike, code, link, wikilink, image, highlight, math, footnote, HTML) with its opening and closing marks as byte ranges, in order of start, the outer before the inner, and each line lists the spans that touch it. Tables and code blocks are kept as line ranges, lists as their items' first lines.

`==highlight==` is not the parser's; it is found inside each text run. A wikilink's alias (`[[Note|words]]`) makes the target and the bar part of the opening mark.

## Hidden marks

`Reader::hidden_on`, per line, in Live mode:

| What | Hidden when |
|---|---|
| A heading's `#`s, a quote's `>`s, a setext underline | the selection touches no part of the line |
| A list item's indentation and marker | always: the bullet, the number or the box stands for them |
| An inline span's marks (strong, emphasis, strike, code, highlight, math) | the selection touches no byte of the span, its ends included |
| A link's, wikilink's or image's marks | the selection lies wholly outside the span (a caret at its end does not reveal it, so typing after a link keeps it rendered) |
| A fence's lines | the selection touches no line of the block: the plate keeps its padding where they were, and the language stands at the top right |
| A table's bars and spaces | always, but in Source: each cell shows its own words, and a span's marks in it hide as they do on any line. The separator has no height |
| A `<br>`, a tag, a comment, a backslash before a mark | as a span's: the selection touches no byte of it; a `<br>` only when the selection lies inside it, and arrows and ⌫ take it whole, so it is one step |
| A block of HTML | the selection touches no line of it: it reads as `html::to_markdown` has it, the first line holding the whole |
| A picture alone on its line | never: with the caret on the line its source stands over it |
| A rule's `---` | the selection touches no part of the line: a line is drawn in its place |

**The marks around the caret, and the caret, show only while the keyboard is in the editor** (`Editor::focused`, read at every layout before the rows are keyed): a press in the margin off the words takes the keyboard back to the window (`Shell::render_note`'s scroller, which hears a press the editor did not stop), and the note reads whole again; a tab opened or shown comes up with no caret, the keyboard the window's until a press in the words asks for it (`show_tab`). The caret begins to blink as the keyboard arrives (`kick_blink` at the gain), not at the next edit. Tried and dropped: the marks keyed on the caret alone, which left a block open after a click beside the note and drew a caret that stood still in a note just opened.

In Source mode nothing is hidden and no widget is drawn, but the styles are kept, so a heading is still large, and a code block keeps its plate, its inset and its colors: the source is the note's text shown whole, not the note shown worse. Reading mode is Edit's own drawing with every mark away and nothing to type in (`Editor::locked`, `revealing`): the same layout, spacing and plates as Edit, as the person asked, so the note looks the same whichever way it is shown; a press follows a link or ticks a task and places no caret, the pointer is an arrow, and the keyboard is not taken. A change of mode never moves the keyboard: a caret placed stays placed and comes back with Edit, and a note left by a click in the margin keeps reading whole. Tried and dropped: the toolkit's markdown view for Reading, which laid the note out another way, knew no wikilinks, and brought the caret back at every switch.

## The layout

`Editor::relayout` walks every line, hashes what its layout depends on, and lays out the ones whose hash changed (`lay_out`). A text row is the shown words (`Shown`, the source with the hidden ranges taken out, with the mapping both ways) shaped by gpui at the row's size with a run for each change of face, weight, style, color or decoration, wrapped at the width less the row's inset; its inset is 26 a list level and 18 a quote level. The row's height is its wrapped rows times its line, plus a heading's room above. A row's widgets (the bullet's disc, the number, the task's box, a guide a list level, a bar a quote level) are placed as it is laid out. The tops are the running sum, and the element's height the total. `Reader` (`layout.rs`) is what a layout goes by, and a row's words are a `Flow`: the paragraphs between one `<br>` and the next, each a `Shown` and its wrapped line. A text row has one Flow and a cell has one, so the caret, a click, a link, a chip, the selection, the find and the input method go by the same code in both.

A code block's words are colored by the toolkit's tree-sitter highlighter for the fence's language (`color_code`, at every relayout, read again only where the block's text changed), with the inks the theme's `highlight` table names, the same the note gets as it reads; a block with no language or no grammar keeps the plain ink. At the plate's top right stands the language's mark (`langs.rs`: its icon from `icons/lang/`, the set emaki and pingfan.org share, painted in the language's own color, and its name), on the hidden fence's row. As the note reads the toolkit draws the same mark through `lang_marks` (UPSTREAM.md), its icon in its own colors. Its lines and its fences stand `PLATE_X` in from the plate's edge in every mode. A fence's row is a line of code tall, hidden or shown, and the language's mark stands in the middle of it.

Every run's font carries the fallbacks for the characters its face lacks (`Look::text_fallbacks`, `mono_fallbacks`, from `fonts::cjk_fallbacks`): a serif's Chinese behind a serif, a sans serif's behind the rest, so a note set in Source Serif reads in Songti and not in the system's sans. The reading view and the note's title get the same.

The element is one child of the scroller the window draws around it (`Shell::render_note`), so the toolkit's scrollbar and the scroll router serve it; the caret asks for a scroll (`scroll_to_caret`) in `prepaint`, where the rows and the visible bounds are known.

## Paint

Each visible row: the plates behind code blocks first, then a row's chips (inline code's plate, a highlight's wash), the selection's boxes (a wrapped row at a time, and a little past a line's end when the selection runs on), the words, an input method's underline, the widgets, and last the caret, 1.5 wide and as tall as the words it stands in (`rhythm::CARET`, 1.2 of their size, in the middle of the line, so it is one height on a blank line and over any script), blinking at 530 ms on the editor's own timer while it is focused. `CaretAt` keeps the line apart from the bar: ↑, ↓ and the view go by the line, and only the drawing by the bar. The pointer: a press puts the caret, a double click takes the word and a triple the line, a drag extends by the grain the press began with; a press on a task's box flips it, and on a rendered link follows it (`EditorEvent::OpenLink`, `OpenNote`), with no caret moved.

## The keys

| Key | Does |
|---|---|
| ↩ | A new line; in a list, the next item with the same marker, the next number or an empty box; on an empty item, the marker goes; in a quote, the next quoted line |
| ⇧↩ | A plain new line |
| ⇥, ⇧⇥ | A list line in or out a level by a tab; the lines of a selection together; outside a list, a tab |
| ⌫, ⌦, ⌥⌫, ⌘⌫ | Back, forward, a word back, to the line's start; ⌫ right after a list marker with nothing typed takes the marker |
| ←→↑↓ and ⇧ | By grapheme; by wrapped row and then by line, keeping the column the first press began in |
| ⌥←, ⌥→ | By word |
| ⌘←, ⌘→, Home, End | The start of the words (past the marks), then the line's start; the line's end |
| ⌘↑, ⌘↓ | The text's start and end |
| ⌘B, ⌘I | Bold and italic: a span of the kind around the caret is unwrapped, a selection or the word under the caret wrapped, an empty pair opened or closed |
| ⌘↩ | A task's box flipped; a bullet made a task; a plain line made a task |
| ⌘Z, ⇧⌘Z, ⌘A, ⌘C, ⌘X, ⌘V | The toolkit's actions |
| ⌘E | Reading and back (the window's) |

Numbered lists count from their first item after every change (`renumber`), one step of undo with the change.

## Tables

`markdown.rs` keeps, with a table's words, each cell's bytes in its line (`Table::cells`, from `cell_ranges`); pulldown-cmark's own offsets miss a missing cell. A row is a line of its own with a real height (`Row::Table`), so the caret, ↑ and ↓, a press and the view's scrolling all work by the line as they do anywhere. They share a `Grid`, kept in the cache by the table's first line.

As far as the room goes each column is as wide as its longest line asks. Past that each keeps its longest word and the room left is shared by what the rest ask (`shared`), as a browser sets a table, so a word breaks only where the room gives out; a wide character can break anywhere, so a column of Chinese gives way first. A table is as wide as it needs and no wider.

**A person's widths are in the file.** A drag of a column's edge rewrites the separator's dashes so they add up to 100, each column's share of the room (`Sheet::set_shares`, `rhythm::shares_of`). It is a legal table to every other reader, Pandoc reads the same dashes the same way, and any other count of dashes says nothing, so a table laid out by hand is not read as widths by accident. A drag is held in `hold` and laid out live, and written once, at the release, as one step of undo.

| Key in a cell | Does |
|---|---|
| ⇥, ⇧⇥ | The next cell, the one before; past the last, a new row |
| ↩ | The cell under; in the last row, a new row |
| ⇧↩ | A line break, written `<br>` |
| ← → | Through the words, then on to the next cell, and out of the table at its ends |
| ⌘←, ⌘→, Home, End | The cell's start and end |
| ⌘A | The cell; again, the note |

The pointer over a table brings its handles (`Control`, `paint_controls`): a handle on the row's left edge and the column's top edge, a strip past the right edge and the foot to add a column or a row, and each inner edge to drag. A handle pressed opens its menu; dragged, it moves its row or column, with a line where it will land. The handles stand out in the page's margin, so the element has a second, wider hitbox (`wide`) that takes only those presses. None of this shows in Read mode or on a locked note, and in Source a table is its lines. A cell with the caret has a ring round it.

A table is added from the right click on an empty line.

## HTML

A tag among the words becomes a span like any other (`span_of`): `<b>` is `Strong`, `<mark>` is `Highlight`, so ⌘B unwraps a `<b>` as it does `**`. `<u>`, `<sub>`, `<sup>`, `<small>`, a color and an unknown tag have kinds of their own. A tag left open ends with its block, and what has no look of its own is hidden as a mark is (`Quiet`). Text has one size to a line in gpui, so `<sup>2</sup>` is drawn as the character `²` (`Shown::with`, `raised`), mapped back byte for byte, and a letter Unicode has no raised form of stands as it is.

A block is translated to markdown (`html::to_markdown`) and laid out as a note of its own (`Reader::nested`), so a `<table>` is a table, an `<ul>` a list and an `<h2>` a heading, by the code a note uses. `<p align="center">`, `<center>` and `text-align` stand the block across the page. `<details>` shows open, with `▾` before its summary, since a note holds no state to fold it by. A block that reads as nothing (a comment, a lone `</div>`) keeps a line, so the caret can reach it.

## Pictures

A picture alone on its line is drawn (`Row::Picture`), from a markdown link, `![[a.png]]` or `<img>`, as large as it is or as its width asks (`![alt|300](src)`, `<img width>`), never past the words' width. Its size is read from the top of its file during layout, so the note does not jump when the pixels arrive off the window's thread (`read_pictures`). It is found as `pdf::find_picture` finds it, so the canvas and the page show the same file. Among words a picture is still its alt text, in the link's color, and opens in the system's viewer at a click. SVG is not drawn on the canvas.

## Spelling and grammar

`writing.rs`, with `onyx_core::check`, the check Emaki runs (`emaki/docs/composer.md`): Harper for English, in the process, nothing sent anywhere; the system's checker for the spelling of Spanish, French and German (`sys::spelling`). Settings › Text › Spelling turns it on and names the language.

`markdown::prose` blanks what is not prose byte for byte (code, math, marks, addresses, bars, a block of HTML), so offsets stay the note's, and cuts the rest into blocks: a paragraph, a heading, a list item, a table's cell. Each block is checked apart, so a list is not read as one sentence, and is read again only when it changed (`runs`): a long note costs what the paragraph being typed costs. A rule that read across something blanked is dropped (`same_text`): `and `x` and` is no repeated word. A capital is asked only of a paragraph, and read from the words as written, so a sentence after code is not marked as starting there. Text in other scripts is passed by.

The marks move with the words until the text is read again (`check::carry`), the check runs 300 ms after the typing pauses, and a misspelling at the caret waits 1.2 s more. Red is spelling and blue grammar. A right click on a mark puts its corrections first (`issue_entries`), then Learn Spelling (`~/.onyx/words.txt`) and Ignore: a spelling until the app quits, grammar kept (`~/.onyx/ignored.txt`). A correction is one step of undo. Marks do not show as the note reads.

## Not yet

Pictures among words. Folding `<details>`. A markdown block between two tags keeps no alignment from them (`<div align="center">`, a blank line, markdown, `</div>`). A cell selection across cells. These are the next things.

## Probing

`ONYX_GO=live;end` opens the note in Live mode with the caret at its end, so the view scrolls there; `source` and `reading` show the other modes, and `tops` prints where each line stands, which is how a change of spacing is measured (`docs/probing.md`). A synthetic click does not reach the element in a background window, so what a press does is checked by hand in the dev build.
