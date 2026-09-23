//! SafeHTML.swift — the conservative README-style HTML annotator.
//!
//! A source annotation, not an HTML DOM: every range points back into the
//! original buffer so editing and copying stay lossless. Unknown tags, event
//! attributes, risky URL schemes and malformed nesting make the whole range
//! unsafe, so callers render the original bytes as inert literal text. HTML
//! export has its own, separate safety rules.

use std::collections::HashMap;

use objc2_foundation::NSStringCompareOptions;

use crate::ns_range::{NS_NOT_FOUND, NSRange};
use crate::swift_text::{self, ns::NSStringExt};

/// The deliberately small HTML vocabulary Downright may present as content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SafeHTMLKind {
    Paragraph {
        align: Option<SafeHTMLAlignment>,
    },
    Heading {
        level: isize,
    },
    Strong,
    Emphasis,
    Link {
        destination: String,
        title: Option<String>,
    },
    Image {
        source: String,
        alt: String,
    },
    /// A recognized but deliberately non-rendered tag, such as a remote image.
    Inert,
    LineBreak,
    Details {
        open: bool,
    },
    /// A closing `</details>` emitted in a separate Markdown HTML block.
    DetailsClosing,
    Summary,
    Table,
    TableRow,
    TableCell {
        header: bool,
        align: Option<SafeHTMLAlignment>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SafeHTMLAlignment {
    Left,
    Center,
    Right,
    Justify,
}

impl SafeHTMLAlignment {
    pub fn raw_value(&self) -> &'static str {
        match self {
            SafeHTMLAlignment::Left => "left",
            SafeHTMLAlignment::Center => "center",
            SafeHTMLAlignment::Right => "right",
            SafeHTMLAlignment::Justify => "justify",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<SafeHTMLAlignment> {
        match raw {
            "left" => Some(SafeHTMLAlignment::Left),
            "center" => Some(SafeHTMLAlignment::Center),
            "right" => Some(SafeHTMLAlignment::Right),
            "justify" => Some(SafeHTMLAlignment::Justify),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeHTMLAnnotation {
    pub kind: SafeHTMLKind,
    /// The complete element, including its opening and closing tags.
    pub range: NSRange,
    /// The content between an element's tags.
    pub content_range: NSRange,
    /// Opening and closing tag source ranges.
    pub tag_ranges: Vec<NSRange>,
}

impl SafeHTMLAnnotation {
    pub fn new(kind: SafeHTMLKind, range: NSRange, content_range: NSRange, tag_ranges: Vec<NSRange>) -> Self {
        SafeHTMLAnnotation { kind, range, content_range, tag_ranges }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeHTMLDocument {
    pub range: NSRange,
    pub annotations: Vec<SafeHTMLAnnotation>,
    /// `false` means the complete source range must remain literal.
    pub is_safe: bool,
}

impl SafeHTMLDocument {
    pub fn new(range: NSRange, annotations: Vec<SafeHTMLAnnotation>, is_safe: bool) -> SafeHTMLDocument {
        SafeHTMLDocument { range, annotations, is_safe }
    }

    pub fn tag_ranges(&self) -> Vec<NSRange> {
        let mut ranges: Vec<NSRange> = self.annotations.iter().flat_map(|a| a.tag_ranges.iter().copied()).collect();
        ranges.sort_by_key(|range| range.location);
        ranges
    }

    pub fn hidden_tag_ranges(&self) -> Vec<NSRange> {
        self.tag_ranges()
    }
}

/// A conservative, non-executing parser for README-style presentational HTML.
pub struct SafeHTMLParser;

impl SafeHTMLParser {
    /// `parse(_ text: String, range:)`.
    pub fn parse(text: &str, range: Option<NSRange>) -> Option<SafeHTMLDocument> {
        let source = swift_text::ns::utf16(text);
        Self::parse_ns(&source, range)
    }

    /// `parse(_ source: NSString, range:)`: the Markdown parser already holds
    /// the UTF-16 source, so no per-block substring is made to rule HTML out.
    pub fn parse_ns(source: &[u16], range: Option<NSRange>) -> Option<SafeHTMLDocument> {
        let bounds = range.unwrap_or(NSRange::new(0, source.length()));
        if !(bounds.location >= 0 && bounds.length > 0 && bounds.upper_bound() <= source.length()) {
            return None;
        }
        // `range(of: "<", options: .literal, range: bounds)`.
        source.find_unit(0x3C, bounds)?;

        let mut parser = Parser::new(source, bounds);
        Some(parser.parse())
    }

    const ALLOWED_NAMES: [&'static str; 22] = [
        "p", "h1", "h2", "h3", "h4", "h5", "h6", "strong", "b", "em", "i", "a", "img", "br", "details", "summary", "table", "thead",
        "tbody", "tr", "th", "td",
    ];

    /// `allowedNames.contains(name)`, answering with the set's own spelling.
    /// Every later test of a tag name is Swift `==` (canonical equivalence)
    /// against these ASCII names, so carrying the member's spelling is exact.
    fn allowed_name(name: &str) -> Option<&'static str> {
        Self::ALLOWED_NAMES.iter().copied().find(|allowed| swift_text::str_eq(allowed, name))
    }

    /// `attributes(in:for:)`: walks Characters, as Swift's `String.Index` does.
    fn attributes(raw: &str, name: &str) -> Option<HashMap<String, String>> {
        let mut result: HashMap<String, String> = HashMap::new();
        let mut characters: Vec<(usize, &str)> = Vec::with_capacity(raw.len());
        let mut offset = 0;
        for g in swift_text::graphemes(raw) {
            characters.push((offset, g));
            offset += g.len();
        }
        let end = characters.len();
        let byte = |i: usize| if i < end { characters[i].0 } else { raw.len() };
        let at = |i: usize| characters[i].1;

        let mut index = 0usize;
        while index < end {
            while index < end && swift_text::is_whitespace(at(index)) {
                index += 1;
            }
            if index >= end {
                break;
            }
            let start = index;
            while index < end && !swift_text::is_whitespace(at(index)) && !swift_text::char_is(at(index), '=') {
                index += 1;
            }
            let key = swift_text::lowercased(&raw[byte(start)..byte(index)]);
            if key.is_empty() || swift_text::has_prefix(&key, "on") {
                return None;
            }
            while index < end && swift_text::is_whitespace(at(index)) {
                index += 1;
            }
            if name == "details" && swift_text::str_eq(&key, "open") && (index == end || !swift_text::char_is(at(index), '=')) {
                swift_text::dict_insert(&mut result, key, String::new());
                continue;
            }
            if !(index < end && swift_text::char_is(at(index), '=')) {
                return None;
            }
            index += 1;
            while index < end && swift_text::is_whitespace(at(index)) {
                index += 1;
            }
            if index >= end {
                return None;
            }
            let quote = at(index);
            let quote_char = if swift_text::char_is(quote, '"') {
                '"'
            } else if swift_text::char_is(quote, '\'') {
                '\''
            } else {
                return None;
            };
            index += 1;
            let value_start = index;
            while index < end && !swift_text::char_is(at(index), quote_char) {
                index += 1;
            }
            if index >= end {
                return None;
            }
            let value = raw[byte(value_start)..byte(index)].to_owned();
            index += 1;
            if !(Self::allowed_attribute(&key, name)
                && ((name == "img" && swift_text::str_eq(&key, "src")) || Self::safe_url(&value, &key, name)))
            {
                return None;
            }
            swift_text::dict_insert(&mut result, key, value);
        }
        Some(result)
    }

    fn allowed_attribute(key: &str, tag: &str) -> bool {
        let is = |literal: &str| swift_text::str_eq(key, literal);
        match tag {
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" | "td" => is("align"),
            "a" => is("href") || is("title"),
            "img" => is("src") || is("alt") || is("title"),
            "details" => is("open"),
            _ => false,
        }
    }

    fn safe_url(value: &str, key: &str, _tag: &str) -> bool {
        let key_is = |literal: &str| swift_text::str_eq(key, literal);
        if !(key_is("href") || key_is("src")) {
            return true;
        }
        let value = swift_text::lowercased(swift_text::trim_whitespaces_and_newlines(value));
        let has = |prefix: &str| swift_text::has_prefix(&value, prefix);
        if !(!swift_text::contains(&value, ":") || has("https:") || has("http:") || has("mailto:") || has("#")) {
            return false;
        }
        if key_is("src") {
            return !has("http:") && !has("https:") && !has("//") && !has("/") && !has("file:") && !has("data:");
        }
        !has("javascript:") && !has("data:") && !has("vbscript:")
    }

    /// `kind(for:attributes:)`. `name` is a member of `ALLOWED_NAMES`.
    fn kind(name: &str, attributes: &HashMap<String, String>) -> Option<SafeHTMLKind> {
        // `SafeHTMLAlignment(rawValue:)` compares with `==`; a lowercased
        // string is canonically equivalent to one of these ASCII words only
        // when it is byte-equal to it, so the byte match is exact.
        let alignment = swift_text::dict_get(attributes, "align")
            .map(|align| swift_text::lowercased(align))
            .and_then(|align| SafeHTMLAlignment::from_raw_value(&align));
        match name {
            "p" => Some(SafeHTMLKind::Paragraph { align: alignment }),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                Some(SafeHTMLKind::Heading { level: swift_text::parse_int(swift_text::drop_first(name, 1)).unwrap_or(1) })
            }
            "strong" | "b" => Some(SafeHTMLKind::Strong),
            "em" | "i" => Some(SafeHTMLKind::Emphasis),
            "a" => {
                let href = swift_text::dict_get(attributes, "href")?;
                Some(SafeHTMLKind::Link { destination: href.clone(), title: swift_text::dict_get(attributes, "title").cloned() })
            }
            "img" => {
                let src = swift_text::dict_get(attributes, "src")?;
                if !Self::safe_local_image_source(src) {
                    return Some(SafeHTMLKind::Inert);
                }
                Some(SafeHTMLKind::Image { source: src.clone(), alt: swift_text::dict_get(attributes, "alt").cloned().unwrap_or_default() })
            }
            "br" => Some(SafeHTMLKind::LineBreak),
            "details" => Some(SafeHTMLKind::Details { open: swift_text::dict_get(attributes, "open").is_some() }),
            "summary" => Some(SafeHTMLKind::Summary),
            "table" | "thead" | "tbody" => Some(SafeHTMLKind::Table),
            "tr" => Some(SafeHTMLKind::TableRow),
            "th" => Some(SafeHTMLKind::TableCell { header: true, align: alignment }),
            "td" => Some(SafeHTMLKind::TableCell { header: false, align: alignment }),
            _ => None,
        }
    }

    fn safe_local_image_source(source: &str) -> bool {
        let value = swift_text::lowercased(swift_text::trim_whitespaces_and_newlines(source));
        let has = |prefix: &str| swift_text::has_prefix(&value, prefix);
        !value.is_empty()
            && !has("http:")
            && !has("https:")
            && !has("//")
            && !has("/")
            && !has("file:")
            && !has("data:")
            && !swift_text::contains(&value, "..")
    }
}

struct OpenElement {
    name: &'static str,
    /// Unused, as in Swift.
    #[allow(dead_code)]
    range: NSRange,
    content_start: isize,
    tag_range: NSRange,
    kind: SafeHTMLKind,
    /// Unused, as in Swift.
    #[allow(dead_code)]
    attributes: HashMap<String, String>,
}

struct Tag {
    name: &'static str,
    attributes: HashMap<String, String>,
    range: NSRange,
    is_closing: bool,
    is_self_closing: bool,
    is_comment_or_declaration: bool,
}

impl Tag {
    fn comment_or_declaration(range: NSRange) -> Tag {
        Tag { name: "", attributes: HashMap::new(), range, is_closing: false, is_self_closing: false, is_comment_or_declaration: true }
    }
}

struct Parser<'a> {
    source: &'a [u16],
    bounds: NSRange,
    cursor: isize,
    stack: Vec<OpenElement>,
    annotations: Vec<SafeHTMLAnnotation>,
    saw_tag: bool,
    rejected: bool,
}

impl<'a> Parser<'a> {
    fn new(source: &'a [u16], bounds: NSRange) -> Parser<'a> {
        Parser { source, bounds, cursor: bounds.location, stack: Vec::new(), annotations: Vec::new(), saw_tag: false, rejected: false }
    }

    fn parse(&mut self) -> SafeHTMLDocument {
        let upper = self.bounds.upper_bound();
        while self.cursor < upper && !self.rejected {
            let Some(opening) = self.next_tag() else { break };
            if opening.is_comment_or_declaration {
                self.rejected = true;
                break;
            }
            self.saw_tag = true;
            if opening.is_closing {
                self.close(opening);
            } else {
                self.open(opening);
            }
        }
        // Swift writes `!trimmed.isEmpty == false`: prefix `!` binds first, so
        // the branch runs only when the rest is blank — where no `<` can be.
        if self.cursor < upper {
            let rest = self.source.substring(NSRange::new(self.cursor, upper - self.cursor));
            if swift_text::trim_whitespaces_and_newlines(&rest).is_empty() {
                // Text is fine; only a non-tag `<` is malformed.
                if swift_text::contains(&rest, "<") {
                    self.rejected = true;
                }
            }
        }
        if !self.stack.is_empty() {
            // swift-markdown may split a README-style details element at blank
            // lines, emitting the closing tag in a later HTML block. Keep that
            // inert container source-addressed; any other open stack is
            // malformed and therefore literal.
            if self.stack.len() == 1 && self.stack[0].name == "details" && self.has_closing_details(upper) {
                let open = self.stack.pop().expect("one open element");
                self.annotations.push(SafeHTMLAnnotation::new(
                    open.kind,
                    open.tag_range,
                    NSRange::new(open.tag_range.upper_bound(), 0),
                    vec![open.tag_range],
                ));
            } else {
                self.rejected = true;
            }
        }
        let annotations = if self.rejected {
            Vec::new()
        } else {
            let mut sorted = std::mem::take(&mut self.annotations);
            // Stable, like Swift's `sorted`.
            sorted.sort_by_key(|annotation| annotation.range.location);
            sorted
        };
        SafeHTMLDocument::new(self.bounds, annotations, self.saw_tag && !self.rejected)
    }

    fn next_tag(&mut self) -> Option<Tag> {
        let upper = self.bounds.upper_bound();
        let Some(start) = self.index_of_angle_bracket(self.cursor) else {
            self.cursor = upper;
            return None;
        };
        if start > self.cursor {
            let text = NSRange::new(self.cursor, start - self.cursor);
            // `text.contains("<")`. A Character equal to `<` needs a U+003C
            // unit, so the substring is built only when one is present.
            if self.source[text.as_usize_range()].contains(&0x3C) && swift_text::contains(&self.source.substring(text), "<") {
                self.rejected = true;
                return None;
            }
        }
        let Some(end) = self.tag_end(start) else {
            self.rejected = true;
            return None;
        };
        let tag_range = NSRange::new(start, end - start + 1);
        let raw = self.source.substring(NSRange::new(start + 1, end - start - 1));
        self.cursor = end + 1;
        if swift_text::has_prefix(&raw, "!")
            || swift_text::has_prefix(&raw, "?")
            || (swift_text::has_prefix(&raw, "/")
                && swift_text::first(swift_text::drop_first(&raw, 1)).is_some_and(|c| swift_text::char_is(c, '!')))
        {
            return Some(Tag::comment_or_declaration(tag_range));
        }
        let closing = swift_text::first(&raw).is_some_and(|c| swift_text::char_is(c, '/'));
        let body: &str = if closing { swift_text::drop_first(&raw, 1) } else { &raw };
        let self_closing = !closing && swift_text::has_suffix(swift_text::trim_whitespaces_and_newlines(body), "/");
        // Swift drops the last Character of the *untrimmed* body.
        let cleaned: &str = if self_closing { swift_text::trim_whitespaces_and_newlines(swift_text::drop_last(body, 1)) } else { body };
        let Some(name_end) = swift_text::first_index_where(cleaned, swift_text::is_whitespace) else {
            let Some(name) = SafeHTMLParser::allowed_name(&swift_text::lowercased(cleaned)) else {
                self.rejected = true;
                return None;
            };
            return Some(Tag {
                name,
                attributes: HashMap::new(),
                range: tag_range,
                is_closing: closing,
                is_self_closing: self_closing,
                is_comment_or_declaration: false,
            });
        };
        let Some(name) = SafeHTMLParser::allowed_name(&swift_text::lowercased(&cleaned[..name_end])) else {
            self.rejected = true;
            return None;
        };
        let rest = &cleaned[name_end..];
        let attrs = if closing { None } else { SafeHTMLParser::attributes(rest, name) };
        let Some(attrs) = attrs else {
            if closing && swift_text::trim_whitespaces_and_newlines(rest).is_empty() {
                return Some(Tag {
                    name,
                    attributes: HashMap::new(),
                    range: tag_range,
                    is_closing: true,
                    is_self_closing: false,
                    is_comment_or_declaration: false,
                });
            }
            self.rejected = true;
            return None;
        };
        Some(Tag {
            name,
            attributes: attrs,
            range: tag_range,
            is_closing: false,
            is_self_closing: self_closing,
            is_comment_or_declaration: false,
        })
    }

    fn open(&mut self, tag: Tag) {
        let Some(kind) = SafeHTMLParser::kind(tag.name, &tag.attributes) else {
            self.rejected = true;
            return;
        };
        let void = tag.name == "br" || tag.name == "img";
        // Always true for a void tag, as in Swift.
        #[allow(clippy::nonminimal_bool)]
        let void_ok = !void || tag.is_self_closing || tag.name == "br" || tag.name == "img";
        if !void_ok {
            self.rejected = true;
            return;
        }
        if void {
            self.annotations.push(SafeHTMLAnnotation::new(kind, tag.range, NSRange::new(tag.range.upper_bound(), 0), vec![tag.range]));
            return;
        }
        if tag.is_self_closing {
            self.rejected = true;
            return;
        }
        self.stack.push(OpenElement {
            name: tag.name,
            range: NSRange::new(tag.range.location, 0),
            content_start: tag.range.upper_bound(),
            tag_range: tag.range,
            kind,
            attributes: tag.attributes,
        });
    }

    fn close(&mut self, tag: Tag) {
        // `stack.popLast()` pops before the names are compared, so a mismatch
        // still discards the open element.
        let open = match self.stack.pop() {
            Some(open) if open.name == tag.name => open,
            _ => {
                // The matching opening tag can live in the preceding HTML block
                // when Markdown holds a blank line inside `<details>`. Safe only
                // for that inert container.
                if self.stack.is_empty() && tag.name == "details" && self.has_opening_details(self.bounds.location) {
                    self.annotations.push(SafeHTMLAnnotation::new(
                        SafeHTMLKind::DetailsClosing,
                        tag.range,
                        NSRange::new(tag.range.location, 0),
                        vec![tag.range],
                    ));
                } else {
                    self.rejected = true;
                }
                return;
            }
        };
        let element_range = NSRange::new(open.tag_range.location, tag.range.upper_bound() - open.tag_range.location);
        let content = NSRange::new(open.content_start, 0.max(tag.range.location - open.content_start));
        self.annotations.push(SafeHTMLAnnotation::new(open.kind, element_range, content, vec![open.tag_range, tag.range]));
    }

    fn tag_end(&self, start: isize) -> Option<isize> {
        let mut quote: u16 = 0;
        let mut index = start + 1;
        while index < self.bounds.upper_bound() {
            let character = self.source.character_at(index);
            if quote != 0 {
                if character == quote {
                    quote = 0;
                }
            } else if character == 0x22 || character == 0x27 {
                quote = character;
            } else if character == 0x3E {
                return Some(index);
            }
            index += 1;
        }
        None
    }

    /// True when the genuine closing fragment appears after `location`: a
    /// bare substring search would let `` `<details>` `` in a code span or
    /// prose satisfy it, so the match must sit where an HTML block line starts.
    fn has_closing_details(&self, location: isize) -> bool {
        let length = self.source.length();
        if location >= length {
            return false;
        }
        with_ns_string(self.source, |ns| {
            let mut search = NSRange::new(location, length - location);
            while search.length > 0 {
                let found = swift_text::ns::foundation::range_of(ns, "</details>", NSStringCompareOptions::CaseInsensitiveSearch, search);
                if found.location == NS_NOT_FOUND {
                    return false;
                }
                if self.is_html_block_line_start(found.location) {
                    return true;
                }
                search = NSRange::new(found.location + 1, length - (found.location + 1));
            }
            false
        })
    }

    /// True when a real `<details …>` opening tag, written as an HTML block,
    /// appears before `location`.
    fn has_opening_details(&self, location: isize) -> bool {
        if location <= 0 {
            return false;
        }
        with_ns_string(self.source, |ns| {
            let mut search = NSRange::new(0, location);
            let options = NSStringCompareOptions::CaseInsensitiveSearch | NSStringCompareOptions::BackwardsSearch;
            while search.length > 0 {
                let found = swift_text::ns::foundation::range_of(ns, "<details", options, search);
                if found.location == NS_NOT_FOUND {
                    return false;
                }
                // The partner must live entirely before this block.
                if found.upper_bound() >= location {
                    search.length = found.location;
                    continue;
                }
                if self.is_html_block_line_start(found.location) {
                    let next = self.source.character_at(found.upper_bound());
                    if next == 0x3E || next == 0x20 || next == 0x09 || next == 0x0A || next == 0x0D {
                        return true;
                    }
                }
                search.length = found.location;
            }
            false
        })
    }

    /// Whether `location` starts an HTML block line: beginning of source or
    /// right after a newline, with no more than three leading spaces. Tabs
    /// fail deliberately.
    fn is_html_block_line_start(&self, location: isize) -> bool {
        let mut index = location;
        let mut spaces = 0;
        while index > 0 {
            let previous = self.source.character_at(index - 1);
            if previous == 0x20 {
                spaces += 1;
                index -= 1;
                if spaces > 3 {
                    return false;
                }
                continue;
            }
            return previous == 0x0A || previous == 0x0D;
        }
        true
    }

    /// `index(of: "<", from:)`.
    fn index_of_angle_bracket(&self, start: isize) -> Option<isize> {
        let upper = self.bounds.upper_bound();
        assert!(start <= upper, "Range requires lowerBound <= upperBound");
        self.source.find_unit(0x3C, NSRange::new(start, upper - start))
    }
}

/// Runs `body` with an `NSString` sharing `units`' storage
/// (`initWithCharactersNoCopy:length:freeWhenDone: NO`), so a Foundation
/// search costs no copy of the document. The string, and anything Foundation
/// autoreleased along with it, is gone before this returns.
fn with_ns_string<R>(units: &[u16], body: impl FnOnce(&objc2_foundation::NSString) -> R) -> R {
    use objc2::AnyThread;
    objc2::rc::autoreleasepool(|_| {
        let pointer = std::ptr::NonNull::new(units.as_ptr() as *mut u16).unwrap_or(std::ptr::NonNull::dangling());
        // SAFETY: `pointer` is valid for `units.len()` code units for the whole
        // closure; Foundation only reads a NoCopy string, and `free_buffer` is
        // false, so it never frees or writes Rust's memory.
        let ns = unsafe {
            objc2_foundation::NSString::initWithCharactersNoCopy_length_freeWhenDone(
                objc2_foundation::NSString::alloc(),
                pointer,
                units.len(),
                false,
            )
        };
        let result = body(&ns);
        drop(ns);
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(document: &SafeHTMLDocument) -> Vec<SafeHTMLKind> {
        document.annotations.iter().map(|a| a.kind.clone()).collect()
    }

    #[test]
    fn rejects_empty_and_angle_free_ranges() {
        assert_eq!(SafeHTMLParser::parse("", None), None);
        assert_eq!(SafeHTMLParser::parse("plain text", None), None);
        assert_eq!(SafeHTMLParser::parse("<p>x</p>", Some(NSRange::new(0, 9))), None);
        assert_eq!(SafeHTMLParser::parse("<p>x</p>", Some(NSRange::new(-1, 2))), None);
    }

    #[test]
    fn swift_precedence_in_the_comment_check() {
        // `raw.hasPrefix("/") && raw.dropFirst().first == "!"` binds tighter
        // than the `||`s around it.
        assert!(!SafeHTMLParser::parse("</!x>", None).unwrap().is_safe);
        assert!(!SafeHTMLParser::parse("<!-- c -->", None).unwrap().is_safe);
    }

    #[test]
    fn mismatched_close_pops_the_open_element() {
        // `<p></details>` in a block after a real `<details>` block: the pop
        // discards `<p>` and the closer pairs across the boundary.
        let text = "<details>\n\n<p></details>";
        let document = SafeHTMLParser::parse(text, Some(NSRange::new(11, 13))).unwrap();
        assert!(document.is_safe);
        assert_eq!(kinds(&document), vec![SafeHTMLKind::DetailsClosing]);
    }

    #[test]
    fn no_copy_string_sees_the_buffer() {
        let units = swift_text::ns::utf16("ab</DETAILS>");
        let found = with_ns_string(&units, |ns| {
            swift_text::ns::foundation::range_of(ns, "</details>", NSStringCompareOptions::CaseInsensitiveSearch, NSRange::new(0, 12))
        });
        assert_eq!(found, NSRange::new(2, 10));
    }
}
