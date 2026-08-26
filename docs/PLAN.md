# Plan — fluent `Document()` and `<cfdocument>` over Typst

Design and phase order. Written 2026-08-26, after proving the Typst embedding
end to end inside a native module.

---

## 1. What is already established

| Fact | Evidence |
|---|---|
| Typst embeds cleanly | `World` is **7 methods**; `src/world.rs` implements it in ~140 lines |
| It is fast | compile **~6 ms**, PDF export **~2 ms** for a 2-page invoice with a table |
| Fonts are the only slow part | ~2.1 s for 904 system faces — loaded **once** into a `OnceLock` |
| Licence is fine | Typst is Apache-2.0; all 93 new crates sit inside RustCFML's accepted set |
| The cost is real | **+31.6 MB** on the engine binary (56.0 → 87.6). Hence: a module |
| The module path works | `rustcfml --build` produces a bespoke binary that compiles Typst → PDF |
| Both halves meet | engine `PdfRead`/`PdfToImage` read back what the module writes |

**Non-obvious constraint discovered:** Typst markup is full of `#`, which is CFML
interpolation. Raw markup in CFML must double every one (`##set`). This is not a
footnote — it is the main reason the fluent builder matters.

---

## 2. Two delivery modes

Both converge on `set_registrar`, so the module code is identical.

* **Cocktail (static)** — `pub fn register(vm: &mut Vm)`, crate-type `rlib`.
  Works **today**; this is what `examples/invoice` uses.
* **`.rcx` (dynamic)** — a stable C ABI with opaque value handles, per
  `RustCFML/planning/NATIVE_EXTENSIONS_PLAN.md`. **Not built yet.** When it
  lands, add an `extern "C"` entry point beside `register` over the same
  functions; `crate-type` already includes `cdylib`.

The plan's central constraint: **every entry point takes a `ctx` handle from day
one**, even while that ctx can do almost nothing, so scope access and CFML
re-entry can arrive later without breaking published extensions.

---

## 3. The fluent `Document()` builder

Modelled on the engine's `Spreadsheet()` (see `RustCFML/docs/spreadsheets.md`):
a shared reference-typed native object, mutators return it so calls chain,
terminals end the chain.

```cfml
Document()
    .pageSize( "a4" ).orientation( "landscape" )
    .margin( top=2, bottom=2, left=1.5, right=1.5, unit="cm" )
    .font( "Helvetica", 11 )
    .header( "Acme Ltd", align="right" )
    .footer( "Page {page} of {total}" )
    .heading( "Invoice INV-1042", level=1 )
    .paragraph( "Thanks for your business." )
    .table( myQuery, header=true )
    .image( "/logo.png", width=120 )
    .pageBreak()
    .html( "<p>Some <b>markup</b></p>" )   // HTML subset -> Typst
    .typst( "##set text(fill: red)" )      // escape hatch: raw Typst
    .write( expandPath( "/invoice.pdf" ), overwrite=true );
```

Terminals: `.write( path )`, `.toBinary()`, `.toImage( page )`, `.toTypst()`.

`.toTypst()` earns its place as the debugging seam — when output is wrong you
need to see the markup that was generated.

**Design rules**
- The builder accumulates a document model and emits Typst **once**, at the
  terminal. Emitting incrementally makes `#set` scoping unpredictable.
- Everything user-supplied is **escaped** on the way in. A caller passing
  `#` or `[` in a paragraph must not be able to inject Typst — same discipline
  as SQL parameters.
- Units are explicit and CFML-ish (`unit="cm"`), never Typst's `2cm` literal.

---

## 4. `<cfdocument>` compatibility

The hard half: **cfdocument takes HTML, Typst does not.** So this needs an
HTML→Typst translator.

Core must lower the tag; a native module cannot register a tag on its own. So:
`<cfdocument>` lowers to a `__cfdocument( attrs, body )` call in the engine, and
the module supplies the implementation — with a clear *"install the document
module"* error when absent. That is a small, honest core change, not a no-op.

**The parsing is already solved**: `HtmlDocument()` (engine, v0.632.0) is a
mutable DOM with CSS selectors. The jsoup work pays for itself here.

### Supported subset — to be agreed before building

| Category | In |
|---|---|
| Block | `h1`–`h6`, `p`, `div`, `br`, `hr`, `ul`/`ol`/`li`, `table`/`thead`/`tbody`/`tr`/`td`/`th`, `blockquote`, `pre`/`code` |
| Inline | `b`/`strong`, `i`/`em`, `u`, `s`, `span`, `a[href]`, `img[src,width,height]`, `sub`/`sup` |
| CSS | `color`, `background-color`, `font-size`, `font-family`, `font-weight`, `font-style`, `text-align`, `width` |
| Page | `<cfdocumentitem type="pagebreak|header|footer">`, `<cfdocumentsection>` |
| Attributes | `format`, `filename`, `overwrite`, `name`, `pagetype`, `pageheight`/`pagewidth`, `orientation`, `margin*`, `unit` |

**Explicitly out, and must be documented as out:** float, position, flexbox,
grid, z-index — anything needing a CSS layout engine. cfdocument users hit these
limits on ACF and Lucee too, but the boundary has to be stated, not discovered.

`cfdocument.currentpagenumber` / `totalpagecount` map onto Typst's `numbering`
and `counter(page)`, which is a genuinely good fit.

---

## 5. Phase order

**Phases 1-3 and 6 are DONE, and templates — the §6 open question — shipped
alongside them (2026-08-26).** What remains is phases 4 and 5: the HTML→Typst
translator and `<cfdocument>` lowering. `docs/typst-coverage.md` is the audit of
what Typst can do that this still does not expose.

**Phases 1-3 are DONE** (2026-08-26) — the builder, the escaping/injection
tests, and images/headers/footers/styling all shipped together, because the
escaping decision (everything is a Typst *string literal*, every verb is code
mode) made the styling verbs fall out for free. See `docs/document-api.md`.
Phase 1 also turned up two engine bugs, both fixed in RustCFML v0.635.0-dev:
named arguments were dropped on a native class's methods, and `rustcfml --build`
embedded its own previous output.

1. **Fluent builder, core verbs** — page setup, headings, paragraphs, tables from
   a query, page breaks, `.write`/`.toBinary`/`.toTypst`. Tests + a real invoice.
2. **Escaping and injection tests** — before anything user-facing ships.
3. **Images, headers/footers, styling.**
4. **HTML→Typst translator** over `HtmlDocument()`, subset above.
5. **`<cfdocument>` lowering in core** + the module implementation.
6. **`.rcx`** — the C ABI, once the engine plan is implemented; this module is
   then its first real consumer and its proving ground.

Phases 1–4 need **no engine change at all**. Phase 5 needs one small one.

---

## 6. Open questions for the next session

1. Confirm the HTML subset in §4 before writing the translator.
2. `.rcx` first, or the fluent builder first? The builder is useful immediately
   and is the better shakedown for the ABI, which argues for builder-first.
3. ~~Should `Document()` also accept a Typst *template file*, so designers can own
   the layout and CFML only supplies data?~~ **Answered: yes, and it should come
   first.** Having written the builder, templates look like the higher-leverage of
   the two remaining directions — more so than `<cfdocument>` parity, because a
   template root also unblocks show rules, `#let`, data loading and
   bibliographies, none of which can resolve while the `World` serves a single
   source file. The design and the security boundary it needs are in
   [typst-coverage.md](typst-coverage.md) §4, along with the rest of the
   exposed/not-exposed audit.

   The one decision inside it worth restating: the caller's data is registered as
   a **virtual `data.json`** for the template to `json()`, not injected as
   generated `#let` code. That keeps the property the builder already has — your
   values are data and can never become Typst code.
