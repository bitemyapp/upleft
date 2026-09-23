//! Port of `Sources/Markdown/Walker/Walkers/MarkupTreeDumper.swift`:
//! `debugDescription(options:)`, the textual tree swift-markdown's own tests
//! compare against. Only `.printSourceLocations` is offered; unique
//! identifiers and block directives are not ported.

use crate::base::markup::Markup;
use crate::base::raw_markup::{Checkbox, MarkupData};
use crate::nodes::tables::ColumnAlignment;

impl Markup<'_> {
    /// `debugDescription(options:)`, with `.printSourceLocations` when
    /// `print_source_locations` is set.
    pub fn debug_description(&self, print_source_locations: bool) -> String {
        let mut dumper = MarkupTreeDumper {
            print_source_locations,
            result: String::new(),
            path: Vec::new(),
        };
        dumper.visit(*self);
        dumper.result
    }
}

/// A walker that dumps a textual representation of a markup tree.
struct MarkupTreeDumper<'a> {
    print_source_locations: bool,
    /// The resulting string built up during dumping.
    result: String,
    /// The current path in the tree so far, used for printing edges.
    path: Vec<Markup<'a>>,
}

/// `split(separator: "\n")` over Swift `Character`s: empty pieces are
/// omitted, and a `\r\n` pair is one character, so it never splits there.
fn split_lines(string: &str) -> Vec<&str> {
    let bytes = string.as_bytes();
    let mut pieces = Vec::new();
    let mut start = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'\n' && !(index > 0 && bytes[index - 1] == b'\r') {
            if index > start {
                pieces.push(&string[start..index]);
            }
            start = index + 1;
        }
    }
    if start < bytes.len() {
        pieces.push(&string[start..]);
    }
    pieces
}

/// `String(prefix.reversed())`: the prefix is built from ASCII spaces and
/// `│`, so reversing scalars is reversing characters.
fn reversed(string: &str) -> String {
    string.chars().rev().collect()
}

impl<'a> MarkupTreeDumper<'a> {
    fn dump(&mut self, markup: Markup<'a>, custom_description: Option<String>) {
        self.indent(markup);
        self.result.push_str(swift_type_name(&markup.data()));
        if self.print_source_locations
            && let Some(range) = markup.range()
        {
            self.result.push_str(" @");
            self.result.push_str(&range.diagnostic_description());
        }
        if let Some(custom_description) = custom_description {
            if !custom_description.starts_with('\n') {
                self.result.push(' ');
            }
            self.result.push_str(&custom_description);
        }
        self.increasing_depth(markup);
    }

    fn line_indent_prefix(&self) -> String {
        let mut prefix = String::new();
        for (depth, element) in self.path.iter().enumerate().rev() {
            let last_child_index = element.parent().map(|parent| parent.child_count() - 1);
            match last_child_index {
                Some(last_child_index) if last_child_index != element.index_in_parent() => {
                    prefix.push_str("  │");
                }
                _ => {
                    if depth > 0 {
                        prefix.push_str("   ");
                    }
                }
            }
        }
        reversed(&prefix)
    }

    fn indent_literal_block(&mut self, string: &str, element: Markup<'a>) -> String {
        self.path.push(element);
        let prefix = self.line_indent_prefix();
        let result = split_lines(string)
            .into_iter()
            .map(|line| format!("{prefix}{line}"))
            .collect::<Vec<_>>()
            .join("\n");
        self.path.pop();
        result
    }

    /// Adds an indentation prefix for a markup element using the current path.
    fn indent(&mut self, markup: Markup<'a>) {
        if !self.path.is_empty() {
            self.result.push('\n');
        }
        let prefix = self.line_indent_prefix();
        self.result.push_str(&prefix);

        let Some(parent) = markup.parent() else {
            return;
        };
        let last_child_index = parent.child_count() - 1;
        let tree_marker = if markup.index_in_parent() == last_child_index {
            "└─ "
        } else {
            "├─ "
        };
        self.result.push_str(tree_marker);
    }

    fn increasing_depth(&mut self, element: Markup<'a>) {
        self.path.push(element);
        for child in element.children() {
            self.visit(child);
        }
        self.path.pop();
    }

    fn visit(&mut self, markup: Markup<'a>) {
        let description = match markup.data() {
            MarkupData::Text { string } => Some(format!("\"{string}\"")),
            MarkupData::HtmlBlock { raw_html } => {
                Some(format!("\n{}", self.indent_literal_block(raw_html, markup)))
            }
            MarkupData::Link { destination, .. } => Some(
                destination
                    .map(|destination| format!("destination: \"{destination}\""))
                    .unwrap_or_default(),
            ),
            MarkupData::Image { source, title } => {
                let mut description = source
                    .map(|source| format!("source: \"{source}\""))
                    .unwrap_or_default();
                if let Some(title) = title {
                    description.push_str(&format!(" title: \"{title}\""));
                }
                Some(description)
            }
            MarkupData::Heading { level } => Some(format!("level: {level}")),
            MarkupData::OrderedList { start_index } => {
                (start_index != 1).then(|| format!("startIndex: {start_index}"))
            }
            MarkupData::CodeBlock { code, language } => {
                let lines = self.indent_literal_block(code, markup);
                Some(format!("language: {}\n{lines}", language.unwrap_or("none")))
            }
            MarkupData::InlineCode { code } => Some(format!("`{code}`")),
            MarkupData::InlineHtml { raw_html } => Some(raw_html.to_owned()),
            MarkupData::CustomInline { .. } => Some("customInline.text".to_owned()),
            MarkupData::ListItem { checkbox } => checkbox.map(|checkbox| {
                match checkbox {
                    Checkbox::Checked => "checkbox: [x]",
                    Checkbox::Unchecked => "checkbox: [ ]",
                }
                .to_owned()
            }),
            MarkupData::Table { column_alignments } => {
                let alignments = column_alignments
                    .iter()
                    .map(|alignment| match alignment {
                        None => "-",
                        Some(ColumnAlignment::Left) => "l",
                        Some(ColumnAlignment::Right) => "r",
                        Some(ColumnAlignment::Center) => "c",
                    })
                    .collect::<Vec<_>>()
                    .join("|");
                Some(format!("alignments: |{alignments}|"))
            }
            MarkupData::SymbolLink { destination } => {
                destination.map(|destination| format!("destination: {destination}"))
            }
            MarkupData::TableCell { colspan, rowspan } => {
                let mut description = String::new();
                if colspan != 1 {
                    description.push_str(&format!(" colspan: {colspan}"));
                }
                if rowspan != 1 {
                    description.push_str(&format!(" rowspan: {rowspan}"));
                }
                let description = description
                    .trim_matches(|c| c == ' ' || c == '\t')
                    .to_owned();
                (!description.is_empty()).then_some(description)
            }
            MarkupData::InlineAttributes { attributes } => {
                Some(format!("attributes: `{attributes}`"))
            }
            _ => None,
        };
        self.dump(markup, description);
    }
}

/// `"\(type(of: markup))"`: the unqualified Swift type name.
fn swift_type_name(data: &MarkupData<'_>) -> &'static str {
    match data {
        MarkupData::TableHead => "Head",
        MarkupData::TableBody => "Body",
        MarkupData::TableRow => "Row",
        MarkupData::TableCell { .. } => "Cell",
        other => other.type_name(),
    }
}
