//! Parser.swift — `MarkdownParser.parse`.
//!
//! Not ported yet: it drives swift-markdown's converter, which is being ported
//! as `upleft-markup`. The entry points exist so the modules that call them
//! (Metrics, DocumentHealth) are complete; tests that need a parse are marked
//! `#[ignore = "needs parser (upleft-markup)"]` until the parser lands.

use std::sync::Arc;

use crate::contracts::ParseOptions;
use crate::model::ParsedDocument;

pub struct MarkdownParser;

impl MarkdownParser {
    /// `MarkdownParser.parse(_:)`.
    pub fn parse(text: &str) -> Arc<ParsedDocument> {
        Self::parse_with(text, ParseOptions::DEFAULT)
    }

    /// `MarkdownParser.parse(_:options:)`.
    pub fn parse_with(text: &str, options: ParseOptions) -> Arc<ParsedDocument> {
        let _ = (text, options);
        unimplemented!("MarkdownParser.parse lands with the upleft-markup port")
    }
}
