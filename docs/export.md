# Export

About `crates/onyx-app/src/export.rs` and, in the core, `pdf.rs` with `pdf/template.typ` and `pdf/onyx.tmTheme` beside it, `docx.rs`, `web.rs` and `reading.rs`: a note set as a PDF, an image, a Word document or a web page, the sheet that shows it, the file it makes, and printing.

The formats are Craft's (PDF, Image, Word, HTML) less those that are the note itself: Markdown, TextBundle and plain text are what the vault already holds.

## Rules

- **The note is never written.** Export takes what the editor holds, changes and all, or the disk's text for a note with no tab, and nothing goes back to the note: it is written as it always is, on its own (`docs/window.md`).
- **What shows is what is made.** The preview draws the pages of the document `pdf::lay_out` gave (`Laid::draw`), and a PDF or an image is written from that same document (`Laid::pdf`, `Laid::png`). Never lay the note out again to write it, or the two can differ. Word and the web page are written from the note (`docx::write`, `web::write`) by the same options and measures (`rhythm`), and the preview shows them as the PDF sets them: Word on its paper, the web page on one long page.
- **A note cannot run Typst.** Its words go in as Typst strings (`quoted`, `words`), each `#` piece ended by `;` so what follows cannot be read as part of it, and its math goes through MiTeX. The world (`Place`) hands out the template, the code theme, MiTeX's files and the pictures the note names, and nothing else of the disk. It has no network, so a picture at an address is its alt text.
- **One bad part does not lose the note.** A missing picture never reaches Typst. A formula or a picture Typst refuses fails the whole document, so when the whole fails each part is tried alone (`Bad`) and what failed is written again as code or as its alt text. The parts are tried only then, so a note with hundreds of formulas costs nothing.
- **A panic in Typst stays on `lay_out`'s thread.** It comes back as an error and a notice, so it cannot take notes with changes down with it. That thread also has a stack of its own (`STACK`), which deep lists and quotes need.
- **The PDF goes through `paths::write_atomic`.** A file that could not be finished leaves what was there.
- **A name the save panel did not ask about is not overwritten** (`pdf::output_path`). The panel confirms the name it was given; a name left without `.pdf` is finished here, and that path was never asked about, so a taken one gets a number.
- **A system font is read only if the note needs it.** The Apple Color Emoji file alone runs to hundreds of megabytes, so `gather` reads a fallback only for a character the chosen faces lack. It reads the note's name as well as its words: a name often carries an emoji, and one missed there printed as a box over the page.
- **Nothing runs past its edge.** A run with nowhere to break (a key, an address, a long token) that is wider than its line may break between any two of its characters, as a browser's `overflow-wrap: anywhere` does: in the words (`long`, `unbroken` in the template), in a code block, and in code among the words (`code-run`). A run that fits is left whole. The break is an empty box, not a zero-width space, which would ride along when the run is copied out of the PDF and break a pasted key.

## How a note becomes a page

`pulldown-cmark` reads the note with the options the editor reads it with, and `==highlight==` is found by one function both use (`pdf::highlights`), so the two read a note alike. `Walk` writes the events as Typst after `head`, which sets the names `template.typ` reads, then the template, the note's name and the contents. `typst::compile` lays it out, `typst_render` draws a page, and `typst_pdf` writes the file.

A line break in a note is a line break on the page, as the editor shows it. The note's name is left out when the note opens with a top heading that says the same. The language of the page's rules (`language`) goes by what most of the note is in. Math is LaTeX written as Typst's by MiTeX, the Typst files of its package kept in `pdf/mitex/` (Apache-2.0), set in New Computer Modern Math, which the core carries for this and no note's words use.

| In a note | On the page |
|---|---|
| A link with a scheme | live, in the accent's link shade |
| A link to a note or a heading, a `[[wikilink]]` | its words, leading to the note set after the words when Linked files is on (below), else plain words |
| An embedded PDF, note or other file (`![](a.pdf)`, `![[Plan]]`, `<embed>`), or a card of one (`<a data-card>`) | a card as the editor's medium one, whatever the line asks: its words or its file's name, a few of a document's words, its kind and size, and a PDF's first page beside them, the whole leading to the file set after the words; alone in its paragraph, else its name leading there; with Linked files off, its words. A card of a page of the web is its link |
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

## Linked files

As Craft sets a document's pages after it, the files a note embeds or links to are set after its words, once each, in the order the note first names them (`attach::gather`): a PDF by its pages, a note as a note is set, and any other file the preview reads as markdown (code, a table, a notebook, a Word document, a deck, a workbook) as that markdown (`preview::read`). A picture is in the words already, and a video or a sound has nothing to set on paper, so neither comes after; a missing file, an address and the note itself are passed by. Only the note's own references are followed: a note set after it brings none of its own, though its link to a file already set here leads there.

The card is the same in every format (`reading::Card`, `file-card` in the template): Typst sets it in a box with round corners, the web page in a linked box, Word in a table of one row with square corners, as Word has no round ones. A PDF's first page is drawn for it and cut to the top two thirds of its width (`attach::card_picture`): Typst sets what overflows a box in the box's middle, so the page cut by the card's edge would show its middle, not its top. The contents and the PDF's outline list what is set after the note under one entry, Appendix, in English whatever the note's language, each file by its name under it (`appendix-entry`, headings laid out where the file starts and not seen); a note set after it keeps its own headings out of them. A note is read in parts (`html::parts`) for what it names as for what it sets, so a file named on the line under a picture is found.

Each file starts on a new page under a quiet line: "↑ the note's name" at the left, leading back to where the note first names it, and the file's name at the right. A file named only in a footnote leads back to the top. A note set after it shows its name as the note's own does (Note's name, unless it opens with a heading that says it); a PDF's pages each stand on a page of their own as large as the words' room holds them, with a hairline edge, as Typst reads a PDF as a picture (`pdf-page` in the template). On the long page of an image or the web page each file comes under a rule instead of on a new page. Word gets bookmarks and links to them (`_OnyxPart`, `_OnyxFrom`), a page break before each file, and a PDF's pages as pictures at twice the words' width in points; the web page gets `id`s and `#` links, a section for each file, and a PDF's pages as pictures in the file (`attach::pages_as_pictures`, one reading of the PDF for all its pages). Printing sets them as the PDF does.

Nothing is read for a note that names no file, so such a note is set exactly as before. A PDF is read once and kept, as Typst's own bytes, for the sheet's next layout, and let go when the sheet closes (`attach::forget`, from `pdf::forget`) or once a print is set. A PDF too large for the preview (`attach::PDF_BYTES`), locked or unreadable is not set after the note and stays its words; one Typst refuses is its line alone (`Bad::attached`). The sheet's Linked files switch (`PdfConfig::attached`, on by default) turns all of this off.

## The sheet

⇧⌘E, File › Export…, or a note's menu in the tree or on its tab. The format stands at the top of the options, PDF, Image, Word or HTML, and is kept with them (`PdfConfig::format`); what does not apply to a format leaves the sheet (an image and a web page have no page numbers and no new page for a heading, a web page no paper). The page stands on the left on the page's tint, with Page 1 of 3 under it and a press each way (← and → too); how it is set stands on the right, its options the union of Obsidian's and Craft's less what does not belong here (no watermark, no dark paper: a page is light, as Obsidian's always is). Every option is kept in `Config::pdf`, as Craft keeps its own, so the next note comes out alike.

- **Page**: the size (A4, Letter, Legal, A5, A3, Tabloid; until one is chosen, what the system's region prints on, `paper_of_locale`), the orientation, the margins.
- **Text**: the font and the code font, each the note's until one is chosen; the size in points, 8 to 16 by a half, which is print's own and not the screen's; the line spacing. The Chinese face and the fallbacks stay the note's (`fonts::pdf_look`).
- **On the page**: the note's name, page numbers (none on a note of one page), contents (none with no heading), color, a new page for each top heading, and linked files (above).

A change is kept and the note laid out again (`set_export`). `SETTLE` makes a run of presses one layout, and `serial` drops the answer to an older one, as `drawing` does for a page. While the first layout runs the sheet shows blank paper of the right size. A new page fades in over the last over `SLIDE` and the two go to its size together, so a turn to landscape moves and nothing jumps. The page is drawn at the screen's own pixels and drawn again when the window has grown enough to show it soft.

↩ or Export… asks the system's panel where to; the panel opens in the last PDF's folder (`UiState::export_folder`, passed over once it is gone), else the note's. Cancelling it leaves the sheet as it was. Once a place is chosen the sheet goes, the file is written off the window's thread, and a quiet line says so; a press on it shows the file (`say_done`).

Closing the sheet lets Typst's cache go (`pdf::forget`). It holds what it laid out, and between options it is what makes the next layout quick (`comemo::evict(8)`).

Not done: exporting a folder, headers and footers, and Mermaid, which prints as code.

## Image and the web page

An image and a web page are set on one page as long as the note (`PdfConfig::long`, `height: auto`), as wide as the chosen paper less its margins, so the preview scrolls rather than turns. An image is that page drawn at two pixels a point, as a Retina screen shows it, fewer for a note so long the picture would pass 32,000 pixels (`Laid::png`). A picture on a long page has no page height to keep within.

## Word and the web page

`reading::read` turns the note into a small tree of blocks once, and both writers set it. Word gets real styles (Title, Heading 1 to 6, Code, Hyperlink, footnotes, a table of contents field), real numbering for lists, real footnotes, and each picture embedded once; the web page is one file with its styles and pictures inside (`data:` addresses), code that wraps, and footnotes at its end. Neither carries Onyx's faces, so a reader without Inter sees their own; syntax colors and set math are the PDF's alone.

## Printing

⌘P, File › Print…, a note's right click, or Print… on the sheet; the path bar has Export alone, as the sheet prints too. It sets the note as the sheet last set it (`print_note`), changes and all, off the window's thread, and hands the PDF to the system (`sys::print_pdf`): PDFKit's print operation, its panel a sheet on the window, so the person has the Mac's own printers, pages, copies, preview and Save as PDF. The paper starts as the page's. From the export sheet it prints what the sheet shows. Off a Mac the PDF opens in the system's viewer, which prints it.

The export sheet comes up by ⇧⌘E, File › Export…, the page icon at the path bar's end, or a right click on the note, in the tree, on its tab or on its words.

## Size

Typst adds about 36 MB to the binary, most of it its text data and code highlighting, whatever a note uses.
