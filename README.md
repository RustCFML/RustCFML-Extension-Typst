# rustcfml-typst

Document generation for [RustCFML](https://github.com/RustCFML/RustCFML), backed by
[Typst](https://github.com/typst/typst) — a fluent `Document()` builder and
`<cfdocument>` compatibility, delivered as a
[**`.rcx` extension**](https://github.com/RustCFML/RustCFML/blob/main/docs/extensions.md)
you install into a stock engine.

> **Status: both ways of making a document work.**
>
> * the fluent **`Document()` builder** — page setup, headings, rich inline text
>   (bold three words mid-sentence, links, footnotes, cross-references),
>   spreadsheet-grade tables from a query, callouts, quotes, code, term lists,
>   images, a table of contents, PDF/A and tagging options;
> * **Typst templates** — a designer owns the `.typ` file, CFML supplies only the
>   data, and show rules, `#let` and `#import` all work.
>
> Full reference: [docs/document-api.md](docs/document-api.md). What Typst can do
> that this still does not expose: [docs/typst-coverage.md](docs/typst-coverage.md).
> `<cfdocument>` and its HTML subset are designed but not built —
> [docs/PLAN.md](docs/PLAN.md) has the phase order.

## What Typst is, and why it is the right backend

[Typst](https://typst.app) is a typesetting system — the modern answer to LaTeX.
You give it markup, it lays the text out and exports a PDF. Two properties make
it a much better fit for generating documents from a program than the usual
HTML-to-PDF pipeline:

* **It is a library, not a subprocess.** No headless browser, no `wkhtmltopdf`,
  no temporary directory, no process to supervise. `typst::compile()` is a
  function call — measured on a two-page invoice with a table: **~6 ms to
  compile, ~2 ms to export the PDF**. Nothing about a request has to leave the
  engine process.
* **It is paged by construction.** Page size, margins, running headers and
  footers, page counters and page breaks are first-class concepts rather than
  properties you coax out of a layout engine designed for a scrolling viewport.
  `cfdocument.currentpagenumber` maps onto `counter(page)` exactly.

And it is Apache-2.0 throughout — all 93 crates it brings sit inside RustCFML's
accepted licence set, which is not a given for this class of dependency.

### How this extension uses it

```
   CFML                       Rust                          Typst
   ────                       ────                          ─────
   Document()            →    a document model              (nothing yet)
     .heading(…)              accumulated, not emitted
     .table(…)                image bytes held aside
        │
        └─ .toTypst()  ─→     the emitter runs ONCE    ─→   markup text
           .toBinary()                                          │
           .write(…)          a World with one source           ▼
                              file, the host's fonts, and  typst::compile()
                              only the images you added         │
                                                                ▼
                                                          typst_pdf::pdf()
```

Three properties fall out of that shape, and they are the reasons the design is
what it is:

1. **The markup is generated once, at the terminal.** The builder accumulates a
   model; nothing is emitted until `.toTypst()`, `.toBinary()` or `.write()`.
   Emitting incrementally would make Typst's `#set` scoping depend on call order
   in ways a caller cannot see.
2. **Your text is data, never code.** Every value you pass becomes a Typst
   *string literal*; every verb is emitted in Typst's *code* mode. A paragraph
   containing `#set page(fill: black)` renders as those characters. Units and
   colours are whitelists, because a measurement is the only place the builder
   emits a bare expression.
3. **The document cannot reach the filesystem or the network.** The compiler's
   `World` serves exactly one source file — the generated `main.typ` — so
   `#import` has nowhere to go, and it serves only the image bytes the builder
   registered. `#image("/etc/passwd")` finds nothing.

**How much of Typst is exposed, and what is missing** — including the largest
gap, which is that a document cannot yet be a *template* — is set out in
[docs/typst-coverage.md](docs/typst-coverage.md).

## What works today

### The builder

```cfml
Document()
    .title( "Invoice INV-1042" )
    .pageSize( "a4" )
    .margin( top=2, bottom=2, left=1.8, right=1.8, unit="cm" )
    .font( family="Helvetica", size=11 )
    .header( content="Acme Ltd", align="right" )
    .footer( content="Page {page} of {total}", align="center" )
    .heading( text="Invoice INV-1042", level=1 )
    .paragraph( text=[ "Please pay ", { text="##1,240.00", bold=true }, " by 31 March." ] )
    .table(
        data          = lines,                       // a query, or arrays, or structs
        columnList    = "description,qty,amount",    // select AND order, like queryColumnList
        headers       = "Item,Qty,Amount",
        columnFormats = { amount = { alignment="right", numberFormat="9,999.00" } },
        headerFormat  = { bold=true, bgcolor="##2c3e50", color="white" },
        stripe        = "##f7f7f7",                  // zebra rows, header excluded
        totals        = "amount"                     // a summed, bold totals row
    )
    .callout( content="Payment is due within 30 days.", fill="##fff8e1" )
    .write( expandPath( "/invoice.pdf" ) );
```

Three conventions come straight from the engine's `Spreadsheet()` builder,
because a CFML developer already knows them: **styling is a plain struct** with
case-insensitive keys, **`data` is polymorphic** (query / array of arrays / array
of structs, exactly as `spreadsheetAddRows`), and **number masks live with the
formatting** rather than in a separate pass. `numberFormat` calls the engine's
*own* `numberFormat()` back over the ABI, so a mask in a document produces the
same string it produces everywhere else in your application.

### Or a template, if a designer owns the layout

```cfml
pdf = Document().template( expandPath( "/templates/statement.typ" ) )
                .data( { reference: "STMT-7", customer: customer, lines: lines } )
                .toBinary();
```

```typst
#let data = json("data.json")
#import "_shared.typ": money, panel

// A show rule restyles every heading at once — the reason templates matter.
#show heading: it => block(below: 1em)[#text(weight: "bold", size: 15pt)[#it.body]]

= Statement #data.reference
#panel[#data.customer.name]
```

`data.json` is not a file on disk: the extension registers your struct under that
name, serialised by the engine's own `serializeJSON`. That keeps the template
idiomatic *and* keeps your values **data** — generating `#let data = …` source
would hand every value in your database a route into Typst's code mode. The
template gets one canonical directory as its root, enforced by Typst's own
resolver; packages (`@preview/…`) stay unresolvable because fetching one would
reach the network. A *generated* document has no root at all.

See [`examples/invoice/templates/`](examples/invoice/templates) for a worked
template, and `examples/invoice/selftest.cfm` for 106 end-to-end checks.

Full method reference: [docs/document-api.md](docs/document-api.md). The raw
surface is still there for hand-written markup:

```cfml
writeOutput( typstFontCount() );          // font faces the host offers
pages = typstPageCount( markup );         // lay out without exporting
pdf   = typstCompile( markup );           // PDF bytes
```

Run [`examples/invoice`](examples/invoice) and you get a two-page invoice, its
generated Typst printed for inspection, and the PDF read straight back in
through the engine's own `PdfRead`/`PdfToImage` — the two halves meeting.

### Named arguments need RustCFML ≥ v0.635.0

Every call above binds its arguments **by name**. Engines before v0.635.0 drop
the names on a native class's methods and bind positionally instead, silently —
so on an older engine every call here must be positional. And note CFML's own
rule, which applies to these methods like any other: you may not mix the two
forms in one call. `.spacer( 1, unit="cm" )` is an error; write
`.spacer( amount=1, unit="cm" )`.

## Installing it

This is a **`.rcx` extension**: precompiled Rust that a *stock* `rustcfml`
binary loads at start-up. No engine rebuild, no toolchain on the server.

Take the file for **your platform** from the
[latest release](https://github.com/RustCFML/RustCFML-Extension-Typst/releases) — one of
`typst-<version>-{macos-aarch64,linux-x86_64,linux-aarch64,windows-x86_64}.rcx`
— and install it:

```sh
rustcfml ext install typst-0.1.0-linux-x86_64.rcx --user
rustcfml ext list
```

…or drop it into your application's `extensions/` directory and check it into the
project. `rustcfml ext list` shows what is installed and whether it loads. See
[RustCFML's extension guide](https://github.com/RustCFML/RustCFML/blob/main/docs/extensions.md).

Each release artifact carries **one platform's** library. Install the wrong one
and the engine says so by name, listing what the archive does contain.

To build it yourself instead:

```sh
rustcfml ext build .                      # produces typst-0.1.0.rcx for THIS platform
rustcfml ext install typst-0.1.0.rcx --user
```

Requires **RustCFML ≥ v0.635.0** (the extension ABI, and named arguments on a
native class's methods).

The only RustCFML dependency is the `rustcfml-module` wrapper. It is not on
crates.io yet, so it is a **path dependency** and building this crate needs a
RustCFML checkout beside it:

```
some-dir/
  RustCFML/          # git clone, at v0.635.1 or later
  rustcfml-typst/    # this repo
```

CI does the same with two sibling checkouts, and pins the engine ref because that
tree is a real build input. When the wrapper is published, this becomes an
ordinary version requirement and the checkout goes away.

### ⚠️ Typst markup and CFML's `#` (only if you write markup by hand)

Typst is built out of `#` — `#set`, `#table`, `#pagebreak` — and `#` is CFML's
interpolation character. Raw Typst inside a CFML string must escape **every** one
as `##`:

```cfml
markup = '##set page(paper: "a4")
= Hello';
```

This is the strongest argument for the fluent builder: it emits the Typst for
you, so application code never types a `#` at all.

## Fonts

Typst needs real font faces, taken from the operating system's font database.
Nothing is embedded: `typst-assets` is 6.7 MB and its fonts are OFL-1.1, which is
not in RustCFML's accepted licence set. `typstFontCount()` reports what the host
offers.

> ⚠️ **A host with no fonts is a real deployment case, and Typst does not fail on
> it — it succeeds.** Measured in a `rust:1-slim` container: a one-paragraph
> document compiled cleanly into a 1,966-byte PDF containing **zero** text-showing
> operators and zero embedded fonts. A blank page, with no error, and every one of
> this extension's end-to-end checks still passed.
>
> So this extension refuses instead: a document that needs a glyph drawn on a
> host with no fonts is an error naming the fix, and `pageCount()` is guarded too
> (a fontless layout does not wrap lines, so it reports the wrong number). A
> document with **no text at all** — an image, a rule — still works without
> fonts, because that is a legitimate thing to want.
>
> On Debian or Ubuntu, `fonts-dejavu-core` is enough. Slim container images
> generally ship none.

Font **metadata** for every face is read once at load (~0.25 s for 904 faces on
a developer Mac); the **data** for a face is read only when a document actually
renders with it, and memoised. That distinction matters: an earlier version held
a parsed `Font` per face, and because a face's data is its whole *file* — a
`.ttc` collection is one file with many faces — it copied `Songti.ttc`'s 64 MB
once per face it contained. The result was **3.4 GB resident before serving a
single request**, which on Apple Silicon is unified memory and duly filled the
machine. Steady state under sustained load is now ~220 MB.

## Licence

This extension's own code is dual-licensed **MIT OR Apache-2.0** — see
[LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE) and
[COPYRIGHT](COPYRIGHT).

It is a thin layer over other people's work. **Typst** (© The Typst Project
Developers, Apache-2.0) does the typesetting; **RustCFML** (© the RustCFML Team,
MIT) is the engine it plugs into.

A built `.rcx` statically links Typst and ~90 other crates, so it carries their
notice obligations. [THIRD-PARTY.txt](THIRD-PARTY.txt) reproduces every one of
them, is generated from the resolved dependency graph
(`./scripts/gen-licenses.sh`), and is **packaged inside the archive** together
with [NOTICE](NOTICE) and the licence files — so the attribution travels with the
library rather than staying behind in this repository.
