//! TextDiff.swift — block-shaped text diff (§8.1).
//!
//! "Changed words inside a modified paragraph are highlighted in the rendered
//! prose — not as +/- source lines." So a `.modified` hunk carries
//! `word_ranges`: which *words* of the new text to mark. Deletions and
//! insertions that touch are merged into one `.modified` hunk, because a
//! paragraph the agent rewrote reads as one change.

use unicode_normalization::UnicodeNormalization;

use crate::contracts::{ChangeHunk, ChangeKind};
use crate::hashing::FNV;
use crate::myers::{Myers, Step};
use crate::ns_range::NSRange;
use crate::swift_text::ns::NSStringExt;

pub struct TextDiff;

impl TextDiff {
    pub fn hunks(old: &str, new: &str) -> Vec<ChangeHunk> {
        if string_eq(old, new) {
            return Vec::new();
        }
        let old_ns = utf16_units(old);
        let new_ns = utf16_units(new);
        let (old_lines, old_hashes) = lines_and_hashes(&old_ns);
        let (new_lines, new_hashes) = lines_and_hashes(&new_ns);

        let Some(script) = Myers::diff_default(&old_hashes, &new_hashes) else {
            // Beyond the distance cap the documents have nothing in common;
            // one whole-document hunk is both true and instant.
            return vec![ChangeHunk::new(
                ChangeKind::Modified,
                NSRange::new(0, new_ns.length()),
                NSRange::new(0, old_ns.length()),
                Vec::new(),
            )];
        };

        let mut state = HunkBuilder {
            old_ns: &old_ns,
            new_ns: &new_ns,
            hunks: Vec::new(),
            deleted: Vec::new(),
            inserted: Vec::new(),
            old_cursor: 0,
            new_cursor: 0,
        };

        for step in script {
            match step {
                Step::Equal { old_index, new_index } => {
                    state.flush();
                    state.old_cursor = old_lines[old_index as usize].upper_bound();
                    state.new_cursor = new_lines[new_index as usize].upper_bound();
                }
                Step::Delete { old_index } => {
                    let line = old_lines[old_index as usize];
                    state.deleted.push(line);
                    state.old_cursor = line.location;
                }
                Step::Insert { new_index } => {
                    let line = new_lines[new_index as usize];
                    state.inserted.push(line);
                    state.new_cursor = line.location;
                }
            }
        }
        state.flush();
        state.hunks
    }

    // MARK: Deletions

    /// A non-degenerate range in the new text for a hunk that has no new
    /// text. A pure deletion's `new_range` is empty, and an empty range is
    /// unrenderable, so anchor it on the single character at the join point
    /// (the character *after* the join, or the last character of the
    /// document when the deletion ran to the end).
    pub fn anchor_range(hunk: &ChangeHunk, length: isize) -> NSRange {
        if hunk.new_range.length > 0 {
            return hunk.new_range;
        }
        if length <= 0 {
            return NSRange::new(0, 0);
        }
        let location = 0.max(hunk.new_range.location).min(length);
        if location < length {
            return NSRange::new(location, 1);
        }
        NSRange::new(length - 1, 1)
    }

    // MARK: Line and word tokenisation

    /// Lines including their terminators, so a hunk's range covers whole
    /// lines and re-decoration never lands mid-line.
    pub fn lines(text: &[u16]) -> Vec<NSRange> {
        let mut out = Vec::new();
        let mut index = 0;
        while index < text.length() {
            let end = text.line_end_after(index);
            out.push(NSRange::new(index, end - index));
            index = end;
        }
        out
    }

    /// Word-level ranges *within the new text* that differ from the old.
    pub fn changed_words(old: &str, _old_range: NSRange, new: &str, new_range: NSRange) -> Vec<NSRange> {
        Self::changed_words_units(&utf16_units(old), &utf16_units(new), new_range)
    }

    /// `changedWords` over the UTF-16 units of the two substrings; `old_range`
    /// is not used by the Swift either.
    fn changed_words_units(old_ns: &[u16], new_ns: &[u16], new_range: NSRange) -> Vec<NSRange> {
        let old_words = Self::words(old_ns);
        let new_words = Self::words(new_ns);

        let old_hashes: Vec<u64> = old_words.iter().map(|&range| FNV::hash_range(old_ns, range)).collect();
        let new_hashes: Vec<u64> = new_words.iter().map(|&range| FNV::hash_range(new_ns, range)).collect();
        let Some(script) = Myers::diff(&old_hashes, &new_hashes, 2048) else {
            return vec![new_range];
        };

        let mut out = Vec::new();
        for step in script {
            if let Step::Insert { new_index } = step {
                let word = new_words[new_index as usize];
                out.push(NSRange::new(new_range.location + word.location, word.length));
            }
        }
        merge(out)
    }

    /// Every word of an inserted span, in coordinates of the whole new text,
    /// so the highlight skips newlines and punctuation.
    pub fn inserted_words(text: &[u16], span: NSRange) -> Vec<NSRange> {
        if span.length <= 0 {
            return Vec::new();
        }
        let body = &text[span.as_usize_range()];
        merge(
            Self::words(body)
                .into_iter()
                .map(|word| NSRange::new(span.location + word.location, word.length))
                .collect(),
        )
    }

    /// Words, punctuation excluded — matching on words rather than on runs of
    /// non-space keeps "the cat." vs "the cat!" to a one-token change.
    pub fn words(text: &[u16]) -> Vec<NSRange> {
        let mut out = Vec::new();
        let mut start: isize = -1;
        for (index, &ch) in text.iter().enumerate() {
            let index = index as isize;
            let is_word = (0x30..=0x39).contains(&ch)
                || (0x41..=0x5A).contains(&ch)
                || (0x61..=0x7A).contains(&ch)
                || ch >= 0x80
                || ch == 0x27
                || ch == 0x2D
                || ch == 0x5F;
            if is_word {
                if start < 0 {
                    start = index;
                }
            } else if start >= 0 {
                out.push(NSRange::new(start, index - start));
                start = -1;
            }
        }
        if start >= 0 {
            out.push(NSRange::new(start, text.length() - start));
        }
        out
    }
}

/// The `flush()` closure's captured state.
struct HunkBuilder<'a> {
    old_ns: &'a [u16],
    new_ns: &'a [u16],
    hunks: Vec<ChangeHunk>,
    deleted: Vec<NSRange>,
    inserted: Vec<NSRange>,
    old_cursor: isize,
    new_cursor: isize,
}

impl HunkBuilder<'_> {
    fn flush(&mut self) {
        if self.deleted.is_empty() && self.inserted.is_empty() {
            return;
        }
        let old_range =
            if self.deleted.is_empty() { NSRange::new(self.old_cursor, 0) } else { span(&self.deleted) };
        let new_range =
            if self.inserted.is_empty() { NSRange::new(self.new_cursor, 0) } else { span(&self.inserted) };

        if self.deleted.is_empty() {
            // Brand-new prose is the strongest signal on the page, so it gets
            // the same word-level highlight a rewritten paragraph gets.
            self.hunks.push(ChangeHunk::new(
                ChangeKind::Inserted,
                new_range,
                old_range,
                TextDiff::inserted_words(self.new_ns, new_range),
            ));
        } else if self.inserted.is_empty() {
            self.hunks.push(ChangeHunk::new(ChangeKind::Deleted, new_range, old_range, Vec::new()));
        } else {
            // `oldNS.substring(with:)` round-trips exactly: line ranges never
            // split a surrogate pair, so the slices are the substrings' units.
            self.hunks.push(ChangeHunk::new(
                ChangeKind::Modified,
                new_range,
                old_range,
                TextDiff::changed_words_units(
                    &self.old_ns[old_range.as_usize_range()],
                    &self.new_ns[new_range.as_usize_range()],
                    new_range,
                ),
            ));
        }
        self.deleted.clear();
        self.inserted.clear();
    }
}

fn span(ranges: &[NSRange]) -> NSRange {
    match (ranges.first(), ranges.last()) {
        (Some(first), Some(last)) => NSRange::new(first.location, last.upper_bound() - first.location),
        _ => NSRange::new(0, 0),
    }
}

/// Adjacent or near-adjacent word ranges become one highlight.
fn merge(ranges: Vec<NSRange>) -> Vec<NSRange> {
    if ranges.len() <= 1 {
        return ranges;
    }
    let mut out: Vec<NSRange> = Vec::with_capacity(ranges.len());
    out.push(ranges[0]);
    for &range in &ranges[1..] {
        let last = *out.last().unwrap();
        if range.location <= last.upper_bound() + 1 {
            *out.last_mut().unwrap() = last.union(range);
        } else {
            out.push(range);
        }
    }
    out
}

/// `lines(of:)` fused with the per-line `FNV.hash(_:range:)` the caller maps
/// over it: one pass over the units instead of two.
fn lines_and_hashes(text: &[u16]) -> (Vec<NSRange>, Vec<u64>) {
    let mut lines = Vec::new();
    let mut hashes = Vec::new();
    let length = text.len();
    let mut index = 0usize;
    while index < length {
        let start = index;
        let mut h = FNV::OFFSET_BASIS;
        // `lineEnd(after:)`: through LF, CR, or CR LF.
        while index < length {
            let c = text[index];
            h = FNV::combine_byte(FNV::combine_byte(h, c as u8), (c >> 8) as u8);
            index += 1;
            if c == 0x0A {
                break;
            }
            if c == 0x0D {
                if index < length && text[index] == 0x0A {
                    h = FNV::combine_byte(FNV::combine_byte(h, 0x0A), 0);
                    index += 1;
                }
                break;
            }
        }
        lines.push(NSRange::new(start as isize, (index - start) as isize));
        hashes.push(h);
    }
    (lines, hashes)
}

/// `text as NSString`, with a single-allocation path for ASCII text.
fn utf16_units(s: &str) -> Vec<u16> {
    if s.is_ascii() {
        return s.bytes().map(u16::from).collect();
    }
    let mut out = Vec::with_capacity(s.len());
    out.extend(s.encode_utf16());
    out
}

/// Swift `String ==` (canonical equivalence), without normalising the shared
/// prefix. Byte-identical text up to the first difference is canonically
/// identical, and an ASCII byte is always a normalisation boundary (ccc 0, no
/// composition with what precedes it), so the NFC comparison can start at the
/// last ASCII byte before the first difference. Same answer as
/// `swift_text::str_eq`; that one normalises from the start, which on a
/// non-ASCII document costs a full NFC pass over the shared prefix. (Also
/// used by `ASTDiff`; belongs in `swift_text`.)
pub(crate) fn string_eq(a: &str, b: &str) -> bool {
    // The same storage is equal without reading it, as Swift's `==` answers
    // for two references to one string buffer.
    if std::ptr::eq(a, b) || a == b {
        return true;
    }
    let (x, y) = (a.as_bytes(), b.as_bytes());
    let first_difference = common_prefix_length(x, y);
    // An ASCII byte both strings share (so a char boundary in each), or 0.
    let start = x[..first_difference].iter().rposition(u8::is_ascii).unwrap_or(0);
    a[start..].nfc().eq(b[start..].nfc())
}

fn common_prefix_length(x: &[u8], y: &[u8]) -> usize {
    let limit = x.len().min(y.len());
    let mut i = 0;
    while i + 16 <= limit && x[i..i + 16] == y[i..i + 16] {
        i += 16;
    }
    while i < limit && x[i] == y[i] {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swift_text;

    #[test]
    fn fused_line_hashes_match_lines_then_hash() {
        for text in ["", "a", "a\n", "a\r\nb\rc\n\n", "\r", "\r\r\n", "x\u{2028}y\n", "é\r\n😀"] {
            let units = utf16_units(text);
            let lines = TextDiff::lines(&units);
            let hashes: Vec<u64> = lines.iter().map(|&r| FNV::hash_range(&units, r)).collect();
            assert_eq!(lines_and_hashes(&units), (lines, hashes), "{text:?}");
        }
    }

    #[test]
    fn string_eq_matches_swift_text() {
        let cases = [
            ("", ""),
            ("abc", "abc"),
            ("abc", "abd"),
            ("é", "e\u{301}"),
            ("xxé!", "xxe\u{301}!"),
            ("xxé!", "xxe\u{301}?"),
            ("\u{212A}", "K"),
            ("a\u{212A}b", "aKb"),
            ("ab", "abc"),
            ("日本語 a", "日本語 b"),
            ("日本語", "日本誤"),
            ("e\u{301}\u{323}", "e\u{323}\u{301}"),
            ("a\u{323}\u{301}", "a\u{301}\u{323}"),
        ];
        for (a, b) in cases {
            assert_eq!(string_eq(a, b), swift_text::str_eq(a, b), "{a:?} vs {b:?}");
            assert_eq!(string_eq(b, a), swift_text::str_eq(b, a), "{b:?} vs {a:?}");
        }
    }

    #[test]
    fn string_eq_agrees_with_str_eq_on_random_combining_text() {
        const ALPHABET: [&str; 14] =
            ["a", "e", "é", "e\u{301}", "\u{301}", "\u{323}", "\u{302}", "ệ", "\u{212B}", "Å", "A\u{30A}", " ", "\r\n", "\u{FEFF}"];
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = |bound: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % bound as u64) as usize
        };
        for _ in 0..20_000 {
            let a: String = (0..next(7)).map(|_| ALPHABET[next(ALPHABET.len())]).collect();
            let b: String = if next(2) == 0 {
                (0..next(7)).map(|_| ALPHABET[next(ALPHABET.len())]).collect()
            } else {
                // A shared prefix, then independent tails.
                let tail: String = (0..next(3)).map(|_| ALPHABET[next(ALPHABET.len())]).collect();
                a[..a.char_indices().map(|(i, _)| i).nth(next(4)).unwrap_or(a.len())].to_owned() + &tail
            };
            assert_eq!(string_eq(&a, &b), swift_text::str_eq(&a, &b), "{a:?} vs {b:?}");
        }
    }
}
