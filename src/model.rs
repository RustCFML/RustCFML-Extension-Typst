//! The document model the fluent builder accumulates.
//!
//! Deliberately dumb: every verb appends to (or sets a field on) this struct
//! and nothing is rendered until a terminal asks for it. Keeping the model
//! inert is what lets `emit` decide `#set` ordering globally instead of
//! guessing it one call at a time.

use std::fmt;

/// A CFML-side measurement — a number plus a unit, never a Typst literal.
///
/// The caller writes `unit="cm"`, not `2cm`, so nothing that reaches Typst is
/// ever assembled out of caller text.
#[derive(Clone, Copy, PartialEq)]
pub struct Length {
    pub value: f64,
    pub unit: Unit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Pt,
    Mm,
    Cm,
    In,
    Em,
    Percent,
    /// Typst's fractional unit — `1fr`, `2fr`. Only meaningful for table
    /// column widths, where it is the idiomatic way to share the page width.
    Fr,
}

impl Unit {
    pub fn parse(s: &str) -> Option<Unit> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "pt" | "point" | "points" | "" => Unit::Pt,
            "mm" => Unit::Mm,
            "cm" => Unit::Cm,
            "in" | "inch" | "inches" => Unit::In,
            "em" => Unit::Em,
            "%" | "pct" | "percent" => Unit::Percent,
            "fr" | "fraction" => Unit::Fr,
            _ => return None,
        })
    }

    fn suffix(self) -> &'static str {
        match self {
            Unit::Pt => "pt",
            Unit::Mm => "mm",
            Unit::Cm => "cm",
            Unit::In => "in",
            Unit::Em => "em",
            Unit::Percent => "%",
            Unit::Fr => "fr",
        }
    }
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A finite number formatted by Rust is always a valid Typst numeric
        // literal; non-finite values are rejected at the API boundary.
        write!(f, "{}{}", self.value, self.unit.suffix())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    pub fn parse(s: &str) -> Option<Align> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "left" | "start" => Align::Left,
            "center" | "centre" | "middle" => Align::Center,
            "right" | "end" => Align::Right,
            "justify" | "justified" => Align::Justify,
            _ => return None,
        })
    }
}

/// Styling, shared by every verb that renders text.
///
/// This is the document counterpart of the engine's spreadsheet **format
/// struct** (`{ bold: true, bgcolor: "##dfe6e9", alignment: "right" }`), and it
/// is deliberately the same shape: one plain CFML struct, case-insensitive keys,
/// accepted anywhere styling is accepted. `numberFormat` lives here too for the
/// same reason it lives in a spreadsheet format — a column's *presentation* and
/// its *number mask* are one decision, not two.
#[derive(Clone, Default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub smallcaps: bool,
    pub size: Option<Length>,
    pub font: Option<String>,
    /// Already-validated Typst colour expression, e.g. `rgb("#ff0000")`.
    pub color: Option<String>,
    /// Cell/box fill. Only honoured where a fill makes sense — table cells and
    /// `.callout()` — because Typst has no "background colour" for loose text.
    pub bgcolor: Option<String>,
    pub align: Option<Align>,
    /// A CFML `numberFormat`/`dateFormat` mask. Never emitted: the *engine*
    /// applies it, through `ctx.call()`, before the value becomes a string.
    /// See `document::format_cell`.
    pub number_format: Option<String>,
    pub date_format: Option<String>,
}

impl TextStyle {
    /// `other`'s set fields win. Used to layer table styling: whole-table
    /// default → column → header, which is the same layering the spreadsheet
    /// builder gets from `formatColumn` after `formatRow`.
    pub fn overlay(&self, other: &TextStyle) -> TextStyle {
        TextStyle {
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            underline: self.underline || other.underline,
            strike: self.strike || other.strike,
            smallcaps: self.smallcaps || other.smallcaps,
            size: other.size.or(self.size),
            font: other.font.clone().or_else(|| self.font.clone()),
            color: other.color.clone().or_else(|| self.color.clone()),
            bgcolor: other.bgcolor.clone().or_else(|| self.bgcolor.clone()),
            align: other.align.or(self.align),
            number_format: other
                .number_format
                .clone()
                .or_else(|| self.number_format.clone()),
            date_format: other
                .date_format
                .clone()
                .or_else(|| self.date_format.clone()),
        }
    }

    /// True when nothing would be emitted for this style.
    pub fn is_plain(&self) -> bool {
        !self.bold
            && !self.italic
            && !self.underline
            && !self.strike
            && !self.smallcaps
            && self.size.is_none()
            && self.font.is_none()
            && self.color.is_none()
            && self.align.is_none()
    }
}

/// One piece of inline content.
///
/// The builder used to style a whole *block* — a paragraph was bold or it was
/// not — which meant there was no way to bold three words mid-sentence. A run
/// is the smallest unit that fixes that, and it is also what makes links and
/// footnotes expressible without an escape hatch.
#[derive(Clone)]
pub enum Run {
    Text {
        text: String,
        style: TextStyle,
    },
    /// A hyperlink. `url` is validated at the API boundary — see
    /// `document::url_arg` — because it is emitted as a Typst string, and a
    /// link is the one place a caller's string becomes navigable.
    Link {
        url: String,
        content: Vec<Run>,
    },
    Footnote {
        content: Vec<Run>,
    },
    /// A cross-reference to a `label`led heading or figure.
    Ref {
        label: String,
    },
    Break,
}

/// Inline content: a sequence of runs. A plain string becomes a single
/// `Run::Text`, so every verb can take either.
pub type Content = Vec<Run>;

#[derive(Clone)]
pub struct Running {
    pub content: Content,
    pub align: Option<Align>,
}

#[derive(Clone, Default)]
pub struct Margin {
    pub top: Option<Length>,
    pub bottom: Option<Length>,
    pub left: Option<Length>,
    pub right: Option<Length>,
}

#[derive(Clone)]
pub struct PageSetup {
    pub paper: String,
    pub landscape: bool,
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub margin: Margin,
    pub numbering: Option<String>,
    /// Multi-column text flow — `#set page(columns: 2)`.
    pub columns: Option<u32>,
    pub fill: Option<String>,
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup {
            paper: "a4".to_string(),
            landscape: false,
            width: None,
            height: None,
            margin: Margin::default(),
            numbering: None,
            columns: None,
            fill: None,
        }
    }
}

/// One table cell: text plus the style that applies to it.
///
/// Cells hold **text**, not `Content`. A query with 10,000 rows and six columns
/// is 60,000 cells; giving each one a `Vec<Run>` would allocate six times that
/// for a capability — a link inside a cell — that `.typst()` already covers.
#[derive(Clone)]
pub struct Cell {
    pub text: String,
    /// Set only where a cell differs from its column. Kept as an option so the
    /// common case costs nothing.
    pub style: Option<TextStyle>,
}

impl Cell {
    pub fn plain(text: impl Into<String>) -> Cell {
        Cell {
            text: text.into(),
            style: None,
        }
    }
}

/// How a table draws its lines.
#[derive(Clone, Default)]
pub enum Stroke {
    /// Typst's own default — a hairline around every cell.
    #[default]
    Default,
    /// `stroke: none`. What `.grid()` uses.
    None,
    Custom {
        color: Option<String>,
        thickness: Option<Length>,
    },
}

/// Everything a table carries. Split out of `Block` because the spreadsheet-grade
/// surface has more knobs than the rest of the model put together.
#[derive(Clone)]
pub struct Table {
    pub rows: Vec<Vec<Cell>>,
    /// Whether row 0 is a header row.
    pub header: bool,
    pub widths: Option<Vec<Length>>,
    pub aligns: Option<Vec<Align>>,
    /// The whole-table default.
    pub style: TextStyle,
    /// Applied over `style` for the header row. Bold, unless the caller says
    /// otherwise.
    pub header_style: TextStyle,
    /// Applied over `style`, by column index.
    pub column_styles: Vec<Option<TextStyle>>,
    /// Fill for alternating body rows — the zebra striping a report wants.
    pub stripe: Option<String>,
    pub fill: Option<String>,
    pub stroke: Stroke,
    pub gutter: Option<Length>,
    pub inset: Option<Length>,
    pub caption: Option<Content>,
    pub label: Option<String>,
}

impl Default for Table {
    fn default() -> Self {
        Table {
            rows: Vec::new(),
            header: true,
            widths: None,
            aligns: None,
            style: TextStyle::default(),
            header_style: TextStyle {
                bold: true,
                ..TextStyle::default()
            },
            column_styles: Vec::new(),
            stripe: None,
            fill: None,
            stroke: Stroke::Default,
            gutter: None,
            inset: None,
            caption: None,
            label: None,
        }
    }
}

/// A bordered, optionally filled box — the callout, the shaded totals panel,
/// the bordered note. Typst's `#block(fill:, stroke:, radius:, inset:)`.
#[derive(Clone)]
pub struct Callout {
    pub content: Content,
    pub style: TextStyle,
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub radius: Option<Length>,
    pub inset: Option<Length>,
    pub width: Option<Length>,
}

#[derive(Clone)]
pub enum Block {
    Heading {
        level: u8,
        content: Content,
        style: TextStyle,
        label: Option<String>,
    },
    Paragraph {
        content: Content,
        style: TextStyle,
    },
    List {
        items: Vec<Content>,
        ordered: bool,
        style: TextStyle,
    },
    /// A description list — `#terms`.
    Terms {
        items: Vec<(Content, Content)>,
        style: TextStyle,
    },
    Quote {
        content: Content,
        attribution: Option<Content>,
        style: TextStyle,
    },
    /// Preformatted text with optional syntax highlighting.
    Code {
        text: String,
        lang: Option<String>,
    },
    /// Boxed: `Table` is by far the largest variant, and a document is mostly
    /// paragraphs.
    Table(Box<Table>),
    Callout(Callout),
    /// A file registered with the world under this virtual name.
    Image {
        file: String,
        width: Option<Length>,
        height: Option<Length>,
        align: Option<Align>,
        caption: Option<Content>,
        label: Option<String>,
    },
    /// Table of contents.
    Outline {
        title: Option<String>,
        depth: Option<u8>,
    },
    PageBreak,
    Rule,
    Spacer(Length),
    /// Raw Typst, supplied by the caller through the `.typst()` escape hatch.
    Raw(String),
}

/// PDF export options — the fields of `typst_pdf::PdfOptions` we let a caller
/// reach. Held as strings/flags and validated at the API boundary so the
/// document model stays free of Typst types.
#[derive(Clone, Default)]
pub struct PdfSettings {
    /// PDF standard names, e.g. `a-2b`, `ua-1`.
    pub standards: Vec<String>,
    /// Typst writes a baseline tagged (accessible) PDF by default; this only
    /// exists so a caller can turn it *off* to shrink the file.
    pub tagged: Option<bool>,
    /// `"1-3,7"` — the same range syntax the Typst CLI takes.
    pub page_ranges: Option<String>,
    pub creator: Option<String>,
    /// Seconds since the Unix epoch, for a reproducible `/CreationDate`.
    pub timestamp: Option<i64>,
}

/// A Typst template the caller supplies, in place of generated markup.
#[derive(Clone)]
pub struct Template {
    /// Absolute path of the `.typ` file that becomes the main source.
    pub main: std::path::PathBuf,
    /// The directory `#import`, `read()` and `#image` resolve under. Canonical.
    pub root: std::path::PathBuf,
}

#[derive(Clone, Default)]
pub struct Document {
    pub title: Option<String>,
    pub author: Option<String>,
    pub keywords: Vec<String>,
    pub page: PageSetup,
    pub font_family: Option<String>,
    pub font_size: Option<Length>,
    pub text_color: Option<String>,
    pub body_align: Option<Align>,
    /// `#set heading(numbering: …)`. Not merely cosmetic: Typst refuses to
    /// `#ref` an unnumbered heading, so cross-references need this on.
    pub heading_numbering: Option<String>,
    pub header: Option<Running>,
    pub footer: Option<Running>,
    pub blocks: Vec<Block>,
    /// Virtual filename → bytes, handed to the `World` so `#image("logo.png")`
    /// resolves without the document being able to reach the real filesystem.
    pub files: Vec<(String, Vec<u8>)>,
    pub pdf: PdfSettings,
    /// When set, this replaces the emitted markup entirely: the template file is
    /// the main source and `data` reaches it as a virtual `data.json`.
    pub template: Option<Template>,
    /// Serialised by the *engine's* `serializeJSON`, so a template sees exactly
    /// the JSON the rest of the application would.
    pub data_json: Option<String>,
}

impl Document {
    /// Whether anything in this document needs a glyph drawn.
    ///
    /// An image-only or rule-only document is legitimate on a host with no fonts;
    /// a document with a paragraph in it is not. See `world::check_fonts`.
    pub fn has_text(&self) -> bool {
        fn any(content: &Content) -> bool {
            content.iter().any(|r| match r {
                Run::Text { text, .. } => !text.is_empty(),
                Run::Link { content, .. } | Run::Footnote { content } => any(content),
                Run::Ref { .. } => true,
                Run::Break => false,
            })
        }
        if self.header.as_ref().map(|h| any(&h.content)) == Some(true)
            || self.footer.as_ref().map(|f| any(&f.content)) == Some(true)
            || self.page.numbering.is_some()
        {
            return true;
        }
        self.blocks.iter().any(|b| match b {
            Block::Heading { content, .. } | Block::Paragraph { content, .. } => any(content),
            Block::List { items, .. } => items.iter().any(any),
            Block::Terms { items, .. } => items.iter().any(|(k, v)| any(k) || any(v)),
            Block::Quote { content, .. } => any(content),
            Block::Code { text, .. } => !text.is_empty(),
            Block::Table(t) => {
                t.rows.iter().any(|r| r.iter().any(|c| !c.text.is_empty()))
                    || t.caption.as_ref().is_some_and(any)
            }
            Block::Callout(c) => any(&c.content),
            Block::Image { caption, .. } => caption.as_ref().is_some_and(any),
            // An outline draws its entries' text; raw markup is unknowable, so
            // assume it draws something rather than let a blank page through.
            Block::Outline { .. } | Block::Raw(_) => true,
            Block::PageBreak | Block::Rule | Block::Spacer(_) => false,
        })
    }
}
