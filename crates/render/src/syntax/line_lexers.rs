//! Port of `Syntax/LineLexers.swift`: two languages whose structure is the
//! line, not the token.

use super::scanner_core::{RunBuilder, Unit, ascii_table, is_blank_unit, is_digit_unit, is_newline_unit, member, of};
use super::syntax_contracts::{SyntaxRun, SyntaxToken};

/// ```` ```diff ```` fences get real diff colouring (§11.3).
pub struct DiffLexer<'a> {
    units: &'a [Unit],
    count: usize,
    builder: RunBuilder,
}

/// Header prefixes, longest first: `---`/`+++` are file headers and must be
/// tested before the `-`/`+` line markers they start with.
const DIFF_HEADERS: [&[u8]; 14] = [
    b"---",
    b"+++",
    b"@@",
    b"diff ",
    b"index ",
    b"new file",
    b"deleted file",
    b"old mode",
    b"new mode",
    b"similarity index",
    b"rename from",
    b"rename to",
    b"Binary files",
    b"\\ No newline",
];

impl<'a> DiffLexer<'a> {
    pub fn highlight(units: &[Unit]) -> Vec<SyntaxRun> {
        let mut lexer = DiffLexer { units, count: units.len(), builder: RunBuilder::new(units.len()) };
        lexer.run();
        lexer.builder.finish()
    }

    fn run(&mut self) {
        let mut line_start = 0;
        while line_start < self.count {
            let mut line_end = line_start;
            while line_end < self.count && !is_newline_unit(self.units[line_end]) {
                line_end += 1;
            }
            self.classify(line_start, line_end);
            line_start = line_end;
            while line_start < self.count && is_newline_unit(self.units[line_start]) {
                line_start += 1;
            }
        }
    }

    fn classify(&mut self, lower: usize, upper: usize) {
        if lower == upper {
            return;
        }
        for header in DIFF_HEADERS {
            if self.matches(header, lower, upper) {
                self.builder.emit(SyntaxToken::DiffHeader, lower, upper);
                return;
            }
        }
        match self.units[lower] {
            u if u == of('+') => self.builder.emit(SyntaxToken::DiffAdded, lower, upper),
            u if u == of('-') => self.builder.emit(SyntaxToken::DiffRemoved, lower, upper),
            _ => self.builder.emit(SyntaxToken::Plain, lower, upper),
        }
    }

    fn matches(&self, bytes: &[u8], pos: usize, limit: usize) -> bool {
        if pos + bytes.len() > limit {
            return false;
        }
        bytes.iter().enumerate().all(|(k, byte)| self.units[pos + k] == *byte as Unit)
    }
}

/// Markdown inside markdown. Block structure is line-oriented; inline
/// structure is a single forward pass that colours delimiters and link
/// destinations and leaves prose alone.
pub struct MarkdownLexer<'a> {
    units: &'a [Unit],
    count: usize,
    builder: RunBuilder,
}

static EMPHASIS_UNITS: [bool; 128] = ascii_table("*_~");

impl<'a> MarkdownLexer<'a> {
    pub fn highlight(units: &[Unit]) -> Vec<SyntaxRun> {
        let mut lexer = MarkdownLexer { units, count: units.len(), builder: RunBuilder::new(units.len()) };
        lexer.run();
        lexer.builder.finish()
    }

    fn run(&mut self) {
        let mut fence: Option<(Unit, usize)> = None;
        let mut line_start = 0;
        while line_start < self.count {
            let mut line_end = line_start;
            while line_end < self.count && !is_newline_unit(self.units[line_end]) {
                line_end += 1;
            }
            if let Some(open) = fence {
                if let Some(close) = self.fence_run(line_start, line_end)
                    && close.0 == open.0
                    && close.1 >= open.1
                {
                    self.builder.emit(SyntaxToken::Attribute, line_start, line_end);
                    fence = None;
                } else {
                    self.builder.emit(SyntaxToken::String, line_start, line_end);
                }
            } else if let Some(open) = self.fence_run(line_start, line_end) {
                self.builder.emit(SyntaxToken::Attribute, line_start, line_end);
                fence = Some(open);
            } else {
                self.classify_block(line_start, line_end);
            }
            line_start = line_end;
            while line_start < self.count && is_newline_unit(self.units[line_start]) {
                line_start += 1;
            }
        }
    }

    /// A run of three or more backticks or tildes with nothing but the info
    /// string after it.
    fn fence_run(&self, lower: usize, upper: usize) -> Option<(Unit, usize)> {
        let mut j = lower;
        while j < upper && is_blank_unit(self.units[j]) {
            j += 1;
        }
        if j >= upper {
            return None;
        }
        let marker = self.units[j];
        if !(marker == of('`') || marker == of('~')) {
            return None;
        }
        let mut length = 0;
        while j < upper && self.units[j] == marker {
            length += 1;
            j += 1;
        }
        if length >= 3 { Some((marker, length)) } else { None }
    }

    fn classify_block(&mut self, lower: usize, upper: usize) {
        if lower == upper {
            return;
        }
        let mut j = lower;
        while j < upper && is_blank_unit(self.units[j]) {
            j += 1;
        }
        if j >= upper {
            return;
        }

        if self.units[j] == of('#') {
            let mut k = j;
            while k < upper && self.units[k] == of('#') {
                k += 1;
            }
            if k - j <= 6 && (k >= upper || is_blank_unit(self.units[k])) {
                self.builder.emit(SyntaxToken::Keyword, j, upper);
                return;
            }
        }
        if self.is_thematic_break(j, upper) {
            self.builder.emit(SyntaxToken::Operator, j, upper);
            return;
        }
        if self.units[j] == of('>') {
            let mut k = j;
            while k < upper && (self.units[k] == of('>') || is_blank_unit(self.units[k])) {
                k += 1;
            }
            self.builder.emit(SyntaxToken::Comment, j, k);
            self.scan_inline(k, upper);
            return;
        }
        if let Some(marker) = self.list_marker(j, upper) {
            self.builder.emit(SyntaxToken::Operator, j, marker);
            self.scan_inline(marker, upper);
            return;
        }
        self.scan_inline(j, upper);
    }

    /// `---`, `***`, `___`: three or more of one character and nothing else.
    fn is_thematic_break(&self, lower: usize, upper: usize) -> bool {
        if lower >= upper {
            return false;
        }
        let first = self.units[lower];
        if !(first == of('-') || first == of('*') || first == of('_')) {
            return false;
        }
        let mut seen = 0;
        for k in lower..upper {
            if self.units[k] == first {
                seen += 1;
                continue;
            }
            if is_blank_unit(self.units[k]) {
                continue;
            }
            return false;
        }
        seen >= 3
    }

    /// End index of a `-`/`*`/`+`/`1.` marker plus its trailing space, or nil.
    fn list_marker(&self, lower: usize, upper: usize) -> Option<usize> {
        let mut j = lower;
        let first = self.units[j];
        if first == of('-') || first == of('*') || first == of('+') {
            j += 1;
        } else if is_digit_unit(first) {
            while j < upper && is_digit_unit(self.units[j]) {
                j += 1;
            }
            if !(j < upper && (self.units[j] == of('.') || self.units[j] == of(')'))) {
                return None;
            }
            j += 1;
        } else {
            return None;
        }
        if !(j < upper && is_blank_unit(self.units[j])) {
            return None;
        }
        while j < upper && is_blank_unit(self.units[j]) {
            j += 1;
        }
        Some(j)
    }

    fn scan_inline(&mut self, lower: usize, upper: usize) {
        let mut j = lower;
        let mut plain_start = j;
        while j < upper {
            let c = self.units[j];
            if c == of('`') {
                self.builder.emit(SyntaxToken::Plain, plain_start, j);
                j = self.scan_code_span(j, upper);
                plain_start = j;
                continue;
            }
            if c == of('[') || (c == of('!') && j + 1 < upper && self.units[j + 1] == of('[')) {
                self.builder.emit(SyntaxToken::Plain, plain_start, j);
                j = self.scan_link(j, upper);
                plain_start = j;
                continue;
            }
            if member(c, &EMPHASIS_UNITS) {
                self.builder.emit(SyntaxToken::Plain, plain_start, j);
                let start = j;
                while j < upper && self.units[j] == c {
                    j += 1;
                }
                self.builder.emit(SyntaxToken::Punctuation, start, j);
                plain_start = j;
                continue;
            }
            j += 1;
        }
        self.builder.emit(SyntaxToken::Plain, plain_start, upper);
    }

    fn scan_code_span(&mut self, start: usize, limit: usize) -> usize {
        let mut j = start;
        let mut ticks = 0;
        while j < limit && self.units[j] == of('`') {
            ticks += 1;
            j += 1;
        }
        while j < limit {
            if self.units[j] == of('`') {
                let mut closing = 0;
                while j < limit && self.units[j] == of('`') {
                    closing += 1;
                    j += 1;
                }
                if closing == ticks {
                    break;
                }
                continue;
            }
            j += 1;
        }
        self.builder.emit(SyntaxToken::String, start, j);
        j
    }

    /// `[label](destination)` and `![alt](src)`.
    fn scan_link(&mut self, start: usize, limit: usize) -> usize {
        let mut j = start;
        if self.units[j] == of('!') {
            j += 1;
        }
        self.builder.emit(SyntaxToken::Punctuation, start, j + 1);
        j += 1;
        let label_start = j;
        let mut depth = 1;
        while j < limit && depth > 0 {
            if self.units[j] == of('[') {
                depth += 1;
            }
            if self.units[j] == of(']') {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            j += 1;
        }
        self.builder.emit(SyntaxToken::Plain, label_start, j);
        if j >= limit {
            return j;
        }
        let close_bracket = j;
        j += 1;
        self.builder.emit(SyntaxToken::Punctuation, close_bracket, j);
        if !(j < limit && (self.units[j] == of('(') || self.units[j] == of('['))) {
            return j;
        }
        let opener = self.units[j];
        let closer = if opener == of('(') { of(')') } else { of(']') };
        let destination_start = j;
        j += 1;
        while j < limit && self.units[j] != closer {
            j += 1;
        }
        j = limit.min(j + 1);
        self.builder.emit(SyntaxToken::String, destination_start, j);
        j
    }
}
