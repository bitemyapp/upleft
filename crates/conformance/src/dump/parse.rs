//! Mirrors `oracle/Sources/downright-oracle/ParseDump.swift`: the canonical
//! dump of `MarkdownParser.parse`, from `upleft-core`. Field names and order
//! match the Swift.

use std::path::Path;

use serde_json::Value;
use upleft_core::model::{
    BlockContent, BlockIdentity, FrontMatter, HeadingNode, InlineKind, InlineSpan, MDBlock, ParsedDocument, PathToken,
    ResolvableToken, TaskItem,
};
use upleft_core::parser::MarkdownParser;
use upleft_core::safe_html::{SafeHTMLDocument, SafeHTMLKind};
use upleft_core::swift_text;
use upleft_core::NSRange;

use super::json::{self, Object};
use super::Failure;

pub fn run(input: &Path, output: &Path) -> Result<(), Failure> {
    let text = super::markup::read_text(input)?;
    json::write(&document(&MarkdownParser::parse(&text)), output)?;
    Ok(())
}

/// `[location, length]`, signed like Swift's `Int`.
pub fn range(range: NSRange) -> Value {
    Value::Array(vec![(range.location as i64).into(), (range.length as i64).into()])
}

pub fn optional_range(value: Option<NSRange>) -> Value {
    value.map_or(Value::Null, range)
}

pub fn string(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |value| value.into())
}

pub fn int(value: isize) -> Value {
    (value as i64).into()
}

pub fn optional_int(value: Option<isize>) -> Value {
    value.map_or(Value::Null, int)
}

/// Swift's `keys.sorted()` over `String` keys.
fn sorted_keys<V>(map: &std::collections::HashMap<String, V>) -> Vec<&String> {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort_by(|a, b| swift_text::str_cmp(a, b));
    keys
}

pub fn document(document: &ParsedDocument) -> Value {
    Object::new()
        .with("length", int(document.length))
        .with("lineStarts", Value::Array(document.line_starts.iter().map(|&s| int(s)).collect()))
        .with("frontMatter", document.front_matter.as_ref().map_or(Value::Null, front_matter))
        .with("root", block(&document.root))
        .with("headings", Value::Array(document.headings.iter().map(heading).collect()))
        .with("tasks", Value::Array(document.tasks.iter().map(task).collect()))
        .with("pathTokens", Value::Array(document.path_tokens.iter().map(resolvable_token).collect()))
        .with(
            "footnotes",
            Value::Array(
                sorted_keys(&document.footnotes)
                    .into_iter()
                    .map(|key| {
                        let block = &document.footnotes[key];
                        Object::new()
                            .with("identifier", key.as_str())
                            .with("range", range(block.range))
                            .with("identity", identity(block.identity))
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "linkReferences",
            Value::Array(
                sorted_keys(&document.link_references)
                    .into_iter()
                    .map(|key| {
                        let reference = &document.link_references[key];
                        Object::new()
                            .with("key", key.as_str())
                            .with("identifier", reference.identifier.as_str())
                            .with("destination", reference.destination.as_str())
                            .with("title", string(reference.title.as_deref()))
                            .with("range", range(reference.range))
                            .build()
                    })
                    .collect(),
            ),
        )
        .build()
}

pub fn identity(identity: BlockIdentity) -> Value {
    Value::Array(vec![int(identity.kind), int(identity.ordinal)])
}

pub fn block(block: &MDBlock) -> Value {
    Object::new()
        .with("content", content(&block.content))
        .with("range", range(block.range))
        .with("contentRange", range(block.content_range))
        .with("markerRange", optional_range(block.marker_range))
        .with("trailingMarkerRange", optional_range(block.trailing_marker_range))
        .with("depth", int(block.depth))
        .with("quoteDepth", int(block.quote_depth))
        .with("subtreeHash", json::hex(block.subtree_hash))
        .with("identity", identity(block.identity))
        .with("safeHTML", block.safe_html.as_ref().map_or(Value::Null, safe_html))
        .with("inlines", Value::Array(block.inlines.iter().map(inline).collect()))
        .with("children", Value::Array(block.children.iter().map(|child| self::block(child)).collect()))
        .build()
}

pub fn content(content: &BlockContent) -> Value {
    match content {
        BlockContent::Document => Object::new().with("kind", "document").build(),
        BlockContent::Heading { level } => Object::new().with("kind", "heading").with("level", int(*level)).build(),
        BlockContent::Paragraph => Object::new().with("kind", "paragraph").build(),
        BlockContent::BlockQuote => Object::new().with("kind", "blockQuote").build(),
        BlockContent::Callout { kind, title } => Object::new()
            .with("kind", "callout")
            .with("calloutKind", kind.raw_value())
            .with("title", string(title.as_deref()))
            .build(),
        BlockContent::List { ordered, start, tight, marker } => Object::new()
            .with("kind", "list")
            .with("ordered", *ordered)
            .with("start", int(*start))
            .with("tight", *tight)
            .with("marker", marker.raw_value())
            .build(),
        BlockContent::ListItem { ordinal, checkbox } => Object::new()
            .with("kind", "listItem")
            .with("ordinal", optional_int(*ordinal))
            .with(
                "checkbox",
                checkbox.map_or(Value::Null, |checkbox| {
                    Object::new().with("isChecked", checkbox.is_checked).with("markRange", range(checkbox.mark_range)).build()
                }),
            )
            .build(),
        BlockContent::CodeBlock { language, is_fenced, content_range } => Object::new()
            .with("kind", "codeBlock")
            .with("language", string(language.as_deref()))
            .with("isFenced", *is_fenced)
            .with("codeRange", range(*content_range))
            .build(),
        BlockContent::Mermaid { source_range } => {
            Object::new().with("kind", "mermaid").with("sourceRange", range(*source_range)).build()
        }
        BlockContent::MathBlock { latex_range } => {
            Object::new().with("kind", "mathBlock").with("latexRange", range(*latex_range)).build()
        }
        BlockContent::Table(data) => Object::new()
            .with("kind", "table")
            .with("alignments", Value::Array(data.alignments.iter().map(|a| a.raw_value().into()).collect()))
            .with("delimiterRange", range(data.delimiter_range))
            .with(
                "rows",
                Value::Array(
                    data.rows
                        .iter()
                        .map(|row| {
                            Object::new()
                                .with("range", range(row.range))
                                .with("isHeader", row.is_header)
                                .with(
                                    "cells",
                                    Value::Array(
                                        row.cells
                                            .iter()
                                            .map(|cell| {
                                                Object::new()
                                                    .with("range", range(cell.range))
                                                    .with("contentRange", range(cell.content_range))
                                                    .with("inlines", Value::Array(cell.inlines.iter().map(inline).collect()))
                                                    .build()
                                            })
                                            .collect(),
                                    ),
                                )
                                .build()
                        })
                        .collect(),
                ),
            )
            .build(),
        BlockContent::ThematicBreak => Object::new().with("kind", "thematicBreak").build(),
        BlockContent::HtmlBlock => Object::new().with("kind", "htmlBlock").build(),
        BlockContent::FrontMatter(value) => Object::new().with("kind", "frontMatter").with("frontMatter", front_matter(value)).build(),
        BlockContent::FootnoteDefinition { identifier } => {
            Object::new().with("kind", "footnoteDefinition").with("identifier", identifier.as_str()).build()
        }
    }
}

pub fn front_matter(value: &FrontMatter) -> Value {
    Object::new()
        .with("range", range(value.range))
        .with("bodyRange", range(value.body_range))
        .with(
            "fields",
            Value::Array(
                value
                    .fields
                    .iter()
                    .map(|field| {
                        Object::new()
                            .with("key", field.key.as_str())
                            .with("value", field.value.as_str())
                            .with("keyRange", range(field.key_range))
                            .with("valueRange", range(field.value_range))
                            .build()
                    })
                    .collect(),
            ),
        )
        .build()
}

pub fn inline(span: &InlineSpan) -> Value {
    Object::new()
        .with("kind", inline_kind(&span.kind))
        .with("range", range(span.range))
        .with("contentRange", range(span.content_range))
        .with("leadingMarkerRange", optional_range(span.leading_marker_range))
        .with("trailingMarkerRange", optional_range(span.trailing_marker_range))
        .with("children", Value::Array(span.children.iter().map(inline).collect()))
        .build()
}

pub fn path_token(token: &PathToken) -> Value {
    Object::new()
        .with("rawPath", token.raw_path.as_str())
        .with("line", optional_int(token.line))
        .with("column", optional_int(token.column))
        .build()
}

pub fn inline_kind(kind: &InlineKind) -> Value {
    let simple = |name: &str| Object::new().with("kind", name).build();
    match kind {
        InlineKind::Text => simple("text"),
        InlineKind::Emphasis => simple("emphasis"),
        InlineKind::Strong => simple("strong"),
        InlineKind::Strikethrough => simple("strikethrough"),
        InlineKind::InlineCode => simple("inlineCode"),
        InlineKind::Link { destination, title } => Object::new()
            .with("kind", "link")
            .with("destination", destination.as_str())
            .with("title", string(title.as_deref()))
            .build(),
        InlineKind::Autolink { destination } => Object::new().with("kind", "autolink").with("destination", destination.as_str()).build(),
        InlineKind::Wikilink { target, label } => Object::new()
            .with("kind", "wikilink")
            .with("target", target.as_str())
            .with("label", string(label.as_deref()))
            .build(),
        InlineKind::Image { source, alt } => {
            Object::new().with("kind", "image").with("source", source.as_str()).with("alt", alt.as_str()).build()
        }
        InlineKind::InlineMath { latex_range } => Object::new().with("kind", "inlineMath").with("latexRange", range(*latex_range)).build(),
        InlineKind::PathToken(token) => Object::new().with("kind", "pathToken").with("token", path_token(token)).build(),
        InlineKind::FootnoteReference { identifier } => {
            Object::new().with("kind", "footnoteReference").with("identifier", identifier.as_str()).build()
        }
        InlineKind::SoftBreak => simple("softBreak"),
        InlineKind::LineBreak => simple("lineBreak"),
        InlineKind::InlineHTML => simple("inlineHTML"),
    }
}

pub fn safe_html(document: &SafeHTMLDocument) -> Value {
    Object::new()
        .with("range", range(document.range))
        .with("isSafe", document.is_safe)
        .with(
            "annotations",
            Value::Array(
                document
                    .annotations
                    .iter()
                    .map(|annotation| {
                        Object::new()
                            .with("kind", safe_html_kind(&annotation.kind))
                            .with("range", range(annotation.range))
                            .with("contentRange", range(annotation.content_range))
                            .with("tagRanges", Value::Array(annotation.tag_ranges.iter().map(|&r| range(r)).collect()))
                            .build()
                    })
                    .collect(),
            ),
        )
        .build()
}

pub fn safe_html_kind(kind: &SafeHTMLKind) -> Value {
    let simple = |name: &str| Object::new().with("kind", name).build();
    match kind {
        SafeHTMLKind::Paragraph { align } => {
            Object::new().with("kind", "paragraph").with("align", string(align.map(|a| a.raw_value()))).build()
        }
        SafeHTMLKind::Heading { level } => Object::new().with("kind", "heading").with("level", int(*level)).build(),
        SafeHTMLKind::Strong => simple("strong"),
        SafeHTMLKind::Emphasis => simple("emphasis"),
        SafeHTMLKind::Link { destination, title } => Object::new()
            .with("kind", "link")
            .with("destination", destination.as_str())
            .with("title", string(title.as_deref()))
            .build(),
        SafeHTMLKind::Image { source, alt } => {
            Object::new().with("kind", "image").with("source", source.as_str()).with("alt", alt.as_str()).build()
        }
        SafeHTMLKind::Inert => simple("inert"),
        SafeHTMLKind::LineBreak => simple("lineBreak"),
        SafeHTMLKind::Details { open } => Object::new().with("kind", "details").with("open", *open).build(),
        SafeHTMLKind::DetailsClosing => simple("detailsClosing"),
        SafeHTMLKind::Summary => simple("summary"),
        SafeHTMLKind::Table => simple("table"),
        SafeHTMLKind::TableRow => simple("tableRow"),
        SafeHTMLKind::TableCell { header, align } => Object::new()
            .with("kind", "tableCell")
            .with("header", *header)
            .with("align", string(align.map(|a| a.raw_value())))
            .build(),
    }
}

pub fn heading(heading: &HeadingNode) -> Value {
    Object::new()
        .with("level", int(heading.level))
        .with("title", heading.title.as_str())
        .with("range", range(heading.range))
        .with("contentRange", range(heading.content_range))
        .with("sectionRange", range(heading.section_range))
        .with("parentIndex", optional_int(heading.parent_index))
        .with("childIndices", Value::Array(heading.child_indices.iter().map(|&i| int(i)).collect()))
        .with("slug", heading.slug.as_str())
        .with("wordCount", int(heading.word_count))
        .build()
}

pub fn task(task: &TaskItem) -> Value {
    Object::new()
        .with("isChecked", task.is_checked)
        .with("markRange", range(task.mark_range))
        .with("contentRange", range(task.content_range))
        .with("text", task.text.as_str())
        .with("headingIndex", optional_int(task.heading_index))
        .with("indentLevel", int(task.indent_level))
        .build()
}

pub fn resolvable_token(token: &ResolvableToken) -> Value {
    Object::new()
        .with("token", path_token(&token.token))
        .with("range", range(token.range))
        .with("fromCodeSpan", token.from_code_span)
        .build()
}
