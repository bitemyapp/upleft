//! Port of `Debugging/VisualDebuggerModel.swift`: the value-only facts the
//! Visual Debugger panel shows for the selection. The debugger never keeps
//! AppKit objects or a text view; the host resolves fonts and attributes once
//! and injects display-safe values.
//!
//! The summary's first line says "Upleft Visual Debugger": the app is always
//! Upleft (AGENTS.md), and the Swift reference is rebranded the same way
//! (`scripts/rebrand.py`).

use std::sync::Arc;

use upleft_core::compatibility::compatibility_diagnostics::{CompatibilityDiagnostic, CompatibilityReport};
use upleft_core::compatibility::render_target::RenderTargetProfile;
use upleft_core::{BlockContent, InlineKind, InlineSpan, NSRange, ParsedDocument};
use upleft_render::render_contracts::RenderMode;

use crate::assets::asset_doctor::AssetDiagnostic;

/// A small, value-only snapshot of the style at the selected source offset.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct VisualDebuggerStyleFacts {
    pub font_family: String,
    pub point_size: f64,
    pub foreground_color: String,
    pub paragraph_alignment: String,
    pub line_height: f64,
    pub line_spacing: f64,
    pub attributes: Vec<String>,
}

impl VisualDebuggerStyleFacts {
    /// `init(fontFamily:pointSize:foregroundColor:paragraphAlignment:lineHeight:lineSpacing:attributes:)`.
    /// Swift defaults every argument to empty or zero, as `Default` does.
    pub fn new(
        font_family: impl Into<String>,
        point_size: f64,
        foreground_color: impl Into<String>,
        paragraph_alignment: impl Into<String>,
        line_height: f64,
        line_spacing: f64,
        attributes: Vec<String>,
    ) -> VisualDebuggerStyleFacts {
        VisualDebuggerStyleFacts {
            font_family: font_family.into(),
            point_size,
            foreground_color: foreground_color.into(),
            paragraph_alignment: paragraph_alignment.into(),
            line_height,
            line_spacing,
            attributes,
        }
    }
}

/// `VisualDebuggerMapping`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualDebuggerMapping {
    pub source_range: NSRange,
    pub text_kit_range: NSRange,
    pub source_offset: isize,
    pub text_kit_offset: isize,
    pub is_canonical: bool,
    pub hidden_source_ranges: Vec<NSRange>,
}

impl VisualDebuggerMapping {
    /// `init(sourceRange:)`: every other argument at Swift's default.
    pub fn new(source_range: NSRange) -> VisualDebuggerMapping {
        Self::with(source_range, NSRange::new(0, 0), None, None, true, Vec::new())
    }

    /// `init(sourceRange:textKitRange:sourceOffset:textKitOffset:isCanonical:hiddenSourceRanges:)`.
    /// A `None` offset defaults to its range's location.
    pub fn with(
        source_range: NSRange,
        text_kit_range: NSRange,
        source_offset: Option<isize>,
        text_kit_offset: Option<isize>,
        is_canonical: bool,
        hidden_source_ranges: Vec<NSRange>,
    ) -> VisualDebuggerMapping {
        VisualDebuggerMapping {
            source_range,
            text_kit_range,
            source_offset: source_offset.unwrap_or(source_range.location),
            text_kit_offset: text_kit_offset.unwrap_or(text_kit_range.location),
            is_canonical,
            hidden_source_ranges,
        }
    }
}

/// `VisualDebuggerInput`. `new` gives Swift's defaults for `style`,
/// `mapping`, `render_target_report` and `assets`; set them directly.
#[derive(Clone, Debug)]
pub struct VisualDebuggerInput {
    pub document: Arc<ParsedDocument>,
    pub selection: NSRange,
    pub mode: RenderMode,
    pub style: VisualDebuggerStyleFacts,
    pub mapping: VisualDebuggerMapping,
    pub render_target_report: Option<CompatibilityReport>,
    pub assets: Vec<AssetDiagnostic>,
}

impl VisualDebuggerInput {
    /// `init(document:selection:mode:)`.
    pub fn new(document: Arc<ParsedDocument>, selection: NSRange, mode: RenderMode) -> VisualDebuggerInput {
        Self::with(document, selection, mode, VisualDebuggerStyleFacts::default(), None, None, Vec::new())
    }

    /// `init(document:selection:mode:style:mapping:renderTargetReport:assets:)`;
    /// a `None` mapping is `VisualDebuggerMapping(sourceRange: selection)`.
    pub fn with(
        document: Arc<ParsedDocument>,
        selection: NSRange,
        mode: RenderMode,
        style: VisualDebuggerStyleFacts,
        mapping: Option<VisualDebuggerMapping>,
        render_target_report: Option<CompatibilityReport>,
        assets: Vec<AssetDiagnostic>,
    ) -> VisualDebuggerInput {
        VisualDebuggerInput {
            document,
            selection,
            mode,
            style,
            mapping: mapping.unwrap_or_else(|| VisualDebuggerMapping::new(selection)),
            render_target_report,
            assets,
        }
    }
}

/// `VisualDebuggerBlockFact`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualDebuggerBlockFact {
    pub kind: String,
    pub range: NSRange,
    pub content_range: NSRange,
    pub depth: isize,
    pub quote_depth: isize,
    pub source: String,
}

/// `VisualDebuggerInlineFact`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualDebuggerInlineFact {
    pub kind: String,
    pub range: NSRange,
    pub content_range: NSRange,
    pub source: String,
}

/// `VisualDebuggerModel`.
#[derive(Clone, Debug)]
pub struct VisualDebuggerModel {
    pub source_selection: NSRange,
    pub focus_offset: isize,
    pub line: isize,
    pub column: isize,
    pub source_text: String,
    pub mode: RenderMode,
    pub block: Option<VisualDebuggerBlockFact>,
    pub inline: Option<VisualDebuggerInlineFact>,
    pub style: VisualDebuggerStyleFacts,
    pub mapping: VisualDebuggerMapping,
    pub render_target: Option<RenderTargetProfile>,
    pub render_diagnostics: Vec<CompatibilityDiagnostic>,
    pub asset_diagnostics: Vec<AssetDiagnostic>,
}

impl VisualDebuggerModel {
    /// `init(input:)`.
    pub fn new(input: &VisualDebuggerInput) -> VisualDebuggerModel {
        let document = &input.document;
        let length = document.length;
        let source = NSRange::new(
            0.max(input.selection.location.min(length)),
            0.max(input.selection.length.min(length - 0.max(input.selection.location.min(length)))),
        );
        let offset = source.location;
        let focus_range = if source.length > 0 { source } else { NSRange::new(offset, 0) };

        let focused_block = document.root.block_at(offset);
        let block = focused_block.as_ref().map(|block| VisualDebuggerBlockFact {
            kind: Self::block_kind(&block.content),
            range: block.range,
            content_range: block.content_range,
            depth: block.depth,
            quote_depth: block.quote_depth,
            source: document.substring(block.range),
        });
        let inline = focused_block.as_ref().and_then(|block| {
            let span = Self::inline_span(&block.inlines, offset)?;
            Some(VisualDebuggerInlineFact {
                kind: Self::inline_kind(&span.kind).to_owned(),
                range: span.range,
                content_range: span.content_range,
                source: document.substring(span.range),
            })
        });

        VisualDebuggerModel {
            source_selection: source,
            focus_offset: offset,
            line: document.line_at(offset),
            column: Self::column(offset, document),
            source_text: document.substring(focus_range),
            mode: input.mode,
            block,
            inline,
            style: input.style.clone(),
            mapping: input.mapping.clone(),
            render_target: input.render_target_report.as_ref().map(|report| report.profile.clone()),
            render_diagnostics: input
                .render_target_report
                .as_ref()
                .map(|report| {
                    report.diagnostics.iter().filter(|diagnostic| Self::intersects(diagnostic.range, source)).cloned().collect()
                })
                .unwrap_or_default(),
            asset_diagnostics: input
                .assets
                .iter()
                .filter(|diagnostic| {
                    Self::intersects(diagnostic.range, source) || Self::intersects(diagnostic.reference.image_range, source)
                })
                .cloned()
                .collect(),
        }
    }

    /// `summary`.
    pub fn summary(&self) -> String {
        let mut lines: Vec<String> = vec![
            "Upleft Visual Debugger".to_owned(),
            format!("Source range: {}", Self::range_text(self.source_selection)),
            format!("Location: line {}, column {}, UTF-16 offset {}", self.line, self.column, self.focus_offset),
            format!("Mode: {}", self.mode.title()),
            format!("TextKit range: {}", Self::range_text(self.mapping.text_kit_range)),
            format!("TextKit offset: {}", self.mapping.text_kit_offset),
            format!("Canonical source offset: {}", if self.mapping.is_canonical { "yes" } else { "no" }),
        ];
        if let Some(block) = &self.block {
            lines.extend([
                format!("Block: {}", block.kind),
                format!("Block range: {}", Self::range_text(block.range)),
                format!("Block content range: {}", Self::range_text(block.content_range)),
                format!("Block depth: {}, quote depth: {}", block.depth, block.quote_depth),
            ]);
        } else {
            lines.push("Block: none".to_owned());
        }
        if let Some(inline) = &self.inline {
            lines.extend([
                format!("Inline: {}", inline.kind),
                format!("Inline range: {}", Self::range_text(inline.range)),
                format!("Inline content range: {}", Self::range_text(inline.content_range)),
            ]);
        } else {
            lines.push("Inline: none".to_owned());
        }
        let family = if self.style.font_family.is_empty() { "unknown".to_owned() } else { self.style.font_family.clone() };
        let point_size =
            if self.style.point_size > 0.0 { format_double("%.1f pt", self.style.point_size) } else { "unknown".to_owned() };
        let alignment =
            if self.style.paragraph_alignment.is_empty() { "unknown".to_owned() } else { self.style.paragraph_alignment.clone() };
        let line_height =
            if self.style.line_height > 0.0 { format_double("%.1f pt", self.style.line_height) } else { "unknown".to_owned() };
        let line_spacing = format_double("%.1f pt", self.style.line_spacing);
        lines.extend([
            format!("Font: {family} {point_size}"),
            format!("Paragraph: {alignment}, line height {line_height}, spacing {line_spacing}"),
        ]);
        if !self.style.attributes.is_empty() {
            lines.push(format!("Visible attributes: {}", self.style.attributes.join(", ")));
        }
        if let Some(render_target) = &self.render_target {
            lines.push(format!("Render target: {}", render_target.name));
            if self.render_diagnostics.is_empty() {
                lines.push("Render diagnostics: none at selection".to_owned());
            } else {
                lines.extend(self.render_diagnostics.iter().map(|diagnostic| {
                    format!("Render diagnostic: {} [{}]", diagnostic.title, diagnostic.capability.raw_value())
                }));
            }
        } else {
            lines.push("Render target: none".to_owned());
        }
        if !self.asset_diagnostics.is_empty() {
            lines.extend(
                self.asset_diagnostics
                    .iter()
                    .map(|diagnostic| format!("Asset: {} [{}]", diagnostic.message, diagnostic.code.raw_value())),
            );
        }
        if !self.mapping.hidden_source_ranges.is_empty() {
            let ranges: Vec<String> = self.mapping.hidden_source_ranges.iter().map(|range| Self::range_text(*range)).collect();
            lines.push(format!("Hidden source ranges: {}", ranges.join(", ")));
        }
        lines.join("\n")
    }

    fn range_text(range: NSRange) -> String {
        format!("{}..<{} (length {})", range.location, range.upper_bound(), range.length)
    }

    fn column(offset: isize, document: &ParsedDocument) -> isize {
        let index = 0.max((document.line_starts.len() as isize - 1).min(document.line_at(offset) - 1));
        let line_start = document.line_starts[index as usize];
        1.max(offset - line_start + 1)
    }

    fn intersects(candidate: NSRange, selection: NSRange) -> bool {
        if selection.length == 0 {
            return candidate.touches(selection.location);
        }
        candidate.intersection(selection).is_some()
    }

    fn inline_span(spans: &[InlineSpan], offset: isize) -> Option<&InlineSpan> {
        for span in spans {
            if let Some(child) = Self::inline_span(&span.children, offset) {
                // Text leaves carry no useful syntax label. Report their
                // nearest semantic parent instead (strong, link, image, ...).
                if matches!(child.kind, InlineKind::Text) {
                    if matches!(span.kind, InlineKind::Text) {
                        return Some(child);
                    }
                    return Some(span);
                }
                return Some(child);
            }
            if span.range.touches(offset) {
                return Some(span);
            }
        }
        None
    }

    fn block_kind(content: &BlockContent) -> String {
        match content {
            BlockContent::Document => "document".to_owned(),
            BlockContent::Heading { level } => format!("heading H{level}"),
            BlockContent::Paragraph => "paragraph".to_owned(),
            BlockContent::BlockQuote => "blockquote".to_owned(),
            BlockContent::Callout { kind, .. } => format!("callout {}", kind.raw_value()),
            BlockContent::List { ordered, marker, .. } => {
                format!("{} list ({})", if *ordered { "ordered" } else { "unordered" }, marker.raw_value())
            }
            BlockContent::ListItem { ordinal, checkbox } => format!(
                "list item{}{}",
                ordinal.map(|ordinal| format!(" #{ordinal}")).unwrap_or_default(),
                if checkbox.is_none() { "" } else { " task" }
            ),
            BlockContent::CodeBlock { language, is_fenced, .. } => format!(
                "{} code{}",
                if *is_fenced { "fenced" } else { "indented" },
                language.as_ref().map(|language| format!(" ({language})")).unwrap_or_default()
            ),
            BlockContent::Mermaid { .. } => "mermaid".to_owned(),
            BlockContent::MathBlock { .. } => "display math".to_owned(),
            BlockContent::Table(_) => "table".to_owned(),
            BlockContent::ThematicBreak => "thematic break".to_owned(),
            BlockContent::HtmlBlock => "HTML block".to_owned(),
            BlockContent::FrontMatter(_) => "front matter".to_owned(),
            BlockContent::FootnoteDefinition { identifier } => format!("footnote definition [{identifier}]"),
        }
    }

    fn inline_kind(kind: &InlineKind) -> &'static str {
        match kind {
            InlineKind::Text => "text",
            InlineKind::Emphasis => "emphasis",
            InlineKind::Strong => "strong",
            InlineKind::Strikethrough => "strikethrough",
            InlineKind::InlineCode => "inline code",
            InlineKind::Link { .. } => "link",
            InlineKind::Autolink { .. } => "autolink",
            InlineKind::Wikilink { .. } => "wikilink",
            InlineKind::Image { .. } => "image",
            InlineKind::InlineMath { .. } => "inline math",
            InlineKind::PathToken(_) => "path token",
            InlineKind::FootnoteReference { .. } => "footnote reference",
            InlineKind::SoftBreak => "soft break",
            InlineKind::LineBreak => "line break",
            InlineKind::InlineHTML => "inline HTML",
        }
    }
}

/// `String(format:)` with one `Double`: Foundation formats it with the C
/// library (`%.1f` of NaN is `nan`, where Rust's `{:.1}` says `NaN`).
fn format_double(format: &str, value: f64) -> String {
    upleft_mermaid::swift::format_f64(format, value)
}
