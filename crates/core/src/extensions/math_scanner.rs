//! Extensions/MathScanner.swift — `$…$`, `$$…$$`, `\(…\)`, `\[…\]` (§4.1).
//!
//! The delimiter rules matter more than the parsing: a false positive turns
//! prose into a broken glyph. Runs only over `.text` inline spans.

use crate::ns_range::NSRange;
use crate::swift_text::{self, ns::NSStringExt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MathMatch {
    /// Whole match including delimiters.
    pub range: NSRange,
    /// The LaTeX between the delimiters.
    pub content_range: NSRange,
    pub is_display: bool,
}

pub struct MathScanner;

impl MathScanner {
    /// All math in `range` of `text`, ascending and non-overlapping.
    pub fn matches(text: &[u16], range: NSRange) -> Vec<MathMatch> {
        let mut out = Vec::new();
        let mut i = range.location;
        let end = range.upper_bound();

        while i < end {
            let ch = text.character_at(i);
            if ch == 0x5C {
                // backslash — `\(`, `\[`
                if let Some(found) = Self::escaped_delimiter(text, i, end) {
                    out.push(found);
                    i = found.range.upper_bound();
                    continue;
                }
                i += 2; // any other escape consumes its escapee
                continue;
            }
            if ch == 0x24
                && let Some(found) = Self::dollar(text, i, end, range.location)
            {
                out.push(found);
                i = found.range.upper_bound();
                continue;
            }
            i += 1;
        }
        out
    }

    /// `\(inline\)` and `\[display\]`.
    fn escaped_delimiter(text: &[u16], start: isize, end: isize) -> Option<MathMatch> {
        let next = text.character_safe_at(start + 1)?;
        let (is_display, closer) = match next {
            0x28 => (false, 0x29u16),
            0x5B => (true, 0x5Du16),
            _ => return None,
        };
        let mut i = start + 2;
        while i + 1 < end {
            if text.character_at(i) == 0x5C && text.character_at(i + 1) == closer {
                let mut backslash_count = 0;
                let mut p = i;
                while p >= start + 2 && text.character_at(p) == 0x5C {
                    backslash_count += 1;
                    p -= 1;
                }
                if backslash_count % 2 == 1 {
                    let content = NSRange::new(start + 2, i - (start + 2));
                    if !(content.length > 0) {
                        return None;
                    }
                    return Some(MathMatch { range: NSRange::new(start, i + 2 - start), content_range: content, is_display });
                }
            }
            i += 1;
        }
        None
    }

    /// `$inline$` and `$$display$$`, with the shell-hostile guard rails.
    fn dollar(text: &[u16], start: isize, end: isize, range_start: isize) -> Option<MathMatch> {
        // An escaped `\$` is not a delimiter.
        if start > range_start && text.character_at(start - 1) == 0x5C {
            return None;
        }

        let is_display = text.character_safe_at(start + 1) == Some(0x24);
        let delimiter_length = if is_display { 2 } else { 1 };
        // A digit immediately before the opener means we are inside a number.
        if let Some(previous) = text.character_safe_at(start - 1)
            && Self::is_digit(previous)
        {
            return None;
        }

        let mut i = start + delimiter_length;
        while i < end {
            let ch = text.character_at(i);
            if ch == 0x5C {
                i += 2;
                continue;
            }
            if ch == 0x24 {
                if is_display {
                    if text.character_safe_at(i + 1) != Some(0x24) {
                        i += 1;
                        continue;
                    }
                } else if text.character_safe_at(i + 1) == Some(0x24) {
                    // `$x$$` — not a plausible inline close.
                    return None;
                }
                let content = NSRange::new(start + delimiter_length, i - start - delimiter_length);
                let close_end = i + delimiter_length;
                if !Self::is_plausible(text, content, close_end, is_display) {
                    return None;
                }
                return Some(MathMatch { range: NSRange::new(start, close_end - start), content_range: content, is_display });
            }
            i += 1;
        }
        None
    }

    /// The rules that keep `echo $PATH`, `$5 and $10` and `$(cmd)` out.
    fn is_plausible(text: &[u16], content: NSRange, close_end: isize, is_display: bool) -> bool {
        if !(content.length > 0) {
            return false;
        }
        let body = text.substring(content);

        if is_display {
            return !swift_text::trim_whitespaces_and_newlines(&body).is_empty();
        }
        // `String.contains("\n")` is Character-wise: a CR LF is not a match.
        if swift_text::contains(&body, "\n") {
            return false;
        }
        let (Some(first), Some(last)) = (swift_text::first(&body), swift_text::last(&body)) else { return false };
        if swift_text::is_whitespace(first) || swift_text::is_whitespace(last) {
            return false;
        }
        // A bare amount: `$100$` is money in a table far more often than maths.
        if swift_text::all_satisfy(&body, |c| swift_text::is_number(c) || swift_text::char_is(c, '.') || swift_text::char_is(c, ',')) {
            return false;
        }
        // `$5$10` — a digit right after the closer means we split a number.
        if let Some(after) = text.character_safe_at(close_end)
            && Self::is_digit(after)
        {
            return false;
        }
        true
    }

    fn is_digit(ch: u16) -> bool {
        (0x30..=0x39).contains(&ch)
    }

    /// A whole-paragraph display block: `$$…$$` or `\[…\]` with nothing else
    /// around it.
    pub fn whole_block(text: &[u16], range: NSRange) -> Option<MathMatch> {
        let mut start = range.location;
        let mut end = range.upper_bound();
        while start < end && Self::is_space(text.character_at(start)) {
            start += 1;
        }
        while end > start && Self::is_space(text.character_at(end - 1)) {
            end -= 1;
        }
        if !(end > start) {
            return None;
        }
        let trimmed = NSRange::new(start, end - start);
        let found = *Self::matches(text, trimmed).first()?;
        if !(found.is_display && found.range == trimmed) {
            return None;
        }
        Some(found)
    }

    fn is_space(ch: u16) -> bool {
        ch == 0x20 || ch == 0x09 || ch == 0x0A || ch == 0x0D
    }
}
