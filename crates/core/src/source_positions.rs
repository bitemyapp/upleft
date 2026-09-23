//! SourcePositions.swift — the line index and position conversion.
//!
//! swift-markdown reports 1-based lines and 1-based UTF-8 byte columns;
//! every range MarkdownCore publishes is a UTF-16 `NSRange`. `SourceMap` is
//! the only place that conversion happens. The conversion from a
//! swift-markdown `SourceRange` (`range(_:lineOffset:)`) lands with the parser
//! port; everything else is here.
//!
//! The file's small `NSString` and `String` helpers live in
//! [`crate::swift_text`] (`line_start_before`, `line_end_after`,
//! `character_safe_at`, `leading_indent`, `indent_columns`, `is_blank_line`)
//! and are re-exported below.

pub use crate::swift_text::ns::NSStringExt;
pub use crate::swift_text::{indent_columns, is_blank_line, leading_indent};

use crate::ns_range::NSRange;
use crate::swift_text::ns::{string_from_utf16, utf16};

/// Line index over a document plus the line/column → UTF-16 conversion.
///
/// Lines are split on LF, CRLF and lone CR, matching CommonMark.
#[derive(Clone, Debug)]
pub struct SourceMap {
    /// The document as UTF-16 code units (`string as NSString`).
    pub text: Vec<u16>,
    pub length: isize,
    /// UTF-16 offset of the first character of each line.
    pub line_starts: Vec<isize>,
    /// UTF-16 offset just past each line's last character, terminator excluded.
    pub line_ends: Vec<isize>,
    /// A pure-ASCII line lets a UTF-8 byte offset be used directly as a
    /// UTF-16 offset.
    line_is_ascii: Vec<bool>,
    /// True when the source contains at least one `<`.
    pub may_contain_html: bool,
}

impl SourceMap {
    pub fn new(string: &str) -> SourceMap {
        let text = utf16(string);
        let length = text.len() as isize;

        let mut starts: Vec<isize> = vec![0];
        let mut ends: Vec<isize> = Vec::new();
        let mut ascii: Vec<bool> = Vec::new();
        let mut offset: isize = 0;
        let mut line_ascii = true;
        let mut saw_cr = false;
        let mut saw_angle_bracket = false;

        for scalar in string.chars() {
            let width: isize = if scalar as u32 > 0xFFFF { 2 } else { 1 };
            if saw_cr {
                saw_cr = false;
                if scalar == '\n' {
                    // CRLF: the line already ended at the CR; the next one
                    // starts after the LF.
                    starts.push(offset + width);
                    offset += width;
                    continue;
                }
                starts.push(offset); // a lone CR ended the previous line
            }
            match scalar {
                '\n' => {
                    ends.push(offset);
                    ascii.push(line_ascii);
                    line_ascii = true;
                    starts.push(offset + width);
                }
                '\r' => {
                    ends.push(offset);
                    ascii.push(line_ascii);
                    line_ascii = true;
                    saw_cr = true;
                }
                '<' => saw_angle_bracket = true,
                _ => {
                    if !scalar.is_ascii() {
                        line_ascii = false;
                    }
                }
            }
            offset += width;
        }
        if saw_cr {
            starts.push(offset);
        }
        ends.push(offset);
        ascii.push(line_ascii);

        SourceMap { text, length, line_starts: starts, line_ends: ends, line_is_ascii: ascii, may_contain_html: saw_angle_bracket }
    }

    #[inline]
    pub fn line_count(&self) -> isize {
        self.line_starts.len() as isize
    }

    /// 0-based index of the line containing `offset`.
    pub fn line_containing(&self, offset: isize) -> isize {
        let (mut lo, mut hi, mut best) = (0isize, self.line_starts.len() as isize - 1, 0isize);
        while lo <= hi {
            let mid = (lo + hi) / 2;
            if self.line_starts[mid as usize] <= offset {
                best = mid;
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        best
    }

    /// UTF-16 offset for a 1-based line and 1-based UTF-8 byte column.
    pub fn offset(&self, line: isize, column: isize) -> isize {
        let index = line - 1;
        if index < 0 {
            return 0;
        }
        if index >= self.line_starts.len() as isize {
            return self.length;
        }
        let start = self.line_starts[index as usize];
        let end = self.line_ends[index as usize];
        let byte_offset = 0.max(column - 1);
        if byte_offset == 0 {
            return start;
        }
        if self.line_is_ascii[index as usize] {
            return end.min(start + byte_offset);
        }

        let mut utf16_offset = start;
        let mut bytes: isize = 0;
        let line_text = string_from_utf16(&self.text[start as usize..end as usize]);
        for scalar in line_text.chars() {
            if bytes >= byte_offset {
                break;
            }
            bytes += SourceMap::utf8_width(scalar);
            utf16_offset += if scalar as u32 > 0xFFFF { 2 } else { 1 };
        }
        end.min(utf16_offset)
    }

    /// Range of line `index` (0-based), terminator excluded.
    pub fn content_range_of_line(&self, index: isize) -> NSRange {
        if !(index >= 0 && index < self.line_starts.len() as isize) {
            return NSRange::new(self.length, 0);
        }
        let i = index as usize;
        NSRange::new(self.line_starts[i], self.line_ends[i] - self.line_starts[i])
    }

    /// Range of line `index` (0-based) including its terminator.
    pub fn full_range_of_line(&self, index: isize) -> NSRange {
        if !(index >= 0 && index < self.line_starts.len() as isize) {
            return NSRange::new(self.length, 0);
        }
        let i = index as usize;
        let end = if i + 1 < self.line_starts.len() { self.line_starts[i + 1] } else { self.length };
        NSRange::new(self.line_starts[i], end - self.line_starts[i])
    }

    /// `string(ofLine:)`.
    pub fn string_of_line(&self, index: isize) -> String {
        self.text.as_slice().substring(self.content_range_of_line(index))
    }

    pub fn utf8_width(scalar: char) -> isize {
        match scalar as u32 {
            0..0x80 => 1,
            0x80..0x800 => 2,
            0x800..0x1_0000 => 3,
            _ => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lf_crlf_and_lone_cr() {
        let map = SourceMap::new("a\nb\r\nc\rd");
        assert_eq!(map.line_starts, vec![0, 2, 5, 7]);
        assert_eq!(map.line_ends, vec![1, 3, 6, 8]);
        assert_eq!(map.string_of_line(2), "c");
    }

    #[test]
    fn converts_utf8_columns() {
        let map = SourceMap::new("é😀x\nab");
        // é is 2 bytes, 😀 4 bytes: column 7 is `x`.
        assert_eq!(map.offset(1, 7), 3);
        assert_eq!(map.offset(2, 2), 6);
        assert_eq!(map.offset(9, 1), map.length);
    }
}
