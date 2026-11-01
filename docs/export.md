# Export to PDF

About `crates/onyx-app/src/export.rs` and `crates/onyx-core/src/pdf.rs`, with `pdf/template.typ` and `pdf/onyx.tmTheme` beside it: a note set on paper, the sheet that shows it, the file it makes, and printing.

## Rules

- **The note is never written.** Export takes what the editor holds, changes and all, or the disk's text for a note with no tab, and nothing goes back to the note: it is still written only when it is left or at ⌘S.
- **What shows is what is made.** The preview draws the pages of the document `pdf::lay_out` gave (`Laid::draw`), and the file is written from that same document (`Laid::pdf`). Never lay the note out again to write it, or the two can differ.
- **A note cannot run Typst.** Its words go in as Typst strings (`quoted`, `words`), each `#` piece ended by `;` so what follows cannot be read as part of it, and its math goes through MiTeX. The world (`Place`) hands out the template, the code theme, MiTeX's files and the pictures the note names, and nothing else of the disk. It has no network, so a picture at an address is its alt text.
- **One bad part does not lose the note.** A missing picture never reaches Typst. A formula or a picture Typst refuses fails the whole document, so when the whole fails each part is tried alone (`Bad`) and what failed is written again as code or as its alt text. The parts are tried only then, so a note with hundreds of formulas costs nothing.
- **A panic in Typst stays on `lay_out`'s thread.** It comes back as an error and a notice, so it cannot take notes with changes down with it. That thread also has a stack of its own (`STACK`), which deep lists and quotes need.
- **The PDF goes through `paths::write_atomic`.** A file that could not be finished leaves what was there.
- **A name the save panel did not ask about is not overwritten** (`pdf::output_path`). The panel confirms the name it was given; a name left without `.pdf` is finished here, and that path was never asked about, so a taken one gets a number.
- **A system font is read only if the note needs it.** The Apple Color Emoji file alone runs to hundreds of megabytes, so `gather` reads a fallback only for a character the chosen faces lack.

## How a note becomes a page

`pulldown-cmark` reads the note with the options the editor reads it with, and `==highlight==` is found by one function both use (`pdf::highlights`), so the two read a note alike. `Walk` writes the events as Typst after `head`, which sets the names `template.typ` reads, then the template, the note's name and the contents. `typst::compile` lays it out, `typst_render` draws a page, and `typst_pdf` writes the file.

A line break in a note is a line break on the page, as the editor shows it. The note's name is left out when the note opens with a top heading that says the same. The language of the page's rules (`language`) goes by what most of the note is in. Math is LaTeX written as Typst's by MiTeX, the Typst files of its package kept in `pdf/mitex/` (Apache-2.0), set in New Computer Modern Math, which the core carries for this and no note's words use.

| In a note | On the page |
|---|---|
| A link with a scheme | live, in the accent's link shade |
| A link to a note or a heading, a `[[wikilink]]` | its words, since it leads nowhere on paper |
| A picture beside the note, from the vault's top, or by a bare name anywhere in the vault | as large as it is or as its width asks (`![alt|300](src)`, `<img width>`), never past the words' width or most of a page |
| A picture missing, at an address, or Typst cannot read it | its alt text |
| A task | a box, ticked and struck when done; a list under it is not struck |
| A footnote | at the foot of the page; a second reference points at the first |
| A table | each column as wide as what stands in it asks, as the canvas sets it, or the widths a person set (`rhythm::shares_of`); the header repeated over a page and no row split |
| Code | the light appearance's inks (`onyx.tmTheme`), a wrapped line standing in from where it began; a long block runs over pages |
| A tag among the words | as the canvas draws it: `<u>`, `<mark>`, `<kbd>`, `<sub>`, `<sup>`, `<a href>`, a color (`#tag-ink`, off when Color is off); `<br>` is a line break |
| A block of HTML | written by `html::to_markdown` and walked as the note is (`Walk::html_block`), so it prints as it shows; `align` and `<center>` stand it across the page |
| A YAML header, a comment, a script, a style sheet | left out |

Tried and dropped: going straight to `Laid::pdf` with no sheet, which made every paper or margin a trip to the save panel and back.

## The sheet

⇧⌘E, File › Export to PDF…, or a note's menu in the tree or on its tab. The page stands on the left on the page's tint, with Page 1 of 3 under it and a press each way (← and → too); how it is set stands on the right, its options the union of Obsidian's and Craft's less what does not belong here (no watermark, no dark paper: a page is light, as Obsidian's always is). Every option is kept in `Config::pdf`, as Craft keeps its own, so the next note comes out alike.

- **Page**: the size (A4, Letter, Legal, A5, A3, Tabloid; until one is chosen, what the system's region prints on, `paper_of_locale`), the orientation, the margins.
- **Text**: the font and the code font, each the note's until one is chosen; the size in points, 8 to 16 by a half, which is print's own and not the screen's; the line spacing. The Chinese face and the fallbacks stay the note's (`fonts::pdf_look`).
- **On the page**: the note's name, page numbers (none on a note of one page), contents (none with no heading), color, a new page for each top heading.

A change is kept and the note laid out again (`set_export`). `SETTLE` makes a run of presses one layout, and `serial` drops the answer to an older one, as `drawing` does for a page. While the first layout runs the sheet shows blank paper of the right size. A new page fades in over the last over `SLIDE` and the two go to its size together, so a turn to landscape moves and nothing jumps. The page is drawn at the screen's own pixels and drawn again when the window has grown enough to show it soft.

↩ or Export… asks the system's panel where to; the panel opens in the last PDF's folder (`UiState::export_folder`, passed over once it is gone), else the note's. Cancelling it leaves the sheet as it was. Once a place is chosen the sheet goes, the file is written off the window's thread, and a quiet line says so; a press on it shows the file (`say_done`).

Closing the sheet lets Typst's cache go (`pdf::forget`). It holds what it laid out, and between options it is what makes the next layout quick (`comemo::evict(8)`).

Not done: exporting a folder, headers and footers, and Mermaid, which prints as code.

## Printing

⌘P, File › Print…, the printer at the path bar's end, a note's right click, or Print… on the sheet. It sets the note as the sheet last set it (`print_note`), changes and all, off the window's thread, and hands the PDF to the system (`sys::print_pdf`): PDFKit's print operation, its panel a sheet on the window, so the person has the Mac's own printers, pages, copies, preview and Save as PDF. The paper starts as the page's. From the export sheet it prints what the sheet shows. Off a Mac the PDF opens in the system's viewer, which prints it.

The export sheet comes up by ⇧⌘E, File › Export to PDF…, the page icon at the path bar's end, or a right click on the note, in the tree, on its tab or on its words.

## Size

Typst adds about 36 MB to the binary, most of it its text data and code highlighting, whatever a note uses.
