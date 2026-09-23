//! Port of `Syntax/MarkupLexer.swift`: HTML and XML. Markup inverts the usual
//! shape (text is the default and code is the exception), so it gets its own
//! scanner rather than a contorted `LanguageSpec`.

use super::scanner_core::{RunBuilder, Unit, ascii_table, is_space_unit, is_word_unit, member, of};
use super::syntax_contracts::{SyntaxRun, SyntaxToken};

pub struct MarkupLexer<'a> {
    units: &'a [Unit],
    count: usize,
    i: usize,
    builder: RunBuilder,
}

const COMMENT_OPEN: &[u8] = b"<!--";
const COMMENT_CLOSE: &[u8] = b"-->";
const CDATA_OPEN: &[u8] = b"<![CDATA[";
const CDATA_CLOSE: &[u8] = b"]]>";
const DECLARATION_OPEN: &[u8] = b"<!";
const PROCESSING_OPEN: &[u8] = b"<?";

static NAME_UNITS: [bool; 128] = ascii_table("-_:.");

impl<'a> MarkupLexer<'a> {
    pub fn highlight(units: &[Unit]) -> Vec<SyntaxRun> {
        let mut lexer = MarkupLexer {
            units,
            count: units.len(),
            i: 0,
            builder: RunBuilder::new(units.len()),
        };
        lexer.run();
        lexer.builder.finish()
    }

    fn run(&mut self) {
        while self.i < self.count {
            let c = self.units[self.i];
            if c == of('<') {
                self.scan_angle_construct();
                continue;
            }
            if c == of('&') {
                self.scan_entity();
                continue;
            }
            let start = self.i;
            while self.i < self.count
                && self.units[self.i] != of('<')
                && self.units[self.i] != of('&')
            {
                self.i += 1;
            }
            self.builder.emit(SyntaxToken::Plain, start, self.i);
        }
    }

    fn scan_angle_construct(&mut self) {
        let start = self.i;
        if self.matches(COMMENT_OPEN) {
            self.i += COMMENT_OPEN.len();
            while self.i < self.count && !self.matches(COMMENT_CLOSE) {
                self.i += 1;
            }
            self.i = self.count.min(self.i + COMMENT_CLOSE.len());
            self.builder.emit(SyntaxToken::Comment, start, self.i);
            return;
        }
        if self.matches(CDATA_OPEN) {
            self.i += CDATA_OPEN.len();
            while self.i < self.count && !self.matches(CDATA_CLOSE) {
                self.i += 1;
            }
            self.i = self.count.min(self.i + CDATA_CLOSE.len());
            self.builder.emit(SyntaxToken::String, start, self.i);
            return;
        }
        // `<!DOCTYPE …>` and `<?xml …?>` are declarations, not elements.
        if self.matches(DECLARATION_OPEN) || self.matches(PROCESSING_OPEN) {
            while self.i < self.count && self.units[self.i] != of('>') {
                self.i += 1;
            }
            self.i = self.count.min(self.i + 1);
            self.builder.emit(SyntaxToken::Attribute, start, self.i);
            return;
        }
        self.i += 1;
        if self.i < self.count && self.units[self.i] == of('/') {
            self.i += 1;
        }
        self.builder.emit(SyntaxToken::Punctuation, start, self.i);
        self.scan_name(SyntaxToken::Type);
        self.scan_attributes();
    }

    fn scan_attributes(&mut self) {
        while self.i < self.count {
            let c = self.units[self.i];
            if is_space_unit(c) {
                self.i += 1;
                continue;
            }
            if c == of('>') || c == of('/') {
                let start = self.i;
                while self.i < self.count
                    && (self.units[self.i] == of('/') || self.units[self.i] == of('>'))
                {
                    self.i += 1;
                }
                self.builder.emit(SyntaxToken::Punctuation, start, self.i);
                return;
            }
            if c == of('=') {
                self.i += 1;
                self.builder.emit(SyntaxToken::Operator, self.i - 1, self.i);
                continue;
            }
            if c == of('"') || c == of('\'') {
                self.scan_quoted(c);
                continue;
            }
            if is_name_unit(c) {
                self.scan_name(SyntaxToken::Attribute);
                continue;
            }
            let start = self.i;
            self.i += 1;
            self.builder.emit(SyntaxToken::Plain, start, self.i);
        }
    }

    fn scan_quoted(&mut self, delimiter: Unit) {
        let start = self.i;
        self.i += 1;
        while self.i < self.count && self.units[self.i] != delimiter {
            self.i += 1;
        }
        self.i = self.count.min(self.i + 1);
        self.builder.emit(SyntaxToken::String, start, self.i);
    }

    fn scan_name(&mut self, token: SyntaxToken) {
        let start = self.i;
        while self.i < self.count && is_name_unit(self.units[self.i]) {
            self.i += 1;
        }
        self.builder.emit(token, start, self.i);
    }

    fn scan_entity(&mut self) {
        let start = self.i;
        let mut j = self.i + 1;
        while j < self.count && (is_name_unit(self.units[j]) || self.units[j] == of('#')) {
            j += 1;
        }
        if !(j < self.count && self.units[j] == of(';') && j > self.i + 1) {
            self.i += 1;
            self.builder.emit(SyntaxToken::Plain, start, self.i);
            return;
        }
        self.i = j + 1;
        self.builder.emit(SyntaxToken::Constant, start, self.i);
    }

    #[inline(always)]
    fn matches(&self, bytes: &[u8]) -> bool {
        if self.i + bytes.len() > self.count {
            return false;
        }
        bytes
            .iter()
            .enumerate()
            .all(|(k, byte)| self.units[self.i + k] == *byte as Unit)
    }
}

#[inline(always)]
fn is_name_unit(c: Unit) -> bool {
    is_word_unit(c) || member(c, &NAME_UNITS)
}
