// How a note looks on paper. `pdf.rs` sets the names this reads ahead of
// it (`base`, `ink`, `accent`, `link-ink`, `plate`, `chip`, `wash`,
// `border`, `muted`, `words`, `code`, `leading`, `gap`, `colored`,
// `numbered`, `words-room`, `title-scale`) and writes the note after it. Sizes go by the editor's
// scale of the words' size.

#import "/mitex/mod.typ": mitex-scope

#set text(font: words, size: base, fill: ink, hyphenate: false)
#set par(leading: leading, spacing: gap)
#set footnote.entry(separator: line(length: 24%, stroke: 0.5pt + border), gap: 0.5em)
#show footnote.entry: set text(0.86 * base)

#set list(marker: ([•], [◦]), indent: 0.3em, body-indent: 0.55em, spacing: leading + 0.1em)
#set enum(indent: 0.3em, body-indent: 0.55em, spacing: leading + 0.1em)

#show link: set text(fill: link-ink)
#show math.equation: set text(font: "New Computer Modern Math")

// A run with nowhere to break, a key or an address, wider than the line
// it stands in: it may break between any two of its characters, as a
// browser's `overflow-wrap: anywhere` does, so it never runs past the
// page or out of its plate. A run that fits is left whole.
#let long = regex("[^\\s\\x{200B}\\p{Han}\\p{Hiragana}\\p{Katakana}\\p{Hangul}]{16,}")
#let unbroken(room) = run => context if measure(run).width > room.to-absolute() { run.text.clusters().join(box()) } else { run }
#show long: unbroken(words-room - 2em)

#set raw(theme: if colored { "/onyx.tmTheme" } else { none })
#show raw: set text(font: code, size: 0.86 * base)
#show raw.where(block: false): it => highlight(fill: chip, radius: 3pt, extent: 1.5pt, it)
// A wrapped line of code stands in from where its line began.
#show raw.where(block: true): it => block(fill: plate, inset: 0.9em, radius: 5pt, width: 100%, above: 1.2em, below: 1.2em, layout(room => {
  show long: unbroken(room.width - 1.5em)
  it.lines.map(line => block(above: 0.5em, below: 0.5em, par(hanging-indent: 1.5em, leading: 0.5em, box(height: 0.72em) + line))).join()
}))

#show heading: set block(above: 1.5em, below: 0.65em)
#show heading: set text(weight: "semibold")
#show heading.where(level: 1): set text(size: heads.at(0) * base, weight: "bold")
#show heading.where(level: 2): set text(size: heads.at(1) * base)
#show heading.where(level: 3): set text(size: heads.at(2) * base)
#show heading.where(level: 4): set text(size: heads.at(3) * base)
#show heading.where(level: 5): set text(size: heads.at(4) * base)
#show heading.where(level: 6): set text(size: heads.at(5) * base)

#show quote.where(block: true): it => block(stroke: (left: 2pt + accent), inset: (left: 1em, y: 0.1em), above: 1.2em, below: 1.2em, it.body)

#set table(stroke: 0.5pt + border, inset: (x: 0.6em, y: 0.5em), fill: (_, y) => if y == 0 { plate })
#set table.cell(breakable: false)
#show table.cell.where(y: 0): set text(weight: "semibold")
#show table: set block(above: 1.2em, below: 1.2em)

#show title: set text(size: title-scale * base, weight: "bold")
#show title: set block(below: 1.5em, stroke: (bottom: 0.5pt + border), inset: (bottom: 0.7em), width: 100%)

#set outline.entry(fill: repeat(gap: 0.4em, text(muted, [.])))
#show outline.entry.where(level: 1): set text(weight: "semibold")

#let mark = highlight.with(fill: wash, extent: 1.5pt, radius: 2pt)
#let tex(s, b) = math.equation(block: b, eval(s, mode: "math", scope: mitex-scope))
#let rule = line(length: 100%, stroke: 0.5pt + border)

// What a tag among the words asks for.
#let code-words(body) = text(font: code, size: 0.86 * base, highlight(fill: chip, radius: 3pt, extent: 1.5pt, top-edge: 1.02em, bottom-edge: -0.3em, body))
// Code among the words. Too wide for a line, it breaks between any two
// characters. Each character is then a piece of its own, and a piece's
// chip would be drawn over the character before, so the pieces' chips
// do not reach past them and each end gets a cap as wide as the reach.
#let code-run(s) = context {
  let set-in(c) = text(font: code, size: 0.86 * base, c)
  if measure(set-in(s)).width <= (words-room - 2em).to-absolute() { return code-words(s) }
  let reach = 1.5pt
  let cap(side) = box(fill: chip, width: reach, height: 0.86 * base * 1.32, baseline: 0.86 * base * 0.3, radius: ((side): reach))
  let piece(c) = set-in(highlight(fill: chip, extent: 0pt, top-edge: 1.02em, bottom-edge: -0.3em, c))
  let cs = s.clusters()
  // A cap stays on the line of the character it closes.
  box(cap("left") + piece(cs.first())) + cs.slice(1, -1).map(piece).join(box()) + box(piece(cs.last()) + cap("right"))
}
#let tag-ink(r, g, b, body) = if colored { text(fill: rgb(r, g, b), body) } else { body }
#let tag-wash(r, g, b, body) = if colored { highlight(fill: rgb(r, g, b, 128), radius: 2pt, extent: 1.5pt, body) } else { body }

// A picture as large as it is, as it asks in pixels, or as a share of
// the words' width, never past that width or most of a page's height.
#let pic(p, alt, asked) = layout(room => context {
  let natural = measure(image(p))
  let wanted = if asked == none { natural.width } else if type(asked) == ratio { asked * room.width } else { asked * 0.75pt }
  // A page as long as the note has no height to keep it within.
  let tallest = if page.height == auto { wanted / natural.width } else { 0.8 * page.height / natural.height }
  let scale = calc.min(wanted / natural.width, room.width / natural.width, tallest)
  align(center, image(p, alt: alt, width: scale * natural.width))
})

#let tick = box(baseline: 0.12em, width: 0.84em, height: 0.84em, radius: 2.5pt, fill: accent, place(dx: 0.19em, dy: 0.42em, curve(stroke: (paint: white, thickness: 0.09em, cap: "round", join: "round"), curve.move((0pt, 0pt)), curve.line((0.16em, 0.17em)), curve.line((0.46em, -0.2em)))))
#let open-box = box(baseline: 0.12em, width: 0.84em, height: 0.84em, radius: 2.5pt, stroke: 0.09em + muted)
#let row(lead, body) = grid(columns: (auto, 1fr), column-gutter: 0.5em, lead, body)
#let task(done, line, rest) = row(if done { tick } else { open-box }, {
  if done { text(muted, strike(line)) } else { line }
  rest
})
#let plain(body) = row(box(width: 0.84em, align(center, [•])), body)
#let tasks(tight, ..items) = list(tight: tight, marker: [], indent: 0.3em, body-indent: 0pt, ..items.pos())

#set page(numbering: if numbered { "1" }, footer: if numbered { context if counter(page).final().first() > 1 { align(center, text(0.8 * base, muted, counter(page).display())) } })

// A file set after the note: a quiet line over it, the way back to
// where the note names it at the left and the file's name at the right;
// a note's name as the note's own stands; a PDF's page as large as the
// page's words can hold it, or on a long page as wide as the words.
#let part-head(back, title, name) = block(width: 100%, below: 1.4em, {
  show link: set text(fill: muted)
  set text(0.86 * base, muted)
  grid(columns: (1fr, auto), column-gutter: 1em, link(back)[↑ #title], name)
})
#let part-title(name) = block(below: 1.5em, stroke: (bottom: 0.5pt + border), inset: (bottom: 0.7em), width: 100%, text(size: title-scale * base, weight: "bold", name))
#let part-gap = { v(2.4em); rule; v(1.2em) }
// A file the note embeds, set after it, as the editor's medium card: its
// name, a few of its words, its kind and size, a PDF's first page beside
// them (the top of it, cut to the card's shape in `attach::card_picture`),
// the whole leading to where the file is set.
#let file-card(target, title, meta, words, thumb) = block(width: 100%, height: 7.4em, above: 1.2em, below: 1.2em, breakable: false, radius: 6pt, stroke: 0.5pt + border, clip: true, link(target, grid(
  columns: if thumb == none { (1fr,) } else { (1fr, 11.1em) },
  rows: (7.4em,),
  block(width: 100%, height: 100%, inset: (x: 1.1em, y: 1em), {
    set text(fill: ink)
    set par(leading: 0.5em, spacing: 0pt)
    block(height: 1.3em, clip: true, below: 0.35em, text(weight: "semibold", title))
    if words != none { block(height: 2.6em, clip: true, text(0.86 * base, muted, words)) }
    place(bottom + left, text(0.8 * base, muted, meta))
  }),
  ..if thumb != none { (block(width: 100%, height: 100%, stroke: (left: 0.5pt + border), fill: white, clip: true, image(thumb, width: 100%, height: 100%, fit: "cover")),) },
)))
// What the contents and the PDF's outline list for what is set after the
// note, laid out where it starts and not seen: Appendix, and each file
// under it.
#let appendix-entry(level, name) = place(hide(heading(level: level, outlined: true, name)))
#let pdf-page(p, n, long) = if long {
  block(width: 100%, stroke: 0.5pt + border, image(p, page: n, width: 100%))
} else {
  block(width: 100%, height: 1fr, layout(room => {
    let natural = measure(image(p, page: n))
    let k = calc.min(room.width / natural.width, room.height / natural.height)
    align(center + horizon, box(stroke: 0.5pt + border, image(p, page: n, width: k * natural.width)))
  }))
}
