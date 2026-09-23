//! Port of `Syntax/BuiltinSyntaxHighlighter.swift`: the shipped
//! `SyntaxHighlighter`, a hand-written lexer.
//!
//! Stateless and therefore safe to share; `shared` exists so the decoration
//! engine does not rebuild language tables per code block.

use super::language_catalog;
use super::syntax_contracts::{SyntaxHighlighter, SyntaxRun};

#[derive(Debug, Default)]
pub struct BuiltinSyntaxHighlighter;

static SHARED: BuiltinSyntaxHighlighter = BuiltinSyntaxHighlighter;

impl BuiltinSyntaxHighlighter {
    pub fn shared() -> &'static BuiltinSyntaxHighlighter {
        &SHARED
    }

    pub fn new() -> Self {
        BuiltinSyntaxHighlighter
    }

    /// Convenience for callers holding a Rust string: Swift's
    /// `highlight(_ code: String, …)` converts with `Array(code.utf16)`.
    pub fn highlight_str(&self, code: &str, language: Option<&str>) -> Vec<SyntaxRun> {
        // The same early exits as the Swift guard, before paying for UTF-16.
        let Some(raw) = language else {
            return Vec::new();
        };
        let Some(canonical) = BuiltinSyntaxHighlighter::canonical_language(raw) else {
            return Vec::new();
        };
        let Some(scanner) = language_catalog::scanner(canonical) else {
            return Vec::new();
        };
        if code.is_empty() {
            return Vec::new();
        }
        let units: Vec<u16> = code.encode_utf16().collect();
        scanner.highlight(&units)
    }

    /// Canonical name for an alias: "ts" → "typescript", "sh" → "bash".
    pub fn canonical_language(raw: &str) -> Option<&'static str> {
        language_catalog::canonical(raw)
    }

    pub fn supported_languages() -> &'static [&'static str] {
        &language_catalog::CANONICAL_NAMES
    }
}

impl SyntaxHighlighter for BuiltinSyntaxHighlighter {
    /// An unknown or absent language yields no runs: the code block still gets
    /// its mono font and tint, it is simply uncoloured.
    fn highlight(&self, code: &[u16], language: Option<&str>) -> Vec<SyntaxRun> {
        let Some(raw) = language else {
            return Vec::new();
        };
        let Some(canonical) = BuiltinSyntaxHighlighter::canonical_language(raw) else {
            return Vec::new();
        };
        let Some(scanner) = language_catalog::scanner(canonical) else {
            return Vec::new();
        };
        if code.is_empty() {
            return Vec::new();
        }
        scanner.highlight(code)
    }

    fn supports(&self, language: &str) -> bool {
        BuiltinSyntaxHighlighter::canonical_language(language).is_some()
    }
}
