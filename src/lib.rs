//! Typst-backed document generation for RustCFML, as a dynamically loaded
//! extension (`.rcx`).
//!
//! Two surfaces, mirroring how the engine ships spreadsheets:
//!
//! * a **fluent `Document()` builder**, BoxLang-flavoured — mutators return the
//!   document so calls chain, terminals (`toBinary`, `write`) end the chain;
//! * a **raw Typst surface** (`typstCompile`, `typstPageCount`) for callers who
//!   want to hand-write markup.
//!
//! ```cfml
//! Document().heading( text = "Invoice", level = 1 )
//!           .table( data = lines, header = true )
//!           .write( expandPath( "/invoice.pdf" ) );
//! ```
//!
//! # Why this is an extension and not part of the engine
//!
//! Typst is ~93 crates and **+31.6 MB** of binary. PDF *reading* belongs in the
//! engine and is there already; document *authoring* is a much larger
//! dependency for a capability not every application wants. Shipping it as a
//! `.rcx` means you opt in by installing a file — and, because the extension
//! ABI is a C ABI over opaque handles, this crate links neither the engine nor
//! its allocator and does not care which rustc built the engine.

mod document;
mod emit;
mod model;
mod world;

use rustcfml_module::{module, Ctx, Error, Result, Value};

use document::CfmlDocument;

fn markup_arg<'a>(args: &[Value<'a>], fname: &str) -> Result<String> {
    match args.first() {
        Some(v) if !v.is_null() => {
            let s = v.to_string();
            if s.trim().is_empty() {
                Err(Error::expression(format!("{}: markup is required", fname)))
            } else {
                Ok(s)
            }
        }
        _ => Err(Error::expression(format!("{}: markup is required", fname))),
    }
}

/// `TypstCompile( markup )` → the PDF as a Binary.
fn typst_compile<'a>(ctx: &'a Ctx, args: &[Value<'a>]) -> Result<Value<'a>> {
    let markup = markup_arg(args, "TypstCompile")?;
    let pdf = world::compile_pdf(&markup).map_err(Error::new)?;
    Ok(ctx.binary(&pdf))
}

/// `TypstPageCount( markup )` → how many pages it lays out to.
fn typst_page_count<'a>(ctx: &'a Ctx, args: &[Value<'a>]) -> Result<Value<'a>> {
    let markup = markup_arg(args, "TypstPageCount")?;
    let n = world::page_count(&markup).map_err(Error::new)?;
    Ok(ctx.int(n as i64))
}

/// `TypstFontCount()` → font faces visible to the engine.
///
/// A diagnostic worth having: "no fonts installed on this host" and "your
/// markup is wrong" produce very similar-looking failures otherwise.
fn typst_font_count<'a>(ctx: &'a Ctx, _args: &[Value<'a>]) -> Result<Value<'a>> {
    Ok(ctx.int(world::font_count() as i64))
}

/// Once per process, before any request. Enumerating the host's fonts took
/// ~2.1 s for 904 faces on the development machine; doing it here means no
/// request ever pays for it, which is exactly what `on_load` is for.
fn warm_fonts() -> Result<()> {
    let _ = world::font_count();
    Ok(())
}

module! {
    name: "typst",
    version: "0.1.0",
    bifs: {
        "Document"         => document::fn_document,
        "isDocumentObject" => document::fn_is_document_object,
        "typstCompile"     => typst_compile,
        "typstPageCount"   => typst_page_count,
        "typstFontCount"   => typst_font_count,
    },
    classes: { CfmlDocument },
    on_load: warm_fonts,
}
