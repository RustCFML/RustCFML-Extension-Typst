// Imported by statement.typ, which proves the template root works: with no
// root — the shape a generated document uses — `#import` cannot resolve at all.

#let money(n) = [#("£" + str(n))]

#let heading-style = (weight: "bold", size: 14pt)

#let panel(body) = block(
  fill: rgb("#f4f6f8"),
  stroke: rgb("#d6dbe0"),
  radius: 3pt,
  inset: 10pt,
  width: 100%,
  body,
)
