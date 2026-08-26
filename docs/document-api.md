# `Document()` — the fluent builder

`Document()` returns a document whose **mutating methods return the document
itself**, so calls chain; terminals (`toBinary`, `toTypst`, `pageCount`, `write`)
end the chain. Same shape as the engine's `Spreadsheet()`, and three of its
conventions are borrowed deliberately:

* **styling is a plain struct** — `format = { bold: true, bgcolor: "##dfe6e9" }`,
  case-insensitive keys, accepted anywhere styling is accepted;
* **`data` is polymorphic** — a query, an array of arrays, or an array of
  structs, exactly as `spreadsheetAddRows` takes;
* **number masks live with the formatting**, so a money column is one decision
  rather than a format call plus a separate pass over the data.

```cfml
Document()
    .title( "Invoice INV-1042" )
    .pageSize( "a4" ).orientation( "portrait" )
    .margin( top=2, bottom=2, left=1.8, right=1.8, unit="cm" )
    .font( family="Helvetica", size=11 )
    .header( "Acme Ltd", align="right" )
    .footer( "Page {page} of {total}", align="center" )
    .heading( text="Invoice INV-1042", level=1 )
    .paragraph( text=[ "Please pay ", { text="##1,240.00", bold=true }, " by 31 March." ] )
    .table(
        data          = lines,
        columnList    = "description,qty,amount",
        headers       = "Item,Qty,Amount",
        columnFormats = { amount = { alignment="right", numberFormat="9,999.00" } },
        totals        = "amount",
        stripe        = "##f7f7f7"
    )
    .callout( content="Payment is due within 30 days.", fill="##fff8e1" )
    .write( expandPath( "/invoice.pdf" ) );
```

Requires **RustCFML ≥ v0.635.0**: the `.rcx` extension ABI, and named arguments
on a native class's methods. Note CFML's own rule — you may not mix positional
and named arguments in one call, so `.spacer( 1, unit="cm" )` is an error and
`.spacer( amount=1, unit="cm" )` is not.

---

## Styling: the format struct

Every content verb takes `format = {…}`. Keys are case-insensitive and follow the
engine's spreadsheet format struct wherever there is an equivalent:

| Key(s) | Effect |
|---|---|
| `bold`, `italic` | Weight and slant |
| `underline`, `strike` / `strikethrough`, `smallcaps` | Decorations |
| `size`, `fontsize` | Points, or `unit` in the same struct |
| `font`, `fontname`, `family` | Font family |
| `color`, `fontcolor` | Text colour |
| `bgcolor`, `backgroundcolor`, `fill` | Cell/box fill (table cells and `callout` only) |
| `align`, `alignment`, `horizontalalignment` | `left` / `center` / `right` / `justify` |
| `numberFormat`, `dataformat` | A CFML `numberFormat` mask — **table columns only** |
| `dateFormat` | A CFML `dateFormat` mask — **table columns only** |

The older individual arguments (`bold=`, `size=`, `color=`, `align=`, `font=`,
`italic=`) still work on the verbs that had them and are layered **over** the
format struct, so `format={bold:true}, italic=true` gives both.

**Units and colours are whitelists.** `unit` is one of `pt`, `mm`, `cm`, `in`,
`em`, `%`, `fr`. A colour is a hex value (`##RGB`, `##RRGGBB`, `##RRGGBBAA`) or
one of Typst's named colours. Anything else is an error, not a passthrough —
these are among the few places the builder emits a bare expression rather than a
string literal, so they are the places that have to be checked.

## Inline content: strings or runs

Anywhere a verb takes `text` or `content`, it takes either a **string** or an
**array of runs**. A run is a string, or a struct carrying the format keys above
plus one of the run kinds:

```cfml
.paragraph( text = [
    "Please pay ",
    { text = "##1,240.00", bold = true },
    " by ",
    { text = "31 March", underline = true },
    { linebreak = true },
    { link = "https://acme.example/pay", text = "Pay online" },
    { text = "the terms", footnote = "See clause 4." },
    "See ", { ref = "terms" }
] )
```

| Run key | Meaning |
|---|---|
| `text` | The content. A nested array is itself runs. |
| `link` | A hyperlink target wrapping `text` |
| `footnote` | A string → emit `text`, then a footnote; `true` → `text` *is* the note |
| `ref` | A cross-reference to a `label` |
| `linebreak` / `break` | An explicit line break |

**Links are checked, not merely escaped.** Only `http`, `https`, `mailto`, `tel`
and same-document targets (`#…`, `/…`) are allowed. A `javascript:` or `data:`
URL is refused, because a PDF viewer will act on it and application data is
exactly where such a string arrives from.

## Page setup

| Method | Arguments | Notes |
|---|---|---|
| `pageSize` / `paper` | `paper`, `width`, `height`, `unit` | `paper` is a Typst paper name (`a4`, `us-letter`, …). `width`+`height` overrides it. |
| `orientation` | `orientation` | `portrait` (default) or `landscape`. |
| `margin` / `margins` | `all`, `top`, `bottom`, `left`, `right`, `unit` | `all` sets every side; named sides override it. |
| `font` | `family`, `size`, `color`, `format` | Document default. |
| `numbering` | `pattern` | Page numbering, e.g. `"1"` or `"1 / 1"`. |
| `headingNumbering` / `numberHeadings` | `pattern` | `"1."`, `"1.1"`, … **Required if you `ref` a heading** — Typst refuses to reference an unnumbered one. |
| `columns` / `pageColumns` | `count` | Multi-column text flow, 1–12. |
| `pageFill` / `background` | `color` | Page background. |
| `title`, `author`, `keywords` | | PDF metadata. `keywords` takes a list or array. |
| `header` / `footer` | `content`, `align`, `format` | `{page}` and `{total}` become live page counters. |

## Content

| Method | Arguments |
|---|---|
| `heading` | `text`, `level` (1–6), `label`, `align`, `bold`, `italic`, `size`, `color`, `font`, `format` |
| `paragraph` / `text` / `p` / `rich` | `text`, `align`, `bold`, `italic`, `size`, `color`, `font`, `format` |
| `link` | `url`, `text`, `align`, `format` — a paragraph that is one link |
| `list` | `items` (array or comma list; each item may itself be runs), `ordered`, `align`, `size`, `color`, `font`, `format` |
| `terms` / `definitions` | `items` — a struct, or an array of `{term, description}` / two-element arrays |
| `quote` / `blockquote` | `text`, `attribution`, `align`, `format` |
| `code` / `pre` | `text`, `language` — preformatted, syntax-highlighted |
| `callout` / `box` / `panel` | `content`, `fill`, `stroke`, `radius`, `inset`, `width`, `unit`, `align`, `format` |
| `image` | `source` (path or binary), `width`, `height`, `align`, `unit`, `caption`, `label` |
| `table` / `grid` / `fromQuery` | see below |
| `outline` / `toc` | `title`, `depth` — table of contents |
| `pageBreak`, `rule` / `hr` | — |
| `spacer` / `space` | `amount`, `unit` |
| `typst` | `markup` — the raw escape hatch |

## Tables

`table` is the spreadsheet-grade verb. `grid` is the same thing with two
different defaults — no header row and no lines — for page furniture rather than
data.

| Argument | Meaning |
|---|---|
| `data` | A **query**, an array of arrays, or an array of structs (the first struct's keys set the column order) |
| `header` | Promote the column names to a bold header row. Default `true` for `table`, `false` for `grid` |
| `columnList` | Which columns to include, **and in what order** — like `queryColumnList`. Naming a column that isn't there is an error |
| `headers` | Display labels, overriding the column names |
| `widths` (or `columns`) | Column widths in `unit`. `fr` is Typst's fractional unit and the usual choice: `widths="1,1,1", unit="fr"` splits the width three ways |
| `align` | Per-column alignment, as a list: `"left,right,right"` |
| `format` | The whole-table default |
| `headerFormat` | Applied over `format` for the header row. Defaults to `{bold:true}` |
| `columnFormats` | Per column: a struct keyed by column name, or an array of formats by position |
| `stripe` | Fill for alternating body rows — zebra striping, header row excluded |
| `fill` | Fill for the whole table |
| `stroke` | `false` or `"none"` turns lines off |
| `strokeColor`, `strokeWidth` | A custom line |
| `gutter`, `inset` | Spacing between and inside cells |
| `totals` | Columns to sum. Appends a bold totals row |
| `totalsLabel` | The label in that row. Default `"Total"` |
| `caption`, `label` | Wraps the table in a numbered figure you can `ref` |

### Number masks are applied by the engine

`columnFormats = { amount = { numberFormat = "9,999.00" } }` does not implement a
mask here — it calls the engine's own `numberFormat()` (and `dateFormat()`)
through the extension ABI. A mask in a document therefore produces exactly the
string the same mask produces everywhere else in your application, which
reimplementing CFML's mask language in Rust would not.

Two consequences worth knowing:

* it costs one engine call per formatted cell, so it happens only for columns
  that asked for a mask. A 10,000-row report with two masked columns is 20,000
  calls; without masks it is none;
* it needs an engine providing **extension capability tier 3**. RustCFML does; a
  smaller host would refuse with a message saying so.

A totals row is summed here and formatted with its column's mask. It is a
*number*, not a formula: a PDF has no recalculation, so `=SUM(C2:C40)` has no
document equivalent. A non-numeric value in a summed column is an error naming
the column and the value, not a silent zero.

## Templates

The other way to make a document: a designer owns a `.typ` file, and CFML
supplies only data.

| Method | Arguments |
|---|---|
| `template` | `path` (a `.typ` file), `root` (defaults to the file's own directory) |
| `data` | `data` — any CFML value |
| `typstRender( template, data [, root] )` | The one-shot BIF form |

```cfml
pdf = Document().template( expandPath( "/templates/statement.typ" ) )
                .data( { reference: "STMT-7", customer: customer, lines: lines } )
                .toBinary();
```

Inside the template, the data is a file:

```typst
#let data = json("data.json")
#import "_shared.typ": money, panel      // siblings under the root resolve

#show heading: it => block(below: 1em)[#text(weight: "bold", size: 15pt)[#it.body]]

= Statement #data.reference
#panel[#data.customer.name]
```

`data.json` is **not** a file on disk — the extension registers your struct under
that name, serialised by the engine's own `serializeJSON`. That is deliberate: it
keeps the template idiomatic (`json()` is how a Typst author already reads data)
*and* it keeps injected values **data**. Generating `#let data = …` source
instead would hand every value in your database a route into Typst's code mode.

**The sandbox.** The root is one canonical directory and every lookup goes
through Typst's own root-relative resolver, so `read("../../etc/hosts")` is
refused, not merely unlikely. Typst **packages** (`@preview/…`) are refused in
both shapes: resolving one reaches the network. Vendor what you need under the
root and `#import` it by path.

A generated document has **no root at all**, which is the difference: it cannot
`#import` or `read` anything, and the only files it can see are the images the
builder registered.

When a template is set it *is* the document — the builder's own blocks are not
emitted, and `.toTypst()` returns the template's source rather than generated
markup, because a debugging seam that showed something other than what compiles
would be worse than none.

## PDF options

`.pdf( standard, tagged, pages, creator, timestamp )` reaches
`typst_pdf::PdfOptions`.

| Argument | Meaning |
|---|---|
| `standard` | One or a list of: `1.4`–`1.7`, `2.0`, `a-1b`, `a-1a`, `a-2b`, `a-2u`, `a-2a`, `a-3b`, `a-3u`, `a-3a`, `a-4`, `a-4f`, `a-4e`, `ua-1` |
| `tagged` | Accessibility tagging. **On by default** — this is only how you turn it off |
| `pages` | `"1-3,7,10-"`, one-indexed and inclusive |
| `creator` | The `/Creator` metadata string |
| `timestamp` | A date, or epoch seconds, for a reproducible `/CreationDate` |

**`pages` requires `tagged=false`.** Typst will not write a tagged PDF with a
page range, and tagging is on by default, so a page range on its own always
failed. It is now refused at the `.pdf()` call, with a message saying what to
pass — dropping accessibility tagging is a real trade-off and not something to do
for you silently.

## Terminals

| Method | Returns |
|---|---|
| `toTypst()` | The markup that will be compiled — generated, or the template's source |
| `toBinary()` / `toPdf()` | The PDF as `Binary` |
| `pageCount()` | Pages, laid out but not exported |
| `write( path, overwrite )` | The document (chainable); writes the PDF |

To rasterise, hand the PDF back to the engine: `imageWrite( pdfToImage( pdfRead(
path ), 1, 300 ), "/page1.png" )`.

## Other functions

| Function | Returns |
|---|---|
| `isDocumentObject( value )` | Whether it is a `Document` |
| `typstCompile( markup )` | PDF bytes from hand-written markup |
| `typstPageCount( markup )` | Pages, without exporting |
| `typstRender( template, data [, root] )` | PDF bytes from a template |
| `typstFontCount()` | Font faces this host offers — `0` means none are installed |
| `typstVersion()` | The Typst release this extension is built against |

## Why your text can't become Typst code

Every value you pass is emitted as a **Typst string literal**, and every verb is
emitted in Typst's *code* mode — `#heading(level: 1)[…]`, `#table(…)`. A
paragraph containing `#set page(fill: black)`, `$x^2$` or a stray `]` renders as
those characters. This is the same discipline as a SQL parameter, and it is
tested directly, including in every inline-run position
(`hostile_text_in_every_inline_position_stays_a_literal`).

The exceptions are all narrow and all checked rather than escaped, because none
of them has a string-literal form: measurements, colours, labels, paper names,
PDF standard names, and link URLs.

`.typst( markup )` exists precisely to be an escape hatch. Treat its argument the
way you would treat SQL you concatenate yourself.

Images are the other place data crosses: the module reads the file (or takes your
`Binary`), sniffs its type from the **bytes**, and registers it with the compiler
under a generated virtual name. The document itself still cannot open a path.
