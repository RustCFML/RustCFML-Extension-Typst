//! `Document()` — the fluent builder, as a `.rcx` native class.
//!
//! Shaped after the engine's `Spreadsheet()`: mutators return the document so
//! calls chain, terminals (`write`, `toBinary`, `toTypst`, `pageCount`) end the
//! chain and return a value.
//!
//! Note `&self` on every method — the extension ABI's contract. The document
//! model therefore lives behind a `Mutex`, which is also what makes this class
//! safe to hand to concurrent requests: a `Document` is a shared, reference-
//! typed CFML value exactly like a `Spreadsheet`.

use std::sync::Mutex;

use rustcfml_module::{Ctx, Error, NativeClass, Result, Value};

use crate::model::{Align, Block, Document, Length, Margin, Running, TableData, TextStyle, Unit};
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
            inner: Mutex::new(Inner { doc: Document::default(), next_file: 0 }),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        // A poisoned lock means a previous call panicked mid-mutation. The
        // document may be half-built, but refusing every later call would turn
        // one bad `.paragraph()` into a dead object; the model is plain data,
        // so recovering is honest here.
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }

    pub fn to_typst(&self) -> String {
        self.with(|i| emit::emit(&i.doc))
    }

    fn to_pdf(&self) -> Result<Vec<u8>> {
        let (markup, files) = self.with(|i| (emit::emit(&i.doc), i.doc.files.clone()));
        world::compile_pdf_with_files(&markup, &files).map_err(Error::new)
    }
}

// ---------------------------------------------------------------------------
// Argument helpers
//
// Every one of these treats "absent" and "null" identically, so a caller can
// name only the arguments they care about and leave the rest to the defaults.
// ---------------------------------------------------------------------------

fn present<'a>(args: &[Value<'a>], i: usize) -> Option<Value<'a>> {
    args.get(i).copied().filter(|v| !v.is_null())
}

fn str_arg(args: &[Value], i: usize) -> Option<String> {
    present(args, i).map(|v| v.to_string())
}

fn text_arg(args: &[Value], i: usize) -> Option<String> {
    str_arg(args, i).filter(|s| !s.trim().is_empty())
}

fn bool_arg(args: &[Value], i: usize) -> Option<bool> {
    present(args, i).map(|v| v.is_true())
}

fn num_arg(args: &[Value], i: usize, what: &str) -> Result<Option<f64>> {
    match present(args, i) {
        None => Ok(None),
        Some(v) => match v.as_f64() {
            Ok(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(Error::expression(format!(
                "Document: {} must be a number, got [{}]",
                what, v
            ))),
        },
    }
}

fn unit_arg(args: &[Value], i: usize) -> Result<Unit> {
    match text_arg(args, i) {
        None => Ok(Unit::Pt),
        Some(s) => Unit::parse(&s).ok_or_else(|| {
            Error::expression(format!(
                "Document: unknown unit [{}] — use pt, mm, cm, in, em, % or fr",
                s
            ))
        }),
    }
}

fn len_arg(args: &[Value], i: usize, unit: Unit, what: &str) -> Result<Option<Length>> {
    Ok(num_arg(args, i, what)?.map(|value| Length { value, unit }))
}

fn align_arg(args: &[Value], i: usize) -> Result<Option<Align>> {
    match text_arg(args, i) {
        None => Ok(None),
        Some(s) => Align::parse(&s).map(Some).ok_or_else(|| {
            Error::expression(format!(
                "Document: unknown alignment [{}] — use left, center, right or justify",
                s
            ))
        }),
    }
}

/// A CFML colour as a Typst colour expression.
///
/// Rejecting anything else is what keeps caller text out of the generated code:
/// a colour is the one place the builder emits a bare expression rather than a
/// string literal, so it is the one place that has to be checked.
fn color_arg(args: &[Value], i: usize) -> Result<Option<String>> {
    let Some(raw) = text_arg(args, i) else { return Ok(None) };
    let s = raw.trim();
    let hex = s.trim_start_matches('#');
    if !hex.is_empty()
        && (hex.len() == 3 || hex.len() == 6 || hex.len() == 8)
        && hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Ok(Some(format!("rgb(\"#{}\")", hex)));
    }
    const NAMED: &[&str] = &[
        "black", "gray", "grey", "silver", "white", "navy", "blue", "aqua", "teal", "eastern",
        "purple", "fuchsia", "maroon", "red", "orange", "yellow", "olive", "green", "lime",
    ];
    let lower = s.to_ascii_lowercase();
    if NAMED.contains(&lower.as_str()) {
        let name = if lower == "grey" { "gray".to_string() } else { lower };
        return Ok(Some(name));
    }
    Err(Error::expression(format!(
        "Document: unknown colour [{}] — use a hex value (##RGB/##RRGGBB) or one of: {}",
        raw,
        NAMED.join(", ")
    )))
}

/// The shared inline-style arguments, wherever they sit in a signature.
fn style_from(
    args: &[Value],
    align_i: usize,
    bold_i: usize,
    italic_i: usize,
    size_i: usize,
    color_i: usize,
    font_i: usize,
) -> Result<TextStyle> {
    Ok(TextStyle {
        bold: bool_arg(args, bold_i).unwrap_or(false),
        italic: bool_arg(args, italic_i).unwrap_or(false),
        size: len_arg(args, size_i, Unit::Pt, "size")?,
        font: text_arg(args, font_i),
        color: color_arg(args, color_i)?,
        align: align_arg(args, align_i)?,
    })
}

/// A list argument: a CFML array, or a comma-delimited string.
fn string_items(v: Option<Value>) -> Result<Vec<String>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.kind() == rustcfml_module::abi::ty::ARRAY {
        let n = v.len()?;
        return Ok((0..n).map(|i| v.get(i).to_string()).collect());
    }
    Ok(v.to_string()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Flatten a CFML value into table rows.
///
/// Accepts a query, an array of arrays, an array of structs (the first struct's
/// keys set the column order) or an array of scalars.
fn table_rows(v: Option<Value>, header: bool) -> Result<TableData> {
    use rustcfml_module::abi::ty;

    let mut rows: Vec<Vec<String>> = Vec::new();
    match v {
        None => {}
        Some(v) if v.kind() == ty::QUERY => {
            let columns = v.query_columns()?;
            if header {
                rows.push(columns.iter().map(|c| c.to_string()).collect());
            }
            // One crossing per COLUMN, not per cell: `query_column` materialises
            // the whole column in a single call, which is the difference
            // between 3 crossings and 30,000 on a 10k-row report.
            let mut cols: Vec<Vec<String>> = Vec::with_capacity(columns.len());
            for i in 0..columns.len() {
                let col = v.query_column(i);
                let n = col.len()?;
                cols.push((0..n).map(|r| col.get(r).to_string()).collect());
            }
            let row_count = v.query_row_count()?;
            for r in 0..row_count {
                rows.push(
                    cols.iter().map(|c| c.get(r).cloned().unwrap_or_default()).collect(),
                );
            }
        }
        Some(v) if v.kind() == ty::ARRAY => {
            let n = v.len()?;
            let first = if n > 0 { Some(v.get(0)) } else { None };
            let struct_keys: Option<Vec<String>> = match first {
                Some(f) if f.kind() == ty::STRUCT => {
                    Some(f.keys()?.iter().map(|k| k.to_string()).collect())
                }
                _ => None,
            };
            if let Some(keys) = &struct_keys {
                if header {
                    rows.push(keys.clone());
                }
                for i in 0..n {
                    let item = v.get(i);
                    if item.kind() != ty::STRUCT {
                        continue;
                    }
                    rows.push(keys.iter().map(|k| item.key(k).to_string()).collect());
                }
            } else {
                for i in 0..n {
                    let item = v.get(i);
                    if item.kind() == ty::ARRAY {
                        let inner = item.len()?;
                        rows.push((0..inner).map(|j| item.get(j).to_string()).collect());
                    } else {
                        rows.push(vec![item.to_string()]);
                    }
                }
            }
        }
        Some(_) => {
            return Err(Error::expression(
                "Document.table() takes a query, an array of arrays or an array of structs"
                    .to_string(),
            ))
        }
    }

    // Ragged input would produce a Typst table with a short final row, which
    // fails to lay out with a message that says nothing useful. Pad here.
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    for row in rows.iter_mut() {
        while row.len() < width {
            row.push(String::new());
        }
    }
    Ok(TableData { rows })
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
        Some(match method.to_ascii_lowercase().as_str() {
            "pagesize" | "paper" => "paper,width,height,unit",
            "orientation" => "orientation",
            "margin" | "margins" => "all,top,bottom,left,right,unit",
            "font" => "family,size,color",
            "numbering" => "pattern",
            "title" => "title",
            "author" => "author",
            "header" | "footer" => "content,align",
            "heading" => "text,level,align,bold,italic,size,color,font",
            "paragraph" | "text" | "p" => "text,align,bold,italic,size,color,font",
            "list" => "items,ordered,align,size,color,font",
            "table" => "data,header,columns,align,unit,size,font",
            "image" => "source,width,height,align,unit",
            "spacer" | "space" => "amount,unit",
            "typst" => "markup",
            "write" | "save" => "path,overwrite",
            "pagebreak" | "rule" | "hr" | "totypst" | "tobinary" | "topdf" | "pagecount" => "",
            _ => return None,
        })
    }

    fn call<'a>(&self, ctx: &'a Ctx, method: &str, args: &[Value<'a>]) -> Result<Value<'a>> {
        match method.to_ascii_lowercase().as_str() {
            // ---- page setup (mutators, return this) ------------------------
            "pagesize" | "paper" => {
                let unit = unit_arg(args, 3)?;
                let paper = text_arg(args, 0);
                let width = len_arg(args, 1, unit, "width")?;
                let height = len_arg(args, 2, unit, "height")?;
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
                let o = str_arg(args, 0).unwrap_or_default();
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
                let unit = unit_arg(args, 5)?;
                let all = len_arg(args, 0, unit, "margin")?;
                let mut m = Margin { top: all, bottom: all, left: all, right: all };
                if let Some(v) = len_arg(args, 1, unit, "top")? {
                    m.top = Some(v);
                }
                if let Some(v) = len_arg(args, 2, unit, "bottom")? {
                    m.bottom = Some(v);
                }
                if let Some(v) = len_arg(args, 3, unit, "left")? {
                    m.left = Some(v);
                }
                if let Some(v) = len_arg(args, 4, unit, "right")? {
                    m.right = Some(v);
                }
                self.with(|i| i.doc.page.margin = m);
                Ok(ctx.this())
            }
            "font" => {
                let family = text_arg(args, 0);
                let size = len_arg(args, 1, Unit::Pt, "size")?;
                let color = color_arg(args, 2)?;
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
                let p = text_arg(args, 0);
                self.with(|i| i.doc.page.numbering = p);
                Ok(ctx.this())
            }
            "title" => {
                let t = str_arg(args, 0);
                self.with(|i| i.doc.title = t);
                Ok(ctx.this())
            }
            "author" => {
                let a = str_arg(args, 0);
                self.with(|i| i.doc.author = a);
                Ok(ctx.this())
            }
            "header" | "footer" => {
                let running =
                    Running { text: str_arg(args, 0).unwrap_or_default(), align: align_arg(args, 1)? };
                let is_header = method.eq_ignore_ascii_case("header");
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
                let text = str_arg(args, 0).unwrap_or_default();
                let level = num_arg(args, 1, "level")?.unwrap_or(1.0);
                if !(1.0..=6.0).contains(&level) {
                    return Err(Error::expression(format!(
                        "Document.heading(): level must be 1-6, got {}",
                        level
                    )));
                }
                let style = style_from(args, 2, 3, 4, 5, 6, 7)?;
                self.with(|i| {
                    i.doc.blocks.push(Block::Heading { level: level as u8, text, style })
                });
                Ok(ctx.this())
            }
            "paragraph" | "text" | "p" => {
                let text = str_arg(args, 0).unwrap_or_default();
                let style = style_from(args, 1, 2, 3, 4, 5, 6)?;
                self.with(|i| i.doc.blocks.push(Block::Paragraph { text, style }));
                Ok(ctx.this())
            }
            "list" => {
                let items = string_items(present(args, 0))?;
                let ordered = bool_arg(args, 1).unwrap_or(false);
                let style = TextStyle {
                    align: align_arg(args, 2)?,
                    size: len_arg(args, 3, Unit::Pt, "size")?,
                    color: color_arg(args, 4)?,
                    font: text_arg(args, 5),
                    ..TextStyle::default()
                };
                self.with(|i| i.doc.blocks.push(Block::List { items, ordered, style }));
                Ok(ctx.this())
            }
            "table" => {
                let header = bool_arg(args, 1).unwrap_or(true);
                let data = table_rows(present(args, 0), header)?;
                let unit = unit_arg(args, 4)?;
                let columns = match present(args, 2) {
                    None => None,
                    Some(v) => {
                        let mut widths = Vec::new();
                        for raw in string_items(Some(v))? {
                            match raw.trim().parse::<f64>() {
                                Ok(n) if n.is_finite() => widths.push(Length { value: n, unit }),
                                _ => {
                                    return Err(Error::expression(format!(
                                        "Document.table(): column width [{}] is not a number",
                                        raw
                                    )))
                                }
                            }
                        }
                        Some(widths)
                    }
                };
                let aligns = match present(args, 3) {
                    None => None,
                    Some(v) => {
                        let mut out = Vec::new();
                        for raw in string_items(Some(v))? {
                            out.push(Align::parse(&raw).ok_or_else(|| {
                                Error::expression(format!(
                                    "Document.table(): unknown alignment [{}]",
                                    raw
                                ))
                            })?);
                        }
                        Some(out)
                    }
                };
                let style = TextStyle {
                    size: len_arg(args, 5, Unit::Pt, "size")?,
                    font: text_arg(args, 6),
                    ..TextStyle::default()
                };
                self.with(|i| {
                    i.doc.blocks.push(Block::Table { data, header, columns, aligns, style })
                });
                Ok(ctx.this())
            }
            "image" => {
                let unit = unit_arg(args, 4)?;
                let source = present(args, 0).ok_or_else(|| {
                    Error::expression("Document.image(): a file path or binary is required")
                })?;
                let bytes = if source.kind() == rustcfml_module::abi::ty::BINARY {
                    source.as_bytes()?.to_vec()
                } else {
                    let path = source.to_string();
                    std::fs::read(&path).map_err(|e| {
                        Error::new(format!("Document.image(): unable to read [{}]: {}", path, e))
                    })?
                };
                let ext = image_extension(&bytes).ok_or_else(|| {
                    Error::expression(
                        "Document.image(): unrecognised image data — PNG, JPEG, GIF and SVG are \
                         supported",
                    )
                })?;
                let width = len_arg(args, 1, unit, "width")?;
                let height = len_arg(args, 2, unit, "height")?;
                let align = align_arg(args, 3)?;
                self.with(|i| {
                    i.next_file += 1;
                    let file = format!("img{}.{}", i.next_file, ext);
                    i.doc.files.push((file.clone(), bytes));
                    i.doc.blocks.push(Block::Image { file, width, height, align });
                });
                Ok(ctx.this())
            }
            "pagebreak" => {
                self.with(|i| i.doc.blocks.push(Block::PageBreak));
                Ok(ctx.this())
            }
            "rule" | "hr" => {
                self.with(|i| i.doc.blocks.push(Block::Rule));
                Ok(ctx.this())
            }
            "spacer" | "space" => {
                let unit = unit_arg(args, 1)?;
                let amount = len_arg(args, 0, unit, "amount")?
                    .unwrap_or(Length { value: 1.0, unit: Unit::Em });
                self.with(|i| i.doc.blocks.push(Block::Spacer(amount)));
                Ok(ctx.this())
            }
            "typst" => {
                // The escape hatch, and the ONLY place caller text becomes Typst
                // code rather than a Typst string literal.
                let markup = str_arg(args, 0).unwrap_or_default();
                self.with(|i| i.doc.blocks.push(Block::Raw(markup)));
                Ok(ctx.this())
            }

            // ---- terminals --------------------------------------------------
            "totypst" => Ok(ctx.string(self.to_typst())),
            "tobinary" | "topdf" => Ok(ctx.binary(&self.to_pdf()?)),
            "pagecount" => {
                let (markup, files) = self.with(|i| (emit::emit(&i.doc), i.doc.files.clone()));
                let n = world::page_count_with_files(&markup, &files).map_err(Error::new)?;
                Ok(ctx.int(n as i64))
            }
            "write" | "save" => {
                let path = str_arg(args, 0).unwrap_or_default();
                if path.trim().is_empty() {
                    return Err(Error::expression("Document.write(): a file path is required"));
                }
                let overwrite = bool_arg(args, 1).unwrap_or(true);
                if !overwrite && std::path::Path::new(&path).exists() {
                    return Err(Error::new(format!(
                        "Document.write(): [{}] already exists and overwrite is false",
                        path
                    )));
                }
                let pdf = self.to_pdf()?;
                std::fs::write(&path, pdf).map_err(|e| {
                    Error::new(format!("Document.write(): unable to write [{}]: {}", path, e))
                })?;
                Ok(ctx.this())
            }
            other => Err(Error::new(format!(
                "Document has no method [{}]. Available: pageSize, orientation, margin, font, \
                 numbering, title, author, header, footer, heading, paragraph, list, table, \
                 image, pageBreak, rule, spacer, typst, toTypst, toBinary, pageCount, write.",
                other
            ))),
        }
    }
}

/// `Document()` — a fresh, empty document.
pub fn fn_document<'a>(ctx: &'a Ctx, _args: &[Value<'a>]) -> Result<Value<'a>> {
    Ok(ctx.new_object(CfmlDocument::new()))
}

/// `IsDocumentObject( value )`
pub fn fn_is_document_object<'a>(ctx: &'a Ctx, args: &[Value<'a>]) -> Result<Value<'a>> {
    let is = args
        .first()
        .map(|v| v.native_class_name().map(|n| n == "Document").unwrap_or(false))
        .unwrap_or(false);
    Ok(ctx.bool(is))
}
