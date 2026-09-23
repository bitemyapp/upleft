//! Compatibility/CompatibilityDiagnostics.swift — checking a parsed document
//! against a renderer profile.
//!
//! The report types are `Codable` in Swift; nothing in MarkdownCore encodes
//! them (see `render_target`), so the encoding is not ported.

use crate::compatibility::render_target::{MarkdownCapabilities, MarkdownCapability, RenderTargetProfile};
use crate::extensions::math_scanner::MathScanner;
use crate::model::{BlockContent, InlineKind, InlineSpan, MDBlock, ParsedDocument};
use crate::ns_range::NSRange;
use crate::swift_text::{
    self, CharSet,
    ns::{NSStringExt, string_from_utf16, utf16},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompatibilitySeverity {
    Warning,
}

impl CompatibilitySeverity {
    pub const ALL_CASES: [CompatibilitySeverity; 1] = [CompatibilitySeverity::Warning];

    pub fn raw_value(&self) -> &'static str {
        match self {
            CompatibilitySeverity::Warning => "warning",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<CompatibilitySeverity> {
        CompatibilitySeverity::ALL_CASES.into_iter().find(|severity| severity.raw_value() == raw)
    }
}

/// A source-local, deterministic explanation of one unsupported construct.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CompatibilityDiagnostic {
    pub id: String,
    pub capability: MarkdownCapability,
    pub range: NSRange,
    pub severity: CompatibilitySeverity,
    pub title: String,
    pub explanation: String,
    pub proposal: Option<CompatibilityTransformProposal>,
}

impl CompatibilityDiagnostic {
    /// `init(capability:range:severity:title:explanation:proposal:)`; Swift
    /// defaults `severity` to `.warning` and `proposal` to `nil`.
    pub fn new(
        capability: MarkdownCapability,
        range: NSRange,
        severity: CompatibilitySeverity,
        title: impl Into<String>,
        explanation: impl Into<String>,
        proposal: Option<CompatibilityTransformProposal>,
    ) -> CompatibilityDiagnostic {
        CompatibilityDiagnostic {
            id: format!("{}:{}:{}", capability.raw_value(), range.location, range.length),
            capability,
            range,
            severity,
            title: title.into(),
            explanation: explanation.into(),
            proposal,
        }
    }
}

/// An optional byte-local edit. `reverse_replacement` makes the proposal
/// reversible without retaining document state or normalizing line endings.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CompatibilityTransformProposal {
    pub range: NSRange,
    pub replacement: String,
    pub reverse_replacement: String,
    pub summary: String,
}

impl CompatibilityTransformProposal {
    pub fn new(
        range: NSRange,
        replacement: impl Into<String>,
        reverse_replacement: impl Into<String>,
        summary: impl Into<String>,
    ) -> CompatibilityTransformProposal {
        CompatibilityTransformProposal {
            range,
            replacement: replacement.into(),
            reverse_replacement: reverse_replacement.into(),
            summary: summary.into(),
        }
    }

    pub fn applying(&self, text: &str) -> Option<String> {
        let mut output = utf16(text);
        if !(self.range.location >= 0 && self.range.upper_bound() <= output.as_slice().length()) {
            return None;
        }
        output.splice(self.range.as_usize_range(), self.replacement.encode_utf16());
        Some(string_from_utf16(&output))
    }

    pub fn reversing(&self, transformed_text: &str) -> Option<String> {
        let transformed_range = NSRange::new(self.range.location, swift_text::utf16_count(&self.replacement));
        let mut output = utf16(transformed_text);
        if !(transformed_range.location >= 0 && transformed_range.upper_bound() <= output.as_slice().length()) {
            return None;
        }
        // `substring(with:) == replacement` compares Swift Strings.
        if !swift_text::str_eq(&output.as_slice().substring(transformed_range), &self.replacement) {
            return None;
        }
        output.splice(transformed_range.as_usize_range(), self.reverse_replacement.encode_utf16());
        Some(string_from_utf16(&output))
    }
}

/// The result of checking one parsed document against one renderer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CompatibilityReport {
    pub profile: RenderTargetProfile,
    pub diagnostics: Vec<CompatibilityDiagnostic>,
}

impl CompatibilityReport {
    pub fn new(profile: RenderTargetProfile, diagnostics: Vec<CompatibilityDiagnostic>) -> CompatibilityReport {
        CompatibilityReport { profile, diagnostics }
    }
}

/// Side-by-side compatibility result. The source side is retained as a
/// profile rather than a rendered copy; rendering belongs to MarkdownRender.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderTargetComparison {
    pub source: RenderTargetProfile,
    pub target: RenderTargetProfile,
    pub source_capabilities: MarkdownCapabilities,
    pub target_capabilities: MarkdownCapabilities,
    pub only_in_source: MarkdownCapabilities,
    pub only_in_target: MarkdownCapabilities,
    pub report: CompatibilityReport,
}

impl RenderTargetComparison {
    pub fn new(source: RenderTargetProfile, target: RenderTargetProfile, report: CompatibilityReport) -> RenderTargetComparison {
        RenderTargetComparison {
            source_capabilities: source.capabilities,
            target_capabilities: target.capabilities,
            only_in_source: source.capabilities.subtracting(target.capabilities),
            only_in_target: target.capabilities.subtracting(source.capabilities),
            source,
            target,
            report,
        }
    }
}

type Finding = (MarkdownCapability, NSRange, &'static str, &'static str, Option<CompatibilityTransformProposal>);

pub struct MarkdownCompatibility;

impl MarkdownCompatibility {
    pub fn diagnose(document: &ParsedDocument, profile: &RenderTargetProfile) -> CompatibilityReport {
        let capabilities = profile.capabilities;
        // Whether `(document.text as NSString).substring(with:)` hands back
        // bridged strings, whose `contains` is Foundation's search.
        let bridged = swift_text::bridges_substrings(&document.utf16);
        let mut findings: Vec<Finding> = Vec::new();

        fn add(
            findings: &mut Vec<Finding>,
            document: &ParsedDocument,
            capability: MarkdownCapability,
            range: NSRange,
            title: &'static str,
            explanation: &'static str,
            proposal: Option<CompatibilityTransformProposal>,
        ) {
            if !(range.location >= 0 && range.length >= 0 && range.upper_bound() <= document.length) {
                return;
            }
            if findings.iter().any(|f| f.0 == capability && f.1 == range) {
                return;
            }
            findings.push((capability, range, title, explanation, proposal));
        }

        fn inline_features(
            span: &InlineSpan,
            findings: &mut Vec<Finding>,
            document: &ParsedDocument,
            capabilities: MarkdownCapabilities,
        ) {
            match &span.kind {
                InlineKind::Strikethrough => {
                    if !capabilities.contains(MarkdownCapabilities::STRIKETHROUGH) {
                        add(
                            findings,
                            document,
                            MarkdownCapability::Strikethrough,
                            span.range,
                            "Strikethrough is not supported",
                            "This renderer treats `~~text~~` as literal text.",
                            None,
                        );
                    }
                }
                InlineKind::InlineMath { .. } => {
                    if !capabilities.contains(MarkdownCapabilities::MATH) {
                        add(
                            findings,
                            document,
                            MarkdownCapability::Math,
                            span.range,
                            "Inline math is not supported",
                            "The `$\u{2026}$` or escaped math expression will not be rendered as mathematics.",
                            None,
                        );
                    }
                }
                InlineKind::Wikilink { target, label } => {
                    if !capabilities.contains(MarkdownCapabilities::WIKILINKS) {
                        let shown = label.as_deref().unwrap_or(target);
                        // `rangeOfCharacter(from: .whitespacesAndNewlines) == nil`
                        let destination = if !target.chars().any(|c| CharSet::WhitespacesAndNewlines.contains(c)) {
                            target.clone()
                        } else {
                            format!("<{target}>")
                        };
                        let replacement = format!("[{shown}]({destination})");
                        let proposal = CompatibilityTransformProposal::new(
                            span.range,
                            replacement,
                            document.utf16.as_slice().substring(span.range),
                            "Convert wikilink to a standard Markdown link",
                        );
                        add(
                            findings,
                            document,
                            MarkdownCapability::Wikilinks,
                            span.range,
                            "Wikilinks are not supported",
                            "The `[[target]]` syntax will remain visible instead of becoming a link.",
                            Some(proposal),
                        );
                    }
                }
                InlineKind::FootnoteReference { .. } => {
                    if !capabilities.contains(MarkdownCapabilities::FOOTNOTES) {
                        add(
                            findings,
                            document,
                            MarkdownCapability::Footnotes,
                            span.range,
                            "Footnotes are not supported",
                            "Footnote references will not resolve in this renderer.",
                            None,
                        );
                    }
                }
                InlineKind::InlineHTML => {
                    if !capabilities.contains(MarkdownCapabilities::RAW_HTML) {
                        add(
                            findings,
                            document,
                            MarkdownCapability::RawHTML,
                            span.range,
                            "Raw HTML is not supported",
                            "Inline HTML is shown as literal text or removed by the renderer.",
                            None,
                        );
                    }
                }
                _ => {}
            }
            for child in &span.children {
                inline_features(child, findings, document, capabilities);
            }
        }

        document.root.walk(&mut |block| {
            match &block.content {
                BlockContent::Table(_) => {
                    if !capabilities.contains(MarkdownCapabilities::TABLES) {
                        add(
                            &mut findings,
                            document,
                            MarkdownCapability::Tables,
                            block.range,
                            "Tables are not supported",
                            "The pipe table will not be laid out as a table.",
                            None,
                        );
                    }
                }
                BlockContent::ListItem { checkbox: Some(checkbox), .. } => {
                    if !capabilities.contains(MarkdownCapabilities::TASK_LISTS) {
                        add(
                            &mut findings,
                            document,
                            MarkdownCapability::TaskLists,
                            checkbox.mark_range,
                            "Task lists are not supported",
                            "The checkbox marker will be rendered as ordinary list text.",
                            None,
                        );
                    }
                }
                BlockContent::MathBlock { .. } => {
                    if !capabilities.contains(MarkdownCapabilities::MATH) {
                        add(
                            &mut findings,
                            document,
                            MarkdownCapability::Math,
                            block.range,
                            "Display math is not supported",
                            "The display formula will not be rendered as mathematics.",
                            None,
                        );
                    }
                }
                BlockContent::Mermaid { .. } => {
                    if !capabilities.contains(MarkdownCapabilities::MERMAID) {
                        add(
                            &mut findings,
                            document,
                            MarkdownCapability::Mermaid,
                            block.range,
                            "Mermaid is not supported",
                            "The fenced diagram will remain a code block or plain text.",
                            None,
                        );
                    }
                }
                BlockContent::Callout { .. } => {
                    if !capabilities.contains(MarkdownCapabilities::CALLOUTS_ALERTS)
                        && let Some(marker) = block.marker_range
                    {
                        add(
                            &mut findings,
                            document,
                            MarkdownCapability::CalloutsAlerts,
                            marker,
                            "Callouts or alerts are not supported",
                            "The callout marker will be treated as an ordinary blockquote.",
                            None,
                        );
                    }
                }
                BlockContent::HtmlBlock => {
                    if !capabilities.contains(MarkdownCapabilities::RAW_HTML) {
                        add(
                            &mut findings,
                            document,
                            MarkdownCapability::RawHTML,
                            block.range,
                            "Raw HTML is not supported",
                            "The HTML block will not be interpreted by the renderer.",
                            None,
                        );
                    }
                }
                BlockContent::FrontMatter(front_matter) => {
                    if !capabilities.contains(MarkdownCapabilities::FRONT_MATTER) {
                        add(
                            &mut findings,
                            document,
                            MarkdownCapability::FrontMatter,
                            front_matter.range,
                            "Front matter is not supported",
                            "The metadata fence and fields are not interpreted by this renderer.",
                            None,
                        );
                    }
                }
                _ => {}
            }
            if matches!(block.content, BlockContent::Heading { .. })
                && !capabilities.contains(MarkdownCapabilities::HEADING_ATTRIBUTES)
                && let Some(range) = Self::heading_attributes(document, block, bridged)
            {
                add(
                    &mut findings,
                    document,
                    MarkdownCapability::HeadingAttributes,
                    range,
                    "Heading attributes are not supported",
                    "The `{#id .class}` heading attribute will remain literal text.",
                    None,
                );
            }
            for inline in &block.inlines {
                inline_features(inline, &mut findings, document, capabilities);
            }
        });

        if !capabilities.contains(MarkdownCapabilities::STRIKETHROUGH) {
            for range in Self::strikethrough_ranges(document, bridged) {
                add(
                    &mut findings,
                    document,
                    MarkdownCapability::Strikethrough,
                    range,
                    "Strikethrough is not supported",
                    "This renderer treats `~~text~~` as literal text.",
                    None,
                );
            }
        }

        if !capabilities.contains(MarkdownCapabilities::MATH) {
            for range in Self::inline_math_ranges(document, bridged) {
                add(
                    &mut findings,
                    document,
                    MarkdownCapability::Math,
                    range,
                    "Inline math is not supported",
                    "The math expression will not be rendered as mathematics.",
                    None,
                );
            }
        }

        if !capabilities.contains(MarkdownCapabilities::FOOTNOTES) {
            // Swift iterates `document.footnotes.values` in Dictionary order,
            // which is random per process. Definitions have distinct ranges,
            // so the sort below erases the order; walk them by position to
            // stay deterministic regardless.
            let mut definitions: Vec<NSRange> = document.footnotes.values().map(|definition| definition.range).collect();
            definitions.sort_by(|a, b| a.location.cmp(&b.location).then(a.length.cmp(&b.length)));
            for range in definitions {
                add(
                    &mut findings,
                    document,
                    MarkdownCapability::Footnotes,
                    range,
                    "Footnotes are not supported",
                    "Footnote definitions will not resolve in this renderer.",
                    None,
                );
            }
        }

        // Stable, like Swift's `sorted(by:)`.
        findings.sort_by(|lhs, rhs| {
            if lhs.1.location == rhs.1.location {
                lhs.0.raw_value().cmp(rhs.0.raw_value())
            } else {
                lhs.1.location.cmp(&rhs.1.location)
            }
        });
        let diagnostics = findings
            .into_iter()
            .map(|(capability, range, title, explanation, proposal)| {
                CompatibilityDiagnostic::new(capability, range, CompatibilitySeverity::Warning, title, explanation, proposal)
            })
            .collect();
        CompatibilityReport::new(profile.clone(), diagnostics)
    }

    pub fn compare(document: &ParsedDocument, source: &RenderTargetProfile, target: &RenderTargetProfile) -> RenderTargetComparison {
        RenderTargetComparison::new(source.clone(), target.clone(), Self::diagnose(document, target))
    }

    fn extension_protected_ranges(document: &ParsedDocument) -> Vec<NSRange> {
        let mut ranges: Vec<NSRange> = Vec::new();
        document.root.walk(&mut |block| {
            match block.content {
                BlockContent::FrontMatter(_)
                | BlockContent::CodeBlock { .. }
                | BlockContent::Mermaid { .. }
                | BlockContent::MathBlock { .. }
                | BlockContent::HtmlBlock => ranges.push(block.range),
                _ => {}
            }
            for inline in &block.inlines {
                inline.walk(&mut |span| match span.kind {
                    InlineKind::InlineCode
                    | InlineKind::Link { .. }
                    | InlineKind::Autolink { .. }
                    | InlineKind::Image { .. }
                    | InlineKind::InlineHTML
                    | InlineKind::Wikilink { .. } => ranges.push(span.range),
                    _ => {}
                });
            }
        });
        ranges
    }

    fn strikethrough_ranges(document: &ParsedDocument, bridged: bool) -> Vec<NSRange> {
        let protected = Self::extension_protected_ranges(document);
        let text = document.utf16.as_slice();
        let length = text.length();
        let mut result: Vec<NSRange> = Vec::new();
        let mut cursor = 0isize;
        while cursor + 3 < length {
            if !(text.character_at(cursor) == 0x7E
                && text.character_at(cursor + 1) == 0x7E
                && (cursor == 0 || text.character_at(cursor - 1) != 0x7E)
                && text.character_at(cursor + 2) != 0x7E
                && !protected.iter().any(|range| range.contains(cursor)))
            {
                cursor += 1;
                continue;
            }
            let mut close = cursor + 2;
            while close + 1 < length {
                if text.character_at(close) == 0x7E
                    && text.character_at(close + 1) == 0x7E
                    && (close + 2 == length || text.character_at(close + 2) != 0x7E)
                    && !protected.iter().any(|range| range.contains(close))
                {
                    break;
                }
                close += 1;
            }
            if !(close + 1 < length) {
                break;
            }
            let body = text.substring(NSRange::new(cursor + 2, close - cursor - 2));
            // `body` is a substring of the document's `NSString`: Swift's
            // `contains("\n")` is Character-wise (a CR LF holds no "\n") on a
            // native string, Foundation's search on a bridged one.
            if !body.is_empty()
                && !swift_text::contains_with(&body, "\n", bridged)
                && !swift_text::trim_whitespaces(&body).is_empty()
            {
                result.push(NSRange::new(cursor, close + 2 - cursor));
                cursor = close + 2;
            } else {
                cursor += 2;
            }
        }
        result
    }

    fn inline_math_ranges(document: &ParsedDocument, bridged: bool) -> Vec<NSRange> {
        let protected = Self::extension_protected_ranges(document);
        MathScanner::matches_bridged(&document.utf16, NSRange::new(0, document.length), Some(bridged))
            .into_iter()
            .map(|m| m.range)
            .filter(|&candidate| !protected.iter().any(|range| range.intersection(candidate).is_some()))
            .collect()
    }

    /// `bridged`: whether the document's substrings are bridged; `line` is
    /// one, so the `NSString` substring of it is too.
    fn heading_attributes(document: &ParsedDocument, heading: &MDBlock, bridged: bool) -> Option<NSRange> {
        // `document.substring(heading.range) as NSString`: empty when the
        // range is out of bounds.
        let range = heading.range;
        let ns: &[u16] = if range.location >= 0 && range.upper_bound() <= document.length {
            &document.utf16[range.as_usize_range()]
        } else {
            &[]
        };
        let mut end = ns.length();
        while end > 0 {
            let character = ns.character_at(end - 1);
            if !(character == 0x20 || character == 0x09) {
                break;
            }
            end -= 1;
        }
        if !(end > 1 && ns.character_at(end - 1) == 0x7D) {
            return None;
        }
        let start = (0..end).rev().find(|&i| ns.character_at(i) == 0x7B)?;
        let body = ns.substring(NSRange::new(start + 1, end - start - 2));
        if !(swift_text::contains_with(&body, "#", bridged)
            || swift_text::contains_with(&body, ".", bridged)
            || swift_text::contains_with(&body, "=", bridged))
        {
            return None;
        }
        Some(NSRange::new(heading.range.location + start, end - start))
    }
}

impl MarkdownCapability {
    /// CompatibilityDiagnostics.swift's `displayName` extension.
    pub fn display_name(&self) -> &'static str {
        match self {
            MarkdownCapability::Tables => "Tables",
            MarkdownCapability::TaskLists => "Task lists",
            MarkdownCapability::Strikethrough => "Strikethrough",
            MarkdownCapability::Footnotes => "Footnotes",
            MarkdownCapability::Math => "Math",
            MarkdownCapability::Mermaid => "Mermaid",
            MarkdownCapability::CalloutsAlerts => "Callouts/alerts",
            MarkdownCapability::Wikilinks => "Wikilinks",
            MarkdownCapability::FrontMatter => "Front matter",
            MarkdownCapability::RawHTML => "Raw HTML",
            MarkdownCapability::HeadingAttributes => "Heading attributes",
        }
    }
}
