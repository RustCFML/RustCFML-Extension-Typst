//! Turning the document model into Typst markup.
//!
//! **Everything is emitted in Typst's *code* mode** — `#heading(level: 1)[…]`,
//! `#table(…)`, `#image(…)` — and every piece of caller-supplied text is
//! emitted as a Typst **string literal**, never as markup. That is the whole
//! injection story: a paragraph containing `#set page(fill: black)`,  `$x^2$`
//! or a stray `]` is data, because a string literal has exactly two characters
//! that need escaping (`\` and `"`) and no way out of itself. It is also why
//! the builder never asks the caller to think about `#` — the one ergonomic
//! problem raw Typst-in-CFML has (see the README).
//!
//! The markup is emitted **once**, at a terminal, from the accumulated model.
//! Emitting incrementally would make `#set` scoping depend on call order.
//!
//! The few places a bare expression *is* emitted — measurements, colours, paper
//! names, labels, link URLs, PDF standard names — are all whitelisted or
//! pattern-checked at the API boundary in `document.rs`. That division is the
//! rule: this module assumes its input is already validated, and everything it
//! receives as free text goes through `lit`.

use crate::model::{
    Align, Block, Callout, Cell, Content, Document, Length, Run, Running, Stroke, Table, TextStyle,
};

/// Escape a Rust string into a Typst string literal, quotes included.
///
/// Only `\` and `"` are special inside a Typst string; control characters are
/// emitted as `\u{…}` so a stray NUL or vertical tab in application data can
/// never break the literal.
pub fn lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' | '\t' => out.push_str(&format!("\\u{{{:x}}}", ch as u32)),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Caller text as a Typst *content* expression suitable for `[…]` brackets.
///
/// Embedded newlines become explicit `#linebreak()` calls: a newline inside a
/// string literal would render as a space, which is not what a caller who put
/// one there meant.
pub fn body(s: &str) -> String {
    if s.is_empty() {
        return String::from("#\"\"");
    }
    s.split('\n')
        .map(|line| format!("#{}", lit(line)))
        .collect::<Vec<_>>()
        .join("#linebreak()")
}

/// Wrap already-emitted markup in whatever the style asks for.
///
/// Split from `styled` so runs and blocks share it: a run styles its own text,
/// a block styles the concatenation of its runs, and both end up here.
fn wrap(mut markup: String, style: &TextStyle, with_align: bool) -> String {
    let mut args: Vec<String> = Vec::new();
    if style.bold {
        args.push("weight: \"bold\"".into());
    }
    if style.italic {
        args.push("style: \"italic\"".into());
    }
    if let Some(size) = &style.size {
        args.push(format!("size: {}", size));
    }
    if let Some(font) = &style.font {
        args.push(format!("font: {}", lit(font)));
    }
    if let Some(color) = &style.color {
        args.push(format!("fill: {}", color));
    }
    if !args.is_empty() {
        markup = format!("#text({})[{}]", args.join(", "), markup);
    }
    // Decorations are functions, not `text` arguments, so they wrap.
    if style.smallcaps {
        markup = format!("#smallcaps[{}]", markup);
    }
    if style.underline {
        markup = format!("#underline[{}]", markup);
    }
    if style.strike {
        markup = format!("#strike[{}]", markup);
    }
    if with_align {
        if let Some(a) = style.align {
            markup = format!("#align({})[{}]", a.as_typst(), markup);
        }
    }
    markup
}

/// One inline run as markup.
fn run(r: &Run) -> String {
    match r {
        Run::Text { text, style } => wrap(body(text), style, false),
        Run::Link { url, content } => {
            format!("#link({})[{}]", lit(url), inline(content))
        }
        Run::Footnote { content } => format!("#footnote[{}]", inline(content)),
        // `label()` takes a string, so a reference is as safe as any literal.
        Run::Ref { label } => format!("#ref(label({}))", lit(label)),
        Run::Break => "#linebreak()".to_string(),
    }
}

/// A sequence of runs as markup, with no block-level styling.
pub fn inline(content: &Content) -> String {
    if content.is_empty() {
        return String::from("#\"\"");
    }
    content.iter().map(run).collect::<Vec<_>>().join("")
}

/// A sequence of runs as markup, with the block's own styling applied over it.
fn styled(content: &Content, style: &TextStyle) -> String {
    wrap(inline(content), style, true)
}

impl Align {
    pub fn as_typst(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "left",
        }
    }
}

/// A running header/footer, including the `{page}` / `{total}` placeholders.
///
/// The placeholders are the reason this is not just `styled()`: they have to
/// become live counters, and a counter read needs Typst's `context`.
fn running(r: &Running) -> String {
    // The placeholders only ever appear in a plain-text run — they are a
    // convenience for `.footer( "Page {page}" )`, not a run-level feature.
    let flat: String = r
        .content
        .iter()
        .filter_map(|x| match x {
            Run::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let needs_context = flat.contains("{page}") || flat.contains("{total}");

    let mut parts: Vec<String> = Vec::new();
    for item in &r.content {
        match item {
            Run::Text { text, style } => {
                let mut rest = text.as_str();
                let mut pieces: Vec<String> = Vec::new();
                while let Some(pos) = rest.find('{') {
                    let (before, from) = rest.split_at(pos);
                    if !before.is_empty() {
                        pieces.push(format!("#{}", lit(before)));
                    }
                    if let Some(end) = from.find('}') {
                        match &from[..=end] {
                            "{page}" => pieces.push("#counter(page).display()".into()),
                            "{total}" => pieces.push("#counter(page).final().first()".into()),
                            other => pieces.push(format!("#{}", lit(other))),
                        }
                        rest = &from[end + 1..];
                    } else {
                        pieces.push(format!("#{}", lit(from)));
                        rest = "";
                    }
                }
                if !rest.is_empty() {
                    pieces.push(format!("#{}", lit(rest)));
                }
                if pieces.is_empty() {
                    pieces.push("#\"\"".into());
                }
                parts.push(wrap(pieces.join(""), style, false));
            }
            other => parts.push(run(other)),
        }
    }
    if parts.is_empty() {
        parts.push("#\"\"".into());
    }
    let inner = parts.join("");
    let content = match r.align {
        Some(a) => format!("[#align({})[{}]]", a.as_typst(), inner),
        None => format!("[{}]", inner),
    };
    if needs_context {
        format!("context {}", content)
    } else {
        content
    }
}

/// Emit the whole document.
pub fn emit(doc: &Document) -> String {
    let mut out = String::new();

    // ---- document metadata --------------------------------------------
    let mut meta: Vec<String> = Vec::new();
    if let Some(t) = &doc.title {
        meta.push(format!("title: {}", lit(t)));
    }
    if let Some(a) = &doc.author {
        meta.push(format!("author: {}", lit(a)));
    }
    if !doc.keywords.is_empty() {
        meta.push(format!(
            "keywords: ({},)",
            doc.keywords
                .iter()
                .map(|k| lit(k))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !meta.is_empty() {
        out.push_str(&format!("#set document({})\n", meta.join(", ")));
    }

    // ---- page ----------------------------------------------------------
    let mut page: Vec<String> = Vec::new();
    match (&doc.page.width, &doc.page.height) {
        (Some(w), Some(h)) => {
            page.push(format!("width: {}", w));
            page.push(format!("height: {}", h));
        }
        _ => page.push(format!("paper: {}", lit(&doc.page.paper))),
    }
    if doc.page.landscape {
        page.push("flipped: true".into());
    }
    let m = &doc.page.margin;
    let margins: Vec<String> = [
        ("top", &m.top),
        ("bottom", &m.bottom),
        ("left", &m.left),
        ("right", &m.right),
    ]
    .iter()
    .filter_map(|(k, v)| v.as_ref().map(|len| format!("{}: {}", k, len)))
    .collect();
    if !margins.is_empty() {
        page.push(format!("margin: ({})", margins.join(", ")));
    }
    if let Some(n) = &doc.page.numbering {
        page.push(format!("numbering: {}", lit(n)));
    }
    if let Some(c) = doc.page.columns {
        page.push(format!("columns: {}", c));
    }
    if let Some(f) = &doc.page.fill {
        page.push(format!("fill: {}", f));
    }
    if let Some(h) = &doc.header {
        page.push(format!("header: {}", running(h)));
    }
    if let Some(f) = &doc.footer {
        page.push(format!("footer: {}", running(f)));
    }
    if !page.is_empty() {
        out.push_str(&format!("#set page({})\n", page.join(", ")));
    }

    // ---- default text --------------------------------------------------
    let mut text: Vec<String> = Vec::new();
    if let Some(f) = &doc.font_family {
        text.push(format!("font: {}", lit(f)));
    }
    if let Some(s) = &doc.font_size {
        text.push(format!("size: {}", s));
    }
    if let Some(c) = &doc.text_color {
        text.push(format!("fill: {}", c));
    }
    if !text.is_empty() {
        out.push_str(&format!("#set text({})\n", text.join(", ")));
    }
    if let Some(a) = doc.body_align {
        out.push_str(&format!("#set par(justify: {})\n", a == Align::Justify));
    }
    if let Some(n) = &doc.heading_numbering {
        out.push_str(&format!("#set heading(numbering: {})\n", lit(n)));
    }

    out.push('\n');

    // ---- blocks ----------------------------------------------------------
    for block in &doc.blocks {
        out.push_str(&emit_block(block));
        out.push_str("\n\n");
    }
    out
}

/// `#figure(...)` wrapper plus label, for the blocks that can carry a caption.
fn figure(inner: String, caption: &Option<Content>, label: &Option<String>, kind: &str) -> String {
    let mut out = match caption {
        Some(c) => format!(
            "#figure(caption: [{}], kind: {}, supplement: none)[{}]",
            inline(c),
            kind,
            inner
        ),
        None => inner,
    };
    if let Some(l) = label {
        out.push_str(&format!(" <{}>", l));
    }
    out
}

fn emit_block(block: &Block) -> String {
    match block {
        Block::Heading {
            level,
            content,
            style,
            label,
        } => {
            let mut out = format!("#heading(level: {})[{}]", level, styled(content, style));
            if let Some(l) = label {
                // A label attaches to the element immediately before it.
                out.push_str(&format!(" <{}>", l));
            }
            out
        }
        Block::Paragraph { content, style } => styled(content, style),
        Block::List {
            items,
            ordered,
            style,
        } => {
            let f = if *ordered { "enum" } else { "list" };
            let body: Vec<String> = items
                .iter()
                .map(|i| format!("[{}]", styled(i, style)))
                .collect();
            format!("#{}({})", f, body.join(", "))
        }
        Block::Terms { items, style } => {
            let body: Vec<String> = items
                .iter()
                .map(|(k, v)| format!("terms.item([{}], [{}])", styled(k, style), styled(v, style)))
                .collect();
            format!("#terms({})", body.join(", "))
        }
        Block::Quote {
            content,
            attribution,
            style,
        } => {
            let mut args = vec!["block: true".to_string()];
            if let Some(a) = attribution {
                args.push(format!("attribution: [{}]", inline(a)));
            }
            format!("#quote({})[{}]", args.join(", "), styled(content, style))
        }
        Block::Code { text, lang } => {
            let mut args = vec![lit(text), "block: true".to_string()];
            if let Some(l) = lang {
                args.push(format!("lang: {}", lit(l)));
            }
            format!("#raw({})", args.join(", "))
        }
        Block::Table(t) => {
            let table = emit_table(t);
            figure(table, &t.caption, &t.label, "table")
        }
        Block::Callout(c) => emit_callout(c),
        Block::Image {
            file,
            width,
            height,
            align,
            caption,
            label,
        } => {
            let mut args = vec![lit(file)];
            if let Some(w) = width {
                args.push(format!("width: {}", w));
            }
            if let Some(h) = height {
                args.push(format!("height: {}", h));
            }
            let img = format!("#image({})", args.join(", "));
            let img = figure(img, caption, label, "image");
            match align {
                Some(a) => format!("#align({})[{}]", a.as_typst(), img),
                None => img,
            }
        }
        Block::Outline { title, depth } => {
            let mut args: Vec<String> = Vec::new();
            match title {
                Some(t) => args.push(format!("title: [{}]", body(t))),
                None => args.push("title: none".into()),
            }
            if let Some(d) = depth {
                args.push(format!("depth: {}", d));
            }
            format!("#outline({})", args.join(", "))
        }
        Block::PageBreak => "#pagebreak()".into(),
        Block::Rule => "#line(length: 100%)".into(),
        Block::Spacer(len) => format!("#v({})", len),
        Block::Raw(markup) => markup.clone(),
    }
}

fn emit_callout(c: &Callout) -> String {
    let mut args: Vec<String> = Vec::new();
    if let Some(f) = c.fill.as_ref().or(c.style.bgcolor.as_ref()) {
        args.push(format!("fill: {}", f));
    }
    if let Some(s) = &c.stroke {
        args.push(format!("stroke: {}", s));
    }
    if let Some(r) = &c.radius {
        args.push(format!("radius: {}", r));
    }
    args.push(format!(
        "inset: {}",
        c.inset.unwrap_or(Length {
            value: 8.0,
            unit: crate::model::Unit::Pt
        })
    ));
    if let Some(w) = &c.width {
        args.push(format!("width: {}", w));
    } else {
        args.push("width: 100%".into());
    }
    format!(
        "#block({})[{}]",
        args.join(", "),
        styled(&c.content, &c.style)
    )
}

/// One table cell, with the style that column/header/cell layering produced.
///
/// A cell only becomes a `table.cell(...)` when it needs to — a fill or its own
/// alignment. Otherwise it stays a bare `[…]`, which keeps a 10,000-row table's
/// markup roughly the size of its data.
fn emit_cell(cell: &Cell, style: &TextStyle) -> String {
    let effective = match &cell.style {
        Some(s) => style.overlay(s),
        None => style.clone(),
    };
    let inner = wrap(body(&cell.text), &effective, false);
    let mut args: Vec<String> = Vec::new();
    if let Some(bg) = &effective.bgcolor {
        args.push(format!("fill: {}", bg));
    }
    if let Some(a) = effective.align {
        args.push(format!("align: {}", a.as_typst()));
    }
    if args.is_empty() {
        format!("[{}]", inner)
    } else {
        format!("table.cell({})[{}]", args.join(", "), inner)
    }
}

fn emit_table(t: &Table) -> String {
    let ncols = t.rows.first().map(|r| r.len()).unwrap_or(0).max(1);
    let mut args: Vec<String> = Vec::new();

    match &t.widths {
        Some(widths) if !widths.is_empty() => args.push(format!(
            "columns: ({},)",
            widths
                .iter()
                .map(|w| w.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        _ => args.push(format!("columns: {}", ncols)),
    }
    match &t.aligns {
        Some(a) if !a.is_empty() => args.push(format!(
            "align: ({},)",
            a.iter()
                .map(|x| x.as_typst())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        // Pin left rather than leaving Typst's `auto`, which INHERITS from the
        // enclosing alignment — and `#figure` centres its content. Without this,
        // adding a caption silently centre-aligned every cell that had no
        // alignment of its own. Found by looking at the rendered PDF; the markup
        // and the compile were both perfectly fine.
        _ => args.push("align: left".into()),
    }
    match &t.stroke {
        Stroke::Default => {}
        Stroke::None => args.push("stroke: none".into()),
        Stroke::Custom { color, thickness } => {
            let mut parts: Vec<String> = Vec::new();
            if let Some(c) = color {
                parts.push(format!("paint: {}", c));
            }
            if let Some(th) = thickness {
                parts.push(format!("thickness: {}", th));
            }
            args.push(format!("stroke: ({})", parts.join(", ")));
        }
    }
    if let Some(g) = &t.gutter {
        args.push(format!("gutter: {}", g));
    }
    if let Some(i) = &t.inset {
        args.push(format!("inset: {}", i));
    }

    // Row striping is a function of the row index, which is why it cannot be a
    // per-cell fill: Typst wants `fill: (x, y) => …` and the header row (y = 0,
    // when there is one) must not be counted.
    let hdr = if t.header { 1 } else { 0 };
    match (&t.stripe, &t.fill) {
        (Some(stripe), fill) => {
            let base = fill.clone().unwrap_or_else(|| "none".to_string());
            args.push(format!(
                "fill: (x, y) => if y >= {hdr} and calc.odd(y - {hdr}) {{ {stripe} }} else {{ {base} }}"
            ));
        }
        (None, Some(fill)) => args.push(format!("fill: {}", fill)),
        (None, None) => {}
    }

    let column_style = |col: usize| -> TextStyle {
        match t.column_styles.get(col).and_then(|s| s.as_ref()) {
            Some(cs) => t.style.overlay(cs),
            None => t.style.clone(),
        }
    };

    let mut rows = t.rows.iter();
    if t.header {
        if let Some(first) = rows.next() {
            let cells: Vec<String> = first
                .iter()
                .enumerate()
                .map(|(col, c)| {
                    // The header wins over the column: a right-aligned money
                    // column still gets a bold header, and a caller who sets
                    // headerFormat.alignment gets that instead.
                    let style = column_style(col).overlay(&t.header_style);
                    emit_cell(c, &style)
                })
                .collect();
            args.push(format!("table.header({})", cells.join(", ")));
        }
    }
    for row in rows {
        for (col, c) in row.iter().enumerate() {
            args.push(emit_cell(c, &column_style(col)));
        }
    }
    format!("#table(\n  {},\n)", args.join(",\n  "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PageSetup, Unit};

    fn doc_with(blocks: Vec<Block>) -> Document {
        Document {
            blocks,
            page: PageSetup::default(),
            ..Document::default()
        }
    }

    fn text(s: &str) -> Content {
        vec![Run::Text {
            text: s.to_string(),
            style: TextStyle::default(),
        }]
    }

    fn cells(rows: &[&[&str]]) -> Vec<Vec<Cell>> {
        rows.iter()
            .map(|r| r.iter().map(|c| Cell::plain(*c)).collect())
            .collect()
    }

    #[test]
    fn has_text_distinguishes_a_document_that_needs_a_glyph() {
        // The zero-font guard keys on this, so a wrong answer either lets a
        // blank PDF through or refuses a legitimate image-only one.
        assert!(!doc_with(vec![]).has_text());
        assert!(!doc_with(vec![Block::PageBreak, Block::Rule]).has_text());
        assert!(!doc_with(vec![Block::Image {
            file: "a.png".into(),
            width: None,
            height: None,
            align: None,
            caption: None,
            label: None,
        }])
        .has_text());

        assert!(doc_with(vec![Block::Paragraph {
            content: text("hello"),
            style: TextStyle::default(),
        }])
        .has_text());
        assert!(doc_with(vec![Block::Table(Box::new(Table {
            rows: cells(&[&["x"]]),
            ..Table::default()
        }))])
        .has_text());
        // A footnote's text still needs drawing even though the run nests.
        assert!(doc_with(vec![Block::Paragraph {
            content: vec![Run::Footnote {
                content: text("note")
            }],
            style: TextStyle::default(),
        }])
        .has_text());
        // Page numbering draws digits with no block involved at all.
        let mut doc = doc_with(vec![Block::Rule]);
        doc.page.numbering = Some("1".into());
        assert!(doc.has_text());
    }

    #[test]
    fn string_literals_are_escaped() {
        assert_eq!(lit(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(lit("tab\there"), r#""tab\u{9}here""#);
    }

    #[test]
    fn newlines_become_explicit_linebreaks() {
        // A newline inside a Typst string literal renders as a space, which is
        // never what a caller who typed one meant.
        assert_eq!(body("one\ntwo"), r##"#"one"#linebreak()#"two""##);
    }

    #[test]
    fn caller_text_cannot_become_typst_code() {
        // The injection test. Every one of these is Typst code if it reaches
        // markup mode; all of them must come out inside a string literal.
        for hostile in [
            "#set page(fill: black)",
            "] #pagebreak() [",
            "$x^2$",
            "= Not a heading",
            "#import \"@preview/evil:1.0.0\": *",
        ] {
            let out = emit(&doc_with(vec![Block::Paragraph {
                content: text(hostile),
                style: TextStyle::default(),
            }]));
            let body_start = out.find("\n\n").expect("header/body separator");
            let rendered = &out[body_start..];
            // The body is EXACTLY one splice of one string literal — there is
            // no room for a second `#`, a stray `[`, or anything else the
            // caller's text might have wanted to become.
            assert_eq!(
                rendered.trim(),
                format!("#{}", lit(hostile)),
                "hostile text must be emitted as a single string literal"
            );
        }
    }

    #[test]
    fn hostile_text_in_every_inline_position_stays_a_literal() {
        // Runs multiplied the number of places caller text lands. Each one has
        // to keep the property the block-level test above proves.
        let hostile = "#pagebreak()";
        let runs: Content = vec![
            Run::Text {
                text: hostile.into(),
                style: TextStyle {
                    bold: true,
                    ..TextStyle::default()
                },
            },
            Run::Link {
                url: "https://example.com".into(),
                content: text(hostile),
            },
            Run::Footnote {
                content: text(hostile),
            },
        ];
        let out = emit(&doc_with(vec![Block::Paragraph {
            content: runs,
            style: TextStyle::default(),
        }]));
        // Three occurrences, every one of them inside a string literal.
        assert_eq!(out.matches(hostile).count(), 3, "{out}");
        assert_eq!(
            out.matches(&format!("#{}", lit(hostile))).count(),
            3,
            "{out}"
        );
    }

    #[test]
    fn headings_and_styles_use_code_mode() {
        let out = emit(&doc_with(vec![Block::Heading {
            level: 2,
            content: text("Terms"),
            style: TextStyle::default(),
            label: None,
        }]));
        assert!(out.contains(r##"#heading(level: 2)[#"Terms"]"##), "{out}");
    }

    #[test]
    fn a_label_follows_the_element_it_names() {
        let out = emit(&doc_with(vec![
            Block::Heading {
                level: 1,
                content: text("Terms"),
                style: TextStyle::default(),
                label: Some("terms".into()),
            },
            Block::Paragraph {
                content: vec![Run::Ref {
                    label: "terms".into(),
                }],
                style: TextStyle::default(),
            },
        ]));
        assert!(
            out.contains(r##"#heading(level: 1)[#"Terms"] <terms>"##),
            "{out}"
        );
        assert!(out.contains(r##"#ref(label("terms"))"##), "{out}");
    }

    #[test]
    fn tables_emit_a_header_row_once() {
        let t = Table {
            rows: cells(&[&["Item", "Qty"], &["Widget", "3"]]),
            ..Table::default()
        };
        let out = emit(&doc_with(vec![Block::Table(Box::new(t))]));
        assert!(out.contains("columns: 2"), "{out}");
        assert_eq!(out.matches("table.header(").count(), 1, "{out}");
        assert!(out.contains(r##"#text(weight: "bold")[#"Item"]"##), "{out}");
    }

    #[test]
    fn column_styles_layer_under_the_header_style() {
        let t = Table {
            rows: cells(&[&["Item", "Amount"], &["Widget", "3.00"]]),
            column_styles: vec![
                None,
                Some(TextStyle {
                    align: Some(Align::Right),
                    color: Some("rgb(\"#333333\")".into()),
                    ..TextStyle::default()
                }),
            ],
            ..Table::default()
        };
        let out = emit(&doc_with(vec![Block::Table(Box::new(t))]));
        // The body cell takes the column's alignment and colour…
        assert!(
            out.contains(r##"table.cell(align: right)[#text(fill: rgb("#333333"))[#"3.00"]]"##),
            "{out}"
        );
        // …and the header cell keeps the column's alignment while adding bold.
        assert!(
            out.contains(r##"table.cell(align: right)[#text(weight: "bold", fill: rgb("#333333"))[#"Amount"]]"##),
            "{out}"
        );
    }

    #[test]
    fn striping_skips_the_header_row() {
        let t = Table {
            rows: cells(&[&["a"], &["b"]]),
            stripe: Some("rgb(\"#f5f5f5\")".into()),
            ..Table::default()
        };
        let out = emit(&doc_with(vec![Block::Table(Box::new(t))]));
        assert!(
            out.contains("fill: (x, y) => if y >= 1 and calc.odd(y - 1)"),
            "{out}"
        );

        // With no header row there is nothing to skip.
        let t = Table {
            rows: cells(&[&["a"], &["b"]]),
            header: false,
            stripe: Some("rgb(\"#f5f5f5\")".into()),
            ..Table::default()
        };
        let out = emit(&doc_with(vec![Block::Table(Box::new(t))]));
        assert!(
            out.contains("fill: (x, y) => if y >= 0 and calc.odd(y - 0)"),
            "{out}"
        );
        assert!(!out.contains("table.header("), "{out}");
    }

    #[test]
    fn a_captioned_table_keeps_its_cells_left_aligned() {
        // `#figure` centres its content and Typst's table `align` defaults to
        // `auto`, i.e. inherit — so a caption used to re-align the data.
        let t = Table {
            rows: cells(&[&["Item"], &["Widget"]]),
            caption: Some(text("Fees")),
            ..Table::default()
        };
        let out = emit(&doc_with(vec![Block::Table(Box::new(t))]));
        assert!(out.contains("align: left"), "{out}");

        // An explicit per-column list still wins.
        let t = Table {
            rows: cells(&[&["Item", "Amount"]]),
            aligns: Some(vec![Align::Left, Align::Right]),
            ..Table::default()
        };
        let out = emit(&doc_with(vec![Block::Table(Box::new(t))]));
        assert!(out.contains("align: (left, right,)"), "{out}");
    }

    #[test]
    fn a_captioned_table_becomes_a_figure() {
        let t = Table {
            rows: cells(&[&["a"]]),
            caption: Some(text("Fees")),
            label: Some("fees".into()),
            ..Table::default()
        };
        let out = emit(&doc_with(vec![Block::Table(Box::new(t))]));
        assert!(
            out.contains(r##"#figure(caption: [#"Fees"], kind: table"##),
            "{out}"
        );
        assert!(out.trim().ends_with("<fees>"), "{out}");
    }

    #[test]
    fn a_grid_is_a_table_with_no_stroke() {
        let t = Table {
            rows: cells(&[&["a", "b"]]),
            header: false,
            stroke: Stroke::None,
            ..Table::default()
        };
        let out = emit(&doc_with(vec![Block::Table(Box::new(t))]));
        assert!(out.contains("stroke: none"), "{out}");
    }

    #[test]
    fn running_footer_placeholders_become_counters() {
        let mut doc = doc_with(vec![]);
        doc.footer = Some(Running {
            content: text("Page {page} of {total}"),
            align: Some(Align::Center),
        });
        let out = emit(&doc);
        assert!(out.contains("context"), "{out}");
        assert!(out.contains("counter(page).display()"), "{out}");
        assert!(out.contains("counter(page).final().first()"), "{out}");
        // The literal parts are still literals.
        assert!(out.contains(r##"#"Page ""##), "{out}");
    }

    #[test]
    fn page_setup_is_emitted_once_at_the_top() {
        let mut doc = doc_with(vec![Block::PageBreak]);
        doc.page.paper = "a4".into();
        doc.page.landscape = true;
        doc.page.margin.top = Some(Length {
            value: 2.0,
            unit: Unit::Cm,
        });
        doc.font_family = Some("Helvetica".into());
        let out = emit(&doc);
        assert_eq!(out.matches("#set page(").count(), 1, "{out}");
        assert!(out.contains(r#"paper: "a4""#), "{out}");
        assert!(out.contains("flipped: true"), "{out}");
        assert!(out.contains("margin: (top: 2cm)"), "{out}");
        assert!(out.contains(r#"#set text(font: "Helvetica")"#), "{out}");
    }
}
