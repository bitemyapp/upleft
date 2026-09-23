//! Mirrors `oracle/Sources/downright-oracle/MarkupDump.swift`: the
//! swift-markdown tree for `Document(parsing: text, options:
//! [.disableSmartOpts])`, from `upleft-markup`.

use std::path::Path;

use serde_json::Value;
use upleft_markup::{
    Checkbox, ColumnAlignment, Document, Markup, MarkupData, ParseOptions, SourceRange,
};

use super::Failure;
use super::json::{self, Object};

/// Reads a file the way Swift's `String(contentsOf:encoding: .utf8)` does:
/// one leading byte-order mark is dropped, and invalid UTF-8 is an error.
pub fn read_text(path: &Path) -> Result<String, Failure> {
    let mut bytes = std::fs::read(path)?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    String::from_utf8(bytes)
        .map_err(|_| Failure::Error(format!("{}: the file isn’t valid UTF-8", path.display())))
}

pub fn run(input: &Path, output: &Path) -> Result<(), Failure> {
    let text = read_text(input)?;
    let document = Document::parse(&text, ParseOptions::DISABLE_SMART_OPTS);
    json::write(&markup(document.root()), output)?;
    Ok(())
}

fn range(range: Option<SourceRange>) -> Value {
    json::optional(range, |range| {
        Value::Array(vec![
            range.lower_bound.line.into(),
            range.lower_bound.column.into(),
            range.upper_bound.line.into(),
            range.upper_bound.column.into(),
        ])
    })
}

fn string(value: Option<&str>) -> Value {
    json::optional(value, |value| value.into())
}

fn markup(markup: Markup<'_>) -> Value {
    let data = markup.data();
    let mut object = Object::new()
        .with("kind", data.type_name())
        .with("range", range(markup.range()));
    object = match data {
        MarkupData::CodeBlock { code, language } => {
            object.with("code", code).with("language", string(language))
        }
        MarkupData::HtmlBlock { raw_html } => object.with("rawHTML", raw_html),
        MarkupData::Heading { level } => object.with("level", level),
        MarkupData::ListItem { checkbox } => object.with(
            "checkbox",
            json::optional(checkbox, |checkbox| {
                match checkbox {
                    Checkbox::Checked => "checked",
                    Checkbox::Unchecked => "unchecked",
                }
                .into()
            }),
        ),
        MarkupData::OrderedList { start_index } => object.with("startIndex", start_index),
        MarkupData::Table { column_alignments } => object
            .with(
                "columnAlignments",
                Value::Array(
                    column_alignments
                        .iter()
                        .map(|alignment| {
                            json::optional(*alignment, |alignment| {
                                match alignment {
                                    ColumnAlignment::Left => "left",
                                    ColumnAlignment::Center => "center",
                                    ColumnAlignment::Right => "right",
                                }
                                .into()
                            })
                        })
                        .collect(),
                ),
            )
            .with(
                "maxColumnCount",
                markup
                    .max_column_count()
                    .expect("a table has a column count"),
            ),
        MarkupData::TableCell { colspan, rowspan } => {
            object.with("colspan", colspan).with("rowspan", rowspan)
        }
        MarkupData::Link { destination, title } => object
            .with("destination", string(destination))
            .with("title", string(title))
            .with(
                "isAutolink",
                markup.is_autolink().expect("a link has isAutolink"),
            ),
        MarkupData::Image { source, title } => object
            .with("source", string(source))
            .with("title", string(title)),
        MarkupData::InlineCode { code } => object.with("code", code),
        MarkupData::InlineHtml { raw_html } => object.with("rawHTML", raw_html),
        MarkupData::Text { string } => object.with("string", string),
        MarkupData::CustomInline { text } => object.with("text", text),
        MarkupData::SymbolLink { destination } => object.with("destination", string(destination)),
        MarkupData::InlineAttributes { attributes } => object.with("attributes", attributes),
        _ => object,
    };
    if let Some(plain_text) = markup.plain_text() {
        object = object.with("plainText", plain_text);
    }
    object
        .with("indexInParent", markup.index_in_parent())
        .with(
            "children",
            Value::Array(markup.children().map(self::markup).collect()),
        )
        .build()
}
