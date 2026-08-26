//! `Document()` — the fluent builder, as a `.rcx` native class.
//!
//! Shaped after the engine's `Spreadsheet()`: mutators return the document so
//! calls chain, terminals (`write`, `toBinary`, `toTypst`, `pageCount`) end the
//! chain and return a value. Three conventions are borrowed from it deliberately,
//! because a CFML developer already knows them:
//!
//! * **styling is a plain struct** — `{ bold: true, bgcolor: "##dfe6e9" }` —
//!   with case-insensitive keys, accepted anywhere styling is accepted;
//! * **`data` is polymorphic** — a query, an array of arrays, or an array of
//!   structs, exactly as `spreadsheetAddRows` takes;
//! * **number masks belong with the formatting**, so a money column is one
//!   decision (`{ alignment: "right", numberFormat: "##,##0.00" }`) rather than
//!   a formatting call and a separate pass over the data.
//!
//! Note `&self` on every method — the extension ABI's contract. The document
//! model therefore lives behind a `Mutex`, which is also what makes this class
//! safe to hand to concurrent requests: a `Document` is a shared, reference-
//! typed CFML value exactly like a `Spreadsheet`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rustcfml_module::abi::ty;
use rustcfml_module::{Ctx, Error, NativeClass, Result, Value};

use crate::model::{
    Align, Block, Callout, Cell, Content, Document, Length, Margin, Run, Running, Stroke, Table,
    Template, TextStyle, Unit,
};
use crate::{emit, world};

pub struct CfmlDocument {
    inner: Mutex<Inner>,
}

struct Inner {
    doc: Document,
    /// Counter behind the virtual filenames given to embedded images.
    next_file: usize,
}

impl CfmlDocument {
    pub fn new() -> CfmlDocument {
        CfmlDocument {
            inner: Mutex::new(Inner {
                doc: Document::default(),
                next_file: 0,
            }),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        // A poisoned lock means a previous call panicked mid-mutation. The
        // document may be half-built, but refusing every later call would turn
        // one bad `.paragraph()` into a dead object; the model is plain data,
        // so recovering is honest here.
        //
        // NOTHING may call back into CFML while this is held: `ctx.call()` can
        // re-enter the engine, and a template or format callback that touched
        // the same document would self-deadlock. Every arm below resolves its
        // arguments first and takes the lock last.
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }

    fn push(&self, block: Block) {
        self.with(|i| i.doc.blocks.push(block));
    }

    pub fn to_typst(&self) -> String {
        self.with(|i| match &i.doc.template {
            // A template IS the document: showing generated markup that is not
            // what gets compiled would make `.toTypst()` lie, which is the one
            // thing the debugging seam must not do.
            Some(t) => std::fs::read_to_string(&t.main)
                .unwrap_or_else(|e| format!("// unable to read {}: {}", t.main.display(), e)),
            None => emit::emit(&i.doc),
        })
    }

    /// Everything the compiler needs, gathered under one lock.
    fn job(&self) -> world::Job {
        self.with(|i| {
            let mut files = i.doc.files.clone();
            if let Some(json) = &i.doc.data_json {
                files.push(("data.json".to_string(), json.clone().into_bytes()));
            }
            world::Job {
                markup: match &i.doc.template {
                    Some(_) => None,
                    None => Some(emit::emit(&i.doc)),
                },
                template: i.doc.template.clone(),
                files,
                pdf: i.doc.pdf.clone(),
                // A template's source is unknowable from here, and a template
                // is text; assume so rather than let a blank page through.
                has_text: i.doc.template.is_some() || i.doc.has_text(),
            }
        })
    }

    fn to_pdf(&self) -> Result<Vec<u8>> {
        world::compile_pdf(&self.job()).map_err(Error::new)
    }
}

// ---------------------------------------------------------------------------
// Argument access
//
// Arguments are read BY NAME, resolved against the same parameter list that
// `method_params` hands the host. Two dozen named parameters on `.table()` is
// too many to index by hand — the first draft of this file did, and a
// mis-numbered index is a silent wrong answer, not a compile error.
// ---------------------------------------------------------------------------

struct A<'v, 'a> {
    args: &'v [Value<'a>],
    params: &'static str,
}

impl<'v, 'a> A<'v, 'a> {
    fn new(args: &'v [Value<'a>], params: &'static str) -> Self {
        A { args, params }
    }

    /// The declared position of `name`. A name this method does not declare is a
    /// bug in this file, so it panics in debug and reads as absent in release.
    fn index(&self, name: &str) -> Option<usize> {
        let i = self.params.split(',').position(|p| p == name);
        debug_assert!(i.is_some(), "undeclared parameter [{}]", name);
        i
    }

    /// Present and non-null. "Absent" and "null" are treated identically so a
    /// caller can name only the arguments they care about.
    fn val(&self, name: &str) -> Option<Value<'a>> {
        self.args
            .get(self.index(name)?)
            .copied()
            .filter(|v| !v.is_null())
    }

    fn string(&self, name: &str) -> Option<String> {
        self.val(name).map(|v| v.to_string())
    }

    /// A string argument that must not be blank — the common case for text.
    fn text(&self, name: &str) -> Option<String> {
        self.string(name).filter(|s| !s.trim().is_empty())
    }

    fn bool(&self, name: &str) -> Option<bool> {
        self.val(name).map(|v| v.is_true())
    }

    fn num(&self, name: &str) -> Result<Option<f64>> {
        match self.val(name) {
            None => Ok(None),
            Some(v) => match v.as_f64() {
                Ok(n) if n.is_finite() => Ok(Some(n)),
                _ => Err(Error::expression(format!(
                    "Document: {} must be a number, got [{}]",
                    name, v
                ))),
            },
        }
    }

    fn unit(&self) -> Result<Unit> {
        match self.text("unit") {
            None => Ok(Unit::Pt),
            Some(s) => Unit::parse(&s).ok_or_else(|| {
                Error::expression(format!(
                    "Document: unknown unit [{}] — use pt, mm, cm, in, em, % or fr",
                    s
                ))
            }),
        }
    }

    fn len(&self, name: &str, unit: Unit) -> Result<Option<Length>> {
        Ok(self.num(name)?.map(|value| Length { value, unit }))
    }

    fn align(&self, name: &str) -> Result<Option<Align>> {
        match self.text(name) {
            None => Ok(None),
            Some(s) => parse_align(&s).map(Some),
        }
    }

    fn color(&self, name: &str) -> Result<Option<String>> {
        match self.text(name) {
            None => Ok(None),
            Some(s) => parse_color(&s).map(Some),
        }
    }

    /// The style for this call: the `format` struct first, then any legacy
    /// named arguments (`bold=`, `size=`, …) layered over it.
    ///
    /// Both forms are supported on purpose. `format={…}` is the spreadsheet
    /// convention and scales; the individual arguments read better for one-off
    /// emphasis and were the original API.
    fn style(&self, unit: Unit) -> Result<TextStyle> {
        let mut style = match self.val("format") {
            Some(v) => style_from_struct(v)?,
            None => TextStyle::default(),
        };
        if self.bool("bold").unwrap_or(false) {
            style.bold = true;
        }
        if self.bool("italic").unwrap_or(false) {
            style.italic = true;
        }
        if let Some(v) = self.len("size", Unit::Pt)? {
            style.size = Some(v);
        }
        if let Some(v) = self.text("font") {
            style.font = Some(v);
        }
        if let Some(v) = self.color("color")? {
            style.color = Some(v);
        }
        if let Some(v) = self.align("align")? {
            style.align = Some(v);
        }
        let _ = unit;
        Ok(style)
    }
}

fn parse_align(s: &str) -> Result<Align> {
    Align::parse(s).ok_or_else(|| {
        Error::expression(format!(
            "Document: unknown alignment [{}] — use left, center, right or justify",
            s
        ))
    })
}

/// A CFML colour as a Typst colour expression.
///
/// Rejecting anything else is what keeps caller text out of the generated code:
/// a colour is one of the few places the builder emits a bare expression rather
/// than a string literal, so it is one of the few places that has to be checked.
fn parse_color(raw: &str) -> Result<String> {
    let s = raw.trim();
    let hex = s.trim_start_matches('#');
    if !hex.is_empty()
        && (hex.len() == 3 || hex.len() == 6 || hex.len() == 8)
        && hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Ok(format!("rgb(\"#{}\")", hex));
    }
    const NAMED: &[&str] = &[
        "black", "gray", "grey", "silver", "white", "navy", "blue", "aqua", "teal", "eastern",
        "purple", "fuchsia", "maroon", "red", "orange", "yellow", "olive", "green", "lime",
    ];
    let lower = s.to_ascii_lowercase();
    if NAMED.contains(&lower.as_str()) {
        let name = if lower == "grey" {
            "gray".to_string()
        } else {
            lower
        };
        return Ok(name);
    }
    Err(Error::expression(format!(
        "Document: unknown colour [{}] — use a hex value (##RGB/##RRGGBB) or one of: {}",
        raw,
        NAMED.join(", ")
    )))
}

/// A Typst label or reference name.
///
/// Labels are emitted as `<name>`, which is code, so the charset is restricted
/// rather than escaped. There is no string-literal form of a label to fall back
/// on.
fn parse_label(raw: &str) -> Result<String> {
    let s = raw.trim();
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' || c == '.')
    {
        Ok(s.to_string())
    } else {
        Err(Error::expression(format!(
            "Document: [{}] is not a usable label — letters, digits, and - _ : . only",
            raw
        )))
    }
}

/// A link target.
///
/// Emitted as a Typst string, so it cannot become code — but it *can* become
/// navigable, which is its own problem: a `javascript:` or `data:` URL in a
/// PDF viewer is an attack surface, and application data is exactly where such
/// a string arrives from. Only the schemes a document has a reason to link to
/// are allowed.
fn parse_url(raw: &str) -> Result<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(Error::expression("Document: a link needs a url"));
    }
    let lower = s.to_ascii_lowercase();
    const OK: &[&str] = &["http://", "https://", "mailto:", "tel:"];
    if OK.iter().any(|p| lower.starts_with(p)) || s.starts_with('#') || s.starts_with('/') {
        return Ok(s.to_string());
    }
    Err(Error::expression(format!(
        "Document: refusing to link to [{}] — only http, https, mailto, tel and same-document \
         targets are allowed",
        raw
    )))
}

/// Styling from a CFML format struct.
///
/// Key names follow the engine's spreadsheet format struct wherever there is an
/// equivalent, including its aliases, so the two builders read the same.
fn style_from_struct(v: Value) -> Result<TextStyle> {
    if v.kind() != ty::STRUCT {
        return Err(Error::expression(
            "Document: format must be a struct, e.g. { bold: true, alignment: \"right\" }",
        ));
    }
    let flag = |names: &[&str]| names.iter().any(|n| v.key(n).is_true());
    let first_text = |names: &[&str]| -> Option<String> {
        names
            .iter()
            .map(|n| v.key(n))
            .find(|x| !x.is_null())
            .map(|x| x.to_string())
            .filter(|s| !s.trim().is_empty())
    };

    let unit = match first_text(&["unit"]) {
        Some(u) => Unit::parse(&u)
            .ok_or_else(|| Error::expression(format!("Document: unknown unit [{}]", u)))?,
        None => Unit::Pt,
    };
    let size = match first_text(&["size", "fontsize"]) {
        None => None,
        Some(s) => match s.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Some(Length { value: n, unit }),
            _ => {
                return Err(Error::expression(format!(
                    "Document: format.size must be a number, got [{}]",
                    s
                )))
            }
        },
    };
    let color = match first_text(&["color", "fontcolor", "colour"]) {
        Some(c) => Some(parse_color(&c)?),
        None => None,
    };
    let bgcolor = match first_text(&["bgcolor", "backgroundcolor", "fgcolor", "fill"]) {
        Some(c) => Some(parse_color(&c)?),
        None => None,
    };
    let align = match first_text(&["align", "alignment", "horizontalalignment"]) {
        Some(a) => Some(parse_align(&a)?),
        None => None,
    };

    Ok(TextStyle {
        bold: flag(&["bold"]),
        italic: flag(&["italic"]),
        underline: flag(&["underline"]),
        strike: flag(&["strike", "strikethrough", "linethrough"]),
        smallcaps: flag(&["smallcaps"]),
        size,
        font: first_text(&["font", "fontname", "family"]),
        color,
        bgcolor,
        align,
        number_format: first_text(&["numberformat", "dataformat", "numbermask"]),
        date_format: first_text(&["dateformat", "datemask"]),
    })
}

/// Inline content from a CFML value.
///
/// A **string** becomes one plain run — the overwhelmingly common case, and the
/// reason no existing call has to change. An **array** is a sequence of runs,
/// each either a string or a struct:
///
/// ```cfml
/// .paragraph( text = [
///     "Please pay ",
///     { text: "£1,240.00", bold: true },
///     " by ",
///     { text: "31 March", underline: true },
///     { link: "https://acme.example/pay", text: "Pay online" }
/// ] )
/// ```
fn content_from(v: Option<Value>) -> Result<Content> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.kind() != ty::ARRAY {
        return Ok(vec![Run::Text {
            text: v.to_string(),
            style: TextStyle::default(),
        }]);
    }
    let n = v.len()?;
    let mut runs: Content = Vec::with_capacity(n);
    for i in 0..n {
        let item = v.get(i);
        if item.kind() != ty::STRUCT {
            runs.push(Run::Text {
                text: item.to_string(),
                style: TextStyle::default(),
            });
            continue;
        }
        // A run struct carries the same styling keys as a format struct, plus
        // the run kinds. `text` is the content for every kind.
        let inner = || -> Result<Content> {
            let t = item.key("text");
            if t.kind() == ty::ARRAY {
                content_from(Some(t))
            } else {
                Ok(vec![Run::Text {
                    text: t.to_string(),
                    style: style_from_struct(item)?,
                }])
            }
        };
        let link = item.key("link");
        let footnote = item.key("footnote");
        let reference = item.key("ref");
        if !link.is_null() {
            runs.push(Run::Link {
                url: parse_url(&link.to_string())?,
                content: inner()?,
            });
        } else if !footnote.is_null() {
            // Two spellings, because both read naturally and meaning only one
            // of them would be a trap:
            //   { text: "the terms", footnote: "See clause 4" }  → text, then a note
            //   { footnote: true, text: "See clause 4" }         → just the note
            if footnote.is_true() && footnote.kind() == ty::BOOL {
                runs.push(Run::Footnote { content: inner()? });
            } else {
                runs.extend(inner()?);
                runs.push(Run::Footnote {
                    content: vec![Run::Text {
                        text: footnote.to_string(),
                        style: TextStyle::default(),
                    }],
                });
            }
        } else if !reference.is_null() {
            runs.push(Run::Ref {
                label: parse_label(&reference.to_string())?,
            });
        } else if item.key("break").is_true() || item.key("linebreak").is_true() {
            runs.push(Run::Break);
        } else {
            runs.extend(inner()?);
        }
    }
    Ok(runs)
}

/// A list argument: a CFML array, or a comma-delimited string.
fn string_items(v: Option<Value>) -> Result<Vec<String>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.kind() == ty::ARRAY {
        let n = v.len()?;
        return Ok((0..n).map(|i| v.get(i).to_string()).collect());
    }
    Ok(v.to_string()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// A list of measurements, e.g. table column widths.
fn length_items(v: Option<Value>, unit: Unit, what: &str) -> Result<Option<Vec<Length>>> {
    let Some(v) = v else { return Ok(None) };
    let mut out = Vec::new();
    for raw in string_items(Some(v))? {
        // `auto` and `1fr` are the two things a column width wants to be that a
        // plain number is not; `auto` is Typst's own keyword.
        match raw.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => out.push(Length { value: n, unit }),
            _ => {
                return Err(Error::expression(format!(
                    "Document: {} [{}] is not a number",
                    what, raw
                )))
            }
        }
    }
    Ok(Some(out))
}

// ---------------------------------------------------------------------------
// Tabular data
// ---------------------------------------------------------------------------

/// Rows of raw CFML values, plus the header labels the source implies.
struct Rows<'a> {
    header: Vec<String>,
    body: Vec<Vec<Value<'a>>>,
}

/// Flatten a CFML value into rows of raw values.
///
/// Accepts a query, an array of arrays, an array of structs (the first struct's
/// keys set the column order) or an array of scalars — the same polymorphism
/// `spreadsheetAddRows` has.
///
/// `select` names the columns to keep, in order. For a query or an array of
/// structs that is a column selection; for an array of arrays it is ignored,
/// because there are no names to select on.
///
/// Values stay **unstringified** here so a column's number mask can be applied
/// to the value rather than to its default rendering — `numberFormat( "3.5",
/// "0.00" )` and `numberFormat( 3.5, "0.00" )` are not the same call.
fn table_rows<'a>(v: Value<'a>, select: &[String]) -> Result<Rows<'a>> {
    let keep = |names: &[String]| -> Vec<usize> {
        if select.is_empty() {
            return (0..names.len()).collect();
        }
        select
            .iter()
            .filter_map(|want| {
                names
                    .iter()
                    .position(|have| have.eq_ignore_ascii_case(want.trim()))
            })
            .collect()
    };

    if v.kind() == ty::QUERY {
        let columns: Vec<String> = v.query_columns()?.iter().map(|c| c.to_string()).collect();
        let wanted = keep(&columns);
        if !select.is_empty() && wanted.len() != select.len() {
            let missing: Vec<&str> = select
                .iter()
                .map(|s| s.trim())
                .filter(|want| !columns.iter().any(|have| have.eq_ignore_ascii_case(want)))
                .collect();
            return Err(Error::expression(format!(
                "Document.table(): the query has no column(s) [{}]. It has: {}",
                missing.join(", "),
                columns.join(", ")
            )));
        }
        // One crossing per COLUMN, not per cell: `query_column` materialises the
        // whole column in a single call, which is the difference between 3
        // crossings and 30,000 on a 10k-row report.
        let cols: Vec<Value<'a>> = wanted.iter().map(|&i| v.query_column(i)).collect();
        let row_count = v.query_row_count()?;
        let mut body = Vec::with_capacity(row_count);
        for r in 0..row_count {
            body.push(cols.iter().map(|c| c.get(r)).collect());
        }
        return Ok(Rows {
            header: wanted.iter().map(|&i| columns[i].clone()).collect(),
            body,
        });
    }

    if v.kind() == ty::ARRAY {
        let n = v.len()?;
        let first = if n > 0 { Some(v.get(0)) } else { None };
        if let Some(f) = first.filter(|f| f.kind() == ty::STRUCT) {
            let keys: Vec<String> = f.keys()?.iter().map(|k| k.to_string()).collect();
            let wanted = keep(&keys);
            let names: Vec<String> = wanted.iter().map(|&i| keys[i].clone()).collect();
            let mut body = Vec::with_capacity(n);
            for i in 0..n {
                let item = v.get(i);
                if item.kind() != ty::STRUCT {
                    continue;
                }
                body.push(names.iter().map(|k| item.key(k)).collect());
            }
            return Ok(Rows {
                header: names,
                body,
            });
        }
        let mut body = Vec::with_capacity(n);
        for i in 0..n {
            let item = v.get(i);
            if item.kind() == ty::ARRAY {
                let inner = item.len()?;
                body.push((0..inner).map(|j| item.get(j)).collect());
            } else {
                body.push(vec![item]);
            }
        }
        return Ok(Rows {
            header: Vec::new(),
            body,
        });
    }

    Err(Error::expression(
        "Document.table() takes a query, an array of arrays or an array of structs",
    ))
}

/// A value as the string that goes in the cell.
///
/// When the column carries a mask, the **engine** applies it — `numberFormat`,
/// `dateFormat` and `lsCurrencyFormat` are called back through `ctx`. That is
/// deliberate: a mask in a document must produce the same string the same mask
/// produces everywhere else in the application, and reimplementing CFML's mask
/// language here would be a second, subtly different implementation of it.
///
/// The cost is one engine call per formatted cell, so it happens only for
/// columns that actually asked for a mask.
fn format_cell<'a>(ctx: &'a Ctx, v: Value<'a>, style: &TextStyle) -> Result<String> {
    if v.is_null() {
        return Ok(String::new());
    }
    let called = |bif: &str, arg: Value<'a>| -> Result<String> {
        ctx.call(bif, &[v, arg])
            .map(|r| r.to_string())
            .map_err(|e| {
                Error::new(format!(
                    "Document.table(): {} failed while formatting a cell: {}",
                    bif, e
                ))
            })
    };
    if let Some(mask) = &style.number_format {
        return called("numberFormat", ctx.string(mask));
    }
    if let Some(mask) = &style.date_format {
        return called("dateFormat", ctx.string(mask));
    }
    Ok(v.to_string())
}

/// Sniff an image's type from its bytes, not its filename — the same rule the
/// engine's `imageRead` follows, and what Typst keys its decoder on.
fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else if bytes.len() > 4
        && std::str::from_utf8(&bytes[..bytes.len().min(512)])
            .map(|s| s.contains("<svg"))
            .unwrap_or(false)
    {
        Some("svg")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Parameter lists
//
// One const per method, used BOTH to answer `method_params` and to resolve
// arguments by name in `call`. They cannot drift apart.
// ---------------------------------------------------------------------------

const P_PAGESIZE: &str = "paper,width,height,unit";
const P_ORIENTATION: &str = "orientation";
const P_MARGIN: &str = "all,top,bottom,left,right,unit";
const P_FONT: &str = "family,size,color,format";
const P_NUMBERING: &str = "pattern";
const P_HEADINGNUMBERING: &str = "pattern";
const P_PAGECOLUMNS: &str = "count";
const P_PAGEFILL: &str = "color";
const P_TITLE: &str = "title";
const P_AUTHOR: &str = "author";
const P_KEYWORDS: &str = "keywords";
const P_RUNNING: &str = "content,align,format";
const P_HEADING: &str = "text,level,align,bold,italic,size,color,font,format,label";
const P_PARAGRAPH: &str = "text,align,bold,italic,size,color,font,format";
const P_LINK: &str = "url,text,align,format";
const P_LIST: &str = "items,ordered,align,size,color,font,format";
const P_TERMS: &str = "items,format";
const P_QUOTE: &str = "text,attribution,align,format";
const P_CODE: &str = "text,language";
const P_TABLE: &str = "data,header,columns,align,unit,size,font,widths,columnList,headers,\
                       format,headerFormat,columnFormats,stripe,fill,stroke,strokeColor,\
                       strokeWidth,gutter,inset,caption,label,totals,totalsLabel";
const P_CALLOUT: &str = "content,format,fill,stroke,radius,inset,width,unit,align";
const P_IMAGE: &str = "source,width,height,align,unit,caption,label";
const P_OUTLINE: &str = "title,depth";
const P_SPACER: &str = "amount,unit";
const P_TYPST: &str = "markup";
const P_TEMPLATE: &str = "path,root";
const P_DATA: &str = "data";
const P_PDF: &str = "standard,tagged,pages,creator,timestamp";
const P_WRITE: &str = "path,overwrite";
const P_NONE: &str = "";

// ---------------------------------------------------------------------------
// The CFML class
// ---------------------------------------------------------------------------

impl NativeClass for CfmlDocument {
    const CLASS_NAME: &'static str = "Document";

    fn new(_ctx: &Ctx, _args: &[Value]) -> Result<Self> {
        Ok(CfmlDocument::new())
    }

    /// Declared so a named call binds by name. A method the host cannot see
    /// parameter names for has named arguments **refused** rather than bound by
    /// position, which would be a silent wrong answer.
    fn method_params(method: &str) -> Option<&'static str> {
        params_for(method)
    }

    fn call<'a>(&self, ctx: &'a Ctx, method: &str, args: &[Value<'a>]) -> Result<Value<'a>> {
        let m = method.to_ascii_lowercase();
        let params = params_for(&m).ok_or_else(|| unknown_method(method))?;
        let a = A::new(args, params);

        match m.as_str() {
            // ---- page setup (mutators, return this) ------------------------
            "pagesize" | "paper" => {
                let unit = a.unit()?;
                let paper = a.text("paper");
                let width = a.len("width", unit)?;
                let height = a.len("height", unit)?;
                self.with(|i| {
                    if let Some(p) = paper {
                        i.doc.page.paper = p.trim().to_ascii_lowercase();
                        i.doc.page.width = None;
                        i.doc.page.height = None;
                    }
                    if let Some(w) = width {
                        i.doc.page.width = Some(w);
                    }
                    if let Some(h) = height {
                        i.doc.page.height = Some(h);
                    }
                });
                Ok(ctx.this())
            }
            "orientation" => {
                let o = a.string("orientation").unwrap_or_default();
                let landscape = match o.trim().to_ascii_lowercase().as_str() {
                    "landscape" => true,
                    "portrait" => false,
                    other => {
                        return Err(Error::expression(format!(
                            "Document.orientation() takes \"portrait\" or \"landscape\", got [{}]",
                            other
                        )))
                    }
                };
                self.with(|i| i.doc.page.landscape = landscape);
                Ok(ctx.this())
            }
            "margin" | "margins" => {
                let unit = a.unit()?;
                let all = a.len("all", unit)?;
                let mut margin = Margin {
                    top: all,
                    bottom: all,
                    left: all,
                    right: all,
                };
                if let Some(v) = a.len("top", unit)? {
                    margin.top = Some(v);
                }
                if let Some(v) = a.len("bottom", unit)? {
                    margin.bottom = Some(v);
                }
                if let Some(v) = a.len("left", unit)? {
                    margin.left = Some(v);
                }
                if let Some(v) = a.len("right", unit)? {
                    margin.right = Some(v);
                }
                self.with(|i| i.doc.page.margin = margin);
                Ok(ctx.this())
            }
            "font" => {
                let style = a.style(Unit::Pt)?;
                let family = a.text("family").or_else(|| style.font.clone());
                let size = a.len("size", Unit::Pt)?.or(style.size);
                let color = match a.color("color")? {
                    Some(c) => Some(c),
                    None => style.color.clone(),
                };
                self.with(|i| {
                    if family.is_some() {
                        i.doc.font_family = family;
                    }
                    if size.is_some() {
                        i.doc.font_size = size;
                    }
                    if color.is_some() {
                        i.doc.text_color = color;
                    }
                });
                Ok(ctx.this())
            }
            "numbering" => {
                let p = a.text("pattern");
                self.with(|i| i.doc.page.numbering = p);
                Ok(ctx.this())
            }
            "headingnumbering" | "numberheadings" => {
                // Typst REFUSES to `#ref` an unnumbered heading ("cannot
                // reference heading without numbering"), so a document that uses
                // `{ ref = … }` needs this on. It was found by referencing one.
                let p = a.text("pattern").unwrap_or_else(|| "1.".to_string());
                self.with(|i| i.doc.heading_numbering = Some(p));
                Ok(ctx.this())
            }
            "columns" | "pagecolumns" => {
                let n = a.num("count")?.unwrap_or(1.0);
                if !(1.0..=12.0).contains(&n) {
                    return Err(Error::expression(format!(
                        "Document.columns(): count must be 1-12, got {}",
                        n
                    )));
                }
                self.with(|i| i.doc.page.columns = Some(n as u32));
                Ok(ctx.this())
            }
            "pagefill" | "background" => {
                let c = a.color("color")?;
                self.with(|i| i.doc.page.fill = c);
                Ok(ctx.this())
            }
            "title" => {
                let t = a.string("title");
                self.with(|i| i.doc.title = t);
                Ok(ctx.this())
            }
            "author" => {
                let v = a.string("author");
                self.with(|i| i.doc.author = v);
                Ok(ctx.this())
            }
            "keywords" => {
                let k = string_items(a.val("keywords"))?;
                self.with(|i| i.doc.keywords = k);
                Ok(ctx.this())
            }
            "header" | "footer" => {
                let content = content_from(a.val("content"))?;
                let style = a.style(Unit::Pt)?;
                // A running header's own styling belongs to its text, not to a
                // block wrapper — there is no block here.
                let content = if style.is_plain() {
                    content
                } else {
                    content
                        .into_iter()
                        .map(|r| match r {
                            Run::Text { text, style: s } => Run::Text {
                                text,
                                style: style.overlay(&s),
                            },
                            other => other,
                        })
                        .collect()
                };
                let running = Running {
                    content,
                    align: a.align("align")?.or(style.align),
                };
                let is_header = m == "header";
                self.with(|i| {
                    if is_header {
                        i.doc.header = Some(running);
                    } else {
                        i.doc.footer = Some(running);
                    }
                });
                Ok(ctx.this())
            }

            // ---- content ---------------------------------------------------
            "heading" => {
                let content = content_from(a.val("text"))?;
                let level = a.num("level")?.unwrap_or(1.0);
                if !(1.0..=6.0).contains(&level) {
                    return Err(Error::expression(format!(
                        "Document.heading(): level must be 1-6, got {}",
                        level
                    )));
                }
                let style = a.style(Unit::Pt)?;
                let label = match a.text("label") {
                    Some(l) => Some(parse_label(&l)?),
                    None => None,
                };
                self.push(Block::Heading {
                    level: level as u8,
                    content,
                    style,
                    label,
                });
                Ok(ctx.this())
            }
            "paragraph" | "text" | "p" | "rich" | "richtext" => {
                let content = content_from(a.val("text"))?;
                let style = a.style(Unit::Pt)?;
                self.push(Block::Paragraph { content, style });
                Ok(ctx.this())
            }
            "link" => {
                let url = parse_url(&a.string("url").unwrap_or_default())?;
                let inner = match a.val("text") {
                    Some(v) => content_from(Some(v))?,
                    // A link with no text shows its target, which is what a
                    // reader needs when the author gave nothing else.
                    None => vec![Run::Text {
                        text: url.clone(),
                        style: TextStyle::default(),
                    }],
                };
                let style = a.style(Unit::Pt)?;
                self.push(Block::Paragraph {
                    content: vec![Run::Link {
                        url,
                        content: inner,
                    }],
                    style,
                });
                Ok(ctx.this())
            }
            "list" => {
                let raw = a.val("items");
                let items: Vec<Content> = match raw {
                    // An array may hold run arrays/structs per item, so each
                    // element goes through `content_from` on its own.
                    Some(v) if v.kind() == ty::ARRAY => {
                        let n = v.len()?;
                        let mut out = Vec::with_capacity(n);
                        for i in 0..n {
                            out.push(content_from(Some(v.get(i)))?);
                        }
                        out
                    }
                    other => string_items(other)?
                        .into_iter()
                        .map(|s| {
                            vec![Run::Text {
                                text: s,
                                style: TextStyle::default(),
                            }]
                        })
                        .collect(),
                };
                let ordered = a.bool("ordered").unwrap_or(false);
                let style = a.style(Unit::Pt)?;
                self.push(Block::List {
                    items,
                    ordered,
                    style,
                });
                Ok(ctx.this())
            }
            "terms" | "definitions" => {
                let v = a.val("items").ok_or_else(|| {
                    Error::expression("Document.terms(): items is required — a struct, or an array of {term, description}")
                })?;
                let mut items: Vec<(Content, Content)> = Vec::new();
                match v.kind() {
                    ty::STRUCT => {
                        for k in v.keys()? {
                            items.push((
                                vec![Run::Text {
                                    text: k.to_string(),
                                    style: TextStyle::default(),
                                }],
                                content_from(Some(v.key(k)))?,
                            ));
                        }
                    }
                    ty::ARRAY => {
                        let n = v.len()?;
                        for i in 0..n {
                            let item = v.get(i);
                            if item.kind() == ty::ARRAY && item.len()? >= 2 {
                                items.push((
                                    content_from(Some(item.get(0)))?,
                                    content_from(Some(item.get(1)))?,
                                ));
                            } else if item.kind() == ty::STRUCT {
                                items.push((
                                    content_from(Some(item.key("term")))?,
                                    content_from(Some(item.key("description")))?,
                                ));
                            } else {
                                return Err(Error::expression(
                                    "Document.terms(): each item must be a struct \
                                     {term, description} or a two-element array",
                                ));
                            }
                        }
                    }
                    _ => {
                        return Err(Error::expression(
                            "Document.terms(): items must be a struct or an array",
                        ))
                    }
                }
                let style = a.style(Unit::Pt)?;
                self.push(Block::Terms { items, style });
                Ok(ctx.this())
            }
            "quote" | "blockquote" => {
                let content = content_from(a.val("text"))?;
                let attribution = match a.val("attribution") {
                    Some(v) => Some(content_from(Some(v))?),
                    None => None,
                };
                let style = a.style(Unit::Pt)?;
                self.push(Block::Quote {
                    content,
                    attribution,
                    style,
                });
                Ok(ctx.this())
            }
            "code" | "pre" => {
                let text = a.string("text").unwrap_or_default();
                let lang = a.text("language");
                self.push(Block::Code { text, lang });
                Ok(ctx.this())
            }
            "table" | "grid" | "fromquery" => {
                let table = self.build_table(ctx, &a, &m)?;
                self.push(Block::Table(Box::new(table)));
                Ok(ctx.this())
            }
            "callout" | "box" | "panel" => {
                let unit = a.unit()?;
                let content = content_from(a.val("content"))?;
                let mut style = a.style(unit)?;
                if let Some(v) = a.align("align")? {
                    style.align = Some(v);
                }
                let callout = Callout {
                    content,
                    fill: a.color("fill")?.or_else(|| style.bgcolor.clone()),
                    stroke: a.color("stroke")?,
                    radius: a.len("radius", unit)?,
                    inset: a.len("inset", unit)?,
                    width: a.len("width", unit)?,
                    style,
                };
                self.push(Block::Callout(callout));
                Ok(ctx.this())
            }
            "image" => {
                let unit = a.unit()?;
                let source = a.val("source").ok_or_else(|| {
                    Error::expression("Document.image(): a file path or binary is required")
                })?;
                let bytes = if source.kind() == ty::BINARY {
                    source.as_bytes()?.to_vec()
                } else {
                    let path = source.to_string();
                    std::fs::read(&path).map_err(|e| {
                        Error::new(format!(
                            "Document.image(): unable to read [{}]: {}",
                            path, e
                        ))
                    })?
                };
                let ext = image_extension(&bytes).ok_or_else(|| {
                    Error::expression(
                        "Document.image(): unrecognised image data — PNG, JPEG, GIF and SVG are \
                         supported",
                    )
                })?;
                let width = a.len("width", unit)?;
                let height = a.len("height", unit)?;
                let align = a.align("align")?;
                let caption = match a.val("caption") {
                    Some(v) => Some(content_from(Some(v))?),
                    None => None,
                };
                let label = match a.text("label") {
                    Some(l) => Some(parse_label(&l)?),
                    None => None,
                };
                self.with(|i| {
                    i.next_file += 1;
                    let file = format!("img{}.{}", i.next_file, ext);
                    i.doc.files.push((file.clone(), bytes));
                    i.doc.blocks.push(Block::Image {
                        file,
                        width,
                        height,
                        align,
                        caption,
                        label,
                    });
                });
                Ok(ctx.this())
            }
            "outline" | "toc" | "tableofcontents" => {
                let title = a.text("title");
                let depth = match a.num("depth")? {
                    Some(d) if (1.0..=6.0).contains(&d) => Some(d as u8),
                    Some(d) => {
                        return Err(Error::expression(format!(
                            "Document.outline(): depth must be 1-6, got {}",
                            d
                        )))
                    }
                    None => None,
                };
                self.push(Block::Outline { title, depth });
                Ok(ctx.this())
            }
            "pagebreak" => {
                self.push(Block::PageBreak);
                Ok(ctx.this())
            }
            "rule" | "hr" => {
                self.push(Block::Rule);
                Ok(ctx.this())
            }
            "spacer" | "space" => {
                let unit = a.unit()?;
                let amount = a.len("amount", unit)?.unwrap_or(Length {
                    value: 1.0,
                    unit: Unit::Em,
                });
                self.push(Block::Spacer(amount));
                Ok(ctx.this())
            }
            "typst" => {
                // The escape hatch, and the ONLY place caller text becomes Typst
                // code rather than a Typst string literal.
                let markup = a.string("markup").unwrap_or_default();
                self.push(Block::Raw(markup));
                Ok(ctx.this())
            }

            // ---- templates --------------------------------------------------
            "template" => {
                let path = a.text("path").ok_or_else(|| {
                    Error::expression("Document.template(): a path to a .typ file is required")
                })?;
                let template = resolve_template(&path, a.text("root").as_deref())?;
                self.with(|i| i.doc.template = Some(template));
                Ok(ctx.this())
            }
            "data" => {
                let v = a.val("data").unwrap_or_else(|| ctx.strukt());
                let json = serialize_json(ctx, v)?;
                self.with(|i| i.doc.data_json = Some(json));
                Ok(ctx.this())
            }

            // ---- PDF options -------------------------------------------------
            "pdf" | "pdfoptions" => {
                let standards = string_items(a.val("standard"))?
                    .into_iter()
                    .map(|s| s.trim().to_ascii_lowercase())
                    .collect::<Vec<_>>();
                for s in &standards {
                    world::check_pdf_standard(s).map_err(Error::expression)?;
                }
                let tagged = a.bool("tagged");
                let pages = a.text("pages");
                let creator = a.text("creator");
                let timestamp = match a.val("timestamp") {
                    None => None,
                    Some(v) => Some(epoch_seconds(ctx, v)?),
                };
                // Merge first, validate, then commit. Typst refuses to write a
                // tagged PDF with a page range — and tagging is ON by default,
                // so `pages="1-2"` on its own ALWAYS failed, at the terminal,
                // with a message about a setting the caller never touched.
                // Found by exporting a page range.
                let merged = self.with(|i| {
                    let mut p = i.doc.pdf.clone();
                    if !standards.is_empty() {
                        p.standards = standards;
                    }
                    if tagged.is_some() {
                        p.tagged = tagged;
                    }
                    if pages.is_some() {
                        p.page_ranges = pages;
                    }
                    if creator.is_some() {
                        p.creator = creator;
                    }
                    if timestamp.is_some() {
                        p.timestamp = timestamp;
                    }
                    p
                });
                if merged.page_ranges.is_some() && merged.tagged != Some(false) {
                    return Err(Error::expression(
                        "Document.pdf(): a page range cannot be combined with tagged PDF, and \
                         tagged PDF is on by default. Pass tagged=false as well if you want a \
                         page range — that does drop the document's accessibility tagging, which \
                         is why it is not done for you.",
                    ));
                }
                self.with(|i| i.doc.pdf = merged);
                Ok(ctx.this())
            }

            // ---- terminals --------------------------------------------------
            "totypst" => Ok(ctx.string(self.to_typst())),
            "tobinary" | "topdf" => Ok(ctx.binary(&self.to_pdf()?)),
            "pagecount" => {
                let n = world::page_count(&self.job()).map_err(Error::new)?;
                Ok(ctx.int(n as i64))
            }
            "write" | "save" => {
                let path = a.string("path").unwrap_or_default();
                if path.trim().is_empty() {
                    return Err(Error::expression(
                        "Document.write(): a file path is required",
                    ));
                }
                let overwrite = a.bool("overwrite").unwrap_or(true);
                if !overwrite && Path::new(&path).exists() {
                    return Err(Error::new(format!(
                        "Document.write(): [{}] already exists and overwrite is false",
                        path
                    )));
                }
                let pdf = self.to_pdf()?;
                std::fs::write(&path, pdf).map_err(|e| {
                    Error::new(format!(
                        "Document.write(): unable to write [{}]: {}",
                        path, e
                    ))
                })?;
                Ok(ctx.this())
            }
            _ => Err(unknown_method(method)),
        }
    }
}

impl CfmlDocument {
    /// Assemble a table from the twenty-odd arguments `.table()` accepts.
    ///
    /// Kept out of `call` because it is the only arm with real logic, and
    /// because `.grid()` is the same thing with two different defaults.
    fn build_table<'a>(&self, ctx: &'a Ctx, a: &A<'_, 'a>, method: &str) -> Result<Table> {
        let is_grid = method == "grid";
        let unit = a.unit()?;

        let data = a.val("data").ok_or_else(|| {
            Error::expression(format!(
                "Document.{}(): data is required — a query, an array of arrays, or an array of \
                 structs",
                method
            ))
        })?;
        let select = string_items(a.val("columnList"))?;
        let rows = table_rows(data, &select)?;

        // A grid is furniture, not a data table, so it defaults to no header
        // and no lines. Both remain overridable.
        let header = a
            .bool("header")
            .unwrap_or(!is_grid && !rows.header.is_empty());

        let base = a.style(unit)?;
        let header_style = match a.val("headerFormat") {
            Some(v) => style_from_struct(v)?,
            None => TextStyle {
                bold: true,
                ..TextStyle::default()
            },
        };

        // Column styles arrive keyed by column NAME (a struct) or by position
        // (an array) — the same two ways a caller thinks about columns.
        let mut column_styles: Vec<Option<TextStyle>> = vec![None; rows.header.len().max(1)];
        if let Some(v) = a.val("columnFormats") {
            match v.kind() {
                ty::STRUCT => {
                    for key in v.keys()? {
                        let style = style_from_struct(v.key(key))?;
                        match rows.header.iter().position(|h| h.eq_ignore_ascii_case(key)) {
                            Some(i) => {
                                if i >= column_styles.len() {
                                    column_styles.resize(i + 1, None);
                                }
                                column_styles[i] = Some(style);
                            }
                            None => {
                                return Err(Error::expression(format!(
                                    "Document.{}(): columnFormats names [{}], which is not a \
                                     column here. Columns are: {}",
                                    method,
                                    key,
                                    if rows.header.is_empty() {
                                        "(positional — use an array of formats)".to_string()
                                    } else {
                                        rows.header.join(", ")
                                    }
                                )))
                            }
                        }
                    }
                }
                ty::ARRAY => {
                    let n = v.len()?;
                    if n > column_styles.len() {
                        column_styles.resize(n, None);
                    }
                    for (i, slot) in column_styles.iter_mut().enumerate().take(n) {
                        let item = v.get(i);
                        if !item.is_null() {
                            *slot = Some(style_from_struct(item)?);
                        }
                    }
                }
                _ => {
                    return Err(Error::expression(format!(
                        "Document.{}(): columnFormats must be a struct keyed by column name, or \
                         an array of formats",
                        method
                    )))
                }
            }
        }

        // ---- stringify, applying each column's mask ----------------------
        //
        // This is where a query stops being data and becomes cells. It happens
        // BEFORE the lock is taken, because a mask calls back into the engine.
        let mut body: Vec<Vec<Cell>> = Vec::with_capacity(rows.body.len());
        for row in &rows.body {
            let mut cells = Vec::with_capacity(row.len());
            for (col, v) in row.iter().enumerate() {
                let style = column_styles.get(col).and_then(|s| s.as_ref());
                let text = match style {
                    Some(s) if s.number_format.is_some() || s.date_format.is_some() => {
                        format_cell(ctx, *v, s)?
                    }
                    _ => {
                        if v.is_null() {
                            String::new()
                        } else {
                            v.to_string()
                        }
                    }
                };
                cells.push(Cell::plain(text));
            }
            body.push(cells);
        }

        // ---- a totals row ------------------------------------------------
        //
        // The document counterpart of putting `=SUM(C2:C40)` in a spreadsheet:
        // the sum is computed here rather than expressed as a formula, because
        // a PDF has no recalculation.
        let totals = string_items(a.val("totals"))?;
        let mut totals_row: Option<Vec<Cell>> = None;
        if !totals.is_empty() {
            let width = body.first().map(|r| r.len()).unwrap_or(rows.header.len());
            let mut row: Vec<Cell> = (0..width).map(|_| Cell::plain("")).collect();
            let mut labelled = false;
            for want in &totals {
                let Some(col) = rows
                    .header
                    .iter()
                    .position(|h| h.eq_ignore_ascii_case(want.trim()))
                else {
                    return Err(Error::expression(format!(
                        "Document.{}(): totals names [{}], which is not a column here. Columns \
                         are: {}",
                        method,
                        want,
                        rows.header.join(", ")
                    )));
                };
                let mut sum = 0.0_f64;
                for r in &rows.body {
                    if let Some(v) = r.get(col) {
                        if !v.is_null() {
                            // A non-numeric cell in a summed column is a caller
                            // mistake worth naming, not a silent zero.
                            sum += v.as_f64().map_err(|_| {
                                Error::expression(format!(
                                    "Document.{}(): cannot total column [{}] — it contains the \
                                     non-numeric value [{}]",
                                    method, rows.header[col], v
                                ))
                            })?;
                        }
                    }
                }
                let style = column_styles.get(col).and_then(|s| s.as_ref());
                let text = match style {
                    Some(s) if s.number_format.is_some() => {
                        let n = ctx.double(sum);
                        format_cell(ctx, n, s)?
                    }
                    _ => trim_float(sum),
                };
                row[col] = Cell {
                    text,
                    style: Some(TextStyle {
                        bold: true,
                        ..TextStyle::default()
                    }),
                };
                if !labelled {
                    // Put the label in the first cell that is not itself a
                    // total, so "Total" never overwrites a number.
                    if let Some(pos) = (0..width).find(|i| {
                        !totals.iter().any(|w| {
                            rows.header
                                .get(*i)
                                .map(|h| h.eq_ignore_ascii_case(w.trim()))
                                == Some(true)
                        })
                    }) {
                        row[pos] = Cell {
                            text: a.text("totalsLabel").unwrap_or_else(|| "Total".to_string()),
                            style: Some(TextStyle {
                                bold: true,
                                ..TextStyle::default()
                            }),
                        };
                        labelled = true;
                    }
                }
            }
            totals_row = Some(row);
        }

        // ---- assemble ----------------------------------------------------
        let mut all: Vec<Vec<Cell>> = Vec::with_capacity(body.len() + 2);
        // Whether a header row was actually PUSHED, which is not the same as
        // whether one was asked for: `header=true` over an array of arrays has
        // no names to make a header out of. The emitter needs the former, and
        // getting it wrong shifts every stripe by one row.
        let mut pushed_header = false;
        if header {
            let labels = string_items(a.val("headers"))?;
            let head: Vec<Cell> = if labels.is_empty() {
                rows.header
                    .iter()
                    .map(|s| Cell::plain(s.as_str()))
                    .collect()
            } else {
                labels.iter().map(|s| Cell::plain(s.as_str())).collect()
            };
            if !head.is_empty() {
                all.push(head);
                pushed_header = true;
            }
        }
        all.extend(body);
        if let Some(row) = totals_row {
            all.push(row);
        }

        // Ragged input would produce a Typst table with a short final row, which
        // fails to lay out with a message that says nothing useful. Pad here.
        let width = all.iter().map(|r| r.len()).max().unwrap_or(0);
        for row in all.iter_mut() {
            while row.len() < width {
                row.push(Cell::plain(""));
            }
        }

        let widths = match a.val("widths") {
            Some(v) => length_items(Some(v), unit, "column width")?,
            None => length_items(a.val("columns"), unit, "column width")?,
        };
        let aligns = match a.val("align") {
            None => None,
            Some(v) => {
                let mut out = Vec::new();
                for raw in string_items(Some(v))? {
                    out.push(parse_align(&raw)?);
                }
                Some(out)
            }
        };
        let stroke = match a.val("stroke") {
            // `stroke=false` / `stroke="none"` is how a caller turns lines off.
            Some(v) if !v.is_true() || v.to_string().eq_ignore_ascii_case("none") => Stroke::None,
            _ => {
                let color = a.color("strokeColor")?;
                let thickness = a.len("strokeWidth", unit)?;
                if color.is_some() || thickness.is_some() {
                    Stroke::Custom { color, thickness }
                } else if is_grid {
                    Stroke::None
                } else {
                    Stroke::Default
                }
            }
        };

        Ok(Table {
            rows: all,
            header: pushed_header,
            widths,
            aligns,
            style: TextStyle {
                // A table's own `align` is a per-column list, not a block
                // alignment, so it must not leak into the cell style.
                align: None,
                ..base
            },
            header_style,
            column_styles,
            stripe: a.color("stripe")?,
            fill: a.color("fill")?,
            stroke,
            gutter: a.len("gutter", unit)?,
            inset: a.len("inset", unit)?,
            caption: match a.val("caption") {
                Some(v) => Some(content_from(Some(v))?),
                None => None,
            },
            label: match a.text("label") {
                Some(l) => Some(parse_label(&l)?),
                None => None,
            },
        })
    }
}

/// `1234.5` → `"1234.5"`, `1234.0` → `"1234"`.
///
/// A total with no mask should look like the numbers above it, and Rust's
/// default float formatting would print `1234` as `1234` but `0.1+0.2` as
/// `0.30000000000000004`.
fn trim_float(n: f64) -> String {
    if (n - n.round()).abs() < 1e-9 {
        format!("{}", n.round() as i64)
    } else {
        let s = format!("{:.2}", n);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn params_for(method: &str) -> Option<&'static str> {
    Some(match method.to_ascii_lowercase().as_str() {
        "pagesize" | "paper" => P_PAGESIZE,
        "orientation" => P_ORIENTATION,
        "margin" | "margins" => P_MARGIN,
        "font" => P_FONT,
        "numbering" => P_NUMBERING,
        "headingnumbering" | "numberheadings" => P_HEADINGNUMBERING,
        "columns" | "pagecolumns" => P_PAGECOLUMNS,
        "pagefill" | "background" => P_PAGEFILL,
        "title" => P_TITLE,
        "author" => P_AUTHOR,
        "keywords" => P_KEYWORDS,
        "header" | "footer" => P_RUNNING,
        "heading" => P_HEADING,
        "paragraph" | "text" | "p" | "rich" | "richtext" => P_PARAGRAPH,
        "link" => P_LINK,
        "list" => P_LIST,
        "terms" | "definitions" => P_TERMS,
        "quote" | "blockquote" => P_QUOTE,
        "code" | "pre" => P_CODE,
        "table" | "grid" | "fromquery" => P_TABLE,
        "callout" | "box" | "panel" => P_CALLOUT,
        "image" => P_IMAGE,
        "outline" | "toc" | "tableofcontents" => P_OUTLINE,
        "spacer" | "space" => P_SPACER,
        "typst" => P_TYPST,
        "template" => P_TEMPLATE,
        "data" => P_DATA,
        "pdf" | "pdfoptions" => P_PDF,
        "write" | "save" => P_WRITE,
        "pagebreak" | "rule" | "hr" | "totypst" | "tobinary" | "topdf" | "pagecount" => P_NONE,
        _ => return None,
    })
}

fn unknown_method(method: &str) -> Error {
    Error::new(format!(
        "Document has no method [{}]. Available: pageSize, orientation, margin, font, numbering, \
         columns, pageFill, title, author, keywords, header, footer, heading, paragraph, link, \
         list, terms, quote, code, table, grid, callout, image, outline, headingNumbering, \
         pageBreak, rule, spacer, typst, template, data, pdf, toTypst, toBinary, pageCount, write.",
        method
    ))
}

/// Serialise a CFML value with the **engine's** `serializeJSON`.
///
/// Doing it here would mean a second JSON writer with its own opinions about
/// dates, numeric strings and struct key case — and a template author comparing
/// `data.json` with what the rest of the application produces would find them
/// different.
fn serialize_json<'a>(ctx: &'a Ctx, v: Value<'a>) -> Result<String> {
    ctx.call("serializeJSON", &[v])
        .map(|r| r.to_string())
        .map_err(|e| {
            Error::new(format!(
                "Document.data(): could not serialise the data ({}). This needs an engine that \
                 provides extension capability tier 3.",
                e
            ))
        })
}

/// A CFML date/number as seconds since the Unix epoch.
fn epoch_seconds<'a>(ctx: &'a Ctx, v: Value<'a>) -> Result<i64> {
    // A number is already an epoch; anything else goes through the engine's own
    // date handling rather than this module guessing at CFML date formats.
    if let Ok(n) = v.as_i64() {
        if n.abs() > 100_000 {
            return Ok(n);
        }
    }
    let epoch = ctx
        .call("dateDiff", &[ctx.string("s"), ctx.string("1970-01-01"), v])
        .map_err(|e| {
            Error::new(format!(
                "Document.pdf(): timestamp must be a date or an epoch second count ({})",
                e
            ))
        })?;
    epoch.as_i64().map_err(|_| {
        Error::expression("Document.pdf(): timestamp must be a date or an epoch second count")
    })
}

/// Resolve `.template( path, root )` into a canonical main file and root.
///
/// Both are canonicalised, and the main file must sit under the root: the root
/// is the sandbox the compiler's `World` enforces, and a main file outside it
/// would be a hole in the thing being enforced.
pub fn resolve_template_public(path: &str, root: Option<&str>) -> Result<Template> {
    resolve_template(path, root)
}

fn resolve_template(path: &str, root: Option<&str>) -> Result<Template> {
    let main = PathBuf::from(path);
    let main = main.canonicalize().map_err(|e| {
        Error::new(format!(
            "Document.template(): cannot resolve [{}]: {}",
            path, e
        ))
    })?;
    if !main.is_file() {
        return Err(Error::new(format!(
            "Document.template(): [{}] is not a file",
            main.display()
        )));
    }
    let root = match root {
        Some(r) => PathBuf::from(r).canonicalize().map_err(|e| {
            Error::new(format!(
                "Document.template(): cannot resolve root [{}]: {}",
                r, e
            ))
        })?,
        // The template's own directory. A template that imports a sibling
        // `common.typ` is the normal case and should need no configuration.
        None => main
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| Error::new("Document.template(): the path has no directory"))?,
    };
    if !main.starts_with(&root) {
        return Err(Error::new(format!(
            "Document.template(): [{}] is outside the root [{}]",
            main.display(),
            root.display()
        )));
    }
    Ok(Template { main, root })
}

/// `Document()` — a fresh, empty document.
pub fn fn_document<'a>(ctx: &'a Ctx, _args: &[Value<'a>]) -> Result<Value<'a>> {
    Ok(ctx.new_object(CfmlDocument::new()))
}

/// `IsDocumentObject( value )`
pub fn fn_is_document_object<'a>(ctx: &'a Ctx, args: &[Value<'a>]) -> Result<Value<'a>> {
    let is = args
        .first()
        .map(|v| {
            v.native_class_name()
                .map(|n| n == "Document")
                .unwrap_or(false)
        })
        .unwrap_or(false);
    Ok(ctx.bool(is))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_must_be_a_scheme_a_document_has_a_reason_to_link_to() {
        for ok in [
            "https://example.com/x?y=1",
            "http://example.com",
            "mailto:a@b.c",
            "tel:+441234",
            "#section",
            "/local/page",
        ] {
            assert!(parse_url(ok).is_ok(), "{ok} should be allowed");
        }
        // A PDF viewer will happily act on these.
        for bad in [
            "javascript:alert(1)",
            "data:text/html;base64,PHNjcmlwdD4=",
            "file:///etc/passwd",
            "",
        ] {
            assert!(parse_url(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn a_label_is_checked_not_escaped() {
        assert_eq!(parse_label(" fees ").unwrap(), "fees");
        assert!(parse_label("a-b_c.d:e").is_ok());
        // `<`/`>` would close the label; a space would end it early.
        for bad in ["a b", "a>b", "<x", "", "a#b"] {
            assert!(parse_label(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn colours_are_a_whitelist() {
        assert_eq!(parse_color("#ff0000").unwrap(), r##"rgb("#ff0000")"##);
        assert_eq!(parse_color("f00").unwrap(), r##"rgb("#f00")"##);
        assert_eq!(parse_color("Grey").unwrap(), "gray");
        // This is the injection route a bare colour expression would open.
        assert!(parse_color("red); #pagebreak(").is_err());
        assert!(parse_color("luma(50%)").is_err());
    }

    #[test]
    fn totals_with_no_mask_look_like_plain_numbers() {
        assert_eq!(trim_float(1234.0), "1234");
        assert_eq!(trim_float(1234.5), "1234.5");
        assert_eq!(trim_float(0.1 + 0.2), "0.3");
    }
}
