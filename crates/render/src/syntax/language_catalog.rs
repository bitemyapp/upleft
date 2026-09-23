//! Port of `Syntax/LanguageCatalog.swift`: which scanner a canonical language
//! name uses, and the alias table.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use super::generic_lexer::GenericLexer;
use super::language_definitions;
use super::language_spec::LanguageSpec;
use super::line_lexers::{DiffLexer, MarkdownLexer};
use super::markup_lexer::MarkupLexer;
use super::scanner_core::Unit;
use super::syntax_contracts::SyntaxRun;
use crate::swift_compat;

#[derive(Debug, Clone, Copy)]
pub enum LanguageScanner {
    Generic(&'static LanguageSpec),
    Markup,
    Diff,
    Markdown,
    /// A known language we deliberately do not colour.
    Unstyled,
}

impl LanguageScanner {
    pub fn highlight(&self, units: &[Unit]) -> Vec<SyntaxRun> {
        match self {
            LanguageScanner::Generic(spec) => GenericLexer::highlight(units, spec),
            LanguageScanner::Markup => MarkupLexer::highlight(units),
            LanguageScanner::Diff => DiffLexer::highlight(units),
            LanguageScanner::Markdown => MarkdownLexer::highlight(units),
            LanguageScanner::Unstyled => Vec::new(),
        }
    }
}

pub const CANONICAL_NAMES: [&str; 24] = [
    "bash", "c", "cpp", "css", "diff", "go", "html", "java", "javascript", "json", "jsx", "markdown", "objc",
    "plaintext", "python", "ruby", "rust", "sql", "swift", "toml", "tsx", "typescript", "xml", "yaml",
];

/// Alias → canonical. Agents label fences with whatever the ecosystem calls
/// the language, so this has to be generous.
pub const ALIASES: [(&str, &str); 49] = [
    ("sh", "bash"),
    ("shell", "bash"),
    ("zsh", "bash"),
    ("console", "bash"),
    ("shell-session", "bash"),
    ("c++", "cpp"),
    ("cxx", "cpp"),
    ("cc", "cpp"),
    ("hpp", "cpp"),
    ("h", "c"),
    ("objective-c", "objc"),
    ("objectivec", "objc"),
    ("obj-c", "objc"),
    ("m", "objc"),
    ("mm", "objc"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("node", "javascript"),
    ("ts", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("py", "python"),
    ("python3", "python"),
    ("rs", "rust"),
    ("rb", "ruby"),
    ("golang", "go"),
    ("yml", "yaml"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("htm", "html"),
    ("svg", "xml"),
    ("xhtml", "html"),
    ("plist", "xml"),
    ("md", "markdown"),
    ("mdown", "markdown"),
    ("mkd", "markdown"),
    ("patch", "diff"),
    ("udiff", "diff"),
    ("text", "plaintext"),
    ("txt", "plaintext"),
    ("plain", "plaintext"),
    ("none", "plaintext"),
    ("postgres", "sql"),
    ("postgresql", "sql"),
    ("mysql", "sql"),
    ("sqlite", "sql"),
    ("scss", "css"),
    ("less", "css"),
];

/// `LanguageCatalog.canonical(_:)`.
pub fn canonical(raw: &str) -> Option<&'static str> {
    let key = swift_compat::lowercased(swift_compat::trim_whitespaces_and_newlines(raw));
    if key.is_empty() {
        return None;
    }
    if let Some(name) = CANONICAL_NAMES.iter().find(|name| **name == key) {
        return Some(name);
    }
    ALIASES.iter().find(|(alias, _)| *alias == key).map(|(_, canonical)| *canonical)
}

/// Specs are built on first use and cached: constructing twenty `WordTable`s
/// eagerly would cost the launch budget for languages a document never
/// mentions.
static CACHE: LazyLock<Mutex<HashMap<String, LanguageScanner>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn scanner(canonical_name: &str) -> Option<LanguageScanner> {
    let mut cache = CACHE.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(cached) = cache.get(canonical_name) {
        return Some(*cached);
    }
    let built = build(canonical_name)?;
    cache.insert(canonical_name.to_owned(), built);
    Some(built)
}

fn build(name: &str) -> Option<LanguageScanner> {
    match name {
        "html" | "xml" => Some(LanguageScanner::Markup),
        "diff" => Some(LanguageScanner::Diff),
        "markdown" => Some(LanguageScanner::Markdown),
        "plaintext" => Some(LanguageScanner::Unstyled),
        // A spec is built at most once per canonical name for the life of the
        // process, exactly as the Swift cache holds it; leaking gives the
        // scanners a `'static` borrow without reference counting per call.
        _ => language_definitions::spec(name).map(|spec| LanguageScanner::Generic(Box::leak(Box::new(spec)))),
    }
}
