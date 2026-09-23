//! Port of `Engine/HardWrapReflow.swift`: plans display-only substitutions for
//! soft Markdown line breaks. A replacement must preserve UTF-16 coordinates
//! before it is safe to publish.
//!
//! `text` is the storage string's UTF-16 units (`text as NSString` in Swift).

use objc2::rc::Retained;
use objc2_foundation::{NSAttributedString, NSString};
use upleft_core::ns_range::ns_intersection_range;
use upleft_core::{BlockContent, InlineKind, InlineSpan, MDBlock, NSRange, ParsedDocument};

use super::display_map::DisplaySubstitution;

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub ranges: Vec<NSRange>,
    pub substitutions: Vec<DisplaySubstitution>,
}

/// A soft break to join, together with the continuation line's own
/// indentation.
struct SoftBreak {
    terminator: NSRange,
    indents: Vec<NSRange>,
}

pub struct HardWrapReflow;

impl HardWrapReflow {
    pub fn plan(
        document: &ParsedDocument,
        text: &[u16],
        hidden_ranges: &[NSRange],
        excluded_ranges: &[NSRange],
        enabled: bool,
    ) -> Plan {
        if !enabled || text.is_empty() {
            return Plan::default();
        }

        let mut ranges: Vec<NSRange> = Vec::new();
        let mut substitutions: Vec<DisplaySubstitution> = Vec::new();
        document.root.walk(&mut |block| {
            if !matches!(block.content, BlockContent::Paragraph) {
                return;
            }
            if excluded_ranges
                .iter()
                .any(|excluded| ns_intersection_range(*excluded, block.range).length > 0)
            {
                return;
            }
            let element_range = extended_over_hidden_prefix(
                range_including_trailing_separator(block, text),
                text,
                hidden_ranges,
            );
            let soft_breaks = soft_break_ranges(block, element_range, text, hidden_ranges);
            if soft_breaks.is_empty() {
                return;
            }

            ranges.push(element_range);
            for soft_break in &soft_breaks {
                substitutions.push(DisplaySubstitution::replace_hard_wrap(
                    soft_break.terminator,
                    soft_break_replacement(
                        soft_break.terminator,
                        soft_break.terminator.location == element_range.location,
                    ),
                ));
            }
            // The indentation belongs to the *next* physical paragraph, so it
            // cannot ride along in the terminator's substitution — `DisplayMap`
            // refuses an entry that crosses a paragraph boundary.
            for soft_break in &soft_breaks {
                for indent in &soft_break.indents {
                    substitutions.push(DisplaySubstitution::replace_hard_wrap(
                        *indent,
                        zero_width_replacement(indent.length),
                    ));
                }
            }

            let joined: Vec<NSRange> = soft_breaks
                .iter()
                .flat_map(|soft_break| std::iter::once(soft_break.terminator).chain(soft_break.indents.iter().copied()))
                .collect();
            for hidden in hidden_ranges {
                if !(hidden.location >= element_range.location && hidden.upper_bound() <= element_range.upper_bound()) {
                    continue;
                }
                if joined.iter().any(|range| ns_intersection_range(*range, *hidden).length > 0) {
                    continue;
                }
                substitutions.push(DisplaySubstitution::replace_hidden(
                    *hidden,
                    zero_width_replacement(hidden.length),
                ));
            }
        });

        ranges.sort_by_key(|range| range.location);
        Plan { ranges, substitutions }
    }
}

/// The paragraph's hard line breaks that may be softened, in order. A break
/// inside an inline code, math, HTML or explicit-break span is *skipped*.
fn soft_break_ranges(block: &MDBlock, element_range: NSRange, text: &[u16], hidden_ranges: &[NSRange]) -> Vec<SoftBreak> {
    let mut result = Vec::new();
    let upper_bound = element_range.upper_bound().min(text.len() as isize);
    let mut cursor = 0.max(block.range.location);
    while cursor < upper_bound {
        let length = line_terminator_length(text, cursor);
        if length <= 0 {
            cursor += 1;
            continue;
        }
        let range = NSRange::new(cursor, length);
        if range.upper_bound() >= element_range.upper_bound() {
            break;
        }
        if is_protected(range, &block.inlines) {
            cursor = range.upper_bound();
            continue;
        }
        if is_explicit_break(range, text) {
            cursor = range.upper_bound();
            continue;
        }
        result.push(SoftBreak {
            terminator: range,
            indents: continuation_indents(range, element_range.upper_bound(), text, hidden_ranges),
        });
        cursor = range.upper_bound();
    }
    result
}

/// The indentation runs that open the continuation line, in order, stepping
/// over hidden line prefixes (a quote's `> `).
fn continuation_indents(terminator: NSRange, limit: isize, text: &[u16], hidden_ranges: &[NSRange]) -> Vec<NSRange> {
    let mut runs = Vec::new();
    // A stale parsed document can outlive the buffer it was parsed from, so
    // the caller's limit may run past `text.length`; clamp before indexing.
    let limit = limit.min(text.len() as isize);
    let mut cursor = terminator.upper_bound();
    while cursor < limit {
        if let Some(hidden) = hidden_ranges.iter().find(|hidden| hidden.location == cursor)
            && hidden.length > 0
        {
            cursor = hidden.upper_bound().min(limit);
            continue;
        }
        let start = cursor;
        while cursor < limit && is_horizontal_whitespace(text[cursor as usize]) {
            cursor += 1;
        }
        if cursor <= start {
            break;
        }
        runs.push(NSRange::new(start, cursor - start));
    }
    runs
}

#[inline]
fn is_horizontal_whitespace(character: u16) -> bool {
    character == 0x20 || character == 0x09
}

fn range_including_trailing_separator(block: &MDBlock, text: &[u16]) -> NSRange {
    let length = line_terminator_length(text, block.range.upper_bound());
    if !(length > 0 && block.range.upper_bound() + length <= text.len() as isize) {
        return block.range;
    }
    NSRange::new(block.range.location, block.range.length + length)
}

/// Extends a group backwards across a hidden line prefix (a list item's
/// `3. `), refused when the group already opens on a terminator.
fn extended_over_hidden_prefix(range: NSRange, text: &[u16], hidden_ranges: &[NSRange]) -> NSRange {
    let length = text.len() as isize;
    if !(range.location > 0 && range.location < length && line_terminator_length(text, range.location) == 0) {
        return range;
    }
    let start = line_start(range.location, text);
    if start >= range.location {
        return range;
    }
    let mut cursor = start;
    while cursor < range.location {
        if let Some(hidden) = hidden_ranges
            .iter()
            .find(|hidden| hidden.location <= cursor && hidden.upper_bound() > cursor)
        {
            cursor = hidden.upper_bound();
            continue;
        }
        if !is_horizontal_whitespace(text[cursor as usize]) {
            return range;
        }
        cursor += 1;
    }
    NSRange::new(start, range.upper_bound() - start)
}

/// The first character of the physical line containing `offset`.
fn line_start(offset: isize, text: &[u16]) -> isize {
    let mut cursor = offset.min(text.len() as isize);
    while cursor > 0 && line_terminator_length(text, cursor - 1) == 0 {
        cursor -= 1;
    }
    cursor
}

fn is_protected(range: NSRange, spans: &[InlineSpan]) -> bool {
    let mut protected = false;
    for span in spans {
        span.walk(&mut |span| {
            if !(span.range.location <= range.location && span.range.upper_bound() >= range.upper_bound()) {
                return;
            }
            if matches!(
                span.kind,
                InlineKind::InlineCode | InlineKind::InlineMath { .. } | InlineKind::InlineHTML | InlineKind::LineBreak
            ) {
                protected = true;
            }
        });
    }
    protected
}

fn line_terminator_length(text: &[u16], offset: isize) -> isize {
    if !(offset >= 0 && offset < text.len() as isize) {
        return 0;
    }
    let character = text[offset as usize];
    if !(character == 0x0A || character == 0x0D || character == 0x0085 || character == 0x2028 || character == 0x2029) {
        return 0;
    }
    if character == 0x0D && offset + 1 < text.len() as isize && text[(offset + 1) as usize] == 0x0A {
        return 2;
    }
    1
}

fn is_explicit_break(range: NSRange, text: &[u16]) -> bool {
    let before = range.location;
    if before >= 1 && text[(before - 1) as usize] == 0x5C {
        return true;
    }
    before >= 2 && text[(before - 1) as usize] == 0x20 && text[(before - 2) as usize] == 0x20
}

/// One space, padded to the terminator's own length so the substitution
/// preserves UTF-16 coordinates. A terminator that *opens* the element joins
/// nothing, so it contributes no space.
fn soft_break_replacement(range: NSRange, opens_element: bool) -> Retained<NSAttributedString> {
    if opens_element {
        return zero_width_replacement(range.length);
    }
    if range.length <= 1 {
        return NSAttributedString::from_nsstring(&NSString::from_str(" "));
    }
    let mut string = String::from(" ");
    for _ in 0..range.length - 1 {
        string.push('\u{200B}');
    }
    NSAttributedString::from_nsstring(&NSString::from_str(&string))
}

pub(crate) fn zero_width_replacement(length: isize) -> Retained<NSAttributedString> {
    let string: String = std::iter::repeat_n('\u{200B}', 1.max(length) as usize).collect();
    NSAttributedString::from_nsstring(&NSString::from_str(&string))
}
