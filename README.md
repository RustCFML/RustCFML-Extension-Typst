# rustcfml-typst

Document generation for [RustCFML](https://github.com/RustCFML/RustCFML), backed by
[Typst](https://github.com/typst/typst) — a fluent `Document()` builder and
`<cfdocument>` compatibility, delivered as a **native module** rather than as
part of the engine.

> **Status: the fluent `Document()` builder works.** Page setup, headings,
> paragraphs, lists, tables from a query, images, headers/footers, page breaks
> and the `.write()` / `.toBinary()` / `.toTypst()` terminals are all in — see
> [docs/document-api.md](docs/document-api.md). `<cfdocument>` and its HTML
> subset are designed but not built; the phase order is in
> [docs/PLAN.md](docs/PLAN.md).

## Why a module rather than part of the engine

Typst is ~93 crates and **+31.6 MB** on the RustCFML binary (measured: 56.0 → 87.6 MB).
PDF *reading* belongs in the engine and is there already — `PdfRead`, `PdfToImage`
and friends, which cost +4.3 MB. Document *authoring* is a much larger dependency
for a capability not every application wants, so it ships separately and you opt
in by building it.

## What works today

```cfml
Document()
    .title( "Invoice INV-1042" )
    .pageSize( "a4" )
    .margin( top=2, bottom=2, left=1.8, right=1.8, unit="cm" )
    .font( family="Helvetica", size=11 )
    .header( content="Acme Ltd", align="right" )
    .footer( content="Page {page} of {total}", align="center" )
    .heading( text="Invoice INV-1042", level=1 )
    .paragraph( "Thanks for your business." )
    .table( data=lines, header=true, align="left,right,right" )
    .write( expandPath( "/invoice.pdf" ) );
```

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

## Building it

Native modules are linked into a bespoke binary by `rustcfml --build`. Put a thin
crate under your app's `native/` that depends on this one and re-exports its
`register` (see [`examples/invoice`](examples/invoice)), then:

```sh
cd your-app
rustcfml --build .
./app --port 8080
```

Requires **RustCFML ≥ v0.634.1** — earlier versions cannot complete a cocktail
build at all.

`--build` **snapshots your `.cfm` files into the binary**. Re-run it after editing
them, or you will be served the copy from the last build.

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
not in RustCFML's accepted licence set. `typstFontCount()` exists so "this host
has no fonts installed" is distinguishable from "my markup is wrong".

## Licence

MIT OR Apache-2.0, matching Typst (Apache-2.0) and RustCFML.
