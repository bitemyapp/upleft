//! `NSString` semantics over a UTF-16 buffer.
//!
//! Where Downright writes `text as NSString` and indexes it, Upleft holds the
//! text as `[u16]` and uses [`NSStringExt`]. Offsets are UTF-16 code units,
//! exactly as `NSString.character(at:)` counts them.

use crate::ns_range::{NSRange, NS_NOT_FOUND};

/// UTF-16 code units of `s` (`s as NSString`).
#[inline]
pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// `String(utf16CodeUnits:)` / `NSString.substring` bridged back to Swift:
/// a lone surrogate becomes U+FFFD, as Swift's native `String` repairs it.
#[inline]
pub fn string_from_utf16(units: &[u16]) -> String {
    if units.iter().all(|&u| u < 0x80) {
        // SAFETY: every unit is ASCII.
        return units.iter().map(|&u| u as u8 as char).collect();
    }
    String::from_utf16_lossy(units)
}

pub trait NSStringExt {
    /// `NSString.length`.
    fn length(&self) -> isize;
    /// `character(at:)`. Out of range raises in Foundation; here it panics.
    fn character_at(&self, index: isize) -> u16;
    /// SourcePositions.swift's `character(safeAt:)`.
    fn character_safe_at(&self, index: isize) -> Option<u16>;
    /// `substring(with:)`.
    fn substring(&self, range: NSRange) -> String;
    /// `substring(from:)`.
    fn substring_from(&self, location: isize) -> String;
    /// `substring(to:)`.
    fn substring_to(&self, location: isize) -> String;
    /// SourcePositions.swift's `lineStart(before:)`.
    fn line_start_before(&self, offset: isize) -> isize;
    /// SourcePositions.swift's `lineEnd(after:)`.
    fn line_end_after(&self, offset: isize) -> isize;
    /// `lineRange(for:)`: whole lines including their terminators.
    fn line_range_for(&self, range: NSRange) -> NSRange;
    /// `getLineStart(_:end:contentsEnd:for:)`.
    fn line_bounds(&self, range: NSRange) -> (isize, isize, isize);
    /// `range(of:options: .literal, range:)`: code-unit comparison.
    fn range_of_literal(&self, needle: &[u16], range: NSRange) -> NSRange;
    /// `range(of: "<", options: .literal, range:)` for a single unit.
    fn find_unit(&self, unit: u16, range: NSRange) -> Option<isize>;
}

impl NSStringExt for [u16] {
    #[inline]
    fn length(&self) -> isize {
        self.len() as isize
    }

    #[inline]
    fn character_at(&self, index: isize) -> u16 {
        self[index as usize]
    }

    #[inline]
    fn character_safe_at(&self, index: isize) -> Option<u16> {
        if index >= 0 && (index as usize) < self.len() { Some(self[index as usize]) } else { None }
    }

    #[inline]
    fn substring(&self, range: NSRange) -> String {
        string_from_utf16(&self[range.as_usize_range()])
    }

    #[inline]
    fn substring_from(&self, location: isize) -> String {
        string_from_utf16(&self[location as usize..])
    }

    #[inline]
    fn substring_to(&self, location: isize) -> String {
        string_from_utf16(&self[..location as usize])
    }

    fn line_start_before(&self, offset: isize) -> isize {
        let mut i = offset.min(self.length());
        while i > 0 {
            let c = self[(i - 1) as usize];
            if c == 0x0A || c == 0x0D {
                break;
            }
            i -= 1;
        }
        i
    }

    fn line_end_after(&self, offset: isize) -> isize {
        let length = self.length();
        let mut i = offset.max(0);
        while i < length {
            let c = self[i as usize];
            i += 1;
            if c == 0x0A {
                break;
            }
            if c == 0x0D {
                if i < length && self[i as usize] == 0x0A {
                    i += 1;
                }
                break;
            }
        }
        i
    }

    fn line_range_for(&self, range: NSRange) -> NSRange {
        let (start, end, _) = self.line_bounds(range);
        NSRange::new(start, end - start)
    }

    fn line_bounds(&self, range: NSRange) -> (isize, isize, isize) {
        // CFStringGetLineBounds. Terminators: LF, CR, CR LF, NEL, LS, PS.
        let length = self.length();
        let at = |i: isize| -> u16 { if i >= 0 && i < length { self[i as usize] } else { 0 } };
        let is_terminator = |u: u16| matches!(u, 0x0A | 0x0D | 0x85 | 0x2028 | 0x2029);

        let start = if range.location == 0 {
            0
        } else {
            let mut index = range.location;
            // Between the CR and LF of one CR LF: the line began before the CR.
            if at(index - 1) == 0x0D && at(index) == 0x0A {
                index -= 1;
            }
            loop {
                if index == 0 {
                    break 0;
                }
                if is_terminator(at(index - 1)) {
                    break index;
                }
                index -= 1;
            }
        };

        // The last unit of the range, or the unit at an empty range.
        let mut index = range.location + range.length - if range.length > 0 { 1 } else { 0 };
        let mut c = at(index);
        let contents_end;
        let mut separator_length = 1;
        if index < length && c == 0x0A {
            let mut e = index;
            if index > 0 && at(index - 1) == 0x0D {
                separator_length = 2;
                e -= 1;
            }
            contents_end = e;
        } else {
            loop {
                if index >= length {
                    contents_end = length;
                    separator_length = 0;
                    break;
                }
                if is_terminator(c) {
                    contents_end = index;
                    if c == 0x0D && index + 1 < length && at(index + 1) == 0x0A {
                        separator_length = 2;
                    }
                    break;
                }
                index += 1;
                c = at(index);
            }
        }
        (start, contents_end + separator_length, contents_end)
    }

    fn range_of_literal(&self, needle: &[u16], range: NSRange) -> NSRange {
        if needle.is_empty() || range.length < needle.len() as isize {
            return NSRange::new(NS_NOT_FOUND, 0);
        }
        let hay = &self[range.as_usize_range()];
        match hay.windows(needle.len()).position(|w| w == needle) {
            Some(p) => NSRange::new(range.location + p as isize, needle.len() as isize),
            None => NSRange::new(NS_NOT_FOUND, 0),
        }
    }

    #[inline]
    fn find_unit(&self, unit: u16, range: NSRange) -> Option<isize> {
        self[range.as_usize_range()].iter().position(|&u| u == unit).map(|p| range.location + p as isize)
    }
}

/// Calls into Foundation itself, for the few `NSString` operations whose
/// Unicode rules (composed character sequences, case folding, canonical
/// equivalence) are exact only when Foundation does them.
pub mod foundation {
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_foundation::{NSComparisonResult, NSRange as FRange, NSString, NSStringCompareOptions};

    use crate::ns_range::{NSRange, NS_NOT_FOUND};

    /// `s as NSString`. Built from UTF-16 units: `NSString::from_str` decodes
    /// UTF-8 and would drop a leading U+FEFF, which a Swift string keeps.
    fn ns(s: &str) -> Retained<NSString> {
        if s.is_ascii() {
            return NSString::from_str(s);
        }
        ns_from_utf16(&super::utf16(s))
    }

    pub fn ns_from_utf16(units: &[u16]) -> Retained<NSString> {
        // `-initWithCharacters:length:` keeps lone surrogates as they are.
        unsafe {
            NSString::initWithCharacters_length(
                NSString::alloc(),
                std::ptr::NonNull::new(units.as_ptr() as *mut u16).unwrap_or(std::ptr::NonNull::dangling()),
                units.len(),
            )
        }
    }

    /// `String(ns)`: the string's UTF-16 units, lone surrogates repaired.
    ///
    /// Read through CoreFoundation (NSString is toll-free bridged to
    /// CFString) rather than `-getCharacters:range:`, whose method encoding
    /// objc2's debug checks reject on Swift-implemented NSString subclasses.
    pub fn to_string(s: &NSString) -> String {
        let cf: &objc2_core_foundation::CFString = unsafe { &*(s as *const NSString as *const objc2_core_foundation::CFString) };
        let length = cf.length();
        let mut units = vec![0u16; length as usize];
        if length > 0 {
            unsafe { cf.characters(objc2_core_foundation::CFRange::new(0, length), units.as_mut_ptr()) };
        }
        super::string_from_utf16(&units)
    }

    pub fn replacing_occurrences(s: &str, target: &str, replacement: &str) -> String {
        objc2::rc::autoreleasepool(|_| {
            let result = ns(s).stringByReplacingOccurrencesOfString_withString(&ns(target), &ns(replacement));
            to_string(&result)
        })
    }

    pub fn components_separated_by(s: &str, separator: &str) -> Vec<String> {
        objc2::rc::autoreleasepool(|_| {
            let parts = ns(s).componentsSeparatedByString(&ns(separator));
            parts.iter().map(|part| to_string(&part)).collect()
        })
    }

    /// `range(of:) != nil` (non-literal) on an `NSString`.
    pub fn contains(s: &str, needle: &str) -> bool {
        objc2::rc::autoreleasepool(|_| ns(s).rangeOfString(&ns(needle)).location != objc2_foundation::NSNotFound as usize)
    }

    pub fn case_insensitive_compare(a: &str, b: &str) -> std::cmp::Ordering {
        objc2::rc::autoreleasepool(|_| match ns(a).caseInsensitiveCompare(&ns(b)) {
            NSComparisonResult::Ascending => std::cmp::Ordering::Less,
            NSComparisonResult::Descending => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
    }

    /// `localizedStandardCompare(_:)`: Finder-style ordering in the current
    /// locale (case- and diacritic-insensitive, numbers compared by value).
    pub fn localized_standard_compare(a: &str, b: &str) -> std::cmp::Ordering {
        objc2::rc::autoreleasepool(|_| match ns(a).localizedStandardCompare(&ns(b)) {
            NSComparisonResult::Ascending => std::cmp::Ordering::Less,
            NSComparisonResult::Descending => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
    }

    pub fn capitalized(s: &str) -> String {
        objc2::rc::autoreleasepool(|_| to_string(&ns(s).capitalizedString()))
    }

    /// `range(of:options:range:)` on a UTF-16 buffer, through Foundation.
    pub fn range_of(text: &NSString, needle: &str, options: NSStringCompareOptions, range: NSRange) -> NSRange {
        let found = text.rangeOfString_options_range(&ns(needle), options, FRange::new(range.location as usize, range.length as usize));
        if found.location == objc2_foundation::NSNotFound as usize {
            NSRange::new(NS_NOT_FOUND, 0)
        } else {
            NSRange::new(found.location as isize, found.length as isize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lr(s: &str, location: isize, length: isize) -> NSRange {
        utf16(s).as_slice().line_range_for(NSRange::new(location, length))
    }

    #[test]
    fn line_range_matches_foundation() {
        // Expectations recorded from `NSString.lineRange(for:)`.
        assert_eq!(lr("ab\ncd\n", 0, 0), NSRange::new(0, 3));
        assert_eq!(lr("ab\ncd\n", 3, 0), NSRange::new(3, 3));
        assert_eq!(lr("ab\ncd", 4, 0), NSRange::new(3, 2));
        assert_eq!(lr("ab\r\ncd", 0, 0), NSRange::new(0, 4));
        assert_eq!(lr("ab\ncd\n", 6, 0), NSRange::new(6, 0));
        assert_eq!(lr("ab\ncd\nef", 1, 3), NSRange::new(0, 6));
    }
}
