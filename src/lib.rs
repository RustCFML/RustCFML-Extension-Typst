//! Typst-backed document generation for RustCFML.
//!
//! Two surfaces, mirroring how the engine ships spreadsheets:
//!
//! * a **fluent `Document()` builder**, BoxLang-flavoured — mutators return the
//!   document so calls chain, terminals (`toBinary`, `write`) end the chain;
//! * **`<cfdocument>` compatibility**, which takes HTML and needs an HTML→Typst
//!   translation layer.
//!
//! Only the first slice is implemented so far — see `docs/PLAN.md` for the
//! full design and the phase order. What exists today proves the whole chain:
//! Typst embeds, compiles, and returns a PDF from inside a native module.
//!
//! ```cfml
//! bin = TypstCompile( "= Hello#linebreak()World" );   // PDF bytes
//! ```

mod document;
mod emit;
mod model;
mod world;

use rustcfml_cli::{CfmlError, CfmlResult, Value, Vm};

/// Entry point for the **cocktail** (statically linked) delivery path, which is
/// what `rustcfml --build` looks for and what works today.
///
/// The `.rcx` dynamic path will add a separate `extern "C"` entry point beside
/// this one, over the same functions — see `docs/PLAN.md` §"Two delivery modes".
pub fn register(vm: &mut Vm) {
    // The fluent builder — the surface application code should reach for.
    vm.register_native_fn("Document", document::fn_document);
    vm.register_native_fn("isDocumentObject", document::fn_is_document_object);

    // The raw Typst surface, for callers who want to hand-write markup.
    vm.register_native_fn("typstCompile", typst_compile);
    vm.register_native_fn("typstPageCount", typst_page_count);
    vm.register_native_fn("typstFontCount", typst_font_count);
}

fn markup_arg(args: &[Value], fname: &str) -> Result<String, CfmlError> {
    match args.first() {
        Some(v) => {
            let s = v.as_string();
            if s.trim().is_empty() {
                Err(CfmlError::runtime(format!("{}: markup is required", fname)))
            } else {
                Ok(s)
            }
        }
        None => Err(CfmlError::runtime(format!("{}: markup is required", fname))),
    }
}

/// `TypstCompile( markup )` → the PDF as a Binary.
fn typst_compile(args: Vec<Value>) -> CfmlResult {
    let markup = markup_arg(&args, "TypstCompile")?;
    world::compile_pdf(&markup)
        .map(Value::Binary)
        .map_err(CfmlError::runtime)
}

/// `TypstPageCount( markup )` → how many pages it lays out to.
fn typst_page_count(args: Vec<Value>) -> CfmlResult {
    let markup = markup_arg(&args, "TypstPageCount")?;
    world::page_count(&markup)
        .map(|n| Value::Int(n as i64))
        .map_err(CfmlError::runtime)
}

/// `TypstFontCount()` → font faces visible to the engine.
///
/// A diagnostic worth having: "no fonts installed on this host" and "your markup
/// is wrong" produce very similar-looking failures otherwise.
fn typst_font_count(_args: Vec<Value>) -> CfmlResult {
    Ok(Value::Int(world::font_count() as i64))
}
