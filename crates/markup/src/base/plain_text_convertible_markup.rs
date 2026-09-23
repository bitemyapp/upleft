//! Port of `plainText` (`Base/PlainTextConvertibleMarkup.swift`): the default
//! from `Structural Restrictions/InlineContainer.swift` (also restated by
//! `Emphasis`, `Strong` and `Strikethrough`) and the leaf versions in
//! `Inline Nodes/Inline Leaves/*.swift`.
//!
//! An inline container's plain text is its inline children's plain text
//! joined (wrapped in `~` for `Strikethrough`); a leaf contributes its own
//! text. The walk below is iterative so a deeply nested span cannot exhaust
//! the stack, and appends into one buffer instead of joining per level; the
//! result is the same string.

use crate::base::markup::Markup;
use crate::base::raw_markup::MarkupData;

impl<'a> Markup<'a> {
    /// Whether this element's Swift type conforms to `InlineMarkup`.
    pub fn is_inline_markup(&self) -> bool {
        matches!(
            self.data(),
            MarkupData::Text { .. }
                | MarkupData::SoftBreak
                | MarkupData::LineBreak
                | MarkupData::InlineCode { .. }
                | MarkupData::InlineHtml { .. }
                | MarkupData::CustomInline { .. }
                | MarkupData::SymbolLink { .. }
                | MarkupData::Emphasis
                | MarkupData::Strong
                | MarkupData::Strikethrough
                | MarkupData::Link { .. }
                | MarkupData::Image { .. }
                | MarkupData::InlineAttributes { .. }
        )
    }

    /// Whether this element's Swift type conforms to `InlineContainer`
    /// (`Paragraph`, `Heading`, `Table.Cell` and the inline containers).
    pub fn is_inline_container(&self) -> bool {
        matches!(
            self.data(),
            MarkupData::Paragraph
                | MarkupData::Heading { .. }
                | MarkupData::TableCell { .. }
                | MarkupData::Emphasis
                | MarkupData::Strong
                | MarkupData::Strikethrough
                | MarkupData::Link { .. }
                | MarkupData::Image { .. }
                | MarkupData::InlineAttributes { .. }
        )
    }

    /// `(self as? PlainTextConvertibleMarkup)?.plainText`: `None` when the
    /// element's type has no `plainText`.
    pub fn plain_text(&self) -> Option<String> {
        if !(self.is_inline_markup() || self.is_inline_container()) {
            return None;
        }
        let mut out = String::new();
        if append_leaf_plain_text(*self, &mut out) {
            return Some(out);
        }
        // Each open container: its remaining children and what closes it.
        let mut stack = vec![(self.children(), open_container(*self, &mut out))];
        while let Some((children, _)) = stack.last_mut() {
            let Some(child) = children.next() else {
                let (_, close) = stack.pop().expect("non-empty");
                out.push_str(close);
                continue;
            };
            // `children.compactMap { ($0 as? InlineMarkup)?.plainText }`
            if !child.is_inline_markup() {
                continue;
            }
            if !append_leaf_plain_text(child, &mut out) {
                let close = open_container(child, &mut out);
                stack.push((child.children(), close));
            }
        }
        Some(out)
    }
}

/// Starts a container's `plainText` and returns its closing text:
/// `Strikethrough` wraps its children's text in `~`
/// (`Strikethrough.swift`); every other container joins them bare.
fn open_container(markup: Markup<'_>, out: &mut String) -> &'static str {
    match markup.data() {
        MarkupData::Strikethrough => {
            out.push('~');
            "~"
        }
        _ => "",
    }
}

/// Appends a leaf's `plainText` and returns `true`, or returns `false` for an
/// element whose plain text comes from its children.
fn append_leaf_plain_text(markup: Markup<'_>, out: &mut String) -> bool {
    match markup.data() {
        MarkupData::Text { string } => out.push_str(string),
        MarkupData::SoftBreak => out.push(' '),
        MarkupData::LineBreak => out.push('\n'),
        MarkupData::InlineCode { code } => {
            out.push('`');
            out.push_str(code);
            out.push('`');
        }
        MarkupData::InlineHtml { raw_html } => out.push_str(raw_html),
        MarkupData::CustomInline { text } => out.push_str(text),
        MarkupData::SymbolLink { destination } => {
            out.push_str("``");
            out.push_str(destination.unwrap_or(""));
            out.push_str("``");
        }
        _ => return false,
    }
    true
}
