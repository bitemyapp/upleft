//! Port of `Syntax/SyntaxContracts.swift`: the code-highlighting surface
//! (§11.3).
//!
//! `SyntaxHighlighter` is the seam that keeps the lexer decision reversible: a
//! tree-sitter backend could implement it and be swapped in at the one place
//! the decoration engine reads a highlighter from.

use objc2_foundation::NSRange;

/// A highlight class. Deliberately small: `CodeTheme` has exactly these slots
/// and VS Code / Shiki themes have to map onto it (§11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SyntaxToken {
    Plain,
    Keyword,
    String,
    Number,
    Comment,
    Type,
    Function,
    Variable,
    Constant,
    Operator,
    Punctuation,
    Attribute,
    DiffAdded,
    DiffRemoved,
    DiffHeader,
}

impl SyntaxToken {
    /// `CaseIterable.allCases`, in declaration order.
    pub const ALL_CASES: [SyntaxToken; 15] = [
        SyntaxToken::Plain,
        SyntaxToken::Keyword,
        SyntaxToken::String,
        SyntaxToken::Number,
        SyntaxToken::Comment,
        SyntaxToken::Type,
        SyntaxToken::Function,
        SyntaxToken::Variable,
        SyntaxToken::Constant,
        SyntaxToken::Operator,
        SyntaxToken::Punctuation,
        SyntaxToken::Attribute,
        SyntaxToken::DiffAdded,
        SyntaxToken::DiffRemoved,
        SyntaxToken::DiffHeader,
    ];

    /// The Swift `rawValue`.
    pub const fn raw_value(self) -> &'static str {
        match self {
            SyntaxToken::Plain => "plain",
            SyntaxToken::Keyword => "keyword",
            SyntaxToken::String => "string",
            SyntaxToken::Number => "number",
            SyntaxToken::Comment => "comment",
            SyntaxToken::Type => "type",
            SyntaxToken::Function => "function",
            SyntaxToken::Variable => "variable",
            SyntaxToken::Constant => "constant",
            SyntaxToken::Operator => "operator",
            SyntaxToken::Punctuation => "punctuation",
            SyntaxToken::Attribute => "attribute",
            SyntaxToken::DiffAdded => "diffAdded",
            SyntaxToken::DiffRemoved => "diffRemoved",
            SyntaxToken::DiffHeader => "diffHeader",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<SyntaxToken> {
        SyntaxToken::ALL_CASES.into_iter().find(|token| token.raw_value() == raw)
    }
}

/// One classified span. `range` is in UTF-16 units of the code handed to
/// `highlight`, so it applies to text storage without conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxRun {
    pub range: NSRange,
    pub token: SyntaxToken,
}

impl SyntaxRun {
    pub fn new(range: NSRange, token: SyntaxToken) -> Self {
        SyntaxRun { range, token }
    }
}

/// Pluggable so another backend can replace the built-in one later without
/// touching the decoration engine.
///
/// Implementations must return runs that are ascending, non-overlapping, and
/// entirely inside the input. The decoration engine applies them in order and
/// does not sort or clamp.
///
/// Swift's `highlight(_ code: String, …)` immediately takes `Array(code.utf16)`;
/// the port takes the UTF-16 units directly so callers holding an `NSString`
/// or a `Vec<u16>` pay no conversion.
pub trait SyntaxHighlighter: Send + Sync {
    fn highlight(&self, code: &[u16], language: Option<&str>) -> Vec<SyntaxRun>;
    fn supports(&self, language: &str) -> bool;
}
