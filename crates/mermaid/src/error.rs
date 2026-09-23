//! The errors the library throws, one variant per Swift error case. The Swift
//! types are private to their files; [`MermaidError::case_and_values`] is what
//! the conformance dump compares (the Swift side reads them through `Mirror`).

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum MermaidError {
    /// `_ParserEntryError.emptyDiagram` (src_parser.swift).
    EmptyDiagram,
    /// `_ParserEntryError.invalidHeader(String)` (src_parser.swift).
    InvalidHeader(String),
    /// `SequenceParserError.invalidHeader(expected:found:)`.
    SequenceInvalidHeader { expected: String, found: String },
    /// `ClassParserError.invalidHeader(expected:found:)`.
    ClassInvalidHeader { expected: String, found: String },
    /// `ErParserError.invalidHeader(expected:found:)`.
    ErInvalidHeader { expected: String, found: String },
    /// An error from the ELK layout engine.
    Elk(String),
}

impl MermaidError {
    /// The Swift enum case name and the `String` payload values, in order.
    pub fn case_and_values(&self) -> (&'static str, Vec<&str>) {
        match self {
            MermaidError::EmptyDiagram => ("emptyDiagram", vec![]),
            MermaidError::InvalidHeader(found) => ("invalidHeader", vec![found]),
            MermaidError::SequenceInvalidHeader { expected, found }
            | MermaidError::ClassInvalidHeader { expected, found }
            | MermaidError::ErInvalidHeader { expected, found } => ("invalidHeader", vec![expected, found]),
            MermaidError::Elk(message) => ("elk", vec![message]),
        }
    }
}

impl fmt::Display for MermaidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MermaidError::EmptyDiagram => write!(f, "empty diagram"),
            MermaidError::InvalidHeader(found) => write!(f, "invalid header {found:?}"),
            MermaidError::SequenceInvalidHeader { expected, found } => {
                write!(f, "Invalid sequence diagram header. Expected '{expected}', found '{found}'.")
            }
            MermaidError::ClassInvalidHeader { expected, found } => {
                write!(f, "Invalid class diagram header. Expected '{expected}', found '{found}'.")
            }
            MermaidError::ErInvalidHeader { expected, found } => {
                write!(f, "Invalid ER diagram header. Expected '{expected}', found '{found}'.")
            }
            MermaidError::Elk(message) => write!(f, "ELK layout error: {message}"),
        }
    }
}

impl std::error::Error for MermaidError {}
