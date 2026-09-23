//! Extensions/PathTokenScanner.swift — path tokens (§8.4).
//!
//! Finds `src/foo.ts`, `src/foo.ts:42`, `./x/y.md` and anything path-shaped
//! inside an inline code span. It never touches the filesystem; the app
//! decides what exists. False positives are expensive (underlining `and/or`
//! trains the user to ignore the signal), so the shape rules are conservative
//! in prose and relaxed inside a code span. Every shape decision runs on the
//! raw UTF-16 buffer; only a matching token pays for a `String`.

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::model::PathToken;
use crate::ns_range::NSRange;
use crate::swift_text::{self, ns::NSStringExt};

pub struct PathTokenScanner;

/// `PathTokenScanner.Match`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub range: NSRange,
    pub token: PathToken,
}

impl PathTokenScanner {
    /// Extensions common enough in agent output that seeing one is sufficient
    /// evidence on its own, even without a `/`.
    pub const KNOWN_EXTENSIONS: &'static [&'static str] = &[
        "ts", "tsx", "js", "jsx", "mjs", "cjs", "swift", "py", "rb", "go", "rs", "java", "kt", "kts", "c", "h", "cc", "cpp", "hpp", "m",
        "mm", "cs", "php", "pl", "lua", "r", "scala", "clj", "ex", "sh", "bash", "zsh", "fish", "ps1", "bat", "json", "yaml", "yml",
        "toml", "ini", "cfg", "conf", "env", "lock", "properties", "md", "mdx", "mdc", "markdown", "rst", "txt", "adoc", "html",
        "htm", "css", "scss", "sass", "less", "vue", "svelte", "astro", "sql", "graphql", "gql", "proto", "xml", "plist",
        "entitlements", "pbxproj", "xcconfig", "gradle", "cmake", "mk", "dockerfile", "gitignore", "npmrc", "editorconfig", "png",
        "jpg", "jpeg", "gif", "svg", "webp", "pdf", "csv", "tsv",
    ];

    /// `knownExtensions.contains(ext)`: a `Set<String>`, so membership is
    /// canonical equivalence.
    pub fn is_known_extension(ext: &str) -> bool {
        static SET: LazyLock<HashSet<&'static str>> = LazyLock::new(|| PathTokenScanner::KNOWN_EXTENSIONS.iter().copied().collect());
        if SET.contains(ext) {
            return true;
        }
        !ext.is_ascii() && Self::KNOWN_EXTENSIONS.iter().any(|known| swift_text::str_eq(known, ext))
    }

    /// Scans prose. Requires a clear path shape (see `is_path_shaped`).
    pub fn matches(text: &[u16], range: NSRange) -> Vec<Match> {
        let mut out = Vec::new();
        let mut i = range.location;
        let end = range.upper_bound();
        while i < end {
            if !Self::is_token_character(text.character_at(i)) {
                i += 1;
                continue;
            }
            // Only start a token at a word boundary, so `abc/def` inside a
            // longer run is considered as a whole rather than from the middle.
            if i > range.location && Self::is_token_character(text.character_at(i - 1)) {
                i += 1;
                continue;
            }

            let mut j = i;
            while j < end && Self::is_token_character(text.character_at(j)) {
                j += 1;
            }
            let raw = NSRange::new(i, j - i);
            if let Some(found) = Self::evaluate(text, raw, false) {
                out.push(found);
            }
            i = j;
        }
        out
    }

    /// Scans the contents of an inline code span, where a single `/` or a
    /// known extension is enough (§4.1: `` `path/to/file` ``).
    pub fn code_span_match(text: &[u16], range: NSRange) -> Option<Match> {
        Self::evaluate(text, range, true)
    }

    fn evaluate(text: &[u16], raw: NSRange, relaxed: bool) -> Option<Match> {
        let mut range = raw;
        // Trim sentence punctuation the token picked up on the way past.
        while range.length > 0 && Self::is_trailing_punctuation(text.character_at(range.upper_bound() - 1)) {
            range.length -= 1;
        }
        if !(range.length > 1) {
            return None;
        }

        // `file.ts:42` and `file.ts:42:8`: split the location suffix off first
        // so the shape rules see just the path.
        let (path_range, line, column) = Self::strip_location_suffix(text, range);
        if !(path_range.length > 0 && !Self::is_url(text, path_range)) {
            return None;
        }
        if !Self::is_path_shaped(text, path_range, relaxed) {
            return None;
        }

        // `range` deliberately keeps the `:42` suffix: the app underlines and
        // opens the whole token, and only `raw_path` needs the path.
        let body = text.substring(path_range);
        Some(Match { range, token: PathToken::new(body, line, column) })
    }

    /// Splits a trailing `:line` or `:line:column` suffix off `range`. Only a
    /// suffix whose last colon-delimited segment is a positive integer is
    /// split, so inner colons in a path are preserved.
    fn strip_location_suffix(text: &[u16], range: NSRange) -> (NSRange, Option<isize>, Option<isize>) {
        let start = range.location;
        let end = range.upper_bound();
        let mut seg_start = end - 1;
        while seg_start >= start && Self::is_digit(text.character_at(seg_start)) {
            seg_start -= 1;
        }
        let line = if seg_start >= start && text.character_at(seg_start) == 0x3A {
            Self::parse_digits(text, seg_start + 1, end).filter(|&line| line > 0)
        } else {
            None
        };
        let Some(line) = line else { return (range, None, None) };

        let mut col_start = seg_start - 1;
        while col_start >= start && Self::is_digit(text.character_at(col_start)) {
            col_start -= 1;
        }
        if col_start >= start
            && text.character_at(col_start) == 0x3A
            && let Some(column) = Self::parse_digits(text, col_start + 1, seg_start)
            && column > 0
        {
            return (NSRange::new(start, col_start - start), Some(line), Some(column));
        }
        (NSRange::new(start, seg_start - start), Some(line), None)
    }

    /// The conservative shape test. A `/` alone is not enough — `and/or`,
    /// `read/write` and `he/him` all have one.
    fn is_path_shaped(text: &[u16], range: NSRange, relaxed: bool) -> bool {
        let start = range.location;
        let end = range.upper_bound();
        let length = range.length;
        if length >= 2 && text.character_at(start) == 0x2F && text.character_at(start + 1) == 0x2F {
            return false;
        }

        let mut slashes = 0;
        let mut last_dot: isize = -1;
        let mut index = start;
        while index < end {
            let c = text.character_at(index);
            if c == 0x2F {
                slashes += 1;
                last_dot = -1; // a `/` after the last dot kills an extension
            } else if c == 0x2E {
                last_dot = index;
            }
            index += 1;
        }
        let mut has_extension = false;
        if last_dot > start
            && last_dot + 1 < end
            && let Some(ext) = Self::extension_after(text, last_dot, end)
        {
            has_extension = Self::is_known_extension(&ext);
        }

        if relaxed {
            return slashes > 0 || has_extension;
        }
        if has_extension {
            return true;
        }
        if !(slashes > 0) {
            return false;
        }
        if Self::is_anchored(text, start, end) {
            return true;
        }
        if slashes >= 2 {
            return true;
        }
        // One slash, no anchor: only accept when a segment carries a dot,
        // which is what separates `pkg/mod.go` from `and/or`.
        Self::segment_contains_dot(text, start, end)
    }

    /// Lowercased file extension after the last dot, or `None` when the dot is
    /// not the last one or the extension is empty. A surrogate half cannot
    /// become a `UnicodeScalar` and is skipped (`config.🚀` must not trap).
    fn extension_after(text: &[u16], dot: isize, end: isize) -> Option<String> {
        let mut out = String::new();
        for index in (dot + 1)..end {
            let c = text.character_at(index);
            if c == 0x2F {
                return None; // a later `/` means the dot isn't an extension separator
            }
            let Some(scalar) = char::from_u32(Self::ascii_lower(c) as u32) else { continue };
            out.push(scalar);
        }
        if out.is_empty() { None } else { Some(out) }
    }

    fn is_anchored(text: &[u16], start: isize, end: isize) -> bool {
        if !(start < end) {
            return false;
        }
        match text.character_at(start) {
            0x2E => {
                // ./
                if start + 1 < end && text.character_at(start + 1) == 0x2F {
                    return true;
                }
                // ../
                if start + 2 < end && text.character_at(start + 1) == 0x2E && text.character_at(start + 2) == 0x2F {
                    return true;
                }
                false
            }
            0x2F => true, // absolute
            0x7E => start + 1 < end && text.character_at(start + 1) == 0x2F, // ~/
            _ => false,
        }
    }

    fn segment_contains_dot(text: &[u16], start: isize, end: isize) -> bool {
        let mut segment_start = start;
        let mut index = start;
        while index <= end {
            if index == end || text.character_at(index) == 0x2F {
                if (segment_start..index).any(|probe| text.character_at(probe) == 0x2E) {
                    return true;
                }
                segment_start = index + 1;
            }
            index += 1;
        }
        false
    }

    fn is_url(text: &[u16], range: NSRange) -> bool {
        let start = range.location;
        let end = range.upper_bound();
        // `://` anywhere in the token. A one-unit path (`3:16` splits to the
        // path `3`) makes `end - 2` precede `start`, so the scan is clamped.
        for index in start..start.max(end - 2) {
            if text.character_at(index) == 0x3A && text.character_at(index + 1) == 0x2F && text.character_at(index + 2) == 0x2F {
                return true;
            }
        }
        // `www.` prefix, case-insensitive.
        if end - start >= 4 && Self::ascii_lower(text.character_at(start)) == 0x77 {
            let mut matches = true;
            for (offset, expected) in [0x77u16, 0x77, 0x77, 0x2E].into_iter().enumerate() {
                if Self::ascii_lower(text.character_at(start + offset as isize)) != expected {
                    matches = false;
                    break;
                }
            }
            if matches {
                return true;
            }
        }
        // A scheme is everything before the first colon; letters only, and
        // more than one character (`a:b` reads as a path, `mailto:` as a URL).
        if let Some(colon) = (start..end).find(|&colon| text.character_at(colon) == 0x3A)
            && colon - start > 1
            && (start..colon).all(|index| Self::is_ascii_letter(text.character_at(index)))
        {
            return true;
        }
        false
    }

    fn parse_digits(text: &[u16], start: isize, end: isize) -> Option<isize> {
        let mut value: isize = 0;
        for index in start..end {
            let digit = text.character_at(index) as isize - 0x30;
            if value > (isize::MAX - digit) / 10 {
                return None;
            }
            value = value * 10 + digit;
        }
        Some(value)
    }

    #[inline(always)]
    fn is_digit(ch: u16) -> bool {
        (0x30..=0x39).contains(&ch)
    }

    #[inline(always)]
    fn is_ascii_letter(ch: u16) -> bool {
        (0x41..=0x5A).contains(&ch) || (0x61..=0x7A).contains(&ch)
    }

    /// Lowercases an ASCII letter; returns other units unchanged.
    #[inline(always)]
    fn ascii_lower(ch: u16) -> u16 {
        if (0x41..=0x5A).contains(&ch) { ch + 0x20 } else { ch }
    }

    fn is_token_character(ch: u16) -> bool {
        match ch {
            0x2F | 0x2E | 0x2D | 0x5F | 0x7E | 0x40 | 0x3A | 0x2B => true, // / . - _ ~ @ : +
            _ => (0x30..=0x39).contains(&ch) || (0x41..=0x5A).contains(&ch) || (0x61..=0x7A).contains(&ch),
        }
    }

    fn is_trailing_punctuation(ch: u16) -> bool {
        matches!(ch, 0x2E | 0x2C | 0x3B | 0x3A | 0x21 | 0x3F | 0x2D) // . , ; : ! ? -
    }
}
