//! Port of `Syntax/LanguageSpec.swift`. A language is *described*, not
//! hand-coded: one scanner (`GenericLexer`) reads these descriptions. Only
//! genuinely different shapes (markup, diff, markdown) get their own scanner.

use super::scanner_core::Unit;
use super::syntax_contracts::SyntaxToken;
use super::word_table::WordTable;

#[derive(Debug, Clone)]
pub struct BlockCommentSpec {
    pub open: Vec<u8>,
    pub close: Vec<u8>,
    /// Swift and Rust nest `/* /* */ */`; C does not.
    pub nests: bool,
    /// Ruby's `=begin`/`=end` are only comments in column zero.
    pub must_start_line: bool,
}

impl BlockCommentSpec {
    pub fn new(open: &str, close: &str) -> Self {
        BlockCommentSpec::with(open, close, false, false)
    }

    pub fn with(open: &str, close: &str, nests: bool, must_start_line: bool) -> Self {
        BlockCommentSpec { open: open.as_bytes().to_vec(), close: close.as_bytes().to_vec(), nests, must_start_line }
    }
}

#[derive(Debug, Clone)]
pub struct StringSpec {
    pub open: Vec<u8>,
    pub close: Vec<u8>,
    /// `None` for delimiters where backslash is literal (shell `'…'`, Go `` `…` ``).
    pub escape: Option<u8>,
    /// When false an unescaped newline terminates the literal.
    pub spans_lines: bool,
}

impl StringSpec {
    /// `StringSpec(open)` with the Swift defaults: close = open, escape `\`,
    /// single-line.
    pub fn new(open: &str) -> Self {
        StringSpec::with(open, None, Some(b'\\'), false)
    }

    pub fn spanning(open: &str) -> Self {
        StringSpec::with(open, None, Some(b'\\'), true)
    }

    pub fn with(open: &str, close: Option<&str>, escape: Option<u8>, spans_lines: bool) -> Self {
        StringSpec {
            open: open.as_bytes().to_vec(),
            close: close.unwrap_or(open).as_bytes().to_vec(),
            escape,
            spans_lines,
        }
    }

    pub fn escaping(&self, enabled: bool) -> StringSpec {
        let mut copy = self.clone();
        if !enabled {
            copy.escape = None;
        }
        copy
    }
}

/// An identifier that turns an immediately following quote into a literal:
/// Python's `f"…"` and `r"…"`, Rust's `b"…"`.
#[derive(Debug, Clone)]
pub struct StringPrefixSpec {
    /// Lowercased; matching is case-insensitive because Python accepts `F"…"`.
    pub bytes: Vec<u8>,
    /// Python's `r` prefix makes backslash literal.
    pub escapes: bool,
}

impl StringPrefixSpec {
    pub fn new(text: &str) -> Self {
        StringPrefixSpec::with(text, true)
    }

    pub fn with(text: &str, escapes: bool) -> Self {
        // Every prefix literal is ASCII, where `lowercased()` is byte-wise.
        StringPrefixSpec { bytes: text.to_ascii_lowercase().into_bytes(), escapes }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawStringStyle {
    /// `#"…"#`, `##"…"##`, `#"""…"""#`
    SwiftHash,
    /// `r"…"`, `r#"…"#`, `br#"…"#`
    RustHash,
    /// `R"tag(…)tag"`
    CppDelimited,
}

#[derive(Debug, Clone)]
pub struct LanguageSpec {
    pub name: String,

    // Words
    pub words: WordTable,
    pub all_caps_are_constants: bool,
    pub capitalised_are_types: bool,
    pub calls_are_functions: bool,

    // Trivia
    pub line_comments: Vec<Vec<u8>>,
    pub line_comment_needs_word_start: bool,
    pub block_comments: Vec<BlockCommentSpec>,

    // Literals
    /// Ordered longest-open-first so `"""` is tried before `"`.
    pub strings: Vec<StringSpec>,
    pub string_prefixes: Vec<StringPrefixSpec>,
    pub raw_strings: Vec<RawStringStyle>,

    // Identifiers
    pub identifier_extra_starts: Vec<Unit>,
    pub identifier_extra_continues: Vec<Unit>,

    // Sigils
    pub attribute_sigils: Vec<Unit>,
    pub objc_string_sigil: bool,
    pub hash_attributes: bool,
    pub hash_directive_token: Option<SyntaxToken>,
    pub variable_sigils: Vec<Unit>,
    pub has_symbols: bool,
    pub has_lifetimes: bool,

    // Key/value shapes
    pub key_terminators: Vec<Unit>,
    pub keys_from_strings: bool,
    pub keys_from_identifiers: bool,
    pub key_terminator_needs_space: bool,
    pub bracket_section_headers: bool,
}

impl LanguageSpec {
    /// The memberwise initialiser's defaults.
    pub fn named(name: &str) -> LanguageSpec {
        LanguageSpec {
            name: name.to_owned(),
            words: WordTable::empty(),
            all_caps_are_constants: false,
            capitalised_are_types: false,
            calls_are_functions: false,
            line_comments: Vec::new(),
            line_comment_needs_word_start: false,
            block_comments: Vec::new(),
            strings: Vec::new(),
            string_prefixes: Vec::new(),
            raw_strings: Vec::new(),
            identifier_extra_starts: Vec::new(),
            identifier_extra_continues: Vec::new(),
            attribute_sigils: Vec::new(),
            objc_string_sigil: false,
            hash_attributes: false,
            hash_directive_token: None,
            variable_sigils: Vec::new(),
            has_symbols: false,
            has_lifetimes: false,
            key_terminators: Vec::new(),
            keys_from_strings: false,
            keys_from_identifiers: false,
            key_terminator_needs_space: false,
            bracket_section_headers: false,
        }
    }
}
