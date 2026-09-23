//! Contracts.swift — result and option types shared across modules.

use crate::ns_range::NSRange;
use crate::swift_text::ns::{NSStringExt, string_from_utf16, utf16};

// MARK: - Parsing

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseOptions {
    pub detect_front_matter: bool,
    pub detect_math: bool,
    pub detect_callouts: bool,
    pub detect_wikilinks: bool,
    pub detect_path_tokens: bool,
    pub detect_mermaid: bool,
    /// Beyond this UTF-16 length the parser stops running the optional
    /// extension passes (§15 Q4).
    pub extension_pass_limit: isize,
}

impl ParseOptions {
    pub const DEFAULT: ParseOptions = ParseOptions {
        detect_front_matter: true,
        detect_math: true,
        detect_callouts: true,
        detect_wikilinks: true,
        detect_path_tokens: true,
        detect_mermaid: true,
        extension_pass_limit: 5_000_000,
    };

    /// Everything off but the structure — used by the thumbnail generator.
    pub const STRUCTURE_ONLY: ParseOptions = ParseOptions {
        detect_front_matter: true,
        detect_math: false,
        detect_callouts: true,
        detect_wikilinks: false,
        detect_path_tokens: false,
        detect_mermaid: false,
        extension_pass_limit: 5_000_000,
    };

    /// Lean parse for the optional workspace index.
    pub const WORKSPACE_INDEX: ParseOptions = ParseOptions {
        detect_front_matter: true,
        detect_math: false,
        detect_callouts: false,
        detect_wikilinks: true,
        detect_path_tokens: false,
        detect_mermaid: false,
        extension_pass_limit: 5_000_000,
    };
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions::DEFAULT
    }
}

// MARK: - AST diff (§3.5)

/// What changed between two parses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirtySet {
    /// Source ranges (in the *new* document) needing re-decoration.
    pub ranges: Vec<NSRange>,
    /// The caller should redecorate everything.
    pub is_wholesale: bool,
}

impl DirtySet {
    pub fn new(ranges: Vec<NSRange>, is_wholesale: bool) -> DirtySet {
        DirtySet { ranges, is_wholesale }
    }

    pub fn wholesale() -> DirtySet {
        DirtySet { ranges: Vec::new(), is_wholesale: true }
    }

    pub fn none() -> DirtySet {
        DirtySet { ranges: Vec::new(), is_wholesale: false }
    }

    pub fn is_empty(&self) -> bool {
        !self.is_wholesale && self.ranges.is_empty()
    }
}

// MARK: - Text diff (§8.1)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Inserted,
    Deleted,
    Modified,
}

impl ChangeKind {
    pub fn raw_value(&self) -> &'static str {
        match self {
            ChangeKind::Inserted => "inserted",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Modified => "modified",
        }
    }
}

/// A change between two versions of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeHunk {
    pub kind: ChangeKind,
    /// Range in the new text. Zero-length for a pure deletion.
    pub new_range: NSRange,
    /// Range in the old text. Zero-length for a pure insertion.
    pub old_range: NSRange,
    /// Word-level ranges within `new_range` that actually differ.
    pub word_ranges: Vec<NSRange>,
}

impl ChangeHunk {
    pub fn new(kind: ChangeKind, new_range: NSRange, old_range: NSRange, word_ranges: Vec<NSRange>) -> ChangeHunk {
        ChangeHunk { kind, new_range, old_range, word_ranges }
    }
}

// MARK: - Tidy (§9.1)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TidyRule {
    HeadingLevels,
    TablePipes,
    BlankLines,
    CodeFenceLanguages,
    OrderedListNumbers,
    ListMarkers,
    TrailingWhitespace,
}

impl TidyRule {
    pub const ALL_CASES: [TidyRule; 7] = [
        TidyRule::HeadingLevels,
        TidyRule::TablePipes,
        TidyRule::BlankLines,
        TidyRule::CodeFenceLanguages,
        TidyRule::OrderedListNumbers,
        TidyRule::ListMarkers,
        TidyRule::TrailingWhitespace,
    ];

    pub fn raw_value(&self) -> &'static str {
        match self {
            TidyRule::HeadingLevels => "headingLevels",
            TidyRule::TablePipes => "tablePipes",
            TidyRule::BlankLines => "blankLines",
            TidyRule::CodeFenceLanguages => "codeFenceLanguages",
            TidyRule::OrderedListNumbers => "orderedListNumbers",
            TidyRule::ListMarkers => "listMarkers",
            TidyRule::TrailingWhitespace => "trailingWhitespace",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            TidyRule::HeadingLevels => "Fix skipped heading levels",
            TidyRule::TablePipes => "Align table pipes",
            TidyRule::BlankLines => "Collapse blank line runs",
            TidyRule::CodeFenceLanguages => "Add code fence languages",
            TidyRule::OrderedListNumbers => "Renumber ordered lists",
            TidyRule::ListMarkers => "Normalise list markers",
            TidyRule::TrailingWhitespace => "Trim trailing whitespace",
        }
    }
}

/// `Foundation.UUID`: a random (version 4) identifier.
pub type Uuid = uuid::Uuid;

/// One accept/reject-able edit.
#[derive(Clone, Debug, PartialEq)]
pub struct TextEdit {
    pub id: Uuid,
    pub range: NSRange,
    pub replacement: String,
    /// Human-readable description for the diff sheet.
    pub summary: String,
    pub rule: Option<TidyRule>,
}

impl TextEdit {
    pub fn new(range: NSRange, replacement: impl Into<String>, summary: impl Into<String>, rule: Option<TidyRule>) -> TextEdit {
        TextEdit { id: Uuid::new_v4(), range, replacement: replacement.into(), summary: summary.into(), rule }
    }
}

/// `extension Array where Element == TextEdit { func applied(to:) }`:
/// applies edits back to front so earlier offsets stay valid. Overlapping
/// edits keep the later-located one.
pub fn applied(edits: &[TextEdit], text: &str) -> String {
    let mut ns = utf16(text);
    let mut last_start = isize::MAX;
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    // Swift's `sorted(by:)` has been a stable merge sort since Swift 5, so
    // equal locations keep their order in both.
    sorted.sort_by(|a, b| b.range.location.cmp(&a.range.location));
    for edit in sorted {
        if !(edit.range.upper_bound() <= last_start) {
            continue;
        }
        if !(edit.range.location >= 0 && edit.range.upper_bound() <= ns.as_slice().length()) {
            continue;
        }
        let replacement = utf16(&edit.replacement);
        ns.splice(edit.range.as_usize_range(), replacement);
        last_start = edit.range.location;
    }
    string_from_utf16(&ns)
}

// MARK: - Restructuring (§9.2)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListConversion {
    Paragraph,
    BulletList,
    NumberedList,
    TaskList,
    Blockquote,
}

impl ListConversion {
    pub const ALL_CASES: [ListConversion; 5] = [
        ListConversion::Paragraph,
        ListConversion::BulletList,
        ListConversion::NumberedList,
        ListConversion::TaskList,
        ListConversion::Blockquote,
    ];

    pub fn raw_value(&self) -> &'static str {
        match self {
            ListConversion::Paragraph => "paragraph",
            ListConversion::BulletList => "bulletList",
            ListConversion::NumberedList => "numberedList",
            ListConversion::TaskList => "taskList",
            ListConversion::Blockquote => "blockquote",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            ListConversion::Paragraph => "Paragraph",
            ListConversion::BulletList => "Bullet List",
            ListConversion::NumberedList => "Numbered List",
            ListConversion::TaskList => "Task List",
            ListConversion::Blockquote => "Blockquote",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListSortOrder {
    Alphabetical,
    ReverseAlphabetical,
    UncheckedFirst,
    CheckedFirst,
}

impl ListSortOrder {
    pub fn raw_value(&self) -> &'static str {
        match self {
            ListSortOrder::Alphabetical => "alphabetical",
            ListSortOrder::ReverseAlphabetical => "reverseAlphabetical",
            ListSortOrder::UncheckedFirst => "uncheckedFirst",
            ListSortOrder::CheckedFirst => "checkedFirst",
        }
    }
}

// MARK: - Reading metrics (§9.6)

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReadingMetrics {
    pub words: isize,
    pub characters: isize,
    pub sentences: isize,
    /// Minutes at 238 wpm.
    pub read_minutes: f64,
}

impl ReadingMetrics {
    pub fn new(words: isize, characters: isize, sentences: isize, read_minutes: f64) -> ReadingMetrics {
        ReadingMetrics { words, characters, sentences, read_minutes }
    }

    pub const ZERO: ReadingMetrics = ReadingMetrics { words: 0, characters: 0, sentences: 0, read_minutes: 0.0 };
}

// MARK: - Structural zoom (§5.2)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ZoomLevel {
    H1 = 1,
    H2 = 2,
    Headings = 3,
    Skeleton = 4,
    Everything = 5,
}

impl ZoomLevel {
    pub const ALL_CASES: [ZoomLevel; 5] = [ZoomLevel::H1, ZoomLevel::H2, ZoomLevel::Headings, ZoomLevel::Skeleton, ZoomLevel::Everything];

    pub fn raw_value(&self) -> isize {
        *self as isize
    }

    pub fn from_raw_value(raw: isize) -> Option<ZoomLevel> {
        ZoomLevel::ALL_CASES.into_iter().find(|level| level.raw_value() == raw)
    }

    pub fn title(&self) -> &'static str {
        match self {
            ZoomLevel::H1 => "Top level",
            ZoomLevel::H2 => "Two levels",
            ZoomLevel::Headings => "All headings",
            ZoomLevel::Skeleton => "Skeleton",
            ZoomLevel::Everything => "Everything",
        }
    }

    /// Deepest heading level still shown.
    pub fn max_heading_level(&self) -> isize {
        match self {
            ZoomLevel::H1 => 1,
            ZoomLevel::H2 => 2,
            _ => 6,
        }
    }
}

/// Which source ranges survive at a given zoom level.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ZoomPlan {
    /// Ranges to keep visible, ascending and non-overlapping.
    pub visible_ranges: Vec<NSRange>,
    /// Ranges elided.
    pub elided_ranges: Vec<NSRange>,
}

impl ZoomPlan {
    pub fn new(visible_ranges: Vec<NSRange>, elided_ranges: Vec<NSRange>) -> ZoomPlan {
        ZoomPlan { visible_ranges, elided_ranges }
    }

    pub fn all() -> ZoomPlan {
        ZoomPlan::default()
    }

    pub fn is_identity(&self) -> bool {
        self.elided_ranges.is_empty()
    }
}
