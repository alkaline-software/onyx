# Optimization

About keeping Onyx fast and light: how an optimization pass is done, the rules passes have left, and the log of every pass, which is what makes the next one incremental. The studio's method is the constitution's `docs/optimize.md`; this page is Onyx's own part of it.

## Rules

- **A pass follows every round of changes.** Once a set of updates is in, a pass looks at what they added before the next set begins, since what is new is where waste comes in. Its measure is the computer's work: nothing computed, copied, read, decoded or woken for that is not used.
- **A pass has three aims:** the best speed; the least use of the computer (CPU, memory, power, wakeups, disk, processes); and nothing Onyx does lost. The third is never traded for the other two: a change that saves time and changes what a person sees or can do is not made.
- **Measured before and after,** the same way both times, and the numbers go in the log. A release build of the commit before the pass is kept beside the new one, and both run on the same scratch vault (`docs/probing.md`). The first guess is often wrong: this pass's first measure blamed the layout for a note idling at 23% CPU, and it was the first grammar check of that note.
- **Incremental.** A pass starts from the log's last entry and looks only at what changed since the commit that entry names (`git diff <commit>..`), plus what an entry left for later. A pass that is not logged did not happen as far as the next one knows.
- **Nothing slow is asked while a frame is built,** and that includes what a row asks for: a row's call runs once a row a frame, and the window is drawn at every frame of a motion, every key and every blink of a caret. No disk read, `Config::load`, `read_dir`, `is_dir`, JSON parse or copy of a list or of a note's text in `render` or in a row.
- **A lookup made at every frame is answered from memory,** looked at on the disk again no more often than every half second for another process's write, and at once after Onyx's own: the container's vaults (`library::vault_list`, `container_now`, cleared by `forget_vaults`), a vault's folder looks (`folder_look::ensure`, `LOOK_AGAIN`), the person's picture (`Shell::avatar_files`), the folder sheet's own colors (`Picker::own_colors`).
- **A layout asked for again is answered from what it was.** gpui asks for the editor's layout several times a frame, and once with no width. The editor keeps the key of everything its layout reads (`Editor::layout_inputs`: the text's revision, which no other text in the run has had, its reading, the look, the theme's inks, the width, the mode, the selection while its marks show, the check's marks, the drags, the pictures and frames known) and returns at once when it is unchanged; asked with no width, the last width answers. A line's row is keyed by what it is made of in its own bytes, never by the whole text's offsets, and where lines came or went its row and a code block's colors are found by key, not by index. One layout is kept, so the editor is asked for one width a frame: a box that holds it is given a width within its room (`Shell::sheet_w`), never one over it for `max_w_full` to cut, which lays what is in the box out at the width asked and again at the width cut to, each layout taking the other's place.
- **A picture is read at the size it is drawn,** by the system where it can (`sys::picture_at_most`: ImageIO on a Mac, which reads a JPEG or HEIC from its smaller scales of itself), and two at a time (`layout::DECODING`). A photo of 6000 × 6000 decoded whole is 145 MB, and the allocator keeps what it freed: a vault's assets read at once at launch held 2.1 GB for as long as the app ran. A thumbnail is made only while it can be seen (the Assets card open).
- **A picture is never handed to gpui as a path.** gpui keeps every picture it decodes itself, and its copy on the GPU, until the app quits, and decodes every frame of a GIF before it draws one: a GIF of 360 frames held 4 GB. A picture drawn from a file goes through the cache of the view that shows it (`pictures::Kept`, `pictures::through`: a picture's tab, the viewer, the Assets card's SVGs), which lets it go when the view goes and plays a moving picture a frame at a time, after Emaki's; one Onyx decodes itself (a note's pictures, a card's, a PDF's pages) is an `ImageSource::Render`.
- **A picture leaves the GPU with what showed it,** however that went: a note's editor, a frame and a card each let their pictures go as they are dropped (`on_release`), as a tab closed always did (`let_pictures_go`); a picture the note no longer shows goes after the next layout (`Pictures::met`), and a PDF's pages scrolled out of view but those it showed (`Embed::out_of_sight`). Dropped with the tab alone, a copy stayed in the window's atlas until the app quit.
- **A file read for many asks is read once.** A PDF is parsed once while it is unchanged, the last few kept (`preview::open_pdf`); its pages are kept within a size (`KEEP_BYTES`), not a count; its text, a glyph and its place each, is read a page at a time where the pointer comes, and whole only for the find or Select All. The vault's files are listed for a picture's source at most every two seconds (`Pictures::files`).
- **What the network could not give is not asked again at once.** A link card's page that could not be reached is kept as failed for an hour; one link is fetched once at a time; the cache's names are a hash that stays the same between builds, and files not written for two months go at launch (`link_card::prune`).
- **A clock runs only while it has something to move, in a window someone can see.** The caret blinks only in the window in front (`Editor::active`), and stops when the menu bar note's window goes (`Editor::window_gone`). The terminal sleeps until a shell writes (`TermPanel::bell`) or its cursor's blink turns, never on a poll. The vault's watcher sleeps until a change comes (`watch_changes`).
- **A change on the disk costs what it changed.** A note's own words changed, an autosave among them, read the note again and walk no tree; a file's times, rights or extended attributes (iCloud and Spotlight touch them all day) are no change (`vault::quiet`).
- **A window that is closed does no work for what it would show.** The menu bar note, closed, takes the main window's typing once as it opens (`MenuNote::catch_up`), not at every key.
- **What a window starts goes with it:** the terminal's shells end with the panel (`impl Drop for TermPanel`).

## A pass

1. Read the log's last entry: the commit it covers to, and what it left.
2. List what is new since (`git log`, `git diff` from that commit): the files, and the parts of the window they draw.
3. Find the hot paths in what is new: what `render` and each row call (a disk read, a system call, a lock, a copy of a list or a text, a parse, text shaped); clocks and timers (does anything wake while nothing moves, or while the window is behind another app's or closed); the main thread (does it wait); loops off it (how often they turn, what each turn costs); what grows with use (memory, pictures on the GPU, a list drawn whole, maps never pruned).
4. Time each suspect as it is now, in a release build on a scratch vault: CPU over 20 seconds from the process's CPU time (`ps -o time=`), memory from `footprint`, wakeups from `top -stats idlew`; a frame's parts with `ONYX_TRACE=1`; where the time goes with `sample` on the dev build, which keeps its symbols.
5. Fix the cause, in the place every caller goes through.
6. Time it again, the same way.
7. Check that nothing is lost: `cargo test -p onyx-core -p onyx-app`, and a probe copy of each part changed, in each state it has (`docs/probing.md`).
8. Write the log entry.

## Log

Newest first. Each entry: the version and date, the commits covered, what was looked at, what was found and done with numbers, what was looked at and left as it is, and what was left for later.

### The iPhone and iPad branch, 2026-11-24

Covers `e36802b` (v0.5.2) to the commit that adds this entry: gpui moved from zed `ce48461e` to `f8cfc7ed` for its iPhone and iPad backend, the toolkit built against it, and everything for an iPhone and an iPad (the insets, the compact window, the settings as a sheet, the watcher, the trash, the fixed container). The release build of v0.5.2 as installed (a copy signed ad hoc, since the signed one carries iCloud entitlements and is killed outside its bundle) against a release build of this pass, each a probe copy with a scratch home on a vault of three notes, idle from 10 seconds on, over 20 seconds: CPU from `ps -o time=`, memory from `footprint`, wakeups from `top -stats idlew`. A Mac with the person's own apps running; the memory of a copy with a note open moved by a hundred megabytes between two runs of one build (the first grammar check and the faces' loading land differently), so those two rows say only that nothing grew out of that range.

| | v0.5.2 | This pass |
|---|---|---|
| A vault open, nothing open | 0.50% of a core, 106 MB | 0.30% of a core, 119 and 132 MB in two runs |
| A note open | 1.25% of a core, 338 MB | 0.20 and 0.25% of a core, 244 and 360 MB in two runs |
| Idle wakeups in 20 seconds: the vault, the note | 4, 0 | 1 to 2, 0 to 5 |

The newer gpui costs a vault's window some 13 to 26 MB at rest, and nothing at a frame that these measures show. In an iPhone's and an iPad's simulator, a debug build, a note open and the keyboard out: 247 MB and 0.8% of a core on the iPhone, 403 MB and 1.4% on the iPad. Those are a Mac's processor and a debug build, and say nothing of a device: a release build is to be measured on one when one is built (`docs/platform.md`, Not checked).

Found and fixed:
- On an iPhone and an iPad the container was asked of the disk at every asking: `sys::fixed_container` made the app's Documents folder (`create_dir_all`, a `stat` when it is there) each time, and it is asked by the page, the card, the path bar and the settings as each frame is built. The folder is made and its path kept once (`OWN`), and the iCloud container's answer was kept already. Found by reading, not timed.
- The device's kind, for On My iPhone or On My iPad in a path (`sys::short_path`), was asked of UIKit at each path drawn; it is asked once (`PAD`).

Looked at and left as they are:
- `window.insets()` and the room figured from it at every frame (`Shell::render`): four numbers the backend keeps, and arithmetic. Off an iPhone and an iPad they are zero and the frame's outer element is one more element with no padding.
- A compact window draws the page that is off the window too: the note's card behind the sidebar's page is still in the frame. Its editor's layout is answered from what it was and its paint is clipped away, 0.02 ms and 0.01 to 0.05 ms in a trace, and a frame is drawn there only while something moves. Left in the tree so its scrollers keep their true places for the scroll router, which reads them by their bounds.
- The watcher on an iPhone and an iPad (`onyx_core::watch`) sleeps in `kevent` until a folder changes, then lists that one folder: a `stat` an entry, on its own thread. Onyx's own save of a note is such a change, so a folder of 2,000 notes is 2,000 `stat`s every two seconds while a person types in one of them. Left: it is the price of holding one descriptor a folder and not one a note, and it is off the window's thread.
- The launch waits for the iCloud container's answer, up to a second and a half on the window's thread (`sys::await_icloud`), with the launch screen showing. In a simulator the answer is at once. To be timed on a device with iCloud.
- The key bindings are made through `sys::key` (a `String` each for `secondary-`), once at launch.
- The long press's listener, the edge swipe's and the settings' `NARROW`: nothing at a frame but while a finger is down or the sheet is up.

- Later the same day, the pages with nothing open sized for a compact window, no tooltip after a touch, and each font row's first tile as a new install's: a page's words take their font from `Look::words` as the frame is built, two reference counts, and the tooltip's listener asks the window one flag when a hover begins. Then the sidebar and the menus at a hand's size: `widgets::hand` reads one flag of the thread and multiplies, a few dozen times a frame, and `tree_row_h` the same flag where a row's place is summed. Then a compact window's page as the whole screen: two more flags of the thread set a frame (`set_reach`), the status bar's ink asked a frame and told to the system only when it changes (`sys::status_bar`), and the foot bar's presence asked a frame as the strip's ends' are. Nothing runs at a frame that did not.

Left for later: a release build measured on a device (memory at launch with a large vault, the watcher's listing on a phone's storage, the iCloud wait).

### v0.5.2, 2026-11-23

Covers `abcdd95` to the commit that adds this entry: v0.5.1's release and the v0.5.2 branch (the caret under a note's words, the vault's mark, pasted tables, the source pane's look, the fonts' order and the settings' size, the file chosen and its keys, what is done to the files taken back). The release build of v0.5.1 as installed against a release build of this pass, on a scratch vault of 200 notes in 10 folders made of Onyx's and Emaki's docs, a note of 20,000 lines (1 MB) and a note of 150 tables ruled under their heads; the window in front, the keyboard in the note.

| | v0.5.1 | This pass |
|---|---|---|
| The note of 20,000 lines with the source pane beside it, idle from 40 seconds on | 29.3% of a core, 704 MB | 2.8% of a core, 601 MB |
| Layouts of that note over 5 ms in 30 idle seconds, the pane open | 232 | 3 |
| That note alone, idle from 40 seconds on | 1.55% of a core, 688 MB | 0.85 to 1.6% of a core, 615 to 673 MB |
| A vault open, nothing open | 0.75% of a core, 107 MB | 0.70% of a core, 108 MB |
| The note of 150 tables, idle | 1.5% of a core, 387 MB | 1.55% of a core, 388 MB |
| Idle wakeups in 10 seconds: the note alone, and with the source pane | 21, 6 to 7 | 8 to 9, 7 to 10 |

Found and fixed:
- Beside the source pane the whole note was laid out twice at every frame, so twice at every blink of the caret and twice at every key: the note's sheet asked for its reading width, wider than the room the pane left it, and `max_w_full` cut it, so the editor was measured at the width asked (634 in the probe) and again at the width cut to (330), and each layout took the other's place in the editor's one kept layout. It was so since the source pane came, in v0.5.1 too, and showed only on a long note. The sheet's width is now within its room where it is figured (`Shell::sheet_w`), which the find bar reads too. The sheet stands where it stood: the note's part of a shot before and after is the same pixel for pixel. The first measure of this pass blamed nothing, since no pass had timed the pane open with a long note.

Looked at and left as they are, nothing of them at a frame or in a row:
- The file chosen (`Shell::choose`, the menu bar note's `chosen`): a compare of one path at a tab shown; an asset's row is lit by one compare, not by a walk of the rows chosen.
- The files' journal (`file_undo.rs`, `fsops.rs`): the holding folder is read once at launch and once at a quit or a change of vault; a removal is a rename, and reads the assets again off the main thread.
- A table ruled under its head (`rhythm::widths_of`): two short lists of lengths a table, only for a separator that could be widths, when the note is read.
- The editor filling its sheet (`Editor::fill_page`), the source pane's edges (`render_source`, a frame asked for only while its sheet's top moves), the vault's tile (`tile_ink`), the line numbers' lane: arithmetic.
- The menu bar note and the terminal beside the note measure the note at one width a frame, as the note alone does.

Left for later: nothing.

### v0.5.0, 2026-11-16, the list left for later

Covers what the entry below left for later, in the same commit range, with the same release builds of `635b63e` and of this pass on the same scratch vault, a GIF of 360 frames of 1600 × 840 (28 MB, as Emaki's) and R's reference manual (a PDF of 3,948 pages, 10 MB) added to it; PDF timings from a throwaway test of the dev build.

| | Before | This pass |
|---|---|---|
| The GIF in its own tab | 4,136 MB, standing still | 351 MB, playing, 18% of a core |
| A note showing the manual, open | 744 MB | 302 MB |
| The manual paged through in its frame | 872 MB | 418 MB |
| Five of its pages drawn | 215 ms | 29 ms |
| Its pages' sizes read a second time | 37 ms | 20 µs |

Found and fixed:
- A picture in its own tab, and in the viewer, was drawn from its path: gpui kept it, every frame of a GIF and each on the GPU, until the app quit. `pictures.rs` is Emaki's per-view cache, its GIF and WebP player and its tests; the tab, the viewer and the Assets card's SVGs draw through it.
- A PDF was read and parsed again for every page drawn, for its sizes, its text and its outline; its pages were kept by count; a frame scrolled away kept a dozen; and its text, 460 MB for the manual, was read for every PDF a note showed, whether its words were looked at or not.
- A link card whose page could not be reached was fetched again at every opening, two cards of one link fetched twice, its cache was never pruned and was named by a hasher not promised to stay the same between builds; a card's pictures stayed on the GPU after it went.
- A picture's source not where it said walked the whole vault on the main thread, at every letter of a link typed; the sources met were never let go of, nor the pixels of pictures taken out of the note; a note gone from the disk left its tab's pictures on the GPU.
- The Assets card drew every file at every frame; the tree looked up each row in the chosen list by a walk of it, and copied the list for every chosen row it drew; the folds' motions and the tabs' widths kept a key for everything ever seen.
- The source pane made its spelling marks again at every blink of the note's caret; a language other than English sent the whole note to the system's checker at each pause, where only the blocks not seen before go now; undo kept every step for as long as the tab was open (now 1,000).
- The menu bar note watched each tab's folder with a stream and a thread of its own; it keeps one a folder now. Its closed tabs were never let go of past the last few.
- The font list matched every family of the system at every draw while it was open; a hidden player asked the window for the keyboard at every frame.

Measured and left as they are:
- `vault::tree` on the main thread as a vault is shown, 15 to 20 ms for 5,000 notes, once an opening: what follows the opening reads the tree at once.
- The traffic lights placed at every frame (`fold_lights`), some twenty calls to the system, kept for their place.
- The theme copied a few times a frame, small.
- The source pane copies and compares the note at each key, linear in the note.
- The settings' window is drawn at each of the main window's draws, which now stop with the caret behind another app.
- The menu bar note builds its remembered tabs' editors at launch, a few.
- A GIF playing costs 18% of a core, its frames decoded and drawn at their pace.

Left for later: nothing.

### v0.5.0, 2026-11-16

Covers everything up to `635b63e` and the commit that adds this entry: the first pass, over the whole app. Five reviewers read it by part (the frame and the sidebar; the menu bar note, settings and the shared drawing; the editor; pictures and embedded files; the terminal, watchers and threads), and their findings were checked against the code and measured. Release builds of `635b63e` and of this pass, on a scratch vault of 200 notes in 10 folders, Onyx's and Emaki's docs as notes, a note of 830 KB (20,000 lines) of them, ten photos of 6016 × 6016 in its assets and a note showing them; the window in front.

| | Before | This pass |
|---|---|---|
| A vault open, nothing open, with ten large photos in its assets | 2,243 MB | 107 MB |
| A key typed in the note of 20,000 lines (debug) | 235 ms, every line laid out again | 0.9 ms; an Enter 7.6 ms |
| A frame with nothing changed in that note (debug), each ask of the layout | the whole note's lines walked, hashed and their rows looked up | 0.02 to 0.04 ms |
| That note focused, idle, from 40 seconds on | 1.9% CPU, 667 MB | 1.4% CPU, 494 MB |
| The terminal open and quiet, idle wakeups in 10 seconds | 8 | 3 |
| A vault open and quiet, idle wakeups in 10 seconds | 12 | 6 |
| The note of ten photos | 474 MB | 508 MB (the ten pictures at 2400 pixels, as before; read two at a time) |

Found and fixed:
- The Assets card's thumbnails were made at every vault's opening, whether or not the card was open, every picture at once, each decoded whole: the allocator kept the 2.1 GB it freed. Now only while the card is open, two at a time, each read at the size it needs (Rules).
- The editor laid every line out again at every ask of its layout (several a frame, and the window is drawn at every blink of the caret), and a measure with no width laid it out at 600 points. A letter typed made every line under it new, its key being of whole-text offsets, and an Enter every row under it, rows being kept by index; a code block's colors were read again by its grammar under every line added above it, and each range of its colors was walked against every line of the block. Whether the selection touched a span summed every line above the caret for every span of every line.
- The caret blinked on in a window behind another app's, and in the menu bar note's after its window went: the whole window drawn twice a second for no one.
- The hidden menu bar note followed every key typed in the main window in a note both held: the whole note copied four times, read again and checked again, at every key.
- The terminal looked at its shell 30 times a second while it showed, and drew a shell that had ended 30 times a second; closing the window left every shell running with its threads and screen.
- Every autosave walked the whole vault's tree again (the save comes back as a change), and the watcher was looked at every 300 ms while a vault was open.
- The menu bar note's tree was copied whole at every draw, and its pages before a vault read the configuration and the container's folder several times a draw; the vault blocks and tiles looked at their vault's looks file four times each a draw; the person's picture was looked for on the disk twice a draw; the search copied all its results at every draw to show 40; the folder sheet read the configuration twice a draw.
- The spelling check copied the whole note twice at every key to throw the copies away at the next; it takes them once the pause is up.
- The window's state was written once for every change asked, each flushed to the disk; a write behind which newer ones wait is passed over.
- Every tab's pictures stayed on the GPU when a vault was shown or closed, and the menu bar note's when its tab closed.

Measured and left as they are: the first spelling and grammar check of a note, off the main thread, about 5 seconds of a core for the note of 20,000 lines, then answered from its kept runs; the window drawn at each blink in front (1.2 to 1.4% of a core), which a kept view of the editor alone would bring down.

Left for later:
- A picture opened in its own tab, and its viewer, are drawn from the path, which gpui keeps decoded, every frame of a GIF, and on the GPU, until the app quits: port Emaki's `pictures::Kept`, with a GIF played a frame at a time. SVG rows in the Assets card the same, small.
- A PDF's pages are kept by count, not size (about 120 MB at a frame's width, 1 GB at 4× zoom), a frame scrolled out of view keeps its pages, every page read parses the whole file again, and its pages' sizes are read on the main thread in the layout.
- A link card that could not be read writes no cache, so it is fetched again at every opening; the cache is never pruned and its key is `DefaultHasher`'s; two cards of one link fetch twice.
- A picture's source not seen before is found by walking the vault on the main thread in the layout; `Pictures::found` and `drawn` keep sources no longer in the note until it closes; a deleted note's tab (`drop_tab`) does not let its pictures go on the GPU.
- The Assets card draws every file at every frame, and the tree's chosen rows are looked up by a scan per row; `Motion` and `Tabs::fit` keep keys for every folder and title ever seen.
- The source pane copies and diffs the whole note at each key; a language other than English checks the whole note at each pause; undo has no bound.
- The menu bar note watches each tab's folder with a stream of its own; builds every remembered tab's editor at launch; reads the person's name at every opening.
- Settings: the font list matched against every family at every draw while it is open, and the window drawn at every one of the main window's.
- A hidden player asks the window for the keyboard at every frame (`sync_players`); the traffic lights are placed at every frame (`fold_lights`); the theme is copied several times a frame.
- `vault::tree` runs on the main thread as a vault is shown or renamed.
- Not measured here: the window behind another app's, which a probe cannot put there without taking the person's screen; read from the code, nothing in it wakes now but a terminal's output and the watcher's changes.
