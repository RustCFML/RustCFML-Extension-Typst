# What Typst can do, and how much of it this extension exposes

Written 2026-08-26, against Typst 0.15. The point of this page is that the
boundary should be **stated rather than discovered** — the same rule the
`<cfdocument>` plan applies to its HTML subset.

Short version: the builder covers *business documents* — invoices, statements,
reports, letters. It covers very little of Typst's **typesetting** and almost
none of its **programmability**, and the single largest gap is that a document
cannot be a **template**.

---

## 1. How the pieces fit

Typst is a typesetting system — the modern answer to LaTeX. You write markup,
it lays the text out and exports a PDF. Two things make it a better fit for
generating documents from a program than an HTML-to-PDF pipeline:

* it is a **library**, not a subprocess. There is no headless browser, no
  wkhtmltopdf, no temporary directory. `typst::compile()` is a function call:
  ~6 ms for a two-page invoice with a table, ~2 ms to export the PDF;
* it is **paged by construction**. Page size, margins, running headers and
  footers, page counters and page breaks are first-class, not properties you
  coax out of a layout engine designed for a scrolling viewport.

What this extension does with it:

```
   CFML                       Rust                          Typst
   ────                       ────                          ─────
   Document()            →    a document model              (nothing yet)
     .heading(…)              (src/model.rs)
     .table(…)               accumulated, not emitted
     .image(…)                image bytes held aside
        │
        └─ .toTypst()  ─→     the emitter runs once         markup text
           .toBinary()        (src/emit.rs)                     │
           .write(…)                                            ▼
                              a World with one source     typst::compile()
                              file, the host's fonts,           │
                              and only the images you           ▼
                              added (src/world.rs)        typst_pdf::pdf()
                                                                │
                                                                ▼
                                                             PDF bytes
```

Three deliberate properties fall out of that shape:

**The markup is generated once, at the terminal.** The model accumulates; nothing
is emitted until `.toTypst()`/`.toBinary()`/`.write()`. Emitting incrementally
would make Typst's `#set` scoping depend on call order in ways a caller cannot
see.

**Your text is data, never code.** Every value you pass is emitted as a Typst
*string literal*, and every verb is emitted in Typst's *code* mode —
`#heading(level: 1)[#"…"]`, `#table(…)`. A paragraph containing `#set page(fill:
black)` renders as those characters. Measurements and colours are the only place
a bare expression is emitted, which is why they are whitelists rather than
passthroughs.

**The document cannot reach the filesystem or the network.** `World::source()`
serves exactly one file — the generated `main.typ` — and returns `NotFound` for
everything else, so `#import` has nowhere to go. `World::file()` serves only the
images the builder registered by generated name. Neither is an omission; both are
the security boundary, and §4 below is where it gets in the way.

---

## 2. Exposed today

| Typst capability | How you reach it |
|---|---|
| `#set page(paper:, width:, height:, margin:, flipped:, numbering:, columns:, fill:)` | `pageSize` / `paper`, `margin`, `orientation`, `numbering`, `columns`, `pageFill` |
| `#set text(font:, size:, fill:)` | `font`, plus `format` on any verb |
| `#set heading(numbering:)` | `headingNumbering` |
| `#heading(level:)` | `heading`, with `label` |
| Paragraph text | `paragraph` / `text` / `p` |
| **Inline runs** — `#strong`, `#emph`, `#underline`, `#strike`, `#smallcaps`, `#linebreak` | An array of runs anywhere `text` is taken |
| `#link` | A `{ link: … }` run, or `link()` |
| `#footnote` | A `{ footnote: … }` run |
| `#ref` / `<label>` | A `{ ref: … }` run, and `label=` on headings, tables and images |
| `#list` / `#enum` | `list( ordered= )` |
| `#terms` | `terms` |
| `#quote(attribution:)` | `quote` |
| `#raw(lang:, block: true)` | `code` |
| `#block(fill:, stroke:, radius:, inset:, width:)` | `callout` / `box` / `panel` |
| `#table(columns:, align:, fill:, stroke:, gutter:, inset:)`, `table.header`, `table.cell` | `table` — see the API reference; `grid` is the stroke-less variant |
| `#figure(caption:)` | `caption=` on `table` and `image` |
| `#outline(title:, depth:)` | `outline` / `toc` |
| `#image()` | `image`, from a path or `Binary` — crosses as **bytes** |
| `#pagebreak()`, `#v()`, `#line()`, `#align()` | `pageBreak`, `spacer`, `rule`, `align=` |
| Running header/footer, `#counter(page)` | `header` / `footer`, with `{page}` and `{total}` |
| Document metadata | `title`, `author`, `keywords` |
| **`#let`, show rules, `json()`, `#import`** | **Templates** — `template()` + `data()`, or `typstRender()` |
| PDF export options — standards, tagging, page ranges, creator, timestamp | `pdf()` |
| PDF export | `toBinary` / `toPdf` / `write` |
| Layout without export | `pageCount` |
| Anything at all | `.typst( markup )`, and `typstCompile( markup )` |

---

## 3. Not exposed

Grouped by how much it would take, because "missing" and "missing and hard" are
different answers.

### 3.1 More verbs over the same emitter

Nothing structural stands in the way of these; they are model variants and
emitter arms.

* **Grid beyond a table** — `#grid` with cell spans (`colspan`/`rowspan`) and
  per-cell strokes. `grid()` gets you a stroke-less table; it does not get you a
  spanning layout.
* **Shapes and transforms** — `#rect`, `#circle`, `#ellipse`, `#polygon`,
  `#curve`, `#rotate`, `#scale`, `#pad`, `#move`, `#repeat` (dot leaders).
* **`#stack`** and **`#place`** — including floats, i.e. a figure that moves to
  the top of the page.
* **Gradient and tiling fills** — a colour is a solid colour here.
* **Typography controls** — `hyphenate`, `lang`/`region`, `tracking`, `spacing`,
  `baseline`, `#smartquote`, `#sub`/`#super`, `#highlight`, `#overline`.
* **Nested blocks.** A `callout` holds inline runs, not a list or a table. The
  model is flat: blocks are a sequence, not a tree. This is the one entry here
  that is a *design* limit rather than a missing arm, and the one most likely to
  need revisiting.
* **Caption position.** A captioned table's caption sits below it, Typst's
  default for the `table` kind here. Moving it above needs a show rule
  (`#show figure.caption: set …`), so today it needs `.typst()`.
* **Figure supplement.** Captions are emitted with `supplement: none`, so a
  cross-reference to a captioned table renders as a bare number rather than
  "Table 1". Exposing the supplement is a one-argument change.

### 3.2 Needs a concept the builder does not have yet

* **Math mode** — `$ x^2 $`, and the whole of Typst's equation layout. Reachable
  through `.typst()` and, in a template, through the template. Whether a
  document-generation extension should expose maths at all is a genuine question;
  it is *the* thing Typst is best at.
* **Bibliographies and citations** — `#bibliography("refs.bib")`, `#cite`, with
  Hayagriva/BibLaTeX. These now *work inside a template*, because a template root
  can hold the `.bib` file. There is no builder verb for them.
* **Other export formats.** `typst-render` gives PNG, `typst-svg` gives SVG, and
  0.15 has experimental HTML export. We build only `typst_pdf`. Rasterising works
  today by handing the PDF back to the engine — `pdfToImage( pdfRead( … ) )` —
  which is fine, and is one more dependency and one more decode than
  `typst-render` would be.
* **Dates.** `World::today()` deliberately returns `None`, so `datetime.today()`
  inside markup yields nothing. That is on purpose — the same inputs should
  produce the same PDF — and `.pdf( timestamp = )` is how a caller stamps one. A
  template that wants today's date should be given it in `data`.
* **`<cfdocument>` and its HTML subset** — designed in [PLAN.md](PLAN.md) §4–5,
  not built. It needs an HTML→Typst translator, and the inline-run model that
  landed for the builder is the half of it that was missing.

### 3.3 Deliberately out of reach

* **Typst packages** — `#import "@preview/cetz:…"`. Resolving one reaches the
  network, and nothing in this extension does. Vendor a package under a template
  root and `#import` it by path.
* **Arbitrary filesystem access from a generated document.** No root, by
  construction. Templates relax this to exactly one directory.

---

## 4. Templates — what shipped

[PLAN.md](PLAN.md) §6's third open question, answered: yes, and it came first.

```cfml
pdf = Document().template( expandPath( "/templates/statement.typ" ) )
                .data( { reference: "STMT-7", customer: customer, lines: lines } )
                .toBinary();
```

```typst
#let data = json("data.json")
#import "_shared.typ": money, panel

#show heading: it => block(below: 1em)[#text(weight: "bold", size: 15pt)[#it.body]]

= Statement #data.reference
#panel[#data.customer.name]
#table(
  columns: (1fr, auto, auto),
  ..data.lines.map(l => (l.description, str(l.qty), money(l.amount))).flatten(),
)
```

Three decisions inside it are worth stating, because each of them was a choice:

1. **The root is one canonical directory**, defaulting to the template's own,
   and every lookup goes through `VirtualPath::realize` — Typst's own
   root-relative resolver. Not a hand-rolled `starts_with` check: the escape
   cases (`..`, symlinks, absolute paths) are exactly what a hand-rolled version
   gets wrong. `read("../../../../etc/hosts")` is refused with Typst's own
   message, and there is a test asserting the *reason*, not just the failure.
2. **Data crosses as a virtual `data.json`**, not as generated `#let` source. It
   keeps the template idiomatic — `json()` is how a Typst author already reads
   data — and it preserves the property the builder has: your values are data and
   cannot become code. A test proves it by page count, since an executed
   `#pagebreak()` in the data would produce a second page.
3. **`serializeJSON` is the engine's**, called back over the ABI, so `data.json`
   holds exactly the JSON the rest of the application would produce. A second
   JSON writer here would have its own opinions about dates, numeric strings and
   key case.

What it unlocked immediately: `#let`, show rules, `#import`, `json()`/`csv()`/
`read()` within the root, and `.bib` bibliographies — none of which could resolve
while the `World` served a single file.

What it does not remove: the builder still earns its place. It needs no designer,
no template file and no `#` escaping, and it is right when the document *is* just
the data. Templates and the builder are two entry points on the same `World`.

---

## 5. Recommended order from here

1. **`<cfdocument>` and HTML→Typst** (PLAN §4–5) — the compatibility story, and
   the inline-run model it needed now exists.
2. **Nested blocks** (§3.1) — a callout containing a table is the shape people
   will ask for first, and it is a model change rather than a verb.
3. **`typst-render` for PNG/SVG** (§3.2) — removes a PDF round-trip.
4. **Spans, shapes, floats** (§3.1) — as asked for.
