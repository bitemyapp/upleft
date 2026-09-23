//! Port of `Syntax/ScannerCore.swift`: shared scanning primitives.
//!
//! Every lexer here works directly over UTF-16 code units. `NSRange` is
//! UTF-16-based so the offsets emitted are free, and no string is ever built
//! per token.
//!
//! Every syntax-significant character in every supported language is ASCII.
//! Units at or above 0x80, surrogate halves included, are treated uniformly as
//! identifier characters, which also guarantees a surrogate pair is never
//! split across two runs.

use objc2_foundation::NSRange;

use super::syntax_contracts::{SyntaxRun, SyntaxToken};

pub type Unit = u16;

/// `Unit.of("#")`: an ASCII literal as a code unit.
#[inline(always)]
pub const fn of(c: char) -> Unit {
    c as Unit
}

#[inline(always)]
pub fn is_digit_unit(u: Unit) -> bool {
    (0x30..=0x39).contains(&u)
}

#[inline(always)]
pub fn is_letter_unit(u: Unit) -> bool {
    let lower = u | 0x20;
    (0x61..=0x7A).contains(&lower)
}

#[inline(always)]
pub fn is_upper_unit(u: Unit) -> bool {
    (0x41..=0x5A).contains(&u)
}

#[inline(always)]
pub fn is_hex_digit_unit(u: Unit) -> bool {
    if is_digit_unit(u) {
        return true;
    }
    let lower = u | 0x20;
    (0x61..=0x66).contains(&lower)
}

#[inline(always)]
pub fn is_newline_unit(u: Unit) -> bool {
    u == 0x0A || u == 0x0D
}

/// Space or tab: "blank" in the POSIX sense, i.e. horizontal only.
#[inline(always)]
pub fn is_blank_unit(u: Unit) -> bool {
    u == 0x20 || u == 0x09
}

#[inline(always)]
pub fn is_space_unit(u: Unit) -> bool {
    u == 0x20 || (0x09..=0x0D).contains(&u)
}

/// Non-ASCII units are letters as far as the lexers are concerned.
#[inline(always)]
pub fn is_word_unit(u: Unit) -> bool {
    is_letter_unit(u) || is_digit_unit(u) || u == 0x5F || u >= 0x80
}

/// A 128-entry membership table.
pub const fn ascii_table(characters: &str) -> [bool; 128] {
    let mut table = [false; 128];
    let bytes = characters.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        // Swift walks unicode scalars and keeps those below 128; every table
        // literal in the lexers is ASCII, so bytes are the same scalars.
        if bytes[index] < 128 {
            table[bytes[index] as usize] = true;
        }
        index += 1;
    }
    table
}

#[inline(always)]
pub fn member(u: Unit, table: &[bool; 128]) -> bool {
    u < 128 && table[u as usize]
}

/// Collects runs while maintaining the three invariants `SyntaxHighlighter`
/// promises: ascending, non-overlapping, inside the input. Adjacent runs of
/// the same token are merged.
pub struct RunBuilder {
    runs: Vec<SyntaxRun>,
}

impl RunBuilder {
    pub fn new(reserving_for_unit_count: usize) -> Self {
        // Empirically ~1 run per 6 code units of source.
        RunBuilder {
            runs: Vec::with_capacity(8.max(reserving_for_unit_count / 6)),
        }
    }

    #[inline]
    pub fn emit(&mut self, token: SyntaxToken, start: usize, end: usize) {
        // Swift's `Range<Int>` traps on an inverted range; the lexers never
        // build one, and the guard below is the Swift guard.
        debug_assert!(start <= end, "inverted range {start}..<{end}");
        if start >= end {
            return;
        }
        if let Some(last) = self.runs.last_mut()
            && last.token == token
            && last.range.location + last.range.length == start
        {
            last.range.length += end - start;
            return;
        }
        self.runs.push(SyntaxRun {
            range: NSRange::new(start, end - start),
            token,
        });
    }

    pub fn runs(&self) -> &[SyntaxRun] {
        &self.runs
    }

    pub fn finish(self) -> Vec<SyntaxRun> {
        self.runs
    }
}
