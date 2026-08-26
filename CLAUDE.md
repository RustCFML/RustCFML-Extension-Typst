# rustcfml-typst — Claude Code guide

Typst-backed document generation for [RustCFML](https://github.com/RustCFML/RustCFML),
shipped as a dynamically loaded `.rcx` extension. Read this before changing
anything; most of it is a trap that has already cost time once.

## Layout

```
src/model.rs      the document model the builder accumulates — inert, no Typst types
src/emit.rs       model -> Typst markup. THE injection boundary. Unit-tested.
src/document.rs   the CFML class: argument parsing, validation, the .table() logic
src/world.rs      typst::World, the template sandbox, PDF options, fonts
src/lib.rs        module! declaration and the raw typst* BIFs
examples/invoice/ main.cfm (demo), selftest.cfm (108 end-to-end checks), templates/
docs/             document-api.md (reference), typst-coverage.md (gap audit), PLAN.md
```

## Building and testing

**This crate needs a RustCFML checkout beside it** — `rustcfml-module` is a path
dependency, not a crates.io one:

```
some-dir/
  RustCFML/                   # v0.635.1 or later
  RustCFML-Extension-Typst/   # this repo
```

Only the `RustCFML/` sibling's name matters — the path dependency is
`../RustCFML/crates/rustcfml-module`, so this directory can be called anything.
CI checks it out as `rustcfml-typst`.

```bash
cargo test                       # 24 Rust unit tests — emit/validation/date maths
cargo clippy --all-targets       # must be clean; CI runs -D warnings
cargo fmt --check                # must be clean

# The end-to-end suite. Everything in it compiles REAL Typst, so a pass means
# the markup was valid, not merely that it was generated.
rustcfml ext build .
rustcfml ext install typst-0.1.0.rcx --dir examples/invoice/extensions
rustcfml examples/invoice/selftest.cfm      # expect "all checks passed" (108 ok)
rustcfml examples/invoice/main.cfm          # the demo, both builder and template
```

> 🚨 **`cargo test` passing is not evidence.** Every bug found in the last two
> sessions was invisible to the Rust tests, and three of them were invisible to
> the 108 CFML checks as well. See "Verification that actually works" below.

## The rules that keep this safe

**1. Caller text is a Typst STRING LITERAL. Always.**
`emit::lit()` produces the literal; every verb emits Typst *code* mode
(`#heading(level: 1)[#"…"]`). A paragraph containing `#set page(fill: black)`,
`$x^2$` or a stray `]` must render as those characters. Tests
`caller_text_cannot_become_typst_code` and
`hostile_text_in_every_inline_position_stays_a_literal` enforce it — if you add a
place where caller text lands, extend the second one.

**2. The few bare expressions are WHITELISTS, checked in `document.rs`.**
Measurements, colours, labels, paper names, PDF standard names and link URLs have
no string-literal form, so they are validated rather than escaped. Never widen one
without a test. Link URLs are restricted to http/https/mailto/tel/same-document
because a PDF viewer *acts* on `javascript:`.

**3. `emit.rs` assumes its input is already valid.** Validation lives in
`document.rs`. Do not add checks to the emitter; do not skip them in the parser.

**4. Nothing may call back into CFML while the document lock is held.**
`CfmlDocument::with()` holds a `Mutex`, and `ctx.call()` re-enters the engine.
Every arm resolves its arguments first and takes the lock last. Breaking this is a
self-deadlock, not an error.

**5. Read arguments BY NAME.** One `const P_*` string per method answers
`method_params` *and* resolves arguments (`a.val("stripe")`), so they cannot drift.
`.table()` has 24 parameters; a mis-numbered index is a silent wrong answer.

**6. Formatting is the ENGINE's job.** `numberFormat`/`dateFormat` masks call the
engine's own BIFs over the ABI (`ctx.call`, needs host tier 3), and `.data()` uses
the engine's `serializeJSON`. Never reimplement a CFML mask here — a second
implementation of that language would diverge in ways users would have to debug.

**7. Template data crosses as a virtual `data.json`, never as generated `#let`
source.** That is what keeps caller data *data*. If you ever emit `#let data = …`,
you have handed every value in the caller's database a route into code mode.

## Verification that actually works

Three techniques, each of which caught something nothing else did:

**Rasterise and look at it.** `pdfToImage( pdfRead( path ), 1, 1400 )` then read
the PNG. This is the only thing that found the caption bug: `#figure` centres its
content and Typst's table `align` defaults to `auto` (= inherit), so adding
`caption=` silently centre-aligned every cell. Markup fine, compile fine, all 106
checks green at the time. **If a change affects layout, render it.**

⚠️ Small renders show *fake* letter-spacing ("Widget,   large"). Verified against
the 1400px render and the PDF's glyph stream — don't chase it.

**Measure memory with `vmmap -summary`, not `ps`.** `ps` RSS showed 5.8 MB for
processes Activity Monitor reported at 3.26 GB — macOS charges swapped-out dirty
pages, and `ps` counts only resident ones. `vmmap -summary $PID` gives the
**Physical footprint** (the Activity Monitor number) *and* a region table saying
where it went. That is how the font loader's 2.9 GB of `MALLOC_LARGE` was found.
On Apple Silicon this is unified memory, so a large footprint is real RAM.

Recipe for "does it leak?": start `--serve`, probe, then N requests per phase,
probing between phases. Font data plateaus at ~220 MB and stays flat over 600
requests — a plateau is not a leak.

**Test in Docker, on a fontless image.** `rust:1-slim` has **zero fonts**, and
Typst does not fail on that — it compiles happily and writes a PDF with zero
text-showing operators and zero embedded fonts. A blank page, successfully.
`world::check_fonts` now refuses instead. Any change near fonts or `World` gets a
container run.

**Assert on the reason, not just the failure.** A bare `try/catch` "the sandbox
refused it" check also passes when the extension failed to *load*. Check the
message. And prove data inertness by **page count** — an executed `#pagebreak()`
compiles perfectly well.

## Traps

- ⭐⭐⭐**`#` in CFML.** Typst markup is built out of `#`; CFML interpolates it.
  Raw Typst in a CFML string doubles every one (`##set`). Writing tests *about*
  Typst is where this bites. `.typ` files on disk are unaffected.
- ⭐⭐**CFML forbids mixing positional and named arguments in one call.**
  `.spacer( 1, unit="cm" )` is an engine error. Fluent APIs read as if it were
  allowed.
- ⭐⭐**`r#"…"#` cannot contain `"#`.** Use `r##"…"##`. Bites in any test asserting
  on `rgb("#f00")`.
- ⭐**Typst refuses `#ref` to an unnumbered heading.** Hence `.headingNumbering()`.
- ⭐**A PDF page range cannot combine with tagged PDF, and tagging is ON by
  default**, so `pages=` alone always failed. Refused at `.pdf()` now, naming the
  fix. Do not silently disable tagging — dropping accessibility is the caller's
  call.
- ⭐⭐⭐**`fontdb::with_face_data` hands you the whole FILE, not the face.** Never
  build a `Font` per face from it: a `.ttc` is one file with many faces, so that
  copies the file once per face. 904 faces = **3.4 GB at start-up**. Use
  `FontInfo::new(data, index)`, which *borrows*, and load the `Font` lazily.
  `world::startup_holds_font_metadata_only` is the regression test.
- The ABI type constant is `ty::BOOL`, not `ty::BOOLEAN`.
- Docker Desktop bind mounts on macOS are **case-insensitive**: a file named
  `rustcfml` beside a mounted `RustCFML/` resolves to the directory.

## Adding a verb

1. `model.rs` — a `Block` variant, or fields on an existing one. Keep it inert.
   `Block::Table` is boxed; the enum is size-sensitive.
2. `emit.rs` — one arm. Caller text through `body()`/`inline()`, nothing else.
3. `document.rs` — a `const P_YOURVERB` parameter list, an entry in `params_for`,
   an arm in `call`, and an alias if there is an obvious one. Mutators
   `Ok(ctx.this())`; terminals return a value.
4. `unknown_method()` — add the name to the list in the error message.
5. Tests: a unit test in `emit.rs` for the markup, and a check in
   `examples/invoice/selftest.cfm` that **compiles** it.
6. `docs/document-api.md` — the reference table. And move it out of
   `docs/typst-coverage.md` §3 if it was listed as missing.
7. If it affects layout, render it and look.

## Licensing

Dual MIT/Apache-2.0 for this crate's own code; see COPYRIGHT. A `.rcx` statically
links Typst and ~90 crates, so notices must travel with it: `THIRD-PARTY.txt` is
generated (`./scripts/gen-licenses.sh`, needs cargo-about 0.9.1), committed, and
packaged inside the archive along with NOTICE and the licence files. Regenerate
and commit after **any** dependency change.

⚠️ A dependency shipping more than one licence file makes the generated output
filesystem-order dependent, which is why `gen-licenses.sh` squeezes blank lines.
This broke the engine repo's byte-exact CI gate once.

No fonts are bundled: `typst-assets` is 6.7 MB of OFL-1.1 faces, and Typst is
given the host OS fonts at run time instead.

## Releasing

`.github/workflows/release.yml`, on a `v*` tag: four platforms, **one
single-platform `.rcx` each** — never the fat multi-platform archive, which would
make every download carry four libraries so one could be used. Each job downloads
the *released* engine binary and packages with that, because `ext build` bakes the
host's own target triple and ABI major into the manifest. `RUSTCFML_REF` is pinned
deliberately: the engine tree is a real build input.

## House style

- Comments explain *why*, and especially why-not. A comment recording a trap that
  cost an hour is worth more than one restating the code.
- Match the surrounding density. This codebase comments decisions, not syntax.
- Never no-op or stub to get past a blocker without flagging it.
- Lucee/ACF conventions are the reference for anything CFML-visible. The
  `Spreadsheet()` builder in the engine is the reference for *this* API's shape —
  format structs, polymorphic `data`, masks alongside formatting.
