# `Document()` — the fluent builder

`Document()` returns a document whose **mutating methods return the document
itself**, so calls chain; terminals (`toBinary`, `toTypst`, `pageCount`,
`write`) end the chain. Same shape as the engine's `Spreadsheet()`.

```cfml
Document()
    .title( "Invoice INV-1042" )
    .pageSize( "a4" ).orientation( "portrait" )
    .margin( top=2, bottom=2, left=1.8, right=1.8, unit="cm" )
    .font( family="Helvetica", size=11 )
    .header( "Acme Ltd", align="right" )
    .footer( "Page {page} of {total}", align="center" )
    .heading( "Invoice INV-1042", level=1 )
    .paragraph( "Thanks for your business." )
    .table( data=myQuery, header=true, align="left,right,right" )
    .pageBreak()
    .write( expandPath( "/invoice.pdf" ) );
```

Requires **RustCFML ≥ v0.635.0** for named arguments on a native class's
methods. On older engines every call above must be positional.

## Page setup

| Method | Arguments | Notes |
|---|---|---|
| `pageSize` / `paper` | `paper`, `width`, `height`, `unit` | `paper` is a Typst paper name (`a4`, `us-letter`, …). Giving `width`+`height` overrides it. |
| `orientation` | `orientation` | `portrait` (default) or `landscape`. |
| `margin` / `margins` | `all`, `top`, `bottom`, `left`, `right`, `unit` | `all` sets every side; the named sides override it. |
| `font` | `family`, `size`, `color` | Document default. Per-block overrides on the content verbs. |
| `numbering` | `pattern` | Typst page numbering, e.g. `"1"` or `"1 / 1"`. |
| `title` / `author` | `title` / `author` | PDF metadata. |
| `header` / `footer` | `content`, `align` | `{page}` and `{total}` become live page counters. |

## Content

| Method | Arguments |
|---|---|
| `heading` | `text`, `level` (1–6), `align`, `bold`, `italic`, `size`, `color`, `font` |
| `paragraph` / `text` / `p` | `text`, `align`, `bold`, `italic`, `size`, `color`, `font` |
| `list` | `items` (array or comma list), `ordered`, `align`, `size`, `color`, `font` |
| `table` | `data`, `header`, `columns`, `align`, `unit`, `size`, `font` |
| `image` | `source` (path or binary), `width`, `height`, `align`, `unit` |
| `pageBreak`, `rule` / `hr` | — |
| `spacer` / `space` | `amount`, `unit` |
| `typst` | `markup` — the raw escape hatch |

`table` accepts a **query**, an **array of arrays**, or an **array of structs**
(the first struct's keys set the column order). `header=true` (the default)
promotes the first row — the query's column names — to a bold header row.
`columns` is a list of widths in `unit`; `fr` is Typst's fractional unit and is
the usual choice (`columns="1,1,1", unit="fr"` splits the width three ways).

## Terminals

| Method | Returns |
|---|---|
| `toTypst()` | the generated markup — the debugging seam |
| `toBinary()` / `toPdf()` | the PDF as `Binary` |
| `pageCount()` | pages, laid out but not exported |
| `write( path, overwrite )` | the document (chainable); writes the PDF |

To rasterise, hand the PDF back to the engine: `imageWrite( pdfToImage(
pdfRead( path ), 1, 300 ), "/page1.png" )`.

## Units and colours are whitelists

`unit` is one of `pt`, `mm`, `cm`, `in`, `em`, `%`, `fr`. A colour is a hex
value (`##RGB`, `##RRGGBB`, `##RRGGBBAA`) or one of Typst's named colours.
Anything else is an error, not a passthrough — a measurement or colour is the
only place the builder emits a bare expression, so it is the only place that
has to be checked.

## Why your text can't become Typst code

Every value you pass is emitted as a **Typst string literal**, and every verb
is emitted in Typst's *code* mode (`#heading(level: 1)[…]`, `#table(…)`). A
paragraph containing `#set page(fill: black)`, `$x^2$` or a stray `]` renders
as those characters. This is the same discipline as a SQL parameter, and it is
tested directly (`caller_text_cannot_become_typst_code`).

The one exception is `.typst( markup )`, which exists precisely to be an escape
hatch. Treat its argument the way you would treat SQL you concatenate yourself.

Images are the other place data crosses: the module reads the file (or takes
your `Binary`), sniffs its type from the **bytes**, and registers it with the
compiler under a generated virtual name. The document itself still cannot open
a path — `#image("/etc/passwd")` finds nothing, because the only files the
compiler's `World` will serve are the ones you added.
