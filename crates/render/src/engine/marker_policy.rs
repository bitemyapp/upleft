//! Port of `Engine/MarkerPolicy.swift`: which syntax markers are omitted from
//! the display string, given a policy and a caret (§6.1a, §6.1b, §14).
//!
//! Pure functions over the parsed document — no view state, no storage — so the
//! rules can be unit tested exactly as specified and the Quick Look extension
//! gets identical behaviour without importing a view.

use upleft_core::ns_range::ns_intersection_range;
use upleft_core::safe_html::SafeHTMLKind;
use upleft_core::{BlockContent, InlineSpan, MDBlock, NSRange, ParsedDocument};

use super::display_map::RangeSet;
use crate::render_contracts::DecorationPolicy;

pub struct MarkerPolicy;

impl MarkerPolicy {
    /// Ranges to omit, ascending and non-overlapping.
    ///
    ///  * **Block markers** (§6.1a) are hidden in Read and Live and rendered in
    ///    the gutter instead; never revealed inline.
    ///  * **Inline markers** (§6.1b) are hidden per span; when the caret
    ///    touches a span, only *that span's* markers (and its ancestors') come
    ///    back.
    ///  * **Multiple carets** (§14): reveal at the primary caret only.
    pub fn hidden_ranges(
        document: &ParsedDocument,
        policy: DecorationPolicy,
        caret: Option<isize>,
        selections: &[NSRange],
    ) -> Vec<NSRange> {
        if !(policy.hides_block_markers || policy.hides_inline_markers) {
            return Vec::new();
        }

        let mut out: Vec<NSRange> = Vec::with_capacity(256);

        if policy.hides_block_markers {
            // Reference and footnote definitions are document metadata, not
            // body prose. Display substitutions cannot cross a physical
            // paragraph boundary, so split multi-line definitions first.
            // (Dictionary order is irrelevant: `disjoint` sorts.)
            let definitions = document
                .link_references
                .values()
                .map(|reference| reference.range)
                .chain(document.footnotes.values().map(|footnote| footnote.range));
            for definition in definitions {
                out.extend(paragraph_local_ranges(definition, document));
            }
        }

        // Selection is observation, not edit intent. Only a real insertion
        // caret reveals inline markers.
        let reveal_anchors = anchors(policy, caret, selections);

        document.root.walk(&mut |block| {
            if policy.hides_inline_markers
                && let Some(html) = &block.safe_html
                && html.is_safe
                && !reveal_anchors.iter().any(|anchor| touches(*anchor, block.range))
            {
                for annotation in &html.annotations {
                    // Image tags become native image fragments over their
                    // source range, so hiding the range as a marker would also
                    // hide the fragment's display substitution.
                    match annotation.kind {
                        SafeHTMLKind::Image { .. } | SafeHTMLKind::Inert => continue,
                        _ => {}
                    }
                    out.extend_from_slice(&annotation.tag_ranges);
                }
            }
            if policy.hides_block_markers && block_markers_are_hidden(block) {
                if let Some(m) = block.marker_range
                    && m.length > 0
                {
                    out.push(m);
                    out.extend(container_indent_range(m, block, document));
                }
                if let Some(m) = block.trailing_marker_range
                    && m.length > 0
                {
                    out.push(m);
                }
                if matches!(block.content, BlockContent::BlockQuote | BlockContent::Callout { .. }) {
                    // Hide each physical `>` prefix explicitly; swift-markdown
                    // gives only the opening line a marker range.
                    out.extend(quote_prefix_ranges(block, document));
                }
            }
            if !(policy.hides_inline_markers && !block.inlines.is_empty()) {
                return;
            }
            for span in &block.inlines {
                collect_inline_markers(span, &reveal_anchors, &mut out);
            }
        });
        // `disjoint`, not `normalized`: a marker has to keep its own range so a
        // caret reveal can name it.
        RangeSet::disjoint(&out)
    }

    /// Exactly the ranges `hidden_ranges` leaves out because of the caret — the
    /// complement of a caret-aware run against the fully collapsed one.
    pub fn revealed_marker_ranges(
        document: &ParsedDocument,
        policy: DecorationPolicy,
        caret: Option<isize>,
        selections: &[NSRange],
    ) -> Vec<NSRange> {
        if !(policy.hides_inline_markers && policy.reveals_at_caret) {
            return Vec::new();
        }
        let anchors = anchors(policy, caret, selections);
        if anchors.is_empty() {
            return Vec::new();
        }

        // A lone insertion caret can identify its deepest block directly.
        if anchors.len() == 1 {
            let anchor = anchors[0];
            if let Some(block) = document.root.block_at(anchor.location)
                && anchor.location > block.range.location
                && anchor.location < block.range.upper_bound()
            {
                let mut out = Vec::new();
                for span in &block.inlines {
                    collect_revealed(span, &anchors, &mut out);
                }
                return RangeSet::normalized(&out);
            }
        }

        let mut out = Vec::new();
        document.root.walk_pruning(&mut |block| {
            if !anchors.iter().any(|anchor| touches(*anchor, block.range)) {
                return false;
            }
            for span in &block.inlines {
                collect_revealed(span, &anchors, &mut out);
            }
            true
        });
        RangeSet::normalized(&out)
    }
}

/// The container indentation in front of a nested item's marker. Only
/// whitespace is ever taken, and only on the marker's own line.
fn container_indent_range(marker: NSRange, block: &MDBlock, document: &ParsedDocument) -> Option<NSRange> {
    if !(matches!(block.content, BlockContent::ListItem { .. }) && marker.location > 0) {
        return None;
    }
    let text = &document.utf16;
    let mut start = marker.location;
    while start > 0 {
        let character = text[(start - 1) as usize];
        if !(character == 0x20 || character == 0x09) {
            break;
        }
        start -= 1;
    }
    if start >= marker.location {
        return None;
    }
    // Refuse unless the run reaches the line start: anything else means the
    // marker is not what opens this line.
    if !(start == 0 || is_line_terminator(text[(start - 1) as usize])) {
        return None;
    }
    Some(NSRange::new(start, marker.location - start))
}

#[inline]
fn is_line_terminator(character: u16) -> bool {
    character == 0x0A || character == 0x0D || character == 0x0085 || character == 0x2028 || character == 0x2029
}

fn quote_prefix_ranges(block: &MDBlock, document: &ParsedDocument) -> Vec<NSRange> {
    if block.range.length <= 0 {
        return Vec::new();
    }
    let text = &document.utf16;
    let at = |i: isize| text[i as usize];
    let mut result = Vec::new();
    let line_starts = &document.line_starts;

    for (index, &line_start) in line_starts.iter().enumerate() {
        // `where lineStart < block.range.upperBound`: line starts ascend, so
        // no later line can pass the filter.
        if line_start >= block.range.upper_bound() {
            break;
        }
        let line_end = if index + 1 < line_starts.len() { line_starts[index + 1] } else { document.length };
        if !(line_end > block.range.location && line_start >= block.range.location) {
            continue;
        }

        let mut cursor = line_start;
        while cursor < line_end {
            let character = at(cursor);
            if !(character == 0x20 || character == 0x09) {
                break;
            }
            cursor += 1;
        }
        let marker_start = cursor;
        let mut saw_quote = false;
        while cursor < line_end {
            while cursor < line_end {
                let c = at(cursor);
                if !(c == 0x20 || c == 0x09) {
                    break;
                }
                cursor += 1;
            }
            if !(cursor < line_end && at(cursor) == 0x3E) {
                break;
            }
            saw_quote = true;
            cursor += 1;
        }
        if saw_quote {
            while cursor < line_end {
                let c = at(cursor);
                if !(c == 0x20 || c == 0x09) {
                    break;
                }
                cursor += 1;
            }
        }
        if saw_quote && cursor > marker_start {
            result.push(NSRange::new(marker_start, cursor - marker_start));
        }
    }
    result
}

fn paragraph_local_ranges(source_range: NSRange, document: &ParsedDocument) -> Vec<NSRange> {
    if source_range.length <= 0 {
        return Vec::new();
    }
    let mut result = Vec::new();
    for index in 0..document.line_starts.len() {
        let line = document.range_of_line(index as isize + 1);
        if line.location >= source_range.upper_bound() {
            break;
        }
        let intersection = ns_intersection_range(line, source_range);
        if intersection.length > 0 {
            result.push(intersection);
        }
    }
    result
}

/// Front matter and fenced code keep their delimiters in the source text:
/// their fragments absorb those lines as chrome rather than hiding characters.
fn block_markers_are_hidden(block: &MDBlock) -> bool {
    match block.content {
        BlockContent::CodeBlock { .. }
        | BlockContent::Mermaid { .. }
        | BlockContent::FrontMatter(_)
        | BlockContent::MathBlock { .. }
        | BlockContent::Table(_)
        | BlockContent::ThematicBreak => false,
        // Callout syntax is block chrome too.
        BlockContent::Callout { .. } => true,
        _ => true,
    }
}

fn collect_inline_markers(span: &InlineSpan, anchors: &[NSRange], out: &mut Vec<NSRange>) {
    let reveals = span.kind.reveals_markers();
    let revealed = reveals && anchors.iter().any(|anchor| touches(*anchor, span.range));
    if reveals && !revealed {
        for m in [span.leading_marker_range, span.trailing_marker_range].into_iter().flatten() {
            if m.length > 0 {
                out.push(m);
            }
        }
    }
    // Descend regardless: a collapsed outer span can contain a revealed inner
    // one only when the caret is inside, and then the outer is revealed too.
    for child in &span.children {
        collect_inline_markers(child, anchors, out);
    }
}

#[inline]
fn touches(anchor: NSRange, span: NSRange) -> bool {
    if anchor.length == 0 {
        return span.touches(anchor.location);
    }
    anchor.location <= span.upper_bound() && span.location <= anchor.upper_bound()
}

fn anchors(policy: DecorationPolicy, caret: Option<isize>, selections: &[NSRange]) -> Vec<NSRange> {
    if !policy.reveals_at_caret {
        return Vec::new();
    }
    if policy.reveals_at_all_cursors {
        let mut out: Vec<NSRange> = selections.iter().copied().filter(|s| s.length == 0).collect();
        if let Some(caret) = caret
            && !out.iter().any(|s| s.location == caret)
        {
            out.push(NSRange::new(caret, 0));
        }
        return out;
    }
    caret.map(|caret| vec![NSRange::new(caret, 0)]).unwrap_or_default()
}

fn collect_revealed(span: &InlineSpan, anchors: &[NSRange], out: &mut Vec<NSRange>) {
    if span.kind.reveals_markers() && anchors.iter().any(|anchor| touches(*anchor, span.range)) {
        for m in [span.leading_marker_range, span.trailing_marker_range].into_iter().flatten() {
            if m.length > 0 {
                out.push(m);
            }
        }
    }
    for child in &span.children {
        collect_revealed(child, anchors, out);
    }
}
