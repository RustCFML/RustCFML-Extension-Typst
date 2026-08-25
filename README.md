# rustcfml-typst

Document generation for [RustCFML](https://github.com/RustCFML/RustCFML), backed by
[Typst](https://github.com/typst/typst) — a fluent `Document()` builder and
`<cfdocument>` compatibility, delivered as a **native module** rather than as
part of the engine.

> **Status: early.** The Typst embedding is proven end to end (see below) and the
> module builds and runs. The fluent builder and `<cfdocument>` are designed but
> not yet implemented — see [docs/PLAN.md](docs/PLAN.md).

## Why a module rather than part of the engine

Typst is ~93 crates and **+31.6 MB** on the RustCFML binary (measured: 56.0 → 87.6 MB).
PDF *reading* belongs in the engine and is there already — `PdfRead`, `PdfToImage`
and friends, which cost +4.3 MB. Document *authoring* is a much larger dependency
for a capability not every application wants, so it ships separately and you opt
in by building it.

## What works today

```cfml
writeOutput( typstFontCount() );          // font faces the host offers
pages = typstPageCount( markup );         // lay out without exporting
pdf   = typstCompile( markup );           // PDF bytes
```

```
font faces visible to the engine: 904
pages: 2
pdf bytes: 25097
read back: 2 pages, page 1 is 595.28x841.89pt
rasterised page 1 -> 14837 byte png
```

That last pair is the engine's own `PdfRead`/`PdfToImage` reading back what the
module wrote — the two halves meeting.

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

### ⚠️ Typst markup and CFML's `#`

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
