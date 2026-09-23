//! SmartPaste.swift — smart paste (§6.4).
//!
//! A URL over a selection makes a link; HTML on the clipboard converts to
//! markdown; spreadsheet data becomes a markdown table.
//!
//! The HTML converter is deliberately small and tag-driven rather than a
//! general parser: what lands on the clipboard is browser markup for a
//! selection. Anything it does not recognise degrades to its text content.
//!
//! Swift walks the HTML by `Character` with `String.Index`; here the cursor is
//! a byte offset that always sits on a Character boundary, and every
//! Character test goes through [`swift_text`].

use std::borrow::Cow;

use objc2::rc::Retained;
use objc2_foundation::{NSString, NSStringCompareOptions, NSURLComponents};
use unicode_normalization::UnicodeNormalization;

use crate::model::TableAlignment;
use crate::ns_range::NSRange;
use crate::swift_text;
use crate::table_formatter::{Model, TableFormatter};

pub struct SmartPaste;

impl SmartPaste {
    /// `markdown(forHTML:)`.
    pub fn markdown_for_html(html: &str) -> String {
        let mut parser = HtmlToMarkdown::new(html);
        parser.run()
    }

    /// `plainText(forHTML:)`: the plain-text projection used by Paste and
    /// Match Style. Unlike the Markdown conversion it drops links, emphasis
    /// markers and block syntax while keeping the words.
    ///
    /// Like Downright's, this never returns when a `<` has no `>` after it
    /// (the text branch finds that same `<` and makes no progress).
    pub fn plain_text_for_html(html: &str) -> String {
        let mut parser = HtmlTextOnly::new(html);
        parser.run()
    }

    /// `markdownTable(forTabSeparated:)`: spreadsheet and terminal-table
    /// paste. `None` when the text has no tab structure, so the caller can
    /// fall through to a plain paste.
    pub fn markdown_table_for_tab_separated(text: &str) -> Option<String> {
        let normalized =
            swift_text::replacing_occurrences(&swift_text::replacing_occurrences(text, "\r\n", "\n"), "\r", "\n");
        let split = swift_text::split(&normalized, '\n', usize::MAX, false);
        let start = split.iter().position(|line| !line.is_empty()).unwrap_or(split.len());
        let end = split.iter().rposition(|line| !line.is_empty()).map_or(start, |p| (p + 1).max(start));
        let lines: &[&str] = &split[start..end];
        if !(lines.len() >= 2 || lines.first().is_some_and(|line| swift_text::contains(line, "\t"))) {
            return None;
        }
        if !lines.iter().any(|line| swift_text::contains(line, "\t")) {
            return None;
        }

        let rows: Vec<Vec<String>> = lines
            .iter()
            .map(|line| {
                components_separated_by(line, "\t")
                    .iter()
                    .map(|cell| swift_text::replacing_occurrences(swift_text::trim_whitespaces(cell), "|", "\\|"))
                    .collect()
            })
            .collect();
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns <= 1 {
            return None;
        }

        let model = Model::new(
            rows.into_iter().map(|row| pad_row(row, columns)).collect(),
            vec![TableAlignment::None; columns],
            "",
        );
        Some(TableFormatter::render(&model))
    }

    /// `linkified(selection:url:)`: a URL dropped over a selection. `None`
    /// when `url` is not URL-shaped, so pasting arbitrary text over a
    /// selection stays a replacement rather than silently becoming a broken
    /// link.
    pub fn linkified(selection: &str, url: &str) -> Option<String> {
        let trimmed = swift_text::trim_whitespaces_and_newlines(url);
        if trimmed.is_empty() || swift_text::graphemes(trimmed).any(swift_text::is_whitespace) {
            return None;
        }
        let lowercased = swift_text::lowercased(trimmed);
        let normalized: String =
            if swift_text::has_prefix(&lowercased, "www.") { format!("https://{trimmed}") } else { trimmed.to_owned() };
        let components = UrlComponents::parse(&normalized)?;
        let scheme = swift_text::lowercased(components.scheme.as_deref()?);
        if !["http", "https", "mailto"].iter().any(|candidate| swift_text::str_eq(candidate, &scheme)) {
            return None;
        }
        if swift_text::str_eq(&scheme, "http") || swift_text::str_eq(&scheme, "https") {
            match components.host.as_deref() {
                Some(host) if !host.is_empty() => {}
                _ => return None,
            }
        } else if components.path.is_empty() {
            return None;
        }

        let label = swift_text::trim_whitespaces_and_newlines(selection);
        if label.is_empty() {
            return Some(format!("<{normalized}>"));
        }
        let escaped = swift_text::replacing_occurrences(&swift_text::replacing_occurrences(label, "[", "\\["), "]", "\\]");
        let destination = if swift_text::graphemes(&normalized).any(|g| swift_text::char_is(g, '(') || swift_text::char_is(g, ')')) {
            format!("<{normalized}>")
        } else {
            normalized
        };
        Some(format!("[{escaped}]({destination})"))
    }
}

/// `row + [String](repeating: "", count: columns - row.count)`.
fn pad_row(mut row: Vec<String>, columns: usize) -> Vec<String> {
    assert!(columns >= row.len(), "Negative count not allowed");
    row.resize(columns, String::new());
    row
}

// MARK: - URLComponents

/// The parts of Foundation's `URLComponents(string:)` that smart paste reads.
/// `NSURLComponents` is the same parser (checked against Swift's
/// `URLComponents` on this OS, including its encoding of invalid characters
/// and IDNA host mapping).
struct UrlComponents {
    scheme: Option<String>,
    host: Option<String>,
    /// Swift's `path` is non-optional: `""` where `NSURLComponents` has `nil`.
    path: String,
}

impl UrlComponents {
    fn parse(string: &str) -> Option<UrlComponents> {
        // The parts come back as Swift-backed `NSString`s, whose
        // `getCharacters:range:` declares a signed range: objc2's debug
        // encoding check rejects the `NSRange` call `ns::foundation::to_string`
        // makes, so read them through `UTF8String` (`Display`) instead. URL
        // components never hold lone surrogates, so nothing is lost.
        let to_string = |s: &NSString| s.to_string();
        objc2::rc::autoreleasepool(|_| {
            let components: Retained<NSURLComponents> = NSURLComponents::componentsWithString(&ns_string(string))?;
            Some(UrlComponents {
                scheme: components.scheme().map(|s| to_string(&s)),
                host: components.host().map(|s| to_string(&s)),
                path: components.path().map(|s| to_string(&s)).unwrap_or_default(),
            })
        })
    }
}

// MARK: - Character helpers

/// `String(s.prefix { … })` over Characters.
fn prefix_while(s: &str, mut predicate: impl FnMut(&str) -> bool) -> &str {
    let end = swift_text::first_index_where(s, |g| !predicate(g)).unwrap_or(s.len());
    &s[..end]
}

/// Whether the Character starting at byte offset `i` (a Character boundary)
/// is exactly the ASCII byte `b`.
#[inline]
fn character_is_ascii_at(s: &str, i: usize, b: u8) -> bool {
    s.as_bytes().get(i) == Some(&b) && swift_text::is_grapheme_boundary(s, i + 1)
}

/// `s[from...].firstIndex(of: c)` for an ASCII Character `c` that nothing
/// else is canonically equivalent to (`<`, `>`), as a byte offset. `from` is
/// a Character boundary.
fn first_index_of_ascii(s: &str, from: usize, b: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = from;
    while let Some(p) = bytes[i..].iter().position(|&x| x == b) {
        let at = i + p;
        if swift_text::is_grapheme_boundary(s, at) && swift_text::is_grapheme_boundary(s, at + 1) {
            return Some(at);
        }
        i = at + 1;
    }
    None
}

/// A lowercased tag name as Swift's `switch`/`==` sees it: canonical
/// equivalence against ASCII literals is equality of the NFC form.
fn match_key(name: &str) -> Cow<'_, str> {
    if name.is_ascii() { Cow::Borrowed(name) } else { Cow::Owned(name.nfc().collect()) }
}

/// `String((closing ? raw.dropFirst() : raw).prefix { $0.isLetter || $0.isNumber }).lowercased()`.
fn tag_name(body: &str) -> String {
    swift_text::lowercased(prefix_while(body, |g| swift_text::is_letter(g) || swift_text::is_number(g)))
}

/// `text.split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")`.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_word = false;
    for g in swift_text::graphemes(text) {
        if swift_text::is_whitespace(g) {
            in_word = false;
        } else {
            if !in_word && !out.is_empty() {
                out.push(' ');
            }
            in_word = true;
            out.push_str(g);
        }
    }
    out
}

/// UTF-16 offset → byte offset in `s`.
fn byte_offset(s: &str, utf16_offset: isize) -> usize {
    let mut units = 0isize;
    for (index, c) in s.char_indices() {
        if units >= utf16_offset {
            return index;
        }
        units += c.len_utf16() as isize;
    }
    s.len()
}

// MARK: - Plain text

struct HtmlTextOnly<'a> {
    html: &'a str,
    output: String,
    skip_depth: isize,
    pending_space: bool,
}

impl<'a> HtmlTextOnly<'a> {
    fn new(html: &'a str) -> Self {
        HtmlTextOnly { html, output: String::new(), skip_depth: 0, pending_space: false }
    }

    fn run(&mut self) -> String {
        let html = self.html;
        let mut index = 0usize;
        while index < html.len() {
            if character_is_ascii_at(html, index, b'<')
                && let Some(close) = first_index_of_ascii(html, index, b'>')
            {
                let raw = &html[index + 1..close];
                let closing = swift_text::has_prefix(raw, "/");
                let name = tag_name(if closing { swift_text::drop_first(raw, 1) } else { raw });
                let key = match_key(&name);
                if matches!(&*key, "script" | "style" | "head") {
                    self.skip_depth += if closing { -1 } else { 1 };
                    self.skip_depth = 0.max(self.skip_depth);
                } else if self.skip_depth == 0 && !closing {
                    if &*key == "br" || matches!(&*key, "li" | "tr") {
                        self.append_boundary(1);
                    } else if matches!(
                        &*key,
                        "p" | "div" | "blockquote" | "pre" | "details" | "summary" | "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                    ) {
                        self.append_boundary(2);
                    }
                }
                index = close + 1;
                continue;
            }
            let next = first_index_of_ascii(html, index, b'<').unwrap_or(html.len());
            if self.skip_depth == 0 {
                let decoded = Self::decode_entities(&html[index..next]);
                self.append_text(&decoded);
            }
            index = next;
        }
        Self::normalized(&self.output)
    }

    fn append_boundary(&mut self, newlines: isize) {
        if self.output.is_empty() {
            return;
        }
        self.pending_space = false;
        while swift_text::last(&self.output).is_some_and(|g| swift_text::char_is(g, ' ')) {
            self.output.pop();
        }
        let existing = swift_text::graphemes(&self.output).rev().take_while(|g| swift_text::char_is(g, '\n')).count() as isize;
        if existing >= newlines {
            return;
        }
        self.output.push_str(&"\n".repeat((newlines - existing) as usize));
    }

    fn append_text(&mut self, text: &str) {
        let collapsed = collapse_whitespace(text);
        let ends_with_whitespace = swift_text::last(text).is_some_and(swift_text::is_whitespace);
        if collapsed.is_empty() {
            self.pending_space = self.pending_space || ends_with_whitespace;
            return;
        }
        let leading_space = swift_text::first(text).is_some_and(swift_text::is_whitespace);
        if (self.pending_space || leading_space)
            && !self.output.is_empty()
            && !swift_text::has_suffix(&self.output, "\n")
            && let Some(first) = swift_text::first(&collapsed)
            && !['.', ',', ';', ':', '!', '?', ')', ']', '}'].iter().any(|&c| swift_text::char_is(first, c))
        {
            self.output.push(' ');
        }
        self.output.push_str(&collapsed);
        self.pending_space = ends_with_whitespace;
    }

    fn normalized(value: &str) -> String {
        let lines: Vec<String> = components_separated_by(value, "\n")
            .iter()
            .map(|line| swift_text::trim_whitespaces_and_newlines(line).to_owned())
            .collect();
        let mut normalized_lines: Vec<String> = Vec::new();
        let mut blank_lines = 0;
        for line in lines {
            if line.is_empty() {
                blank_lines += 1;
                if blank_lines == 1 && !normalized_lines.is_empty() {
                    normalized_lines.push(String::new());
                }
            } else {
                blank_lines = 0;
                normalized_lines.push(line);
            }
        }
        swift_text::trim_whitespaces_and_newlines(&normalized_lines.join("\n")).to_owned()
    }

    fn decode_entities(value: &str) -> String {
        let mut out = swift_text::replacing_occurrences(value, "&nbsp;", " ");
        out = swift_text::replacing_occurrences(&out, "&amp;", "&");
        out = swift_text::replacing_occurrences(&out, "&lt;", "<");
        out = swift_text::replacing_occurrences(&out, "&gt;", ">");
        out = swift_text::replacing_occurrences(&out, "&quot;", "\"");
        swift_text::replacing_occurrences(&out, "&#39;", "'")
    }
}

// MARK: - HTML → markdown

/// An open list, so `<li>` inside `<ol>` numbers itself.
#[derive(Clone, Copy)]
struct ListEntry {
    ordered: bool,
    index: isize,
}

struct HtmlToMarkdown<'a> {
    html: &'a str,
    out: String,
    list_stack: Vec<ListEntry>,
    quote_depth: isize,
    skip_depth: isize,
    pending_table_rows: Vec<Vec<String>>,
    table_cell: Option<String>,
    table_row: Vec<String>,
    in_table: bool,
    in_pre: bool,
    active_anchor_destination: Option<String>,
}

impl<'a> HtmlToMarkdown<'a> {
    fn new(html: &'a str) -> Self {
        HtmlToMarkdown {
            html,
            out: String::new(),
            list_stack: Vec::new(),
            quote_depth: 0,
            skip_depth: 0,
            pending_table_rows: Vec::new(),
            table_cell: None,
            table_row: Vec::new(),
            in_table: false,
            in_pre: false,
            active_anchor_destination: None,
        }
    }

    fn run(&mut self) -> String {
        let html = self.html;
        let mut index = 0usize;
        while index < html.len() {
            if character_is_ascii_at(html, index, b'<') {
                let Some(close) = first_index_of_ascii(html, index, b'>') else { break };
                self.handle(&html[index + 1..close]);
                index = close + 1;
            } else {
                let next = first_index_of_ascii(html, index, b'<').unwrap_or(html.len());
                self.emit(&html[index..next]);
                index = next;
            }
        }
        swift_text::trim_whitespaces_and_newlines(&Self::collapse_blank_runs(&self.out)).to_owned()
    }

    /// Block tags each contribute their own `\n\n`, so a `</h2><p>` boundary
    /// produces four newlines. Collapse any run to a single blank line, and
    /// drop whitespace-only lines outside fenced code.
    fn collapse_blank_runs(text: &str) -> String {
        let mut cleaned_lines: Vec<String> = Vec::new();
        let mut in_fence = false;
        for line in components_separated_by(text, "\n") {
            let trimmed = swift_text::trim_whitespaces_and_newlines(line);
            if swift_text::has_prefix(trimmed, "```") {
                in_fence = !in_fence;
                cleaned_lines.push(line.to_owned());
            } else if in_fence || !trimmed.is_empty() {
                cleaned_lines.push(line.to_owned());
            } else {
                cleaned_lines.push(String::new());
            }
        }

        let joined = cleaned_lines.join("\n");
        let mut out = String::with_capacity(joined.len());
        let mut newlines = 0;
        for character in swift_text::graphemes(&joined) {
            if swift_text::char_is(character, '\n') {
                newlines += 1;
                if newlines <= 2 {
                    out.push_str(character);
                }
            } else {
                newlines = 0;
                out.push_str(character);
            }
        }
        out
    }

    fn handle(&mut self, raw: &str) {
        let is_closing = swift_text::has_prefix(raw, "/");
        let body = if is_closing { swift_text::drop_first(raw, 1) } else { raw };
        let name = tag_name(body);
        let key = match_key(&name);

        if matches!(&*key, "script" | "style" | "head") {
            self.skip_depth += if is_closing { -1 } else { 1 };
            self.skip_depth = 0.max(self.skip_depth);
            return;
        }
        if self.skip_depth != 0 {
            return;
        }

        match &*key {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = swift_text::parse_int(swift_text::drop_first(&name, 1)).unwrap_or(1);
                if is_closing {
                    self.append("\n\n");
                } else {
                    self.append(&format!("\n\n{} ", repeating("#", level)));
                }
            }
            "p" | "div" => self.append("\n\n"),
            "br" => self.append("  \n"),
            "hr" => self.append("\n\n---\n\n"),
            "strong" | "b" => self.append("**"),
            "em" | "i" => self.append("*"),
            "del" | "s" | "strike" => self.append("~~"),
            "pre" => {
                self.in_pre = !is_closing;
                self.append(if is_closing { "\n```\n\n" } else { "\n\n```\n" });
            }
            "code" => {
                if !self.in_pre {
                    self.append("`");
                }
            }
            "blockquote" => {
                self.quote_depth += if is_closing { -1 } else { 1 };
                self.quote_depth = 0.max(self.quote_depth);
                self.append("\n\n");
            }
            "ul" | "ol" => {
                if is_closing {
                    self.list_stack.pop();
                    // Nested lists are part of the current list item; only the
                    // outer list needs a boundary before the next block.
                    if self.list_stack.is_empty() {
                        self.append("\n");
                    }
                } else {
                    self.list_stack.push(ListEntry { ordered: &*key == "ol", index: 0 });
                    if self.list_stack.len() == 1 {
                        self.append("\n");
                    }
                }
            }
            "li" => {
                if is_closing {
                    return;
                }
                let depth = 0.max(self.list_stack.len() as isize - 1);
                let indent = repeating("  ", depth);
                if self.list_stack.is_empty() {
                    self.append(&format!("\n{indent}- "));
                } else {
                    let last = self.list_stack.len() - 1;
                    self.list_stack[last].index += 1;
                    let entry = self.list_stack[last];
                    let marker = if entry.ordered { format!("{}. ", entry.index) } else { "- ".to_owned() };
                    self.append(&format!("\n{indent}{marker}"));
                }
            }
            "a" => {
                if is_closing {
                    if let Some(destination) = self.active_anchor_destination.clone() {
                        self.append(&format!("]({destination})"));
                    }
                    self.active_anchor_destination = None;
                } else if let Some(href) = Self::attribute("href", body)
                    && let Some(destination) = Self::safe_destination(&href)
                {
                    self.active_anchor_destination = Some(destination);
                    self.append("[");
                } else {
                    self.active_anchor_destination = None;
                }
            }
            "img" => {
                let alt = Self::attribute("alt", body).unwrap_or_default();
                let src = Self::attribute("src", body).unwrap_or_default();
                if let Some(destination) = Self::safe_destination(&src) {
                    let escaped_alt =
                        swift_text::replacing_occurrences(&swift_text::replacing_occurrences(&alt, "[", "\\["), "]", "\\]");
                    self.append(&format!("![{escaped_alt}]({destination})"));
                }
            }
            "table" => {
                if is_closing {
                    self.in_table = false;
                    self.flush_table();
                } else {
                    self.in_table = true;
                    self.pending_table_rows = Vec::new();
                }
            }
            "tr" => {
                if is_closing {
                    let row = std::mem::take(&mut self.table_row);
                    self.pending_table_rows.push(row);
                }
            }
            "td" | "th" => {
                if is_closing {
                    let cell = self.table_cell.take().unwrap_or_default();
                    self.table_row.push(swift_text::trim_whitespaces_and_newlines(&cell).to_owned());
                } else {
                    self.table_cell = Some(String::new());
                }
            }
            _ => {}
        }
    }

    /// Clipboard HTML is untrusted interchange data: keep web, mail, file,
    /// fragment and relative destinations, drop schemes that could become
    /// executable when the resulting Markdown is clicked.
    fn safe_destination(raw: &str) -> Option<String> {
        let value = swift_text::trim_whitespaces_and_newlines(raw);
        if value.is_empty() || swift_text::graphemes(value).any(swift_text::is_whitespace) {
            return None;
        }
        if swift_text::has_prefix(value, "#") {
            return Some(value.to_owned());
        }
        let components = UrlComponents::parse(value)?;
        if let Some(scheme) = components.scheme.as_deref().map(swift_text::lowercased)
            && !["http", "https", "mailto", "file"].iter().any(|candidate| swift_text::str_eq(candidate, &scheme))
        {
            return None;
        }
        Some(if swift_text::contains(value, "(") || swift_text::contains(value, ")") {
            format!("<{value}>")
        } else {
            value.to_owned()
        })
    }

    fn emit(&mut self, raw: &str) {
        if self.skip_depth != 0 {
            return;
        }
        let decoded = Self::decode_entities(raw);
        if let Some(cell) = self.table_cell.as_mut() {
            cell.push_str(&swift_text::replacing_occurrences(&decoded, "\n", " "));
            return;
        }
        if self.in_pre {
            self.append(&decoded);
            return;
        }
        let collapsed =
            swift_text::replacing_occurrences(&swift_text::replacing_occurrences(&decoded, "\n", " "), "\t", " ");
        if !(!swift_text::trim_whitespaces(&collapsed).is_empty() || !swift_text::has_suffix(&self.out, " ")) {
            return;
        }
        let squeezed = Self::squeeze(&collapsed);
        self.append(&squeezed);
    }

    fn append(&mut self, piece: &str) {
        if self.in_table
            && let Some(cell) = self.table_cell.as_mut()
        {
            cell.push_str(piece);
            return;
        }
        if self.in_table && !swift_text::trim_whitespaces_and_newlines(piece).is_empty() {
            return;
        }
        if swift_text::has_prefix(piece, "\n") && self.quote_depth > 0 {
            self.out.push_str(piece);
            self.out.push_str(&repeating("> ", self.quote_depth));
            return;
        }
        self.out.push_str(piece);
    }

    fn flush_table(&mut self) {
        let rows: Vec<Vec<String>> = std::mem::take(&mut self.pending_table_rows).into_iter().filter(|row| !row.is_empty()).collect();
        if rows.is_empty() {
            return;
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let model = Model::new(
            rows.into_iter().map(|row| pad_row(row, columns)).collect(),
            vec![TableAlignment::None; columns],
            "",
        );
        self.out.push_str("\n\n");
        self.out.push_str(&TableFormatter::render(&model));
        self.out.push_str("\n\n");
    }

    fn squeeze(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut last_was_space = false;
        for character in swift_text::graphemes(text) {
            let is_space = swift_text::char_is(character, ' ');
            if is_space && last_was_space {
                continue;
            }
            out.push_str(character);
            last_was_space = is_space;
        }
        out
    }

    /// The value of attribute `name` in `tag`. `href=` must start at an
    /// attribute boundary: a bare substring match would also hit `data-href=`.
    /// Only the first case-insensitive match is considered, as in Swift.
    fn attribute(name: &str, tag: &str) -> Option<String> {
        let needle = format!("{name}=");
        let (lower, upper) = case_insensitive_range(tag, &needle)?;
        let at_boundary = lower == 0 || {
            let previous = character_before(tag, lower);
            swift_text::is_whitespace(previous) || swift_text::char_is(previous, '/')
        };
        if !at_boundary {
            return None;
        }
        let rest = &tag[upper..];
        let quote = swift_text::first(rest)?;
        if swift_text::char_is(quote, '"') || swift_text::char_is(quote, '\'') {
            let rest = swift_text::drop_first(rest, 1);
            let quote_char = if swift_text::char_is(quote, '"') { '"' } else { '\'' };
            let end = swift_text::first_index_of(rest, quote_char)?;
            return Some(Self::decode_entities(&rest[..end]));
        }
        Some(Self::decode_entities(prefix_while(rest, |g| !swift_text::is_whitespace(g) && !swift_text::char_is(g, '>'))))
    }

    fn decode_entities(text: &str) -> String {
        if !swift_text::contains(text, "&") {
            return text.to_owned();
        }
        // `&amp;` must be decoded last so an escaped entity such as `&amp;lt;`
        // resolves to the literal text `&lt;` rather than double-decoding.
        // Swift iterates a Dictionary here (random order); no replacement
        // text contains `&` or can join a neighbouring Character, so the
        // order cannot change the result.
        const NAMED: [(&str, &str); 13] = [
            ("&lt;", "<"),
            ("&gt;", ">"),
            ("&quot;", "\""),
            ("&#39;", "'"),
            ("&apos;", "'"),
            ("&nbsp;", " "),
            ("&mdash;", "—"),
            ("&ndash;", "–"),
            ("&hellip;", "…"),
            ("&ldquo;", "\u{201C}"),
            ("&rdquo;", "\u{201D}"),
            ("&lsquo;", "\u{2018}"),
            ("&rsquo;", "\u{2019}"),
        ];
        let mut out = text.to_owned();
        for (entity, replacement) in NAMED {
            out = swift_text::replacing_occurrences(&out, entity, replacement);
        }
        swift_text::replacing_occurrences(&out, "&amp;", "&")
    }
}

/// `tag.range(of: needle, options: .caseInsensitive)` on a native Swift
/// `String`, as byte offsets. swift-foundation searches Character by
/// Character there, so a match must start and end on Character boundaries
/// (`"img \u{600}src="` has no `src=`: U+0600 prepends to the `s`), which
/// `NSString`'s composed-sequence search does not require. Foundation finds
/// the case-insensitive candidates; a candidate that splits a Character is
/// skipped. An ASCII tag against an ASCII needle is plain ASCII case folding.
fn case_insensitive_range(tag: &str, needle: &str) -> Option<(usize, usize)> {
    if tag.is_ascii() && needle.is_ascii() {
        let hay = tag.as_bytes();
        let pattern = needle.as_bytes();
        if pattern.len() > hay.len() {
            return None;
        }
        return (0..=hay.len() - pattern.len())
            .find(|&start| hay[start..start + pattern.len()].eq_ignore_ascii_case(pattern))
            .map(|start| (start, start + pattern.len()));
    }
    objc2::rc::autoreleasepool(|_| {
        let ns = ns_string(tag);
        let length = swift_text::utf16_count(tag);
        let mut from = 0isize;
        while from < length {
            let found = swift_text::ns::foundation::range_of(
                &ns,
                needle,
                NSStringCompareOptions::CaseInsensitiveSearch,
                NSRange::new(from, length - from),
            );
            if found.location == crate::ns_range::NS_NOT_FOUND {
                return None;
            }
            let lower = byte_offset(tag, found.location);
            let upper = byte_offset(tag, found.upper_bound());
            if swift_text::is_grapheme_boundary(tag, lower) && swift_text::is_grapheme_boundary(tag, upper) {
                return Some((lower, upper));
            }
            from = found.location + 1;
        }
        None
    })
}

/// A Swift `String` bridged to `NSString`: built from its UTF-16 so a
/// leading U+FEFF survives (`NSString::from_str` decodes UTF-8 bytes and
/// drops it as a byte-order mark).
fn ns_string(s: &str) -> Retained<NSString> {
    swift_text::ns::foundation::ns_from_utf16(&swift_text::ns::utf16(s))
}

/// `String.components(separatedBy:)` on a native Swift `String`
/// (swift-foundation): `"\n"` is matched literally, by scalar, so it splits
/// CR LF and `"\n\u{301}"`; any other separator is matched Character by
/// Character, so `"\r"` does not split CR LF and `"\t"` does split
/// `"\t\u{301}"`. (`NSString.components(separatedBy:)` differs on both.)
fn components_separated_by<'s>(s: &'s str, separator: &str) -> Vec<&'s str> {
    if separator == "\n" {
        return s.split('\n').collect();
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < s.len() {
        match swift_text::find(&s[start..], separator) {
            Some(found) if !found.is_empty() => {
                out.push(&s[start..start + found.start]);
                start += found.end;
            }
            _ => break,
        }
    }
    out.push(&s[start..]);
    out
}

/// `s[s.index(before: i)]`: the Character that ends at byte offset `i`.
fn character_before(s: &str, i: usize) -> &str {
    swift_text::last(&s[..i]).unwrap_or("")
}

/// `String(repeating:count:)`, which traps on a negative count.
fn repeating(s: &str, count: isize) -> String {
    assert!(count >= 0, "Negative count not allowed");
    s.repeat(count as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_boundaries_decide_tags() {
        // `<` followed by a combining mark is one Character that is not `<`.
        assert_eq!(SmartPaste::markdown_for_html("a <\u{338}b> c"), "a <\u{338}b> c");
        // Swift 6.4: the CR LF between blocks collapses away.
        assert_eq!(SmartPaste::markdown_for_html("<p>a</p>\r\n<p>b</p>"), "a\n\nb");
    }

    #[test]
    fn attribute_boundaries() {
        assert_eq!(HtmlToMarkdown::attribute("href", "a data-href=\"x\" href=\"y\""), None);
        assert_eq!(HtmlToMarkdown::attribute("href", "a HREF='y'").as_deref(), Some("y"));
        assert_eq!(HtmlToMarkdown::attribute("src", "img/src=a.png>").as_deref(), Some("a.png"));
        assert_eq!(HtmlToMarkdown::attribute("alt", "img alt=\"Café &amp; co\"").as_deref(), Some("Café & co"));
    }
}
