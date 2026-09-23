//! The MarkdownCore (`Sources/MarkdownCore`) types the render contracts name.
//!
//! **Temporary.** `upleft-core` is being ported in parallel; until it merges,
//! these are exact ports of the few definitions `RenderContracts.swift` and
//! `StyleSheet.swift` depend on (`Model.swift`, `Contracts.swift`). Once
//! `upleft-core` lands, this module becomes `pub use upleft_core::…` of the
//! same names and nothing else in this crate changes.

use objc2_foundation::NSRange;

use crate::swift_compat;

/// `CalloutKind` (Model.swift).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalloutKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
    Info,
    Success,
    Question,
    Danger,
    Example,
    Quote,
    Abstract,
    Bug,
    Todo,
}

impl CalloutKind {
    pub const ALL_CASES: [CalloutKind; 14] = [
        CalloutKind::Note,
        CalloutKind::Tip,
        CalloutKind::Important,
        CalloutKind::Warning,
        CalloutKind::Caution,
        CalloutKind::Info,
        CalloutKind::Success,
        CalloutKind::Question,
        CalloutKind::Danger,
        CalloutKind::Example,
        CalloutKind::Quote,
        CalloutKind::Abstract,
        CalloutKind::Bug,
        CalloutKind::Todo,
    ];

    pub const fn raw_value(self) -> &'static str {
        match self {
            CalloutKind::Note => "note",
            CalloutKind::Tip => "tip",
            CalloutKind::Important => "important",
            CalloutKind::Warning => "warning",
            CalloutKind::Caution => "caution",
            CalloutKind::Info => "info",
            CalloutKind::Success => "success",
            CalloutKind::Question => "question",
            CalloutKind::Danger => "danger",
            CalloutKind::Example => "example",
            CalloutKind::Quote => "quote",
            CalloutKind::Abstract => "abstract",
            CalloutKind::Bug => "bug",
            CalloutKind::Todo => "todo",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<CalloutKind> {
        CalloutKind::ALL_CASES.into_iter().find(|kind| kind.raw_value() == raw)
    }

    /// `init?(token:)`: agents emit these in every casing imaginable.
    pub fn from_token(token: &str) -> Option<CalloutKind> {
        CalloutKind::from_raw_value(&swift_compat::lowercased(token))
    }
}

/// `ChangeKind` (Contracts.swift).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Inserted,
    Deleted,
    Modified,
}

impl ChangeKind {
    pub const ALL_KINDS: [ChangeKind; 3] = [ChangeKind::Inserted, ChangeKind::Deleted, ChangeKind::Modified];

    pub const fn raw_value(self) -> &'static str {
        match self {
            ChangeKind::Inserted => "inserted",
            ChangeKind::Deleted => "deleted",
            ChangeKind::Modified => "modified",
        }
    }
}

/// `BlockIdentity` (Model.swift).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockIdentity {
    /// Discriminator for the block's kind, so a heading never matches a table.
    pub kind: i64,
    /// Index among same-kind siblings.
    pub ordinal: i64,
}

/// `TableAlignment` (Model.swift).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableAlignment {
    None,
    Left,
    Center,
    Right,
}

/// `PathToken` (Model.swift).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathToken {
    pub raw_path: String,
    pub line: Option<i64>,
    pub column: Option<i64>,
}

/// `InlineKind` (Model.swift).
#[derive(Debug, Clone, PartialEq)]
pub enum InlineKind {
    Text,
    Emphasis,
    Strong,
    Strikethrough,
    InlineCode,
    Link { destination: String, title: Option<String> },
    Autolink { destination: String },
    Wikilink { target: String, label: Option<String> },
    Image { source: String, alt: String },
    InlineMath { latex_range: NSRange },
    PathToken(PathToken),
    FootnoteReference { identifier: String },
    SoftBreak,
    LineBreak,
    InlineHtml,
}

/// `InlineSpan` (Model.swift).
#[derive(Debug, Clone, PartialEq)]
pub struct InlineSpan {
    pub kind: InlineKind,
    /// Whole span, markers included.
    pub range: NSRange,
    /// Inner content, markers excluded.
    pub content_range: NSRange,
    pub leading_marker_range: Option<NSRange>,
    pub trailing_marker_range: Option<NSRange>,
    pub children: Vec<InlineSpan>,
}

/// `TableCell` (Model.swift).
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    pub range: NSRange,
    pub content_range: NSRange,
    pub inlines: Vec<InlineSpan>,
}

/// `TableRow` (Model.swift).
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub range: NSRange,
    pub cells: Vec<TableCell>,
    pub is_header: bool,
}

/// `TableData` (Model.swift).
#[derive(Debug, Clone, PartialEq)]
pub struct TableData {
    pub rows: Vec<TableRow>,
    pub alignments: Vec<TableAlignment>,
    /// Range of the `|---|:--:|` delimiter row.
    pub delimiter_range: NSRange,
}
