//! Port of `Mermaid/src_text_metrics.swift` (from `original/src/text-metrics.ts`):
//! the character-class width heuristic every layout measures text with.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::swift::{self, Text};

const NARROW_CHARS: &str = "iltfjI1!|.,:;'";
const WIDE_CHARS: &str = "WMwm@%";
const VERY_WIDE_CHARS: &str = "WM";
const SEMI_NARROW_PUNCT: &str = "()[]{}\\/-\"`";

pub const LINE_HEIGHT_RATIO: f64 = 1.3;

#[derive(Debug, Clone, PartialEq)]
pub struct MultilineMetrics {
    pub width: f64,
    pub height: f64,
    pub lines: Vec<String>,
    pub line_height: f64,
}

fn is_combining_mark(code: u32) -> bool {
    (0x0300..=0x036F).contains(&code)
        || (0x1AB0..=0x1AFF).contains(&code)
        || (0x1DC0..=0x1DFF).contains(&code)
        || (0x20D0..=0x20FF).contains(&code)
        || (0xFE20..=0xFE2F).contains(&code)
}

fn is_fullwidth(code: u32) -> bool {
    (0x1100..=0x115F).contains(&code)
        || (0x2E80..=0x2EFF).contains(&code)
        || (0x2F00..=0x2FDF).contains(&code)
        || (0x3000..=0x303F).contains(&code)
        || (0x3040..=0x309F).contains(&code)
        || (0x30A0..=0x30FF).contains(&code)
        || (0x3100..=0x312F).contains(&code)
        || (0x3130..=0x318F).contains(&code)
        || (0x3190..=0x31FF).contains(&code)
        || (0x3200..=0x33FF).contains(&code)
        || (0x3400..=0x4DBF).contains(&code)
        || (0x4E00..=0x9FFF).contains(&code)
        || (0xAC00..=0xD7AF).contains(&code)
        || (0xF900..=0xFAFF).contains(&code)
        || (0xFF00..=0xFF60).contains(&code)
        || (0xFFE0..=0xFFE6).contains(&code)
        || code >= 0x20000
}

thread_local! {
    static EMOJI: RefCell<HashMap<String, bool>> = RefCell::new(HashMap::new());
}

/// `EMOJI_REGEX.firstMatch(in: String(char)) != nil`, with ICU's emoji
/// properties. No ASCII character has `Emoji_Presentation` or
/// `Extended_Pictographic`.
fn is_emoji(grapheme: &str) -> bool {
    if grapheme.is_ascii() {
        return false;
    }
    EMOJI.with(|cache| {
        if let Some(&known) = cache.borrow().get(grapheme) {
            return known;
        }
        let result = swift::regex(r"[\p{Emoji_Presentation}\p{Extended_Pictographic}]", false)
            .is_some_and(|r| r.is_match(&Text::new(grapheme)));
        cache.borrow_mut().insert(grapheme.to_owned(), result);
        result
    })
}

/// `getCharWidth(_:)` for one `Character` (an extended grapheme cluster).
pub fn get_char_width(grapheme: &str) -> f64 {
    let Some(scalar) = grapheme.chars().next() else {
        return 0.0;
    };
    let code = scalar as u32;

    if is_combining_mark(code) {
        return 0.0;
    }
    if is_fullwidth(code) || is_emoji(grapheme) {
        return 2.0;
    }
    if swift::grapheme_is(grapheme, ' ') {
        return 0.3;
    }
    if swift::character_in(grapheme, VERY_WIDE_CHARS) {
        return 1.5;
    }
    if swift::character_in(grapheme, WIDE_CHARS) {
        return 1.2;
    }
    if swift::character_in(grapheme, NARROW_CHARS) {
        return 0.4;
    }
    if swift::character_in(grapheme, SEMI_NARROW_PUNCT) {
        return 0.5;
    }
    if swift::grapheme_is(grapheme, 'r') {
        return 0.8;
    }
    if (65..=90).contains(&code) {
        return 1.2;
    }
    if (48..=57).contains(&code) {
        return 1.0;
    }
    1.0
}

/// `measureTextWidth(_:fontSize:fontWeight:)`.
pub fn measure_text_width(text: &str, font_size: f64, font_weight: i64) -> f64 {
    let base_ratio = if font_weight >= 600 {
        0.60
    } else if font_weight >= 500 {
        0.57
    } else {
        0.54
    };

    let mut total_width = 0.0;
    if text.is_ascii() && !text.contains('\r') {
        for b in text.as_bytes() {
            // Each byte is one Character.
            let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_ref(b)) };
            total_width += get_char_width(s);
        }
    } else {
        for grapheme in upleft_swift_text::graphemes(text) {
            total_width += get_char_width(grapheme);
        }
    }
    let min_padding = font_size * 0.15;
    total_width * font_size * base_ratio + min_padding
}

/// `measureMultilineText(_:fontSize:fontWeight:)`.
pub fn measure_multiline_text(text: &str, font_size: f64, font_weight: i64) -> MultilineMetrics {
    let lines = swift::components_separated_by_newline(text);
    let line_height = font_size * LINE_HEIGHT_RATIO;

    let mut max_width = 0.0;
    for line in &lines {
        let plain = strip_formatting_tags(line);
        let w = measure_text_width(&plain, font_size, font_weight);
        if w > max_width {
            max_width = w;
        }
    }

    MultilineMetrics {
        width: max_width,
        height: lines.len() as f64 * line_height,
        lines: lines.iter().map(|s| (*s).to_owned()).collect(),
        line_height,
    }
}

fn strip_formatting_tags(text: &str) -> String {
    if !text.contains('<') {
        return text.to_owned();
    }
    swift::regex_replace(text, r"</?(?:b|strong|i|em|u|s|del)\s*>", "", true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(get_char_width("i"), 0.4);
        assert_eq!(get_char_width("W"), 1.5);
        assert_eq!(get_char_width("m"), 1.2);
        assert_eq!(get_char_width(" "), 0.3);
        assert_eq!(get_char_width("r"), 0.8);
        assert_eq!(get_char_width("A"), 1.2);
        assert_eq!(get_char_width("日"), 2.0);
        assert_eq!(get_char_width("🚀"), 2.0);
        assert_eq!(get_char_width("\u{301}"), 0.0);
        let w = measure_text_width("Start", 13.0, 500);
        assert_eq!(w, (1.2 + 0.4 + 1.0 + 0.8 + 0.4) * 13.0 * 0.57 + 13.0 * 0.15);
    }
}
