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

/// Inline styling, shared by every verb that renders text.
#[derive(Clone, Default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub size: Option<Length>,
    pub font: Option<String>,
    /// Already-validated Typst colour expression, e.g. `rgb("#ff0000")`.
    pub color: Option<String>,
    pub align: Option<Align>,
}

#[derive(Clone)]
pub struct Running {
    pub text: String,
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
        }
    }
}

/// Rows of already-stringified cells. Queries, arrays of arrays and arrays of
/// structs all normalise to this before they reach the emitter.
#[derive(Clone)]
pub struct TableData {
    pub rows: Vec<Vec<String>>,
}

#[derive(Clone)]
pub enum Block {
    Heading { level: u8, text: String, style: TextStyle },
    Paragraph { text: String, style: TextStyle },
    List { items: Vec<String>, ordered: bool, style: TextStyle },
    Table {
        data: TableData,
        header: bool,
        columns: Option<Vec<Length>>,
        aligns: Option<Vec<Align>>,
        style: TextStyle,
    },
    /// A file registered with the world under this virtual name.
    Image { file: String, width: Option<Length>, height: Option<Length>, align: Option<Align> },
    PageBreak,
    Rule,
    Spacer(Length),
    /// Raw Typst, supplied by the caller through the `.typst()` escape hatch.
    Raw(String),
}

#[derive(Clone, Default)]
pub struct Document {
    pub title: Option<String>,
    pub author: Option<String>,
    pub page: PageSetup,
    pub font_family: Option<String>,
    pub font_size: Option<Length>,
    pub text_color: Option<String>,
    pub body_align: Option<Align>,
    pub header: Option<Running>,
    pub footer: Option<Running>,
    pub blocks: Vec<Block>,
    /// Virtual filename → bytes, handed to the `World` so `#image("logo.png")`
    /// resolves without the document being able to reach the real filesystem.
    pub files: Vec<(String, Vec<u8>)>,
}
