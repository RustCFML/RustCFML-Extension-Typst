// A Typst template. The designer owns this file; CFML supplies only the data.
//
// `data.json` is not a file on disk — the extension registers the caller's
// struct under that name, serialised by the engine's own serializeJSON. That is
// why the data is DATA: there is no code path by which a value in here becomes
// Typst source.
#let data = json("data.json")

#import "_shared.typ": money, panel

#set page(paper: "a4", margin: 2cm, numbering: "1 of 1")
#set text(font: "Helvetica", size: 10pt)

// A show rule — the reason templates matter. One line restyles every heading in
// the document, and nothing in CFML has to know about it.
#show heading: it => block(below: 1em)[#text(weight: "bold", size: 15pt)[#it.body]]

= Statement #data.reference

#panel[
  #data.customer.name \
  #data.customer.address
]

#v(1em)

#table(
  columns: (1fr, auto, auto),
  align: (left, right, right),
  stroke: none,
  table.hline(),
  table.header([*Description*], [*Qty*], [*Amount*]),
  table.hline(),
  ..data.lines.map(l => (l.description, str(l.qty), money(l.amount))).flatten(),
  table.hline(),
  [*Total*], [], [*#money(data.lines.map(l => l.amount).sum())*],
  table.hline(),
)

#v(2em)
#text(size: 8pt)[Generated for #data.customer.name on #data.date.]
