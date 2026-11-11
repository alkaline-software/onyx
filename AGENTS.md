# AGENTS.md

## What this is

**Onyx** is a note-taking app: a folder of plain markdown files, a vault, shown in a sidebar, and a note drawn as it reads while you write it, as Obsidian's live preview does. Rust and GPUI, built to the Alkaline Software constitution; it took the place of MiniNotes, a Swift menu bar app, at v0.5.0. **Only the Mac version is published.** The code keeps the constitution's Windows and Linux paths, but no release is cut for them and nothing is checked there; iOS and iPadOS are wanted later, and nothing is done for them yet.

## The one idea

**The file is the note.** Onyx keeps nothing of its own about a note: no database, no index, no copy. The sidebar is the folder as the disk has it, the editor holds the file's text exactly, and a note is written whole two seconds after its first change, and when it is left. Another app may write the same folder, and Onyx follows (`docs/window.md`, The vault). This is why a vault made elsewhere opens as it is, and why nothing is lost when Onyx is not running.

## The standard

Onyx is held to the bar of the work that wins Awwwards, the Webbys and the FWA (`docs/design.md`; read it before changing anything a person sees): minimal, calm, intuitive, finished in every state. Every change meets it unasked.

- **Nothing jumps.** What appears, goes, opens, closes, grows or moves does so over a moment, with the pattern its neighbors use (`widgets.rs`, `menu.rs`, `shell.rs`). No pause at the end of a motion.
- **Words are few.** A row's name says what it is; a line under it only where the name does not say it all.
- **Look before saying it is done:** in both appearances, in a long language, and in a narrow window (`docs/probing.md`).

## Rules that hold everywhere

Each is here because breaking it loses data or brings back a fixed bug. The doc named has the story.

- **A note is written whole, through `onyx_core::vault::write`**, a temp file and a rename, never in place and never in part: two seconds after its first unsaved change (a timer later keys do not reset), and when it is left; never on each keystroke, and never when its text is what the disk has. (`docs/window.md`)
- **A note that has changes is never replaced by what the disk holds.** The disk's text is joined with what was typed (`merge::merge`), every change of both kept. (`docs/window.md`)
- **Nothing goes straight off the disk.** A note or a folder deleted goes to the system's trash (`vault::trash`). (`docs/window.md`)
- **A row's height goes by its kind, never by what stands in it or beside it**, in a note and in a frame, from `Rhythm`: a word typed on a blank line moves nothing. (`docs/editor.md`)
- **The editor never slices a line at a byte that is not a character's boundary**, and the caret steps by grapheme (`lines::Doc`). (`docs/editor.md`)
- **Typed text arrives through gpui's input handler, never through a key binding**, so an input method composes in place. Keys that move or change are actions. (`docs/editor.md`)
- **A change made in many places at once is one step of undo** (`Doc::replace_many`): a list renumbered after Enter is not ten steps. (`docs/editor.md`)
- **A motion that repeats runs on the window's clock** (`ambient::phase`), never `Animation::repeat`.
- **A scroll gesture, its momentum included, moves only the scroller it began in**, never what the pointer is carried onto. Every scroller that goes up and down names itself to the router as it is drawn (`Scrollers::name`); anything else that takes the wheel (the terminal, the source pane's editor) names where it is and what moves it (`name_own`); a gesture begun over nothing that scrolls moves none. A new one is checked with `flick=` from it onto its neighbor and back (`docs/probing.md`). Every scroller has the toolkit's bar: it stays a second after the last movement, then fades over half a second (`vendor/gpui-component`, `scrollbar.rs`).
- **A button is `choice_pill`**, a pill with an icon before its word. The toolkit's `Button` is only for an icon alone.
- **Every word the window shows goes through `i18n::t`** and is in `locales/words.json` in every language; `cargo test -p onyx-app` fails otherwise.
- **A key is drawn on caps** through `key_text`, `kbd_hint` or `tip`, named as the Mac has it and bound as `secondary-<key>`.
- **Every child process starts through `onyx_core::process::command`**, never `Command::new`; the terminal's shell is the one exception, a pty's child that portable-pty starts (`pty.rs`).
- **Per-system calls live in `sys.rs`**, each with a body for every system. (`docs/platform.md`)
- **One copy of gpui.** The pin is in `Cargo.lock`. Changes to the toolkit go in `vendor/gpui-component/`, marked `(Studio addition.)` and listed in its `UPSTREAM.md`. Build from the root, never inside `vendor/`.

## Before you edit, read

| You are about to touch | Read first |
|---|---|
| `editor.rs`, `editor/`, `embed.rs`, `viewer.rs`, `layout.rs`, `lines.rs`, `markdown.rs`, `tables.rs`, `source.rs`, `sublime.rs`; `onyx-core/src/rhythm.rs`, `html.rs`, `check.rs`: the live preview, the source pane, spacing, the caret, the keys, tables, HTML, pictures, spelling | `docs/editor.md` |
| `shell.rs`, `shell/library.rs`, `tabs.rs`, `terminal.rs`, `ui_state.rs`, `main.rs`: the sidebar, the vault, a note's life, the tabs, the strip, the terminal, the landing page, the menus; `onyx-core/src/vault.rs`, `pty.rs`, `shell_marks.rs` | `docs/window.md` |
| `settings.rs`, `look.rs`, `fonts.rs`, `assets.rs`, `themes/onyx.json`, `scripts/icon/`; anything a person sees, new or changed | `docs/design.md`, then the doc of the part it is in |
| `sys.rs`, `csd.rs`, `update.rs`, `process.rs`, `vendor/`, `Cargo.toml`, `.github/workflows/`, any `cfg` for an OS | `docs/platform.md` |
| `export.rs`, `onyx-core/src/pdf.rs`, `docx.rs`, `web.rs`, `reading.rs`, `pdf/template.typ`: Export (PDF, image, Word, web page), printing, its sheet, how a note looks on paper | `docs/export.md` |
| Checking a change in the window from a script | `docs/probing.md` |
| Bumping the version and cutting a release | `docs/release.md` |

## Layout

```
Cargo.toml                 the workspace and the one version
crates/onyx-core/          everything without a window
  src/vault.rs               the folder of notes: the tree, new, rename, trash, wikilinks, the watcher
  src/config.rs              the settings file
  src/paths.rs               ~/.onyx, overridden by ONYX_HOME; atomic writes
  src/update.rs              the newest release and its installer
  src/link_card.rs           a web link's card: the page's head read, its pictures kept in the cache
  src/process.rs             every child process, with no console window on Windows
  src/pty.rs                 the terminal's shell on a pty, and its screen model (vt100)
  src/shell_marks.rs         the shell started so its prompt marks itself (OSC 133)
  src/pdf.rs                 a note set as a PDF or an image by Typst; pdf/ holds its template, code theme and math
  src/docx.rs, src/web.rs    a note as a Word document and as a web page, from reading.rs's tree of its blocks
  src/rhythm.rs              the note's vertical measures, for the canvas and the page
  src/html.rs                the HTML a note may hold, read one way for both
  src/merge.rs               a note changed on the disk and in the window at once, joined
  src/check.rs               spelling and grammar
  src/preview.rs             any file read for the preview: as markdown where it can be (code, CSV, notebooks, HTML, Office), else its kind; a PDF's pages
  src/bin/onyx-core.rs       paths | config | update | vault <folder> [new <name>] | pdf <note> <out> <fonts>
  tests/core.rs
crates/onyx-app/           the window
  src/main.rs                menus, key bindings, the main window, the probe's variables
  src/shell.rs               the frame, the sidebar (the vault's tree), the strip, the landing page, a note's life, the tabs
  src/shell/library.rs       the vault's assets: the sidebar's stack of cards, the Assets card, an asset's tab, a rename that follows its links
  src/tabs.rs                the tabs' drawing and motion: a folder's on one pill, the plate, the widths
  src/export.rs              Export: the sheet, its formats, its preview, the file; printing
  src/media.rs               a video or a sound in the preview, played by the system's web view
  src/viewer.rs              a note's picture over the window, grown from where it stands
  src/embed.rs               a file a note embeds, in a frame: a PDF's pages, a note, a document; the bar every embedded file has, a picture's too; zoom, selection, sidebar
  src/editor.rs              the live-preview editor: paint, the caret, the keys, the input method
  src/editor/cells.rs        a table on the canvas: the caret in its cells, its handles and edges
  src/source.rs              the note's source in a pane beside it, a code editor that follows the note and is followed
  src/sublime.rs             Sublime Text's keys in the source pane, as Emaki's editor has them
  src/terminal.rs            the terminal at the page's right: the vault's shells, a tab each, as Emaki's shell terminal
  src/card.rs                a link alone on its line as a card, small, medium or large: a file's, a web page's
  src/editor/cards.rs        a link shown as a link, a card or the whole file: its line written again
  src/editor/writing.rs      a note's spelling and grammar, off the window's thread
  src/layout.rs              a note laid out: Rhythm, Reader, Flow, the rows, the grid
  src/tables.rs              a table's lines changed as a whole, as text
  src/lines.rs               a note's text as lines: places, graphemes, edits, undo
  src/markdown.rs            what each line is and every inline span with its marks
  src/settings.rs            the settings window: Appearance, Editor, About
  src/widgets.rs             the shared drawing: choice_pill, key_text, tip, page_in, Motion
  src/menu.rs                the app's own menu, in and out in motion
  src/scroll.rs              a gesture stays in the scroller it began in
  src/ambient.rs             the window's clock
  src/look.rs                appearance, accents, code's inks
  src/fonts.rs               the faces carried, and which each place uses
  src/assets.rs              the icons and pictures built in
  src/file_icons.rs          Catppuccin's file icons for the tree
  src/i18n.rs                the window's words in eight languages
  src/sys.rs                 per-system calls
  src/csd.rs                 the window's own buttons where the system draws none
  src/ui_state.rs            what the window remembers: bounds, sidebar, vault, tabs, note, source, widths, recent vaults
  assets/ themes/ locales/   icons, fonts, the app icon; the palette; the words
  macos/ windows/ linux/     what each system's package needs
vendor/                    the toolkit, the blocks crate and the terminal's screen model (vt100), each with an UPSTREAM.md
scripts/                   the dev build, the icon, the release
docs/                      how each part works and its traps; release.md; probing.md
```

## Running it

```
cargo test -p onyx-core -p onyx-app
cargo build -p onyx-app && scripts/dev-app.sh
```

One dev build at a time: two copies share `~/.onyx` and both write what the window remembers. After a change to the app, build it and run `scripts/relaunch.sh` as the last command, without being asked; a change to documents alone needs no relaunch. Probe a change with a copy of its own first (`docs/probing.md`), never the copy the person is writing in. On the maintainer's Mac the vaults in Onyx's iCloud folder (`~/Library/Mobile Documents/iCloud~com~alkalinesoftware~onyx/Documents/`) are copies kept for testing: a probe may use and change them.

## Writing things down

- How a part works, a trap, or a fact learned about gpui or the toolkit goes in that part's doc under `docs/`, in its `## Rules` when breaking it loses data or brings back a bug.
- This file gets a line only for a rule every session follows whatever it works on, or a row in the table above. Keep it under 300 lines.
- Write what is so now and why, in plain words, with no hard wraps, American spelling and no em dashes (the constitution's `docs/docs.md`). An approach tried and dropped gets one sentence, where someone might try it again. What was checked and how belongs in the reply to the person, not in a doc.

## Testing

```
cargo test -p onyx-core
cargo test -p onyx-app
```

The core's tests point `ONYX_HOME` at a temp folder and make a vault of their own in another (`tests/core.rs`); inherit that for anything touching disk. The app's tests hold the words to the table, the icons to the assets, and the editor's text model and markdown reading to what they promise, with no window. `rust.yml` runs both on every push, on macOS, Windows and Linux.
