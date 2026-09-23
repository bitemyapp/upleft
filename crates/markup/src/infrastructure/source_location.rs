//! Port of `Sources/Markdown/Infrastructure/SourceLocation.swift`.
//!
//! swift-markdown's `SourceLocation` also carries an optional source `URL`.
//! Downright never passes one to `Document(parsing:)`, so every location it
//! can observe has `source == nil`, and the port leaves the field out.

use std::fmt;

/// A location in a source file: a 1-based line and a 1-based column counted
/// in UTF-8 bytes from the start of the line (Swift `Int`s).
///
/// Ordering is Swift's `<`: by line, then by column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceLocation {
    /// The line number of the location.
    pub line: i64,
    /// The number of bytes in UTF-8 encoding from the start of the line to
    /// the character at this source location.
    pub column: i64,
}

impl SourceLocation {
    pub const fn new(line: i64, column: i64) -> SourceLocation {
        SourceLocation { line, column }
    }
}

/// `description`: `line:column` (no path, since the source is always nil).
impl fmt::Display for SourceLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.line, self.column)
    }
}

/// A range in a source file: Swift's `Range<SourceLocation>`, a half-open
/// range whose lower bound never exceeds its upper bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceRange {
    pub lower_bound: SourceLocation,
    pub upper_bound: SourceLocation,
}

impl SourceRange {
    /// `lowerBound..<upperBound`. Like Swift's range operator, this traps
    /// when the bounds are out of order.
    pub fn new(lower_bound: SourceLocation, upper_bound: SourceLocation) -> SourceRange {
        assert!(
            lower_bound <= upper_bound,
            "Range requires lowerBound <= upperBound"
        );
        SourceRange {
            lower_bound,
            upper_bound,
        }
    }

    /// `diagnosticDescription(includePath:)`. With no source URL the path is
    /// always empty, so `includePath` makes no difference.
    pub fn diagnostic_description(&self) -> String {
        let mut result = self.lower_bound.to_string();
        if self.lower_bound != self.upper_bound {
            result.push('-');
            result.push_str(&self.upper_bound.to_string());
        }
        result
    }
}
