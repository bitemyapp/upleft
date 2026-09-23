//! Port of `Mermaid/src_multiline_utils.swift` (from
//! `original/src/multiline-utils.ts`). Only `normalizeBrTags` and
//! `stripFormattingTags` are reachable from the image path; the SVG text
//! helpers are not ported.

use crate::swift;

/// `normalizeBrTags(_:)`: unquote, `<br>` and a literal `\n` to newlines,
/// drop `<sub>`/`<sup>`/`<small>`/`<mark>`, Markdown emphasis to tags.
pub fn normalize_br_tags(label: &str) -> String {
    let unquoted = if swift::has_prefix(label, "\"") && swift::has_suffix(label, "\"") && swift::character_count(label) >= 2 {
        swift::drop_last(swift::drop_first(label, 1), 1)
    } else {
        label
    };

    let mut result = unquoted.to_owned();
    if result.contains('<') {
        result = swift::regex_replace(&result, r"<br\s*/?>", "\n", true);
    }
    if result.contains("\\n") {
        result = result.replace("\\n", "\n");
    }
    if result.contains('<') {
        result = swift::regex_replace(&result, r"</?(?:sub|sup|small|mark)\s*>", "", true);
    }

    // Markdown formatting -> HTML tags (order matters)
    if result.contains('*') {
        result = swift::regex_replace(&result, r"\*\*(.+?)\*\*", "<b>$1</b>", false);
        result = swift::regex_replace(&result, r"\*([^\s*](?:[^*]*[^\s*])?)\*", "<i>$1</i>", false);
    }
    if result.contains('~') {
        result = swift::regex_replace(&result, r"~~(.+?)~~", "<s>$1</s>", false);
    }

    result
}

/// `stripFormattingTags(_:)`.
pub fn strip_formatting_tags(text: &str) -> String {
    swift::regex_replace(text, r"</?(?:b|strong|i|em|u|s|del)\s*>", "", true)
}
