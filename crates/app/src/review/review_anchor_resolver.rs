//! Port of `Sources/DownrightApp/Review/ReviewAnchorResolver.swift`.
//!
//! Positions are UTF-16 offsets, and the text is searched through `NSString`
//! as in Swift: `range(of:options:range:)` with no options is Foundation's
//! non-literal search, so it is called through objc2 rather than
//! reimplemented. Substrings are read the way a bridged `NSString` becomes a
//! Swift `String` (an unpaired surrogate reads as U+FFFD), and compared with
//! Swift's `==` (canonical equivalence).

use objc2::rc::Retained;
use objc2_foundation::{NSRange as FRange, NSString, NSStringCompareOptions};
use upleft_core::ns_range::NSRange;

use crate::review::review_sidecar::{ReviewAnchor, ReviewAnchorStatus, ReviewResolution};

/// `NSNotFound`.
const NOT_FOUND: usize = isize::MAX as usize;

/// A string as `NSString` sees it: its UTF-16 units, and the object itself
/// for Foundation's searches.
pub(crate) struct Source {
    pub units: Vec<u16>,
    pub ns: Retained<NSString>,
}

impl Source {
    pub fn new(text: &str) -> Source {
        Source { units: text.encode_utf16().collect(), ns: NSString::from_str(text) }
    }

    /// `(text as NSString).length`.
    pub fn length(&self) -> isize {
        self.units.len() as isize
    }

    /// `source.substring(with: range)` as a Swift `String`.
    pub fn substring(&self, range: NSRange) -> String {
        String::from_utf16_lossy(&self.units[range.as_usize_range()])
    }
}

/// `ReviewAnchorResolver`.
pub struct ReviewAnchorResolver;

/// The default `contextLength`.
pub const CONTEXT_LENGTH: isize = 48;

impl ReviewAnchorResolver {
    /// `makeAnchor(in:range:contextLength:)`.
    pub fn make_anchor(text: &str, range: NSRange, context_length: isize) -> Option<ReviewAnchor> {
        let source = Source::new(text);
        if !(range.location >= 0 && range.length > 0 && range.upper_bound() <= source.length()) {
            return None;
        }
        let before_start = 0.max(range.location - context_length);
        let before = source.substring(NSRange::new(before_start, range.location - before_start));
        let after_end = source.length().min(range.upper_bound() + context_length);
        let after = source.substring(NSRange::new(range.upper_bound(), after_end - range.upper_bound()));
        Some(ReviewAnchor {
            range,
            selected_text: source.substring(range),
            before_fingerprint: Self::fingerprint(&before),
            after_fingerprint: Self::fingerprint(&after),
        })
    }

    /// `resolve(_:in:contextLength:)`.
    pub fn resolve(anchor: &ReviewAnchor, text: &str, context_length: isize) -> ReviewResolution {
        let source = Source::new(text);
        let selected_length = upleft_swift_text::utf16_count(&anchor.selected_text);
        if selected_length <= 0 {
            return ReviewResolution { status: ReviewAnchorStatus::Orphan, range: None };
        }

        // The range is decoded from an adjacent, repository-controlled JSON
        // file. Never return its length unless it agrees with the selected
        // text, and avoid arithmetic on an attacker-supplied Int.
        if anchor.range.length == selected_length && Self::matches(anchor, anchor.range.location, &source, context_length) {
            return ReviewResolution { status: ReviewAnchorStatus::Exact, range: Some(anchor.range) };
        }

        let needle = NSString::from_str(&anchor.selected_text);
        let mut cursor: isize = 0;
        let mut found_selected = false;
        while cursor < source.length() {
            let found = source.ns.rangeOfString_options_range(
                &needle,
                NSStringCompareOptions::empty(),
                FRange::new(cursor as usize, (source.length() - cursor) as usize),
            );
            if found.location == NOT_FOUND {
                break;
            }
            let found = NSRange::new(found.location as isize, found.length as isize);
            found_selected = true;
            if Self::matches(anchor, found.location, &source, context_length) {
                return ReviewResolution { status: ReviewAnchorStatus::Shifted, range: Some(found) };
            }
            cursor = found.location + 1.max(found.length);
        }
        ReviewResolution {
            status: if found_selected { ReviewAnchorStatus::Stale } else { ReviewAnchorStatus::Orphan },
            range: None,
        }
    }

    /// FNV-1a over the UTF-8 bytes, printed as `String(hash, radix: 16)`.
    pub fn fingerprint(value: &str) -> String {
        let mut hash: u64 = 14_695_981_039_346_656_037;
        for byte in value.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(1_099_511_628_211);
        }
        format!("{hash:x}")
    }

    fn matches(anchor: &ReviewAnchor, location: isize, source: &Source, context_length: isize) -> bool {
        let selected_length = upleft_swift_text::utf16_count(&anchor.selected_text);
        if !(location >= 0 && selected_length <= source.length() && location <= source.length() - selected_length) {
            return false;
        }
        if !upleft_swift_text::str_eq(&source.substring(NSRange::new(location, selected_length)), &anchor.selected_text) {
            return false;
        }
        let before_start = 0.max(location - context_length);
        let before = source.substring(NSRange::new(before_start, location - before_start));
        let after_location = location + selected_length;
        let after_end = source.length().min(after_location + context_length);
        let after = source.substring(NSRange::new(after_location, after_end - after_location));
        Self::fingerprint(&before) == anchor.before_fingerprint && Self::fingerprint(&after) == anchor.after_fingerprint
    }
}
