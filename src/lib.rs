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
//! Delivered as a `.rcx` extension: see RustCFML's `docs/extensions.md`.

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
    let pdf = world::compile_markup(&markup).map_err(Error::new)?;
    Ok(ctx.binary(&pdf))
}

/// `TypstPageCount( markup )` → how many pages it lays out to.
fn typst_page_count<'a>(ctx: &'a Ctx, args: &[Value<'a>]) -> Result<Value<'a>> {
    let markup = markup_arg(args, "TypstPageCount")?;
    let n = world::page_count_markup(&markup).map_err(Error::new)?;
    Ok(ctx.int(n as i64))
}

/// `TypstFontCount()` → font faces visible to the engine.
///
/// A diagnostic worth having: "no fonts installed on this host" and "your
/// markup is wrong" produce very similar-looking failures otherwise.
fn typst_font_count<'a>(ctx: &'a Ctx, _args: &[Value<'a>]) -> Result<Value<'a>> {
    Ok(ctx.int(world::font_count() as i64))
}

/// `TypstVersion()` → the Typst release this extension is built against.
///
/// Worth exposing because a template written against Typst 0.15 is not
/// guaranteed to compile under a later one, and "which Typst is in there?" is
/// otherwise unanswerable from CFML.
fn typst_version<'a>(ctx: &'a Ctx, _args: &[Value<'a>]) -> Result<Value<'a>> {
    Ok(ctx.string(world::version()))
}

/// `TypstRender( template, data [, root] )` → the PDF as a Binary.
///
/// The one-shot form of `Document().template( … ).data( … ).toBinary()`, for the
/// common case where the template *is* the document and CFML only supplies the
/// data. `data` reaches the template as a virtual `data.json`, serialised by the
/// engine's own `serializeJSON`.
fn typst_render<'a>(ctx: &'a Ctx, args: &[Value<'a>]) -> Result<Value<'a>> {
    let path = match args.first() {
        Some(v) if !v.is_null() => v.to_string(),
        _ => {
            return Err(Error::expression(
                "TypstRender: a path to a .typ template is required",
            ))
        }
    };
    let root = args.get(2).filter(|v| !v.is_null()).map(|v| v.to_string());
    let template = document::resolve_template_public(&path, root.as_deref())?;

    let mut files = Vec::new();
    if let Some(data) = args.get(1).filter(|v| !v.is_null()) {
        let json = ctx
            .call("serializeJSON", &[*data])
            .map(|r| r.to_string())
            .map_err(|e| {
                Error::new(format!(
                    "TypstRender: could not serialise the data ({}). This needs an engine that \
                     provides extension capability tier 3.",
                    e
                ))
            })?;
        files.push(("data.json".to_string(), json.into_bytes()));
    }

    let pdf = world::compile_pdf(&world::Job {
        markup: None,
        template: Some(template),
        files,
        pdf: Default::default(),
        has_text: true,
    })
    .map_err(Error::new)?;
    Ok(ctx.binary(&pdf))
}

/// Once per process, before any request. Enumerating and parsing the host's
/// fonts costs ~0.25 s for 904 faces on the development machine; doing it here
/// means no request ever pays for it, which is exactly what `on_load` is for.
///
/// It used to cost ~2.1 s and 3.4 GB, because it built a `Font` per face and so
/// copied each font FILE once per face in it. See `world::Fonts`.
fn warm_fonts(_ctx: &Ctx, _settings: Value) -> Result<()> {
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
        "typstVersion"     => typst_version,
        "typstRender"      => typst_render,
    },
    classes: { CfmlDocument },
    on_load: warm_fonts,
}
