//! Extensions/FrontMatterScanner.swift — YAML front matter (§4.1).
//!
//! Runs before cmark sees the text, and the body is parsed without it: `---`
//! on line 1 followed by `title: X` would otherwise parse as a thematic break
//! plus a setext H2 and shift every range after it.
//!
//! A metadata card, not a YAML engine: scalars, quoted strings, inline
//! `[a, b]` lists, block sequences and `|`/`>` block scalars. Anything else is
//! silently ignored rather than failing (§5.1).

use crate::model::{FrontMatter, FrontMatterField};
use crate::ns_range::NSRange;
use crate::source_positions::SourceMap;
use crate::swift_text;

pub struct FrontMatterScanner;

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockScalarStyle {
    /// `|`: newlines kept.
    Literal,
    /// `>`: continuation lines folded on spaces.
    Folded,
}

impl FrontMatterScanner {
    /// Detects front matter, which is only front matter when `---` is the very
    /// first line of the file.
    pub fn scan(map: &SourceMap) -> Option<FrontMatter> {
        if !(map.line_count() > 1) {
            return None;
        }
        if !Self::is_fence(&map.string_of_line(0), true) {
            return None;
        }

        let closing_line = (1..map.line_count()).find(|&line| Self::is_fence(&map.string_of_line(line), false))?;

        let body_start = map.line_starts[1];
        let body_end = map.line_starts[closing_line as usize];
        let range = NSRange::new(0, map.full_range_of_line(closing_line).upper_bound());
        let body_range = NSRange::new(body_start, 0.max(body_end - body_start));
        let fields = Self::parse_fields(map, 1, closing_line);
        Some(FrontMatter::new(fields, range, body_range))
    }

    fn is_fence(line: &str, opening: bool) -> bool {
        let trimmed = swift_text::trim_whitespaces(line);
        swift_text::str_eq(trimmed, "---") || (!opening && swift_text::str_eq(trimmed, "..."))
    }

    fn parse_fields(map: &SourceMap, from: isize, to: isize) -> Vec<FrontMatterField> {
        let mut fields = Vec::new();
        let mut line = from;
        while line < to {
            let text = map.string_of_line(line);
            let line_range = map.content_range_of_line(line);
            // Swift's `defer { line += 1 }`: runs after every `continue` too.
            let current = line;
            line += 1;

            if swift_text::is_blank_line(&text) || swift_text::count(swift_text::leading_indent(&text)) != 0 {
                continue;
            }
            if swift_text::has_prefix(&text, "#") {
                continue;
            }
            let Some(colon) = swift_text::first_index_of(&text, ':') else { continue };

            let key = swift_text::trim_whitespaces(&text[..colon]);
            if !(!key.is_empty() && swift_text::all_satisfy(key, Self::is_key_character)) {
                continue;
            }

            // `text.index(after: colon)`: the Character at `colon` equals `:`,
            // and only U+003A itself does, so it is one byte long.
            let raw_value = &text[colon + 1..];
            let key_range = NSRange::new(line_range.location, swift_text::utf16_count(key));
            let value_start = line_range.location + swift_text::utf16_count(&text) - swift_text::utf16_count(raw_value);
            let trimmed_value = swift_text::trim_whitespaces(raw_value);

            // `key: |` and `key: >` block scalars span the following indented
            // lines, which are folded into the value.
            if Self::is_block_scalar_indicator(trimmed_value) {
                let style =
                    if swift_text::contains(trimmed_value, ">") { BlockScalarStyle::Folded } else { BlockScalarStyle::Literal };
                let (parts, consumed) = Self::block_scalar(map, current + 1, to);
                if !parts.is_empty() {
                    let end = map.content_range_of_line(current + consumed).upper_bound();
                    let value = if style == BlockScalarStyle::Literal { parts.join("\n") } else { parts.join(" ") };
                    fields.push(FrontMatterField::new(
                        key,
                        value,
                        key_range,
                        NSRange::new(value_start, 0.max(end - value_start)),
                    ));
                    line += consumed;
                }
                continue;
            }

            if trimmed_value.is_empty() {
                // `tags:` followed by an indented `- a` block sequence.
                let (items, consumed) = Self::block_sequence(map, current + 1, to);
                if !items.is_empty() {
                    let end = map.content_range_of_line(current + consumed).upper_bound();
                    fields.push(FrontMatterField::new(
                        key,
                        items.join(", "),
                        key_range,
                        NSRange::new(value_start, 0.max(end - value_start)),
                    ));
                    line += consumed;
                }
                continue;
            }

            let value_range = NSRange::new(value_start, swift_text::utf16_count(raw_value));
            fields.push(FrontMatterField::new(key, Self::normalise(trimmed_value), key_range, value_range));
        }
        fields
    }

    fn is_key_character(ch: &str) -> bool {
        swift_text::is_letter(ch)
            || swift_text::is_number(ch)
            || swift_text::char_is(ch, '_')
            || swift_text::char_is(ch, '-')
            || swift_text::char_is(ch, '.')
            || swift_text::char_is(ch, ' ')
    }

    fn is_block_scalar_indicator(value: &str) -> bool {
        let indicator = swift_text::filter(value, |c| swift_text::char_is(c, '|') || swift_text::char_is(c, '>'));
        if !(swift_text::count(&indicator) == 1
            && swift_text::all_satisfy(value, |c| {
                swift_text::char_is(c, '|') || swift_text::char_is(c, '>') || swift_text::char_is(c, '-') || swift_text::char_is(c, '+')
            }))
        {
            return false;
        }
        ["|", ">", "|-", ">-", "|+", ">+"].iter().any(|form| swift_text::str_eq(value, form))
    }

    /// Consumes the indented (or blank) lines that make up a block scalar,
    /// returning the content lines and how many lines were consumed.
    fn block_scalar(map: &SourceMap, start: isize, limit: isize) -> (Vec<String>, isize) {
        let mut parts = Vec::new();
        let mut line = start;
        while line < limit {
            let text = map.string_of_line(line);
            if swift_text::is_blank_line(&text) {
                line += 1;
                continue;
            }
            let indent = swift_text::leading_indent(&text);
            if indent.is_empty() {
                break;
            }
            let dropped = swift_text::drop_first(&text, swift_text::count(indent));
            parts.push(swift_text::trim_whitespaces_and_newlines(dropped).to_owned());
            line += 1;
        }
        (parts, line - start)
    }

    /// Consumes `  - item` lines, returning the items and how many lines they took.
    fn block_sequence(map: &SourceMap, start: isize, limit: isize) -> (Vec<String>, isize) {
        let mut items = Vec::new();
        let mut line = start;
        while line < limit {
            let text = map.string_of_line(line);
            let trimmed = swift_text::trim_whitespaces(&text);
            if !(!swift_text::leading_indent(&text).is_empty()
                && (swift_text::has_prefix(trimmed, "- ") || swift_text::str_eq(trimmed, "-")))
            {
                break;
            }
            items.push(Self::normalise(swift_text::trim_whitespaces(swift_text::drop_first(trimmed, 1))));
            line += 1;
        }
        (items, line - start)
    }

    /// Unquotes scalars and flattens inline `[a, b]` lists to `a, b`.
    fn normalise(value: &str) -> String {
        let has_prefix = |p: &str| swift_text::has_prefix(value, p);
        let has_suffix = |p: &str| swift_text::has_suffix(value, p);
        if swift_text::count(value) >= 2 && ((has_prefix("\"") && has_suffix("\"")) || (has_prefix("'") && has_suffix("'"))) {
            swift_text::drop_last(swift_text::drop_first(value, 1), 1).to_owned()
        } else if has_prefix("[") && has_suffix("]") {
            let inner = swift_text::drop_last(swift_text::drop_first(value, 1), 1);
            Self::split_inline_array(inner)
                .iter()
                .map(|item| Self::normalise(swift_text::trim_whitespaces(item)))
                .filter(|item| !item.is_empty())
                .collect::<Vec<String>>()
                .join(", ")
        } else {
            value.to_owned()
        }
    }

    fn split_inline_array(inner: &str) -> Vec<String> {
        let mut items = Vec::new();
        let mut current = String::new();
        let mut in_double_quotes = false;
        let mut in_single_quotes = false;
        let mut is_escaped = false;

        for ch in swift_text::graphemes(inner) {
            if is_escaped {
                current.push_str(ch);
                is_escaped = false;
                continue;
            }
            if swift_text::char_is(ch, '\\') {
                current.push_str(ch);
                is_escaped = true;
                continue;
            }
            if swift_text::char_is(ch, '"') && !in_single_quotes {
                in_double_quotes = !in_double_quotes;
                current.push_str(ch);
            } else if swift_text::char_is(ch, '\'') && !in_double_quotes {
                in_single_quotes = !in_single_quotes;
                current.push_str(ch);
            } else if swift_text::char_is(ch, ',') && !in_double_quotes && !in_single_quotes {
                items.push(std::mem::take(&mut current));
            } else {
                current.push_str(ch);
            }
        }
        if !current.is_empty() {
            items.push(current);
        }
        items
    }
}
