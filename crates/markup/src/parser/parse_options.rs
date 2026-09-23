//! Port of `Sources/Markdown/Parser/ParseOptions.swift`.
//!
//! `parseBlockDirectives` and `parseMinimalDoxygen` select swift-markdown's
//! `BlockDirectiveParser`, which Downright never enables and which is not
//! ported, so those two options are not offered here.

use std::ops::{BitOr, BitOrAssign};

/// Options for parsing Markdown (Swift `OptionSet` over `UInt`; the raw
/// values are swift-markdown's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ParseOptions {
    pub raw_value: u64,
}

impl ParseOptions {
    pub const EMPTY: ParseOptions = ParseOptions { raw_value: 0 };

    /// Enable interpretation of symbol links from inline code spans
    /// surrounded by two backticks.
    pub const PARSE_SYMBOL_LINKS: ParseOptions = ParseOptions { raw_value: 1 << 1 };

    /// Disable converting straight quotes to curly, `---` to em dashes, `--`
    /// to en dashes during parsing. Downright always passes this.
    pub const DISABLE_SMART_OPTS: ParseOptions = ParseOptions { raw_value: 1 << 2 };

    /// Disable including a `data-sourcepos` attribute on all block elements
    /// during parsing.
    pub const DISABLE_SOURCE_POS_OPTS: ParseOptions = ParseOptions { raw_value: 1 << 4 };

    pub const fn contains(self, other: ParseOptions) -> bool {
        self.raw_value & other.raw_value == other.raw_value
    }
}

impl BitOr for ParseOptions {
    type Output = ParseOptions;

    fn bitor(self, other: ParseOptions) -> ParseOptions {
        ParseOptions { raw_value: self.raw_value | other.raw_value }
    }
}

impl BitOrAssign for ParseOptions {
    fn bitor_assign(&mut self, other: ParseOptions) {
        self.raw_value |= other.raw_value;
    }
}
