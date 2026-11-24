# Changelog

## v0.6.0

1. **Nothing moves at a press.** A press changes only what is in its own place: a control that does not apply stays where it is, dimmed, and one whose words change keeps one width, in Settings and in Export.
2. **Notices.** A notice hangs over the page for a moment and no longer pushes the note down.
3. **Updates.** The Updates button says what a check found on itself (Checking…, Up to date, Check failed), with a failure's reason in its tooltip.
4. **Fonts.** A new install starts with the first face of each row: Source Serif for a note's words.
5. **iPhone and iPad.** Onyx now runs on both, with a layout of its own on an iPhone; it is not on the App Store yet.

**Fixes**

- The terminal's lower corners stay round.
- A long container path in Settings stays on one line, cut at its start.
- Why keys were refused as the menu bar note's shortcut, or Open at login by the system, is said in place.

## v0.5.2

1. **Keys for files.** ⌘⌫ moves the chosen note or folder to the Trash, and ⌘Z and ⌘⇧Z undo and redo a removal, a move or a rename, in the main window and in the menu bar note.
2. **One chosen note.** A click on a tab chooses its note as a click in the sidebar does, and the note of the last tab you close stays chosen, so the keys still act on it.
3. **A calmer vault.** A vault's block is gray like the rest of the sidebar, with its color on a solid tile of its own.
4. **The source pane.** Its line numbers take less room, its ground is a shade darker, and its sheet scrolls away under the fade as the note's does.
5. **Fonts.** The faces stand in a new order, in a settings window wide enough to show each name whole.

**Fixes**

- A click anywhere under a note's words puts the caret at their end, with the text pointer over all of it.
- A table pasted from elsewhere, ruled under its head, opens at its full width.
- A long note with the source pane open no longer keeps the processor busy while nothing moves.

## v0.5.1

1. **The menu bar note.** MiniNotes comes back inside Onyx: any note in a small window under Onyx's item in the menu bar, or at ⌃⌥⌘O from any app, with the vault's tree, tabs and the same editor as the main window.
2. **Vaults.** One container holds your vaults, chosen in Settings; each vault is a block with its own mark, listed from the sidebar's foot, and a vault or any folder can take an icon and a color that travel with it.
3. **Export and print.** A note becomes a PDF, an image, a Word document or a web page, or goes to the printer, with every page previewed first.
4. **Tables.** Edited cell by cell, with handles to move rows and columns, menus to add, align and sort, and widths that fit their words.
5. **Pictures, files and cards.** A picture is resized and placed as one piece, a PDF or a document stands in a frame of its own, and a link can show as a small, medium or large card.
6. **The source and the terminal.** The note as written opens beside it with Sublime Text's keys, and a terminal with tabs opens at the page's right in the vault's folder.
7. **Spelling and grammar**, checked as you write.
8. **A new icon.** A note on a moonlit pebble, in the Dock and in the menu bar.

**Fixes**

- Far less memory and CPU: pictures decoded at the size they show and let go with their view, PDFs read as needed, and nothing ticking while nothing moves.
- A scroll's momentum stays in the pane it began in.

## v0.5.0

1. **Onyx.** MiniNotes is Onyx: a note-taking app of its own, built anew in Rust and GPUI, with the window, the settings, the motion and the languages of the Alkaline Software apps.
2. **A vault.** Notes are plain markdown files in a folder of your own, made new in iCloud Drive or wherever you say, or opened as it is, and a note is written when its tab closes or at ⌘S, never while you type.
3. **The sidebar.** It is that folder, with notes and subfolders made in place, renamed or moved to the trash, whatever another app does there shown at once, and an edge you drag to widen it or fold it to a rail.
4. **Live preview.** A note is drawn as it reads while you write it: headings, bold and italic, lists with their bullets, numbers and boxes, quotes, code on a plate with its language's colors and its language's mark, tables as a grid, rules and links, with the marks of whatever the caret is in shown as written, as Obsidian does.
5. **Tabs.** Every open note is a tab as wide as its name, a folder's tabs stand together on one pill, a home tab shows the vault's page, and the tabs open again at the next launch in their order.
6. **Three ways to see a note.** As it reads while editing, as it is written, or as it reads alone, at ⌃1, ⌃2 and ⌃3 or ⌘E, one choice for the whole app, as Narrow, Middle or Wide is at ⌥1, ⌥2 and ⌥3.
7. **Keys.** ⌘K searches every note, ⌘F finds a phrase in one, ⌘B and ⌘I toggle bold and italic, ⌘↩ a task's box, Tab and ⇧Tab a list's level, ↩ carries a list or a quote on, and lists renumber themselves.
8. **Fonts and size.** A note set in one of eight faces, four serifs and four sans serifs, code in one of four monospace faces, the size a preset or your own for each, and Chinese in a carried face of your choice or one you name from the Mac.
9. **Eight languages.** English, Chinese (Simplified and Traditional), Japanese, Korean, French, German and Spanish, following the system's.
10. **Updates.** Onyx checks GitHub for a newer release once a day and installs it at a press, with no Sparkle feed.
