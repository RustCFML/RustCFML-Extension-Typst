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

use crate::model::{Align, Block, Document, Length, Running, TableData, TextStyle};

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

/// `text` plus whatever styling the caller asked for, as **markup** — i.e. the
/// inside of a `[…]` content block.
///
/// Returning markup rather than a code expression is what keeps the callers
/// simple: a paragraph emits it straight into the document, a heading or table
/// cell wraps it in `[` … `]`, and nothing ever double-brackets.
fn styled(text: &str, style: &TextStyle) -> String {
    let mut markup = body(text);
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
    if let Some(a) = style.align {
        markup = format!("#align({})[{}]", a.as_typst(), markup);
    }
    markup
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
    let needs_context = r.text.contains("{page}") || r.text.contains("{total}");
    let mut parts: Vec<String> = Vec::new();
    let mut rest = r.text.as_str();
    while let Some(pos) = rest.find('{') {
        let (before, from) = rest.split_at(pos);
        if !before.is_empty() {
            parts.push(format!("#{}", lit(before)));
        }
        if let Some(end) = from.find('}') {
            match &from[..=end] {
                "{page}" => parts.push("#counter(page).display()".into()),
                "{total}" => parts.push("#counter(page).final().first()".into()),
                other => parts.push(format!("#{}", lit(other))),
            }
            rest = &from[end + 1..];
        } else {
            parts.push(format!("#{}", lit(from)));
            rest = "";
        }
    }
    if !rest.is_empty() {
        parts.push(format!("#{}", lit(rest)));
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
    let margins: Vec<String> = [("top", &m.top), ("bottom", &m.bottom), ("left", &m.left), ("right", &m.right)]
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|len| format!("{}: {}", k, len)))
        .collect();
    if !margins.is_empty() {
        page.push(format!("margin: ({})", margins.join(", ")));
    }
    if let Some(n) = &doc.page.numbering {
        page.push(format!("numbering: {}", lit(n)));
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

    out.push('\n');

    // ---- blocks ----------------------------------------------------------
    for block in &doc.blocks {
        out.push_str(&emit_block(block));
        out.push_str("\n\n");
    }
    out
}

fn emit_block(block: &Block) -> String {
    match block {
        Block::Heading { level, text, style } => {
            format!("#heading(level: {})[{}]", level, styled(text, style))
        }
        Block::Paragraph { text, style } => styled(text, style),
        Block::List { items, ordered, style } => {
            let f = if *ordered { "enum" } else { "list" };
            let body: Vec<String> = items.iter().map(|i| format!("[{}]", styled(i, style))).collect();
            format!("#{}({})", f, body.join(", "))
        }
        Block::Table { data, header, columns, aligns, style } => emit_table(data, *header, columns, aligns, style),
        Block::Image { file, width, height, align } => {
            let mut args = vec![lit(file)];
            if let Some(w) = width {
                args.push(format!("width: {}", w));
            }
            if let Some(h) = height {
                args.push(format!("height: {}", h));
            }
            let img = format!("#image({})", args.join(", "));
            match align {
                Some(a) => format!("#align({})[{}]", a.as_typst(), img),
                None => img,
            }
        }
        Block::PageBreak => "#pagebreak()".into(),
        Block::Rule => "#line(length: 100%)".into(),
        Block::Spacer(len) => format!("#v({})", len),
        Block::Raw(markup) => markup.clone(),
    }
}

fn emit_table(
    data: &TableData,
    header: bool,
    columns: &Option<Vec<Length>>,
    aligns: &Option<Vec<Align>>,
    style: &TextStyle,
) -> String {
    let ncols = data.rows.first().map(|r| r.len()).unwrap_or(0).max(1);
    let mut args: Vec<String> = Vec::new();
    match columns {
        Some(widths) if !widths.is_empty() => args.push(format!(
            "columns: ({},)",
            widths.iter().map(|w| w.to_string()).collect::<Vec<_>>().join(", ")
        )),
        _ => args.push(format!("columns: {}", ncols)),
    }
    if let Some(a) = aligns {
        if !a.is_empty() {
            args.push(format!(
                "align: ({},)",
                a.iter().map(|x| x.as_typst()).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    let mut rows = data.rows.iter();
    if header {
        if let Some(first) = rows.next() {
            let bold = TextStyle { bold: true, ..style.clone() };
            let cells: Vec<String> = first.iter().map(|c| format!("[{}]", styled(c, &bold))).collect();
            args.push(format!("table.header({})", cells.join(", ")));
        }
    }
    for row in rows {
        for c in row {
            args.push(format!("[{}]", styled(c, style)));
        }
    }
    format!("#table(\n  {},\n)", args.join(",\n  "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PageSetup, Unit};

    fn doc_with(blocks: Vec<Block>) -> Document {
        Document { blocks, page: PageSetup::default(), ..Document::default() }
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
                text: hostile.to_string(),
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
    fn headings_and_styles_use_code_mode() {
        let out = emit(&doc_with(vec![Block::Heading {
            level: 2,
            text: "Terms".into(),
            style: TextStyle::default(),
        }]));
        assert!(out.contains(r##"#heading(level: 2)[#"Terms"]"##), "{out}");
    }

    #[test]
    fn tables_emit_a_header_row_once() {
        let data = TableData {
            rows: vec![
                vec!["Item".into(), "Qty".into()],
                vec!["Widget".into(), "3".into()],
            ],
        };
        let out = emit(&doc_with(vec![Block::Table {
            data,
            header: true,
            columns: None,
            aligns: None,
            style: TextStyle::default(),
        }]));
        assert!(out.contains("columns: 2"), "{out}");
        assert_eq!(out.matches("table.header(").count(), 1, "{out}");
        assert!(out.contains(r##"#text(weight: "bold")[#"Item"]"##), "{out}");
    }

    #[test]
    fn running_footer_placeholders_become_counters() {
        let mut doc = doc_with(vec![]);
        doc.footer = Some(Running { text: "Page {page} of {total}".into(), align: Some(Align::Center) });
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
        doc.page.margin.top = Some(Length { value: 2.0, unit: Unit::Cm });
        doc.font_family = Some("Helvetica".into());
        let out = emit(&doc);
        assert_eq!(out.matches("#set page(").count(), 1, "{out}");
        assert!(out.contains(r#"paper: "a4""#), "{out}");
        assert!(out.contains("flipped: true"), "{out}");
        assert!(out.contains("margin: (top: 2cm)"), "{out}");
        assert!(out.contains(r#"#set text(font: "Helvetica")"#), "{out}");
    }
}
