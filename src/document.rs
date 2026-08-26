//! `Document()` — the fluent builder, and the CFML-facing method table.
//!
//! Shaped after the engine's `Spreadsheet()` (see `docs/spreadsheets.md`):
//! a shared, reference-typed native object whose mutators return the object
//! itself so calls chain, and whose terminals (`write`, `toBinary`, `toTypst`,
//! `pageCount`) end the chain and return a value.

use std::sync::{Arc, RwLock, Weak};

use rustcfml_cli::{CfmlError, CfmlNative, CfmlResult, Value};

use crate::model::{Align, Block, Document, Length, Margin, Running, TableData, TextStyle, Unit};
use crate::{emit, world};

pub struct CfmlDocument {
    doc: Document,
    /// Set at construction via `Arc::new_cyclic` so a mutator can hand back the
    /// *same* handle rather than a copy of a growing document.
    self_ref: Option<Weak<RwLock<CfmlDocument>>>,
    /// Counter behind the virtual filenames given to embedded images.
    next_file: usize,
}

impl std::fmt::Debug for CfmlDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("blocks", &self.doc.blocks.len())
            .field("paper", &self.doc.page.paper)
            .finish()
    }
}

impl CfmlDocument {
    pub fn new() -> CfmlDocument {
        CfmlDocument { doc: Document::default(), self_ref: None, next_file: 0 }
    }

    pub fn into_value(mut self) -> Value {
        let arc: Arc<RwLock<CfmlDocument>> = Arc::new_cyclic(|weak| {
            self.self_ref = Some(weak.clone());
            RwLock::new(self)
        });
        Value::NativeObject(arc)
    }

    fn this(&self) -> Value {
        match self.self_ref.as_ref().and_then(|w| w.upgrade()) {
            Some(arc) => Value::NativeObject(arc),
            None => Value::Null,
        }
    }

    /// The generated Typst markup. The debugging seam: when a document comes
    /// out wrong you need to see what was actually compiled.
    pub fn to_typst(&self) -> String {
        emit::emit(&self.doc)
    }

    fn to_pdf(&self) -> Result<Vec<u8>, CfmlError> {
        world::compile_pdf_with_files(&self.to_typst(), &self.doc.files).map_err(CfmlError::runtime)
    }
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

fn present(v: Option<&Value>) -> Option<&Value> {
    match v {
        Some(Value::Null) | None => None,
        Some(v) => Some(v),
    }
}

fn str_arg(a: &[Value], i: usize) -> Option<String> {
    present(a.get(i)).map(|v| v.as_string())
}

fn bool_arg(a: &[Value], i: usize) -> Option<bool> {
    present(a.get(i)).map(|v| v.is_true())
}

fn num_arg(a: &[Value], i: usize, what: &str) -> Result<Option<f64>, CfmlError> {
    match present(a.get(i)) {
        None => Ok(None),
        Some(v) => {
            let s = v.as_string();
            match s.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => Ok(Some(n)),
                _ => Err(CfmlError::runtime(format!(
                    "Document: {} must be a number, got [{}]",
                    what, s
                ))),
            }
        }
    }
}

fn unit_arg(a: &[Value], i: usize) -> Result<Unit, CfmlError> {
    match str_arg(a, i) {
        None => Ok(Unit::Pt),
        Some(s) => Unit::parse(&s).ok_or_else(|| {
            CfmlError::runtime(format!(
                "Document: unknown unit [{}] — use pt, mm, cm, in, em, % or fr",
                s
            ))
        }),
    }
}

fn len_arg(a: &[Value], i: usize, unit: Unit, what: &str) -> Result<Option<Length>, CfmlError> {
    Ok(num_arg(a, i, what)?.map(|value| Length { value, unit }))
}

fn align_arg(a: &[Value], i: usize) -> Result<Option<Align>, CfmlError> {
    match str_arg(a, i).filter(|s| !s.trim().is_empty()) {
        None => Ok(None),
        Some(s) => Align::parse(&s).map(Some).ok_or_else(|| {
            CfmlError::runtime(format!(
                "Document: unknown alignment [{}] — use left, center, right or justify",
                s
            ))
        }),
    }
}

/// A CFML colour (`"red"`, `"#c00"`, `"##ff9900"`) as a Typst colour
/// expression. Rejecting anything else is what keeps caller text out of the
/// generated code — a colour is the one place a raw expression is emitted.
fn color_arg(a: &[Value], i: usize) -> Result<Option<String>, CfmlError> {
    let Some(raw) = str_arg(a, i).filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let s = raw.trim();
    let hex = s.trim_start_matches('#');
    if !hex.is_empty()
        && (hex.len() == 3 || hex.len() == 6 || hex.len() == 8)
        && hex.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Ok(Some(format!("rgb(\"#{}\")", hex)));
    }
    // Typst's own named colours, plus the CSS names that mean the same thing.
    const NAMED: &[&str] = &[
        "black", "gray", "grey", "silver", "white", "navy", "blue", "aqua", "teal", "eastern",
        "purple", "fuchsia", "maroon", "red", "orange", "yellow", "olive", "green", "lime",
    ];
    let lower = s.to_ascii_lowercase();
    if NAMED.contains(&lower.as_str()) {
        let name = if lower == "grey" { "gray".to_string() } else { lower };
        return Ok(Some(name));
    }
    Err(CfmlError::runtime(format!(
        "Document: unknown colour [{}] — use a hex value (##RGB/##RRGGBB) or one of: {}",
        raw,
        NAMED.join(", ")
    )))
}

/// Pull the shared inline-style arguments off a call.
fn style_from(
    a: &[Value],
    align_i: usize,
    bold_i: usize,
    italic_i: usize,
    size_i: usize,
    color_i: usize,
    font_i: usize,
) -> Result<TextStyle, CfmlError> {
    Ok(TextStyle {
        bold: bool_arg(a, bold_i).unwrap_or(false),
        italic: bool_arg(a, italic_i).unwrap_or(false),
        size: len_arg(a, size_i, Unit::Pt, "size")?,
        font: str_arg(a, font_i).filter(|s| !s.trim().is_empty()),
        color: color_arg(a, color_i)?,
        align: align_arg(a, align_i)?,
    })
}

/// Flatten a CFML value into the rows of a table.
///
/// Accepts a query, an array of arrays, an array of structs (keys of the first
/// struct set the column order) or a single array of scalars.
fn table_rows(v: Option<&Value>, header: bool) -> Result<TableData, CfmlError> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    match present(v) {
        None => {}
        Some(Value::Query(q)) => {
            let columns = q.columns();
            if header {
                rows.push(columns.clone());
            }
            for r in 0..q.row_count() {
                let row = q.get_row(r);
                let cells = columns
                    .iter()
                    .map(|c| {
                        row.as_ref()
                            .and_then(|m| {
                                m.iter()
                                    .find(|(k, _)| k.eq_ignore_ascii_case(c))
                                    .map(|(_, val)| val.as_string())
                            })
                            .unwrap_or_default()
                    })
                    .collect();
                rows.push(cells);
            }
        }
        Some(Value::Array(arr)) => {
            let items = arr.snapshot();
            // An array of structs takes its column order from the first row.
            let struct_keys: Option<Vec<String>> = match items.first() {
                Some(Value::Struct(s)) => {
                    Some(s.snapshot().into_iter().map(|(k, _)| k.to_string()).collect())
                }
                _ => None,
            };
            if let Some(keys) = &struct_keys {
                if header {
                    rows.push(keys.clone());
                }
                for item in &items {
                    if let Value::Struct(s) = item {
                        rows.push(
                            keys.iter()
                                .map(|k| s.get_ci(k).map(|v| v.as_string()).unwrap_or_default())
                                .collect(),
                        );
                    }
                }
            } else {
                for item in &items {
                    match item {
                        Value::Array(inner) => {
                            rows.push(inner.snapshot().iter().map(|c| c.as_string()).collect())
                        }
                        other => rows.push(vec![other.as_string()]),
                    }
                }
            }
        }
        Some(other) => {
            return Err(CfmlError::runtime(format!(
                "Document.table() takes a query, an array of arrays or an array of structs, got {}",
                other.type_name()
            )))
        }
    }
    // Ragged input would produce a Typst table with a short final row, which
    // fails to lay out with a confusing message. Pad here instead.
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    for row in rows.iter_mut() {
        while row.len() < width {
            row.push(String::new());
        }
    }
    Ok(TableData { rows })
}

fn string_items(v: Option<&Value>) -> Vec<String> {
    match present(v) {
        Some(Value::Array(arr)) => arr.snapshot().iter().map(|x| x.as_string()).collect(),
        Some(other) => other
            .as_string()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// The CFML method table
// ---------------------------------------------------------------------------

impl CfmlNative for CfmlDocument {
    fn class_name(&self) -> &str {
        "Document"
    }

    /// Declared so a named call binds by name. Without this the engine refuses
    /// named arguments outright — see `CfmlNative::method_params`.
    fn method_params(&self, method: &str) -> Option<&'static [&'static str]> {
        Some(match method.to_ascii_lowercase().as_str() {
            "pagesize" | "paper" => &["paper", "width", "height", "unit"][..],
            "orientation" => &["orientation"][..],
            "margin" | "margins" => &["all", "top", "bottom", "left", "right", "unit"][..],
            "font" => &["family", "size", "color"][..],
            "numbering" => &["pattern"][..],
            "title" => &["title"][..],
            "author" => &["author"][..],
            "header" | "footer" => &["content", "align"][..],
            "heading" => {
                &["text", "level", "align", "bold", "italic", "size", "color", "font"][..]
            }
            "paragraph" | "text" | "p" => {
                &["text", "align", "bold", "italic", "size", "color", "font"][..]
            }
            "list" => &["items", "ordered", "align", "size", "color", "font"][..],
            "table" => &["data", "header", "columns", "align", "unit", "size", "font"][..],
            "image" => &["source", "width", "height", "align", "unit"][..],
            "spacer" | "space" => &["amount", "unit"][..],
            "typst" => &["markup"][..],
            "save" | "write" => &["path", "overwrite"][..],
            "pagebreak" | "rule" | "hr" | "totypst" | "tobinary" | "topdf" | "pagecount" => {
                &[][..]
            }
            _ => return None,
        })
    }

    fn call_method(&mut self, name: &str, args: Vec<Value>) -> CfmlResult {
        let a = args.as_slice();
        match name.to_ascii_lowercase().as_str() {
            // ---- page setup (mutators, return this) ------------------------
            "pagesize" | "paper" => {
                if let Some(p) = str_arg(a, 0).filter(|s| !s.trim().is_empty()) {
                    self.doc.page.paper = p.trim().to_ascii_lowercase();
                    self.doc.page.width = None;
                    self.doc.page.height = None;
                }
                let unit = unit_arg(a, 3)?;
                if let Some(w) = len_arg(a, 1, unit, "width")? {
                    self.doc.page.width = Some(w);
                }
                if let Some(h) = len_arg(a, 2, unit, "height")? {
                    self.doc.page.height = Some(h);
                }
                Ok(self.this())
            }
            "orientation" => {
                let o = str_arg(a, 0).unwrap_or_default();
                self.doc.page.landscape = match o.trim().to_ascii_lowercase().as_str() {
                    "landscape" => true,
                    "portrait" => false,
                    other => {
                        return Err(CfmlError::runtime(format!(
                            "Document.orientation() takes \"portrait\" or \"landscape\", got [{}]",
                            other
                        )))
                    }
                };
                Ok(self.this())
            }
            "margin" | "margins" => {
                let unit = unit_arg(a, 5)?;
                let all = len_arg(a, 0, unit, "margin")?;
                let mut m = Margin {
                    top: all,
                    bottom: all,
                    left: all,
                    right: all,
                };
                if let Some(v) = len_arg(a, 1, unit, "top")? {
                    m.top = Some(v);
                }
                if let Some(v) = len_arg(a, 2, unit, "bottom")? {
                    m.bottom = Some(v);
                }
                if let Some(v) = len_arg(a, 3, unit, "left")? {
                    m.left = Some(v);
                }
                if let Some(v) = len_arg(a, 4, unit, "right")? {
                    m.right = Some(v);
                }
                self.doc.page.margin = m;
                Ok(self.this())
            }
            "font" => {
                if let Some(f) = str_arg(a, 0).filter(|s| !s.trim().is_empty()) {
                    self.doc.font_family = Some(f);
                }
                if let Some(s) = len_arg(a, 1, Unit::Pt, "size")? {
                    self.doc.font_size = Some(s);
                }
                if let Some(c) = color_arg(a, 2)? {
                    self.doc.text_color = Some(c);
                }
                Ok(self.this())
            }
            "numbering" => {
                self.doc.page.numbering = str_arg(a, 0).filter(|s| !s.is_empty());
                Ok(self.this())
            }
            "title" => {
                self.doc.title = str_arg(a, 0);
                Ok(self.this())
            }
            "author" => {
                self.doc.author = str_arg(a, 0);
                Ok(self.this())
            }
            "header" | "footer" => {
                let running = Running {
                    text: str_arg(a, 0).unwrap_or_default(),
                    align: align_arg(a, 1)?,
                };
                if name.eq_ignore_ascii_case("header") {
                    self.doc.header = Some(running);
                } else {
                    self.doc.footer = Some(running);
                }
                Ok(self.this())
            }

            // ---- content ---------------------------------------------------
            "heading" => {
                let text = str_arg(a, 0).unwrap_or_default();
                let level = num_arg(a, 1, "level")?.unwrap_or(1.0);
                if !(1.0..=6.0).contains(&level) {
                    return Err(CfmlError::runtime(format!(
                        "Document.heading(): level must be 1-6, got {}",
                        level
                    )));
                }
                let style = style_from(a, 2, 3, 4, 5, 6, 7)?;
                self.doc.blocks.push(Block::Heading { level: level as u8, text, style });
                Ok(self.this())
            }
            "paragraph" | "text" | "p" => {
                let text = str_arg(a, 0).unwrap_or_default();
                let style = style_from(a, 1, 2, 3, 4, 5, 6)?;
                self.doc.blocks.push(Block::Paragraph { text, style });
                Ok(self.this())
            }
            "list" => {
                let items = string_items(a.first());
                let ordered = bool_arg(a, 1).unwrap_or(false);
                let style = TextStyle {
                    align: align_arg(a, 2)?,
                    size: len_arg(a, 3, Unit::Pt, "size")?,
                    color: color_arg(a, 4)?,
                    font: str_arg(a, 5).filter(|s| !s.trim().is_empty()),
                    ..TextStyle::default()
                };
                self.doc.blocks.push(Block::List { items, ordered, style });
                Ok(self.this())
            }
            "table" => {
                let header = bool_arg(a, 1).unwrap_or(true);
                let data = table_rows(a.first(), header)?;
                let unit = unit_arg(a, 4)?;
                let columns: Option<Vec<Length>> = match present(a.get(2)) {
                    None => None,
                    Some(v) => {
                        let mut widths = Vec::new();
                        for raw in string_items(Some(v)) {
                            match raw.trim().parse::<f64>() {
                                Ok(n) if n.is_finite() => {
                                    widths.push(Length { value: n, unit })
                                }
                                _ => {
                                    return Err(CfmlError::runtime(format!(
                                        "Document.table(): column width [{}] is not a number",
                                        raw
                                    )))
                                }
                            }
                        }
                        Some(widths)
                    }
                };
                let aligns: Option<Vec<Align>> = match present(a.get(3)) {
                    None => None,
                    Some(v) => {
                        let mut out = Vec::new();
                        for raw in string_items(Some(v)) {
                            out.push(Align::parse(&raw).ok_or_else(|| {
                                CfmlError::runtime(format!(
                                    "Document.table(): unknown alignment [{}]",
                                    raw
                                ))
                            })?);
                        }
                        Some(out)
                    }
                };
                let style = TextStyle {
                    size: len_arg(a, 5, Unit::Pt, "size")?,
                    font: str_arg(a, 6).filter(|s| !s.trim().is_empty()),
                    ..TextStyle::default()
                };
                self.doc.blocks.push(Block::Table { data, header, columns, aligns, style });
                Ok(self.this())
            }
            "image" => {
                let unit = unit_arg(a, 4)?;
                let bytes = match present(a.first()) {
                    Some(Value::Binary(b)) => b.clone(),
                    Some(v) => {
                        let path = v.as_string();
                        std::fs::read(&path).map_err(|e| {
                            CfmlError::runtime(format!(
                                "Document.image(): unable to read [{}]: {}",
                                path, e
                            ))
                        })?
                    }
                    None => {
                        return Err(CfmlError::runtime(
                            "Document.image(): a file path or binary is required".to_string(),
                        ))
                    }
                };
                let ext = image_extension(&bytes).ok_or_else(|| {
                    CfmlError::runtime(
                        "Document.image(): unrecognised image data — PNG, JPEG, GIF and SVG are \
                         supported"
                            .to_string(),
                    )
                })?;
                self.next_file += 1;
                let file = format!("img{}.{}", self.next_file, ext);
                self.doc.files.push((file.clone(), bytes));
                self.doc.blocks.push(Block::Image {
                    file,
                    width: len_arg(a, 1, unit, "width")?,
                    height: len_arg(a, 2, unit, "height")?,
                    align: align_arg(a, 3)?,
                });
                Ok(self.this())
            }
            "pagebreak" => {
                self.doc.blocks.push(Block::PageBreak);
                Ok(self.this())
            }
            "rule" | "hr" => {
                self.doc.blocks.push(Block::Rule);
                Ok(self.this())
            }
            "spacer" | "space" => {
                let unit = unit_arg(a, 1)?;
                let amount = len_arg(a, 0, unit, "amount")?
                    .unwrap_or(Length { value: 1.0, unit: Unit::Em });
                self.doc.blocks.push(Block::Spacer(amount));
                Ok(self.this())
            }
            "typst" => {
                // The escape hatch, and the ONLY place caller text becomes
                // Typst code rather than a Typst string. Documented as such.
                self.doc.blocks.push(Block::Raw(str_arg(a, 0).unwrap_or_default()));
                Ok(self.this())
            }

            // ---- terminals --------------------------------------------------
            "totypst" => Ok(Value::string(self.to_typst())),
            "tobinary" | "topdf" => Ok(Value::Binary(self.to_pdf()?)),
            "pagecount" => {
                let n = world::page_count_with_files(&self.to_typst(), &self.doc.files)
                    .map_err(CfmlError::runtime)?;
                Ok(Value::Int(n as i64))
            }
            "write" | "save" => {
                let path = str_arg(a, 0).unwrap_or_default();
                if path.trim().is_empty() {
                    return Err(CfmlError::runtime(
                        "Document.write(): a file path is required".to_string(),
                    ));
                }
                let overwrite = bool_arg(a, 1).unwrap_or(true);
                if !overwrite && std::path::Path::new(&path).exists() {
                    return Err(CfmlError::runtime(format!(
                        "Document.write(): [{}] already exists and overwrite is false",
                        path
                    )));
                }
                let pdf = self.to_pdf()?;
                std::fs::write(&path, pdf).map_err(|e| {
                    CfmlError::runtime(format!("Document.write(): unable to write [{}]: {}", path, e))
                })?;
                Ok(self.this())
            }
            other => Err(CfmlError::runtime(format!(
                "Document has no method [{}]. Available: pageSize, orientation, margin, font, \
                 numbering, title, author, header, footer, heading, paragraph, list, table, \
                 image, pageBreak, rule, spacer, typst, toTypst, toBinary, pageCount, write.",
                other
            ))),
        }
    }
}

/// Sniff an image's type from its bytes, not its filename — the same rule the
/// engine's `imageRead` follows, and the extension Typst keys its decoder on.
fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF8") {
        Some("gif")
    } else if bytes
        .iter()
        .take(512)
        .collect::<Vec<_>>()
        .windows(4)
        .any(|w| w == [&b'<', &b's', &b'v', &b'g'])
    {
        Some("svg")
    } else {
        None
    }
}

/// `Document()` — a fresh, empty document.
pub fn fn_document(_args: Vec<Value>) -> CfmlResult {
    Ok(CfmlDocument::new().into_value())
}

/// `IsDocumentObject( value )`
pub fn fn_is_document_object(args: Vec<Value>) -> CfmlResult {
    let is = matches!(args.first(), Some(Value::NativeObject(o))
        if o.read().map(|g| g.class_name() == "Document").unwrap_or(false));
    Ok(Value::Bool(is))
}
