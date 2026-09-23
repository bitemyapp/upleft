//! Upleft's port of Downright's `MarkdownCore`.
//!
//! One module per Swift file, same names in snake_case. Swift `internal`
//! declarations are `pub` here, because the conformance oracle and the tests
//! reach them the way Downright's `@testable` tests do.
//!
//! Support modules with no Swift counterpart:
//!
//! * [`ns_range`] — Foundation's `NSRange` (signed, like Swift's `Int`).
//! * [`swift_text`] — Swift `String`/`Character`, `CharacterSet` and
//!   `NSString` semantics, with Unicode tables generated from the Swift
//!   runtime.
//!
//! Parser.swift, Inlines.swift and the swift-markdown half of
//! SourcePositions.swift wait for `upleft-markup`; [`parser`] holds the entry
//! points meanwhile.

pub mod ns_range;
pub mod swift_text;

pub mod ast_diff;
pub mod contracts;
pub mod derived;
pub mod document_io;
pub mod hashing;
pub mod list_editing;
pub mod metrics;
pub mod model;
pub mod myers;
pub mod parser;
pub mod restructure;
pub mod safe_html;
pub mod smart_paste;
pub mod source_positions;
pub mod structural_zoom;
pub mod table_formatter;
pub mod task_worklist;
pub mod text_diff;
pub mod tidy;

pub mod compatibility;
pub mod editing;
pub mod extensions;
pub mod health;

pub use contracts::*;
pub use model::*;
pub use ns_range::{NS_NOT_FOUND, NSRange};
