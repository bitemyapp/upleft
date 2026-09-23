//! Port of `Sources/DownrightApp/Support/FindEngine.swift`: find and replace
//! (§9.4), the per-window [`FindSession`], and cross-file [`SiblingSearch`].
//!
//! Search runs over the **source** text, always, through `NSRegularExpression`
//! exactly as the Swift does (called through objc2), so every match range is a
//! UTF-16 `NSRange` and every regex, case-folding and word-boundary rule is
//! ICU's. A non-regex query is escaped with `escapedPattern(for:)`.
//!
//! [`SiblingSearch::search`] takes the URL list the caller already has (the
//! sidebar's `SiblingScanner` list in the app); it does not depend on the
//! scanner itself.

use std::path::Path;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_foundation::{
    NSMatchingOptions, NSRange as FRange, NSRegularExpression, NSRegularExpressionOptions, NSString,
    NSTextCheckingResult,
};
use upleft_core::document_io::DocumentIO;
use upleft_core::parser::MarkdownParser;
use upleft_core::{BlockContent, HeadingNode, NSRange, ParseOptions, ParsedDocument, TextEdit};
use upleft_foundation::url::FileUrl;
use upleft_swift_text as swift;

// MARK: - Query

/// `FindQuery`. `Equatable` compares `text` with Swift `String ==`
/// (canonical equivalence), hence the hand-written [`PartialEq`].
#[derive(Clone, Debug, Default)]
pub struct FindQuery {
    pub text: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    /// Restricts the search, for "in selection" mode.
    pub scope: Option<NSRange>,
}

impl PartialEq for FindQuery {
    fn eq(&self, other: &Self) -> bool {
        swift::str_eq(&self.text, &other.text)
            && self.is_regex == other.is_regex
            && self.case_sensitive == other.case_sensitive
            && self.whole_word == other.whole_word
            && self.scope == other.scope
    }
}

impl Eq for FindQuery {}

impl FindQuery {
    /// `FindQuery(text:)` with the memberwise defaults.
    pub fn new(text: impl Into<String>) -> FindQuery {
        FindQuery { text: text.into(), ..FindQuery::default() }
    }

    /// `FindQuery(text:isRegex: true)`.
    pub fn regex(text: impl Into<String>) -> FindQuery {
        FindQuery { text: text.into(), is_regex: true, ..FindQuery::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// `pattern`: the text, escaped unless it is a regex, wrapped in word
    /// boundaries for whole-word mode.
    pub fn pattern(&self) -> Option<String> {
        if self.text.is_empty() {
            return None;
        }
        let escaped = if self.is_regex { self.text.clone() } else { escaped_pattern(&self.text) };
        Some(if self.whole_word { format!("\\b(?:{escaped})\\b") } else { escaped })
    }

    pub fn options(&self) -> NSRegularExpressionOptions {
        if self.case_sensitive { NSRegularExpressionOptions::empty() } else { NSRegularExpressionOptions::CaseInsensitive }
    }
}

/// `text as NSString`. Built from UTF-16 units for non-ASCII text:
/// `NSString::from_str` decodes UTF-8 and drops a leading U+FEFF, which a
/// Swift string bridged to `NSString` keeps.
pub fn ns_string(text: &str) -> Retained<NSString> {
    if text.is_ascii() {
        return NSString::from_str(text);
    }
    swift::ns::foundation::ns_from_utf16(&swift::ns::utf16(text))
}

/// `String(ns)`.
pub fn string(ns: &NSString) -> String {
    swift::ns::foundation::to_string(ns)
}

/// `NSRegularExpression.escapedPattern(for:)`.
pub fn escaped_pattern(text: &str) -> String {
    objc2::rc::autoreleasepool(|_| string(&NSRegularExpression::escapedPatternForString(&ns_string(text))))
}

/// `try? NSRegularExpression(pattern:options:)`.
pub fn regular_expression(pattern: &str, options: NSRegularExpressionOptions) -> Option<Retained<NSRegularExpression>> {
    NSRegularExpression::initWithPattern_options_error(NSRegularExpression::alloc(), &ns_string(pattern), options).ok()
}

pub(crate) fn frange(range: NSRange) -> FRange {
    FRange::new(range.location as usize, range.length as usize)
}

pub(crate) fn range_of(result: &NSTextCheckingResult) -> NSRange {
    let range = result.range();
    NSRange::new(range.location as isize, range.length as isize)
}

// MARK: - Engine

/// A search's text, regex and results, kept so a replacement can use the
/// captures of the search that found the hit (`FindEngine.SearchResult`).
pub struct SearchResult {
    pub text: String,
    ns_text: Retained<NSString>,
    pub regex: Retained<NSRegularExpression>,
    pub results: Vec<Retained<NSTextCheckingResult>>,
}

impl SearchResult {
    /// `regex.replacementString(for:in:offset: 0, template:)`.
    fn replacement_string(&self, result: &NSTextCheckingResult, template: &str) -> String {
        objc2::rc::autoreleasepool(|_| {
            string(&self.regex.replacementStringForResult_inString_offset_template(
                result,
                &self.ns_text,
                0,
                &ns_string(template),
            ))
        })
    }
}

pub struct FindEngine;

impl FindEngine {
    /// All matches, ascending. Empty rather than an error on a half-typed
    /// regex.
    pub fn matches(text: &str, query: &FindQuery) -> Vec<NSRange> {
        Self::search(text, query).map_or_else(Vec::new, |search| search.results.iter().map(|r| range_of(r)).collect())
    }

    pub fn search(text: &str, query: &FindQuery) -> Option<SearchResult> {
        objc2::rc::autoreleasepool(|_| {
            let pattern = query.pattern()?;
            let regex = regular_expression(&pattern, query.options())?;
            let ns_text = ns_string(text);
            let full = NSRange::new(0, ns_text.length() as isize);
            let scope = query.scope.map_or(full, |scope| ns_intersection_range(scope, full));
            if !(scope.length > 0) {
                return None;
            }
            let matches = regex.matchesInString_options_range(&ns_text, NSMatchingOptions::empty(), frange(scope));
            let results = matches.iter().filter(|result| result.range().length > 0).collect();
            Some(SearchResult { text: text.to_owned(), ns_text, regex, results })
        })
    }

    /// Whether a partially typed regex is currently valid, for the field's
    /// error affordance.
    pub fn is_valid(query: &FindQuery) -> bool {
        let Some(pattern) = query.pattern() else { return true };
        objc2::rc::autoreleasepool(|_| regular_expression(&pattern, query.options()).is_some())
    }

    /// Expands `$1`-style references when the query is a regex; otherwise
    /// the template is literal.
    pub fn replacement(match_range: NSRange, text: &str, query: &FindQuery, template: &str) -> String {
        if !query.is_regex {
            return template.to_owned();
        }
        let Some(search) = Self::search(text, query) else { return template.to_owned() };
        let Some(result) = search.results.iter().find(|result| range_of(result) == match_range) else {
            return template.to_owned();
        };
        // Preserve the original search scope: narrowing it to the matched
        // text changes lookarounds, anchors, and captures.
        search.replacement_string(result, template)
    }

    pub fn replace_all_edits(text: &str, query: &FindQuery, template: &str) -> Vec<TextEdit> {
        let Some(search) = Self::search(text, query) else { return Vec::new() };
        search
            .results
            .iter()
            .map(|result| {
                let replacement =
                    if query.is_regex { search.replacement_string(result, template) } else { template.to_owned() };
                TextEdit::new(range_of(result), replacement, "Replace", None)
            })
            .collect()
    }
}

/// Foundation's `NSIntersectionRange`, called as Swift calls it (the
/// fields' bit patterns reinterpreted as `NSUInteger`).
pub(crate) fn ns_intersection_range(a: NSRange, b: NSRange) -> NSRange {
    unsafe extern "C" {
        fn NSIntersectionRange(range1: FRange, range2: FRange) -> FRange;
    }
    // SAFETY: a pure Foundation function over two value types.
    let result = unsafe { NSIntersectionRange(frange(a), frange(b)) };
    NSRange::new(result.location as isize, result.length as isize)
}

// MARK: - Session

/// Tracks the current match across edits so ⌘G means "the next one after
/// where I am".
#[derive(Default)]
pub struct FindSession {
    query: FindQuery,
    matches: Vec<NSRange>,
    current_index: Option<usize>,
    search_result: Option<SearchResult>,
}

impl FindSession {
    pub fn new() -> FindSession {
        FindSession::default()
    }

    pub fn query(&self) -> &FindQuery {
        &self.query
    }

    pub fn matches(&self) -> &[NSRange] {
        &self.matches
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    pub fn count(&self) -> usize {
        self.matches.len()
    }

    pub fn status_text(&self) -> String {
        if self.query.is_empty() {
            return String::new();
        }
        if self.matches.is_empty() {
            return "No matches".into();
        }
        match self.current_index {
            None => format!("{} matches", self.matches.len()),
            Some(index) => format!("{} of {}", index + 1, self.matches.len()),
        }
    }

    pub fn current_match(&self) -> Option<NSRange> {
        let index = self.current_index?;
        self.matches.get(index).copied()
    }

    pub fn update(&mut self, query: FindQuery, text: &str, caret: isize) {
        self.query = query;
        self.search_result = FindEngine::search(text, &self.query);
        self.matches = self
            .search_result
            .as_ref()
            .map_or_else(Vec::new, |search| search.results.iter().map(|r| range_of(r)).collect());
        self.current_index = self
            .matches
            .iter()
            .position(|range| range.location >= caret)
            .or(if self.matches.is_empty() { None } else { Some(0) });
    }

    /// Uses the captures from the search that selected this hit. An edited
    /// snapshot (compared UTF-16 unit for unit) re-runs the search from the
    /// live caret first.
    pub fn replacement_edit(&mut self, text: &str, template: &str, caret: isize) -> Option<TextEdit> {
        // UTF-8 equality is UTF-16 equality for well-formed text.
        if self.search_result.as_ref().is_none_or(|search| search.text != text) {
            let query = self.query.clone();
            self.update(query, text, caret);
        }
        let search = self.search_result.as_ref()?;
        let index = self.current_index?;
        let result = search.results.get(index)?;
        let replacement =
            if self.query.is_regex { search.replacement_string(result, template) } else { template.to_owned() };
        Some(TextEdit::new(range_of(result), replacement, "Replace", None))
    }

    pub fn advance(&mut self, forward: bool) -> Option<NSRange> {
        if self.matches.is_empty() {
            return None;
        }
        let count = self.matches.len() as isize;
        let index = self.current_index.map_or(if forward { -1 } else { 0 }, |index| index as isize);
        let next = if forward { (index + 1) % count } else { (index - 1 + count) % count };
        self.current_index = Some(next as usize);
        Some(self.matches[next as usize])
    }

    pub fn clear(&mut self) {
        self.query = FindQuery::default();
        self.matches = Vec::new();
        self.current_index = None;
        self.search_result = None;
    }
}

// MARK: - Cross-file search (§9.4, ⌘⇧F)

/// One sibling-file hit (`SiblingSearch.Hit`).
#[derive(Clone, Debug, PartialEq)]
pub struct SiblingHit {
    pub url: FileUrl,
    pub display_name: String,
    /// Range within that file's text.
    pub range: NSRange,
    /// The containing paragraph or heading line, for rendered context.
    pub context_range: NSRange,
    pub context_text: String,
    pub heading_title: Option<String>,
    pub line_number: isize,
}

impl SiblingHit {
    /// `id`: `"\(url.path):\(range.location)"`.
    pub fn id(&self) -> String {
        format!("{}:{}", self.url.path(), self.range.location)
    }
}

/// Searches sibling files and returns enough context to render each hit.
/// Still no index and no vault (§2): it reads the file list the caller
/// already has, on demand.
pub struct SiblingSearch;

impl SiblingSearch {
    pub const MAXIMUM_FILE_BYTES: i64 = 4 * 1_024 * 1_024;
    pub const MAXIMUM_TOTAL_BYTES: i64 = 32 * 1_024 * 1_024;

    /// `search(_:in:)` with the default `limitPerFile: 20` and a
    /// `shouldCancel` that never cancels.
    pub fn search_default(query: &FindQuery, urls: &[FileUrl]) -> Vec<SiblingHit> {
        Self::search(query, urls, 20, &|| false)
    }

    pub fn search(
        query: &FindQuery,
        urls: &[FileUrl],
        limit_per_file: usize,
        should_cancel: &(dyn Fn() -> bool + Sync),
    ) -> Vec<SiblingHit> {
        let mut hits = Vec::new();
        let mut bytes_read: i64 = 0;
        for url in urls {
            if should_cancel() {
                return hits;
            }
            let path = url.path();
            let Some(size) = file_size(Path::new(&path)) else { continue };
            if !(size >= 0 && size <= Self::MAXIMUM_FILE_BYTES && bytes_read + size <= Self::MAXIMUM_TOTAL_BYTES) {
                continue;
            }
            bytes_read += size;
            let Ok((text, _)) = DocumentIO::read(Path::new(&path)) else { continue };
            if should_cancel() {
                return hits;
            }
            let ranges = FindEngine::matches(&text, query);
            if ranges.is_empty() {
                continue;
            }
            if should_cancel() {
                return hits;
            }

            let document = MarkdownParser::parse_with(&text, ParseOptions::STRUCTURE_ONLY);
            for &range in ranges.iter().take(limit_per_file) {
                if should_cancel() {
                    return hits;
                }
                let context = Self::context_range(range, &document);
                hits.push(SiblingHit {
                    url: url.clone(),
                    display_name: url.deleting_path_extension().last_path_component(),
                    range,
                    context_range: context,
                    context_text: document.substring(context),
                    heading_title: Self::enclosing_heading(range, &document).map(|heading| heading.title.clone()),
                    line_number: document.line_at(range.location),
                });
            }
        }
        hits
    }

    fn context_range(range: NSRange, document: &ParsedDocument) -> NSRange {
        let Some(block) = document.root.block_at(range.location) else {
            return document.range_of_line(document.line_at(range.location));
        };
        // A whole code block or table as context is too much; a line is enough.
        match block.content {
            BlockContent::Paragraph | BlockContent::Heading { .. } => block.range,
            _ => document.range_of_line(document.line_at(range.location)),
        }
    }

    fn enclosing_heading(range: NSRange, document: &ParsedDocument) -> Option<&HeadingNode> {
        document.headings.iter().rev().find(|heading| heading.range.location <= range.location)
    }
}

/// `url.resourceValues(forKeys: [.fileSizeKey]).fileSize`: the size of a
/// regular file, or of a symbolic link itself (resource values do not follow
/// links); `nil` for a directory or a path that cannot be examined.
pub(crate) fn file_size(path: &Path) -> Option<i64> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.is_dir() {
        return None;
    }
    Some(metadata.len() as i64)
}
