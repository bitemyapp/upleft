//! Port of `Syntax/GenericLexer.swift`: the shared scanner. One pass, no
//! backtracking beyond a fixed lookahead, no regular expressions.
//!
//! Order of attempts inside the loop is the whole correctness story: trivia
//! before literals means a comment marker inside a string is never a comment
//! and a quote inside a comment is never a string.

use super::language_spec::{LanguageSpec, RawStringStyle, StringPrefixSpec, StringSpec};
use super::scanner_core::{
    RunBuilder, Unit, ascii_table, is_blank_unit, is_digit_unit, is_hex_digit_unit, is_letter_unit,
    is_newline_unit, is_space_unit, is_upper_unit, is_word_unit, member, of,
};
use super::syntax_contracts::{SyntaxRun, SyntaxToken};

pub struct GenericLexer<'a> {
    spec: &'a LanguageSpec,
    units: &'a [Unit],
    count: usize,
    i: usize,
    builder: RunBuilder,
}

static OPERATOR_UNITS: [bool; 128] = ascii_table("+-*/%=<>!&|^~?");
static PUNCTUATION_UNITS: [bool; 128] = ascii_table("()[]{},;.:");

impl<'a> GenericLexer<'a> {
    pub fn new(spec: &'a LanguageSpec, units: &'a [Unit]) -> Self {
        GenericLexer {
            spec,
            units,
            count: units.len(),
            i: 0,
            builder: RunBuilder::new(units.len()),
        }
    }

    pub fn highlight(units: &[Unit], spec: &LanguageSpec) -> Vec<SyntaxRun> {
        let mut lexer = GenericLexer::new(spec, units);
        lexer.run();
        lexer.builder.finish()
    }

    fn run(&mut self) {
        let spec = self.spec;
        while self.i < self.count {
            let c = self.units[self.i];
            if is_space_unit(c) {
                self.i += 1;
                continue;
            }
            let start = self.i;

            if self.scan_trivia() {
                self.builder.emit(SyntaxToken::Comment, start, self.i);
                continue;
            }
            if self.scan_raw_string() {
                self.builder.emit(SyntaxToken::String, start, self.i);
                continue;
            }
            if self.scan_sigil(start) {
                continue;
            }
            if let Some(string) = self.match_string_open(self.i as isize) {
                self.scan_string(string);
                // `"key": value`: a quoted key is an attribute, not a value.
                let is_key = spec.keys_from_strings && self.is_followed_by_key_terminator();
                self.builder.emit(
                    if is_key {
                        SyntaxToken::Attribute
                    } else {
                        SyntaxToken::String
                    },
                    start,
                    self.i,
                );
                continue;
            }
            if self.is_number_start() {
                self.scan_number();
                self.builder.emit(SyntaxToken::Number, start, self.i);
                continue;
            }
            if self.is_identifier_start(self.i as isize) {
                self.scan_identifier_token(start);
                continue;
            }
            if spec.bracket_section_headers && c == of('[') && self.is_at_line_start(self.i) {
                self.scan_balanced(of('['), of(']'));
                self.builder.emit(SyntaxToken::Type, start, self.i);
                continue;
            }
            if is_operator_unit(c) {
                while self.i < self.count && is_operator_unit(self.units[self.i]) {
                    self.i += 1;
                }
                self.builder.emit(SyntaxToken::Operator, start, self.i);
                continue;
            }
            self.i += 1;
            self.builder.emit(
                if is_punctuation_unit(c) {
                    SyntaxToken::Punctuation
                } else {
                    SyntaxToken::Plain
                },
                start,
                self.i,
            );
        }
    }

    // MARK: - Primitives

    #[inline(always)]
    fn matches(&self, bytes: &[u8], pos: isize) -> bool {
        if pos < 0 || pos as usize + bytes.len() > self.count {
            return false;
        }
        let pos = pos as usize;
        for (k, byte) in bytes.iter().enumerate() {
            if self.units[pos + k] != *byte as Unit {
                return false;
            }
        }
        true
    }

    #[inline(always)]
    fn unit(&self, offset: isize) -> Unit {
        let index = self.i as isize + offset;
        if index < self.count as isize && index >= 0 {
            self.units[index as usize]
        } else {
            0
        }
    }

    /// Only blanks precede `pos` on its line.
    fn is_at_line_start(&self, pos: usize) -> bool {
        let mut j = pos as isize - 1;
        while j >= 0 && is_blank_unit(self.units[j as usize]) {
            j -= 1;
        }
        j < 0 || is_newline_unit(self.units[j as usize])
    }

    /// Bash's rule for `#`: it opens a comment only at the start of a word.
    fn is_at_word_start(&self, pos: usize) -> bool {
        if pos == 0 {
            return true;
        }
        let previous = self.units[pos - 1];
        if is_space_unit(previous) {
            return true;
        }
        previous == of(';')
            || previous == of('&')
            || previous == of('|')
            || previous == of('(')
            || previous == of(')')
            || previous == of('`')
    }

    // MARK: - Trivia

    fn scan_trivia(&mut self) -> bool {
        let spec = self.spec;
        for marker in &spec.line_comments {
            if !self.matches(marker, self.i as isize) {
                continue;
            }
            if spec.line_comment_needs_word_start && !self.is_at_word_start(self.i) {
                continue;
            }
            self.i += marker.len();
            while self.i < self.count && !is_newline_unit(self.units[self.i]) {
                self.i += 1;
            }
            return true;
        }
        for comment in &spec.block_comments {
            if !self.matches(&comment.open, self.i as isize) {
                continue;
            }
            if comment.must_start_line && !self.is_at_line_start(self.i) {
                continue;
            }
            self.i += comment.open.len();
            let mut depth = 1;
            while self.i < self.count {
                if comment.nests && self.matches(&comment.open, self.i as isize) {
                    depth += 1;
                    self.i += comment.open.len();
                    continue;
                }
                if self.matches(&comment.close, self.i as isize) {
                    self.i += comment.close.len();
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                self.i += 1;
            }
            return true;
        }
        false
    }

    // MARK: - Strings

    fn match_string_open(&self, pos: isize) -> Option<&'a StringSpec> {
        self.spec
            .strings
            .iter()
            .find(|string| self.matches(&string.open, pos))
    }

    fn scan_string(&mut self, string: &StringSpec) {
        self.i += string.open.len();
        while self.i < self.count {
            let c = self.units[self.i];
            if let Some(escape) = string.escape
                && c == escape as Unit
            {
                self.i += 2.min(self.count - self.i);
                continue;
            }
            if !string.spans_lines && is_newline_unit(c) {
                return;
            }
            if self.matches(&string.close, self.i as isize) {
                self.i += string.close.len();
                return;
            }
            self.i += 1;
        }
    }

    /// Raw-string forms are checked before identifiers because their prefixes
    /// (`r`, `br`, `R`) are identifier characters.
    fn scan_raw_string(&mut self) -> bool {
        for style in &self.spec.raw_strings {
            match style {
                RawStringStyle::SwiftHash => {
                    if self.scan_swift_raw_string() {
                        return true;
                    }
                }
                RawStringStyle::RustHash => {
                    if self.scan_rust_raw_string() {
                        return true;
                    }
                }
                RawStringStyle::CppDelimited => {
                    if self.scan_cpp_raw_string() {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// `#"…"#`, `##"…"##`, `#"""…"""#`.
    fn scan_swift_raw_string(&mut self) -> bool {
        let save = self.i;
        let mut hashes = 0;
        while self.i < self.count && self.units[self.i] == of('#') {
            hashes += 1;
            self.i += 1;
        }
        if hashes == 0 {
            self.i = save;
            return false;
        }
        let quotes = if self.matches(&[0x22, 0x22, 0x22], self.i as isize) {
            3
        } else if self.unit(0) == of('"') {
            1
        } else {
            0
        };
        if quotes == 0 {
            self.i = save;
            return false;
        }
        self.i += quotes;
        self.scan_to_raw_terminator(quotes, hashes);
        true
    }

    /// `r"…"`, `r#"…"#`, `br"…"`, `br#"…"#`.
    fn scan_rust_raw_string(&mut self) -> bool {
        let save = self.i;
        let mut j = self.i;
        if j < self.count && self.units[j] == of('b') {
            j += 1;
        }
        if !(j < self.count && self.units[j] == of('r')) {
            self.i = save;
            return false;
        }
        j += 1;
        let mut hashes = 0;
        while j < self.count && self.units[j] == of('#') {
            hashes += 1;
            j += 1;
        }
        if !(j < self.count && self.units[j] == of('"')) {
            self.i = save;
            return false;
        }
        self.i = j + 1;
        self.scan_to_raw_terminator(1, hashes);
        true
    }

    /// `R"tag(…)tag"`.
    fn scan_cpp_raw_string(&mut self) -> bool {
        if !(self.unit(0) == of('R') && self.unit(1) == of('"')) {
            return false;
        }
        self.i += 2;
        let mut tag: Vec<Unit> = Vec::new();
        while self.i < self.count
            && self.units[self.i] != of('(')
            && !is_newline_unit(self.units[self.i])
        {
            tag.push(self.units[self.i]);
            self.i += 1;
        }
        if !(self.i < self.count && self.units[self.i] == of('(')) {
            return true;
        }
        self.i += 1;
        while self.i < self.count {
            if self.units[self.i] == of(')') && self.matches_terminator(&tag, self.i + 1) {
                self.i += 1 + tag.len() + 1;
                return true;
            }
            self.i += 1;
        }
        true
    }

    fn matches_terminator(&self, tag: &[Unit], pos: usize) -> bool {
        if pos + tag.len() >= self.count {
            return false;
        }
        for (k, unit) in tag.iter().enumerate() {
            if self.units[pos + k] != *unit {
                return false;
            }
        }
        self.units[pos + tag.len()] == of('"')
    }

    fn scan_to_raw_terminator(&mut self, quotes: usize, hashes: usize) {
        while self.i < self.count {
            if self.units[self.i] == of('"') {
                let mut k = 0;
                while k < quotes && self.i + k < self.count && self.units[self.i + k] == of('"') {
                    k += 1;
                }
                if k == quotes {
                    let mut h = 0;
                    while h < hashes
                        && self.i + quotes + h < self.count
                        && self.units[self.i + quotes + h] == of('#')
                    {
                        h += 1;
                    }
                    if h == hashes {
                        self.i += quotes + hashes;
                        return;
                    }
                }
                self.i += 1.max(k);
                continue;
            }
            self.i += 1;
        }
    }

    // MARK: - Sigils

    fn scan_sigil(&mut self, start: usize) -> bool {
        let spec = self.spec;
        let c = self.units[self.i];

        // Objective-C `@"…"`: the sigil belongs to the literal.
        if spec.objc_string_sigil
            && c == of('@')
            && let Some(string) = self.match_string_open(self.i as isize + 1)
        {
            self.i += 1;
            self.scan_string(string);
            self.builder.emit(SyntaxToken::String, start, self.i);
            return true;
        }
        if spec.variable_sigils.contains(&c) {
            let mut j = self.i + 1;
            while j < self.count && self.units[j] == c {
                j += 1; // `$$`, Ruby `@@ivar`
            }
            if j < self.count && (is_word_unit(self.units[j]) || self.units[j] == of('{')) {
                self.i = j;
                if self.units[self.i] == of('{') {
                    self.scan_balanced(of('{'), of('}'));
                } else {
                    while self.i < self.count && self.is_identifier_continue(self.units[self.i]) {
                        self.i += 1;
                    }
                }
                self.builder.emit(SyntaxToken::Variable, start, self.i);
                return true;
            }
        }
        if spec.attribute_sigils.contains(&c) && self.is_identifier_start(self.i as isize + 1) {
            self.i += 1;
            while self.i < self.count && self.is_identifier_continue(self.units[self.i]) {
                self.i += 1;
            }
            self.builder.emit(SyntaxToken::Attribute, start, self.i);
            return true;
        }
        // Rust attributes read as one unit.
        if spec.hash_attributes
            && c == of('#')
            && (self.unit(1) == of('[') || self.unit(1) == of('!'))
        {
            self.i += 1;
            if self.i < self.count && self.units[self.i] == of('!') {
                self.i += 1;
            }
            if self.i < self.count && self.units[self.i] == of('[') {
                self.scan_balanced(of('['), of(']'));
            }
            self.builder.emit(SyntaxToken::Attribute, start, self.i);
            return true;
        }
        if let Some(token) = spec.hash_directive_token
            && c == of('#')
        {
            self.i += 1;
            while self.i < self.count && self.is_identifier_continue(self.units[self.i]) {
                self.i += 1;
            }
            self.builder.emit(token, start, self.i);
            return true;
        }
        if spec.has_symbols
            && c == of(':')
            && self.is_identifier_start(self.i as isize + 1)
            && self.unit(-1) != of(':')
        {
            self.i += 1;
            while self.i < self.count && self.is_identifier_continue(self.units[self.i]) {
                self.i += 1;
            }
            self.builder.emit(SyntaxToken::Constant, start, self.i);
            return true;
        }
        if spec.has_lifetimes && c == of('\'') && !self.is_character_literal(self.i) {
            self.i += 1;
            while self.i < self.count && self.is_identifier_continue(self.units[self.i]) {
                self.i += 1;
            }
            self.builder.emit(SyntaxToken::Keyword, start, self.i);
            return true;
        }
        false
    }

    /// `'a'`, `'\n'`, `'😀'`: anything else after a quote is a Rust lifetime.
    fn is_character_literal(&self, pos: usize) -> bool {
        if pos + 2 >= self.count {
            return false;
        }
        if self.units[pos + 1] == of('\\') {
            return true;
        }
        if self.units[pos + 2] == of('\'') {
            return true;
        }
        // A surrogate pair is two units wide.
        if self.units[pos + 1] >= 0xD800
            && self.units[pos + 1] <= 0xDBFF
            && pos + 3 < self.count
            && self.units[pos + 3] == of('\'')
        {
            return true;
        }
        false
    }

    fn scan_balanced(&mut self, open: Unit, close: Unit) {
        let mut depth = 0i64;
        loop {
            let c = self.units[self.i];
            if c == open {
                depth += 1;
            }
            if c == close {
                depth -= 1;
            }
            self.i += 1;
            if !(self.i < self.count && depth > 0) {
                break;
            }
        }
    }

    // MARK: - Numbers

    fn is_number_start(&self) -> bool {
        let c = self.units[self.i];
        if is_digit_unit(c) {
            return true;
        }
        c == of('.') && is_digit_unit(self.unit(1))
    }

    /// Deliberately permissive about suffixes and strict about the decimal
    /// point: `.` is only consumed when a digit follows.
    fn scan_number(&mut self) {
        if self.units[self.i] == of('0') && self.i + 1 < self.count {
            let kind = self.units[self.i + 1] | 0x20;
            if kind == of('x') {
                self.i += 2;
                while self.i < self.count
                    && (is_hex_digit_unit(self.units[self.i]) || self.units[self.i] == of('_'))
                {
                    self.i += 1;
                }
                self.scan_exponent(of('p'));
                self.scan_numeric_suffix();
                return;
            }
            if kind == of('b') || kind == of('o') {
                self.i += 2;
                while self.i < self.count
                    && (is_digit_unit(self.units[self.i]) || self.units[self.i] == of('_'))
                {
                    self.i += 1;
                }
                self.scan_numeric_suffix();
                return;
            }
        }
        while self.i < self.count
            && (is_digit_unit(self.units[self.i]) || self.units[self.i] == of('_'))
        {
            self.i += 1;
        }
        if self.i < self.count && self.units[self.i] == of('.') && is_digit_unit(self.unit(1)) {
            self.i += 1;
            while self.i < self.count
                && (is_digit_unit(self.units[self.i]) || self.units[self.i] == of('_'))
            {
                self.i += 1;
            }
        }
        self.scan_exponent(of('e'));
        self.scan_numeric_suffix();
    }

    fn scan_exponent(&mut self, marker: Unit) {
        if !(self.i < self.count && (self.units[self.i] | 0x20) == marker) {
            return;
        }
        let mut j = self.i + 1;
        if j < self.count && (self.units[j] == of('+') || self.units[j] == of('-')) {
            j += 1;
        }
        if !(j < self.count && is_digit_unit(self.units[j])) {
            return;
        }
        self.i = j;
        while self.i < self.count && is_digit_unit(self.units[self.i]) {
            self.i += 1;
        }
    }

    fn scan_numeric_suffix(&mut self) {
        while self.i < self.count
            && (is_letter_unit(self.units[self.i])
                || is_digit_unit(self.units[self.i])
                || self.units[self.i] == of('_'))
        {
            self.i += 1;
        }
    }

    // MARK: - Identifiers

    fn is_identifier_start(&self, pos: isize) -> bool {
        if !(pos >= 0 && (pos as usize) < self.count) {
            return false;
        }
        let pos = pos as usize;
        let c = self.units[pos];
        if is_letter_unit(c) || c == of('_') || c >= 0x80 {
            return true;
        }
        if !self.spec.identifier_extra_starts.contains(&c) {
            return false;
        }
        // An extra start only counts when a real identifier character follows,
        // so CSS `-5px` is a number and `--main` is a custom property.
        let next = if pos + 1 < self.count {
            self.units[pos + 1]
        } else {
            0
        };
        is_letter_unit(next) || next == of('_') || self.spec.identifier_extra_starts.contains(&next)
    }

    #[inline(always)]
    fn is_identifier_continue(&self, c: Unit) -> bool {
        is_word_unit(c)
            || self.spec.identifier_extra_continues.contains(&c)
            || self.spec.identifier_extra_starts.contains(&c)
    }

    fn scan_identifier_token(&mut self, start: usize) {
        let spec = self.spec;
        while self.i < self.count && self.is_identifier_continue(self.units[self.i]) {
            self.i += 1;
        }
        let (word_start, word_end) = (start, self.i);

        // `f"…"`, `r'…'`, `b"…"`: the identifier is the literal's prefix.
        if let Some(prefix) = self.match_string_prefix(word_start, word_end)
            && let Some(string) = self.match_string_open(self.i as isize)
        {
            let string = string.escaping(prefix.escapes);
            self.scan_string(&string);
            self.builder.emit(SyntaxToken::String, start, self.i);
            return;
        }
        if let Some(token) = spec.words.lookup(self.units, word_start, word_end) {
            self.builder.emit(token, word_start, word_end);
            return;
        }
        if spec.all_caps_are_constants && self.is_screaming_case(word_start, word_end) {
            self.builder
                .emit(SyntaxToken::Constant, word_start, word_end);
            return;
        }
        if spec.calls_are_functions && self.next_non_blank() == of('(') {
            self.builder
                .emit(SyntaxToken::Function, word_start, word_end);
            return;
        }
        if spec.keys_from_identifiers && self.is_followed_by_key_terminator() {
            self.builder
                .emit(SyntaxToken::Attribute, word_start, word_end);
            return;
        }
        if spec.capitalised_are_types && is_upper_unit(self.units[start]) {
            self.builder.emit(SyntaxToken::Type, word_start, word_end);
            return;
        }
        self.builder.emit(SyntaxToken::Plain, word_start, word_end);
    }

    fn match_string_prefix(
        &self,
        word_start: usize,
        word_end: usize,
    ) -> Option<&'a StringPrefixSpec> {
        if !(self.i < self.count && !self.spec.string_prefixes.is_empty()) {
            return None;
        }
        let word_count = word_end - word_start;
        for prefix in &self.spec.string_prefixes {
            if prefix.bytes.len() != word_count {
                continue;
            }
            let mut matched = true;
            for k in 0..prefix.bytes.len() {
                let unit = self.units[word_start + k];
                let lowered = if unit < 0x80 && (0x41..=0x5A).contains(&unit) {
                    unit + 0x20
                } else {
                    unit
                };
                if lowered != prefix.bytes[k] as Unit {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Some(prefix);
            }
        }
        None
    }

    /// `MAX_RETRIES` but not `X`, `_`, or `HTTPResponse`.
    fn is_screaming_case(&self, word_start: usize, word_end: usize) -> bool {
        if word_end - word_start < 2 {
            return false;
        }
        let mut saw_letter = false;
        for k in word_start..word_end {
            let c = self.units[k];
            if is_upper_unit(c) {
                saw_letter = true;
                continue;
            }
            if is_digit_unit(c) || c == of('_') {
                continue;
            }
            return false;
        }
        saw_letter
    }

    fn next_non_blank(&self) -> Unit {
        let mut j = self.i;
        while j < self.count && is_blank_unit(self.units[j]) {
            j += 1;
        }
        if j < self.count { self.units[j] } else { 0 }
    }

    fn is_followed_by_key_terminator(&self) -> bool {
        let mut j = self.i;
        while j < self.count && is_blank_unit(self.units[j]) {
            j += 1;
        }
        if !(j < self.count && self.spec.key_terminators.contains(&self.units[j])) {
            return false;
        }
        if !self.spec.key_terminator_needs_space {
            return true;
        }
        let after = j + 1;
        after >= self.count || is_space_unit(self.units[after])
    }
}

#[inline(always)]
fn is_operator_unit(c: Unit) -> bool {
    member(c, &OPERATOR_UNITS)
}

#[inline(always)]
fn is_punctuation_unit(c: Unit) -> bool {
    member(c, &PUNCTUATION_UNITS)
}
