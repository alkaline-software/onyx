// How a note looks on paper. `pdf.rs` sets the names this reads ahead of
// it (`base`, `ink`, `accent`, `link-ink`, `plate`, `chip`, `wash`,
// `border`, `muted`, `words`, `code`, `leading`, `gap`, `colored`,
// `numbered`) and writes the note after it. Sizes go by the editor's
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

#set raw(theme: if colored { "/onyx.tmTheme" } else { none })
#show raw: set text(font: code, size: 0.86 * base)
#show raw.where(block: false): it => highlight(fill: chip, radius: 3pt, extent: 1.5pt, it)
// A wrapped line of code stands in from where its line began.
#show raw.where(block: true): it => block(fill: plate, inset: 0.9em, radius: 5pt, width: 100%, above: 1.2em, below: 1.2em, it.lines.map(line => block(above: 0.5em, below: 0.5em, par(hanging-indent: 1.5em, leading: 0.5em, box(height: 0.72em) + line))).join())

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

#show title: set text(size: 2 * base, weight: "bold")
#show title: set block(below: 1.5em, stroke: (bottom: 0.5pt + border), inset: (bottom: 0.7em), width: 100%)

#set outline.entry(fill: repeat(gap: 0.4em, text(muted, [.])))
#show outline.entry.where(level: 1): set text(weight: "semibold")

#let mark = highlight.with(fill: wash, extent: 1.5pt, radius: 2pt)
#let tex(s, b) = math.equation(block: b, eval(s, mode: "math", scope: mitex-scope))
#let rule = line(length: 100%, stroke: 0.5pt + border)

// What a tag among the words asks for.
#let code-words(body) = highlight(fill: chip, radius: 3pt, extent: 1.5pt, text(font: code, size: 0.86 * base, body))
#let tag-ink(r, g, b, body) = if colored { text(fill: rgb(r, g, b), body) } else { body }
#let tag-wash(r, g, b, body) = if colored { highlight(fill: rgb(r, g, b, 128), radius: 2pt, extent: 1.5pt, body) } else { body }

// A picture as large as it is or as it asks in pixels, never past the
// words' width or most of a page.
#let pic(p, alt, asked) = layout(room => context {
  let natural = measure(image(p))
  let wanted = if asked != none { asked * 0.75pt } else { natural.width }
  let scale = calc.min(wanted / natural.width, room.width / natural.width, 0.8 * page.height / natural.height)
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
