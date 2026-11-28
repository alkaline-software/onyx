# Probing

How to look at a state of the window from a script, without the person's hands.

## Rules

- Probe with a second copy, never the copy the person is using: `ONYX_HOME` at a scratch folder, and `ONYX_VAULT` at a vault made for the probe, never the person's.
- Under `ONYX_GO` a copy leaves nothing outside its window: a link or a file to open is said on stderr (`open …`) and not opened, what ⌘C or a menu's Copy would give is said (`copy "…"`) and the clipboard left alone, and what would go to the system's trash is said (`trash …`) and stays: a file removed is held in the copy's home (`state/held`) as in any launch, so ⌘Z can be probed, and is never handed to the Trash from there. A probe's note can hold any link, and a press that lands on the wrong row of a menu does no harm.
- A real click or key a script posts goes to the screen, not to the copy, and can land in the person's own windows even when the copy said it was in front. Use the `press=`, `key=` and `caret=` steps of `ONYX_GO`, which stay in the copy; `ONYX_TRACE` prints `mouse down` when a press reaches the editor.
- A key posted with `CGEvent.postToPid` to the copy's own process id stays in the copy and goes the whole way a typed key does: AppKit's key window, its first responder, gpui's input handler (`ONYX_TRACE` prints `key typed`). It is the way to check that a window takes typing at all, which `type=` and `menunote:type=` skip by calling the editor: the menu bar note took every `type=` while no real key reached it.
- A system panel holds a copy open: with `print` or a save panel up, `ONYX_SHOT` never quits it, and it stays on the person's screen. Start the copy on its own, keep its own process id (not a pipeline's) and end it by that id, then check that none is left.
- The steps that are not presses or keys (`caret=`, `type=`, `live`, `source`) run as the copy starts, before the first layout and before any press, whatever their place in the list; to order a mode among presses, send its key (`key=ctrl-1`, `key=ctrl-2`). The mode is remembered in the copy's home, so a run says the mode it wants.
- The person's own pointer, passing over the copy's window, moves its pointer too: a `point=` or `pichover=` state can be gone by the picture.
- Say what was checked and what was not. A synthetic click does not reach a click handler in a background window, so a picture of the state a click leads to does not show the click works.

## A copy

```
cargo build -p onyx-app
ONYX_HOME=<scratch>/home ONYX_VAULT=<scratch>/vault ONYX_NOTE=<scratch>/vault/Welcome.md ONYX_GO=live ONYX_SHOT=<file.png> ./target/debug/Onyx
```

| Variable | What it does |
|---|---|
| `ONYX_HOME` | the copy's own settings and state |
| `ONYX_APPEARANCE` | `light` or `dark`, for this launch only; never written back |
| `ONYX_LANG` | a language's code (`de`, `zh-Hans`), over the system's |
| `ONYX_VAULT` | the vault to open, over the remembered one; nothing is written to the window's state about it |
| `ONYX_NOTE` | the note to open in it |
| `ONYX_SETTINGS` | open the settings on that section (`general`, `font`, `terminal`, `menubar`, `about`) |
| `ONYX_GO` | steps, `;` between: `focus` (the keyboard in the note, which a synthetic press does not do in a background window, so the caret shows), `print` (⌘P; the panel is the system's and no picture of the window holds it), `cell=<row>,<col>` (the caret in that cell of the note's first table), `hover=<row>,<col>` (the pointer over it, so the handles show), `table=<what>,<row>,<col>` (a choice of a table's menu by its id, `row-below`, `col-right`, `align-right`, `sort-down`), `picture=<line>,<what>` (the picture on that line placed, `left`, `center`, `right`, sized, `auto` or `full`, given a width, or opened in the viewer, `view`, as its bar, its edge and its menu do), `pichover=<line>` (the pointer over that picture, so its edge takes a drag), `layout=<line>,<byte>,<what>` (the link at that byte of the line shown as `link`, `small`, `medium`, `large` or `preview`, as its menu does; a web card reads its page over the network into the copy's home), `point=<x>,<y>` (the pointer moved there in the window, nothing pressed), `wheel=<x>,<y>,<dy>` (a wheel's turn there, a gesture of its own, through the scroll router), `cmdwheel=<x>,<y>,<dy>` (the same with ⌘ held, which zooms a frame's PDF), `pinch=<x>,<y>,<d>` (a pinch there, 0.1 a tenth larger), `where` (the caret and the selection's other end, line and byte, and the note's scroll, on stderr), `files` (the note showing, the tabs, the rows chosen and lit, where the keyboard is and the vault's notes on the disk, on stderr, in its turn among the presses and keys: with `key=cmd-w`, `key=cmd-backspace`, `key=cmd-z` and `key=cmd-shift-z` it checks the file chosen and its keys), `type=<words>`, `text` (what the editor holds, on stderr), `tops` (where each line stands, on stderr, for measuring spacing; each takes a step of 400 ms, so a run of them also waits for the check), `updatefound` (a newer version waits: the gear's and About's dots; nothing is fetched), `checking`, `uptodate`, `checkfailed`, `downloading`, `installfailed` (each state the Updates button comes to; nothing is fetched), `check` (the button's own press: a real check, fetched), `loginrefused` (Open at login refused by the system: the mark before its switch), `shortcutwrong` (keys refused as the menu bar note's shortcut: Shortcut's line in the danger's ink), `notice` (a notice of a note not written, hung over the page), `note` (the vault's first note), `notes` (its first three, as tabs), `close` (⌘W: the tab showing, then the window), `hints` (the tabs' numbers, as ⌘ held shows them), `end` (the caret at the note's end, the view with it), `source` (the source pane shown or put away, as ⌘⇧M does), `srctype=<words>` (the keyboard in the source pane and those words typed there through its input handler, as keys are; the pane is made if it has not shown), `srcmenu=<x>,<y>` (a right click in the source pane at that place in the window, in its turn), `term` (the terminal shown or put away, as ⌘⇧S does), `termtype:<words>` (those words typed in the terminal's shell, then Return), `termput:<words>` (the same without Return), `termcursor` (where the shell's cursor is, its size, where its typing begins and the screen's text, on stderr), `term:tab+` and `term:tab-` (a tab made, the one showing closed), `term:sel:<unit>,<line>,<col>,<line>,<col>` (a selection, 0 to 3 by the gap, letter, word or row, 4 more for a block; what it copies on stderr), `term:copied` (the toast), `term:clear` (⌘K); each terminal step lands in its turn, after the first drawing, `wait=<ms>` (that much longer before the next step in its turn), `flick=<x>,<y>,<to x>,<to y>,<dy>` (a gesture begun at the first place and its momentum carried to the second, ten events of `dy`, as a finger lifted and the pointer moved on: the scroller it began in moves and the one under the second place does not; `where` and `termcursor` say how far each went), `term:findopen` (the terminal's find card, as ⌘F there), `term:find:<words>` (those words in it, and how many it found on stderr), `term:findnext`, `term:mark:<text>` and `term:commit:<text>` (text an input method is composing, then typed, as gpui's input handler gives it), `menunote` (the menu bar note opened or closed, as the item does), `menunote:hold=<file>` (that file in a tab), `menunote:close` (⌘W), `menunote:reopen` (⌘⇧T), `menunote:tab=<n>` (⌘ and that number), `menunote:drag=<x>,<y>,<to x>,<to y>` (a left drag in its window, in its points: a tab along its pill, a pill by its mark; the card stands `SHADE`, 32, in from the window's left, the room for its shadow), `menunote:look=<folder>` (that folder's icon and color sheet), `menunote:looksearch=<words>`, `menunote:lookcolor` (the new color's panel), `menunote:type=<words>` (typed in it), `menunote:pin`, `menunote:key=<keystroke>` (a key pressed in its window, as `key=` is in the main one), `menunote:press=<x>,<y>[,<clicks>]` (a left press and release there, `2` a double click), `menunote:point=<x>,<y>` (the pointer moved there, nothing pressed: a cut name's tip), `menunote:title=<name>` (the note's name typed and kept, as ↩ does; it renames the file, so the vault must be one the probe can lose), `menunote:tree` (the tree shut or opened, as ⌘⇧B does), `menunote:vaults` (the vault's menu), `menunote:vault=<folder>`, `menunote:fold=<folder>` (opened or shut), `menunote:pick=<file>`, `menunote:new=<folder>` (a new note's row there), `menunote:name=<words>` (its name given, as ↩ does), `menunote:onyx` (Open in Onyx), `menunote:shut` (another app taking the keyboard), `menunote:say` (its state, the text and the disk's, whether the window and the editor have the keyboard, on stderr), `menunote:files` (its tabs, the file chosen, where the keyboard is and the vault's notes on the disk, as `files` says the main window's) and `menunote:main` (the main window's vault and note), each in its turn; a probe's copy puts its own item in the menu bar while it runs, `sidebar`, `home` (the vault's page with the tabs left open), `scroll=<n>` (the note scrolled that far down, a big number for its end), `caret=<n>` (the caret to the start of that line, by the code the keys use, the view with it), `assets` (the sidebar's assets card open), `preview=<file>` (that file in a tab, as a press on it in the assets card opens it), `renamefile=<path>::<name>` (a file renamed as the rename sheet does it, an asset's links in every note with it, in its turn after a `preview=` before it; the vault must be one the probe can lose), `media` (what the preview's player has read of its file, a moment on: `media <ready state>,<width>,<height>,<length>,<error>` on stderr), `attach=<file>` (that file brought into the note at the caret, as a paste or a drop of it is: copied into the vault's `.assets` and referred to; the vault must be one the probe can lose), `press=<x>,<y>` (a left press and release at that place in the window, half a second on, sent to this window alone, so it goes through the hitboxes as a real one does, `press=<x>,<y>,<clicks>` for a double or triple click, `rpress=<x>,<y>` for the right button; presses and keys land apart, each on what the last drew, and the picture waits for them), `drag=<x>,<y>,<to x>,<to y>` (the same, moved before the release), `altdrag=` (the same with ⌥ held), `hold=` (the same with the button still down, for a picture mid-drag), `size=<w>,<h>` (the window to that size with no pointer in it, as a tiling key does), `sel` (what ⌘C would copy, on stderr), `key=<keystroke>` (a key as gpui names it, `enter`, `escape`, `cmd-a`, the same way; not typed text: a letter sent this way types nothing, in the note or in a field), `title=<name>` (the keyboard in the note's name and that name in it, not yet kept; a probe that keeps one needs a vault it can lose), `search` (the search palette with a phrase typed), `find` (the find bar over the note with a phrase typed), `find=<words>` (the same with those words), `menu` (the first note's menu), `rename` (its rename sheet), `vault` (the new-vault sheet; with no container, the system's folder panel, which a probe cannot answer), `export` (the export sheet on the note showing), `pdf=<option>:<value>` (an option of the sheet as `Config::pdf` names it, `format:image`, `paper:a5`, `margins:large`, `size:12`, `landscape:on`), `pdfpage=<n>` (that page, from 0), `pdfto=<file>` (the file written there in the sheet's format with no save panel, as the panel's answer would be); any value keeps the window over the others |
| `ONYX_SHOT` | a picture of the window (the settings when they are open) after it is drawn, then quit; `ONYX_SHOT_FRAME=1` keeps the system's shadow and rim; `ONYX_SHOT_OF=menunote` takes the menu bar note's window instead |
| `ONYX_TRACE` | prints, on stderr, when a key or a click arrives and when the frame after it begins, and what the editor's layout and paint cost, for finding where a wait is; `ONYX_GO=type` types a few words as the input method would |

Keys can be sent to a probe copy with `osascript -e 'tell application "System Events" to keystroke "ab"'` while it is in front, which is how the gap between a key and its frame was measured. Whether the source shows is remembered in the copy's home, and `source` turns it, so a probe that wants it says how it stands first or starts from a home without it. The terminal never shows at a launch. A process may take a picture of its own windows without Screen Recording access, which a terminal running a probe usually lacks.

## A press, checked

A press moves nothing but what it changes (`docs/design.md`, A press moves nothing), and a picture of each state alone does not show that: a row that grew by one line looks finished on its own. Each state a press leads to is shot beside the state before it, at the same section, language and appearance, and compared:

```
ONYX_HOME=<scratch>/home ONYX_VAULT=<scratch>/vault ONYX_SETTINGS=about ONYX_GO=wait=700 ONYX_SHOT=<dir>/rest.png ./target/debug/Onyx
ONYX_HOME=<scratch>/home ONYX_VAULT=<scratch>/vault ONYX_SETTINGS=about ONYX_GO=uptodate ONYX_SHOT=<dir>/uptodate.png ./target/debug/Onyx
Rscript scripts/press-diff.R <dir>/rest.png <dir>/uptodate.png
```

It prints, for the left column and the page apart, the box in points that holds every pixel that differs: the settings' rail is the left column by default, and `--split 272` makes it the main window's sidebar, `--split 640` the export sheet's page. The box must sit inside the control pressed (the Updates button is 216 by 28 at the right of its row), or in the rail for a dot. A box that reaches a row's or a card's edge, or anything under the control, is a press that moved the layout; a box that spans several controls is looked at in the two pictures, since it may hold two changes in place (a plate that slid and a row dimmed). The traffic lights differ between two launches as the window comes forward, and say nothing. A state a setting draws (a picture chosen, a size, a language, a container, an export's paper) is shot from a home whose `config.json` says so; a step that sets an option (`pdf=`) writes it to the home, so each shot starts from a copy of the same home. Done in English and in German and Japanese, whose words are longest; the widths that hold them were measured with the text system (`window.text_system().shape_line`) over every label each control can show, in every language.

## A corner, checked

A sharp corner is a few pixels at a box's corner and does not show in a picture of the whole window. Each rounded box's corners are cut from the probe's picture and enlarged eight times with no smoothing, in both appearances, with what can reach them there: a lit row, the terminal (`term`), a frame's pages scrolled to its foot (`wheel=`), the tree scrolled under its card's foot:

```
Rscript scripts/corners.R <dir>/term.png <dir>/corners.png term-bl=686,684 term-br=1043,684
```

Each `name=x,y` is a 20-point square from its top left, in points. A corner is round in the strip: nothing of what is inside the box (a strip, a ground, a row, a page) shows square past the curve. The menu bar note is pictured with `ONYX_SHOT_OF=menunote`.

## An iPhone and an iPad

A simulator stands in for the device, with no Simulator window open: it is booted and driven from the command line, and nothing reaches the person's screen.

```
cargo build -p onyx-app --target aarch64-apple-ios-sim
ONYX_HOME=<scratch>/home ONYX_VAULT=<scratch>/vault ONYX_GO='tap=70,276;wait=500;tap=200,300' scripts/ios-app.sh --log <file.log> "iPhone 17 Pro"
xcrun simctl io "iPhone 17 Pro" screenshot <file.png>
```

`scripts/ios-app.sh` makes the bundle, installs it in the simulator named (else the one booted, else an iPad) and starts it with every `ONYX_` variable handed on. A variable with a space in it is not handed on, so a step's words have none. A simulator's app reads the Mac's own files, so the scratch home and vault are the Mac's. `ONYX_SHOT` takes no picture there: `simctl io … screenshot` does, from outside, whenever it is asked, so wait for the steps first. `--log` keeps what the app prints (`where`, `files`, `uikeys`), through the simulator's console; `where` says how many files are open too (`open 1`), which on an iPhone is never more than one.

A device is the person's own: `scripts/ios-app.sh --device` signs the release build with the development profile and puts it on the device joined to the Mac, under Onyx's own identifier, so it stands where the store's copy would and opens the person's vaults. `ONYX_GO` and `--log` work there as in a simulator, but nothing on a device is a probe's to change, no picture is taken from the Mac, and a scratch home or vault on the Mac means nothing to it. Look at a device with the person, for what a simulator cannot say: iCloud, speed in the hand, a real finger.

The steps of `ONYX_GO` for a finger and a keyboard, each in its turn, in the window's points from its top left (the status bar is part of the window, so a row's place is its place in the picture divided by the screen's scale):

| Step | What it does |
|---|---|
| `tap=<x>,<y>[,<taps>]` | a touch begun and ended there; `2` a double tap |
| `pan=<x>,<y>,<to x>,<to y>` | a touch begun at the first place, moved to the second in ten steps and lifted: what is under it scrolls, and goes on by its own momentum |
| `longtap=<x>,<y>` | a touch held 700 ms, then lifted: the right button there, so a row's or a note's menu |
| `back=<how far>` | the swipe from the screen's edge, begun 12 points in and pulled in ten steps with a frame drawn between each, 0 to 1, let go there: past a half the system says it went back. `back=<how far>,1` leaves the finger down there. Sent in one frame, as it once was, it could not show a step lost to a page that had moved |
| `roll=<x>,<y>,<dx>,<dy>` | a thumb's tap that rolls: down there, moved by so much in four steps, and lifted. Past ten points gpui reads it as a pan, and on a button it is to press all the same |
| `panhold=<x>,<y>,<to x>,<to y>` | a finger's pan as `pan=` moves, held at its end 1.5 s before it lifts: a scroller pulled past its end shows its stretch then; `holds` on stderr as it stops |
| `rest=<x>,<y>` | a finger that rests there 450 ms and is taken back by the system: no tap, no long press, and for that while what is under it is lit. `rests` on stderr as it lands, to time a picture by |
| `open=<part>` | the vault's first note with that in its name, shown: for a device, whose paths a script does not know |
| `uikeys=<words>` | typed as the keyboard types them: handed to the view that has the keyboard by UIKit's own `insertText:`, from the run loop, and `uikeys true` on stderr when a view took them |
| `uidelete` | the keyboard's delete key once, the same way |

`tap`, `pan`, `roll`, `rest`, `longtap` and `back` go into gpui as the screen's touches do and through its own reading of them, so they check everything from there on, not UIKit's hit test. `uikeys` and `uidelete` go the whole way a typed key does (UIKit, gpui's backend, the input handler), which `type=` skips; they need the keyboard out, which it is once a tap has put the caret in words or in a field. A mouse's steps (`press=`, `drag=`) work there too and say nothing about a finger.

`reading` turns Reading on or off (`docs/window.md`), as the sidebar's step does, as the copy starts. `mark` prints `mark` at its turn: a script that watches the log takes a picture at each, so one launch gives a picture of every state the steps pass through. A press during a fling is `pan=…;wait=-900;tap=…`, the tap landing while the pan's momentum still counts. A tap while the keyboard is out lands on the window under the keys, where no finger reaches: keep such steps above the keyboard.

The system's glass is not gpui's to draw, so `shoot` does not show it: `simctl io … screenshot` does.

### A finger through UIKit

`ONYX_GO`'s touches go into gpui and skip UIKit, so they cannot check what UIKit does with a finger: the glass under it, the system's edit menu and loupe, the keyboard's own keys. `scripts/touch.R` starts the app installed in a simulator from a UI test (`scripts/touch/`, an XcodeGen project built once) and gives it XCTest's touches, which go the whole way a finger does:

```
Rscript scripts/touch.R "iPhone 17 Pro" 'wait 2500;double 129,319;wait 800;shot /tmp/a.png;menu;pick Copy' ONYX_HOME=<scratch>/home ONYX_VAULT=<scratch>/vault ONYX_TRACE=<scratch>/trace.log
```

The steps, in points from the screen's top left: `tap x,y`, `double x,y`, `hold x,y[,ms]`, `drag x,y,tx,ty[,held ms]` (down, held, moved and lifted), `holddrag x,y,tx,ty[,held ms]` (held long enough to be a hold first, then moved), `keys <letters>` (each pressed on the system keyboard, `\n` its Return, whatever it is labeled), `wait ms`, `shot <file.png>`, `menu` (prints the system's edit menu's choices) and `pick <choice>` (presses one). The `ONYX_` variables after the steps go to the app. A UI test's app has no console, so `ONYX_TRACE=/path` writes the trace to that file. XCTest waits on each step, so a picture in the middle of a hold or a drag is taken from a recording (`simctl io … recordVideo`) cut into frames.

Look at an iPhone and an iPad both, each in its states: no vault, a vault with no note, a note, the keyboard out and its bar, Reading on, the settings' sections, a menu. A small iPhone on an older system is worth a simulator of its own (an iPhone SE on iOS 18: 375 points wide, a home button, 60 frames).

On a device `ONYX_TRACE=1` with `--log` says what a simulator cannot, the Mac's processor and disk being what a simulator runs on: `stamp <ms> shell render begins` at each frame and `turn ends` when the window's thread is free again, `open note`, `note read`, `tab shown` round a note's opening, `trace slow line` for a line that took over 2 ms to lay out (what it is made of, never its words), `trace bring <n> files not at hand` as a vault is asked of iCloud, `glass flush` and `glass told` round the frame's plates told to the system (the frame is drawn and presented by the first of them), and `trace asks` for what the system's keyboard asked of the editor between two frames. The core's CLI runs in a simulator as a plain process, for what the system answers there: `xcrun simctl spawn <simulator> target/aarch64-apple-ios-sim/debug/onyx-core trash <path>`.

## The core from a terminal

```
cargo run -p onyx-core -- paths | config | update
cargo run -p onyx-core -- vault <folder>
cargo run -p onyx-core -- vault <folder> new <name>
cargo run -p onyx-core -- trash <path>
cargo run -p onyx-core -- --try-update <version> <copy of Onyx.app>
```

`vault <folder>` prints the tree as the window shows it, and how long it took to read, which is how a vault of thousands of notes is timed before anything is made faster.
