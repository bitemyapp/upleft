//! Port of `Sources/DownrightQL/QuickLookLoader.swift`.
//!
//! Reads preview input without ever materialising more than the policy
//! allows. The initial stat is only a hint. The handle read is independently
//! capped, so replacing a small file with a large one between stat and open
//! cannot turn the full path into an unbounded allocation.

use std::io::Read;
use std::path::Path;

use upleft_core::document_io::string_from_data;
use upleft_core::model::TextEncodingKind;

use crate::quick_look_policy::{Presentation, QuickLookPolicy};

/// The result of a bounded Quick Look load. A file that was small when the
/// preview request began can grow before its handle is opened; in that case
/// it is treated exactly like the large-file path instead of being read
/// whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuickLookLoadedContent {
    Full(String),
    Prefix(String),
}

/// `QuickLookLoader`.
pub struct QuickLookLoader;

impl QuickLookLoader {
    /// `load(contentsOf:hintedByteCount:beforeRead:)`.
    pub fn load(url: &Path, hinted_byte_count: isize, before_read: Option<&dyn Fn()>) -> Option<QuickLookLoadedContent> {
        let hinted_presentation = QuickLookPolicy::presentation(hinted_byte_count);
        let limit = match hinted_presentation {
            Presentation::Full => QuickLookPolicy::FULL_READ_LIMIT_BYTES,
            Presentation::Prefix { .. } => QuickLookPolicy::PREFIX_READ_LIMIT_BYTES,
        } as usize;

        if let Some(before_read) = before_read {
            before_read();
        }
        // `FileHandle(forReadingFrom:)`; the handle is closed on return.
        let handle = std::fs::File::open(url).ok()?;

        // One sentinel byte tells us whether the file exceeded the cap while
        // keeping the allocation strictly bounded (limit + 1 bytes).
        // `try? handle.read(upToCount:)` is `nil` at end of file, so an empty
        // file yields no preview (Swift 6.4 on macOS 26).
        let mut bounded_data = Vec::new();
        handle.take(limit as u64 + 1).read_to_end(&mut bounded_data).ok()?;
        if bounded_data.is_empty() {
            return None;
        }
        let did_exceed_limit = bounded_data.len() > limit;
        let data = if did_exceed_limit { &bounded_data[..limit] } else { &bounded_data[..] };
        let text = decode(data)?;

        match hinted_presentation {
            Presentation::Prefix { .. } => Some(QuickLookLoadedContent::Prefix(text)),
            Presentation::Full => Some(if did_exceed_limit {
                QuickLookLoadedContent::Prefix(text)
            } else {
                QuickLookLoadedContent::Full(text)
            }),
        }
    }
}

fn decode(data: &[u8]) -> Option<String> {
    let bytes = &data[..data.len().min(4)];
    let (bom_length, encoding, code_unit_width) = if bytes.len() >= 4 && bytes == [0xFF, 0xFE, 0x00, 0x00] {
        (4, TextEncodingKind::Utf32LE, 4)
    } else if bytes.len() >= 4 && bytes == [0x00, 0x00, 0xFE, 0xFF] {
        (4, TextEncodingKind::Utf32BE, 4)
    } else if bytes.len() >= 3 && bytes[..3] == [0xEF, 0xBB, 0xBF] {
        (3, TextEncodingKind::Utf8, 1)
    } else if bytes.len() >= 2 && bytes[..2] == [0xFF, 0xFE] {
        (2, TextEncodingKind::Utf16LE, 2)
    } else if bytes.len() >= 2 && bytes[..2] == [0xFE, 0xFF] {
        (2, TextEncodingKind::Utf16BE, 2)
    } else if let Some((encoding, width)) = sniff_bomless_encoding(data) {
        (0, encoding, width)
    } else if string_from_data(data, TextEncodingKind::Utf8).is_some() {
        (0, TextEncodingKind::Utf8, 1)
    } else {
        (0, TextEncodingKind::Latin1, 1)
    };

    let mut body = &data[bom_length.min(data.len())..];
    if code_unit_width > 1 && body.len() % code_unit_width != 0 {
        body = &body[..body.len() - body.len() % code_unit_width];
    }

    let text = match string_from_data(body, encoding) {
        Some(decoded) => decoded,
        None => {
            // A UTF-8 head can end in a torn scalar. Trim only the small
            // trailing suffix needed to recover a valid preview.
            if encoding != TextEncodingKind::Utf8 {
                return None;
            }
            let mut recovered = None;
            for drop in 1..=3usize {
                if body.len() > drop {
                    recovered = string_from_data(&body[..body.len() - drop], TextEncodingKind::Utf8);
                    if recovered.is_some() {
                        break;
                    }
                }
            }
            recovered?
        }
    };
    Some(normalize_line_endings(&text))
}

fn sniff_bomless_encoding(data: &[u8]) -> Option<(TextEncodingKind, usize)> {
    if data.len() < 4 {
        return None;
    }
    let bytes = &data[..data.len().min(256)];
    let mut even_nuls = 0;
    let mut odd_nuls = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == 0 {
            if index % 2 == 0 {
                even_nuls += 1;
            } else {
                odd_nuls += 1;
            }
        }
    }
    let count = bytes.len() as f64;
    let even_ratio = even_nuls as f64 / count;
    let odd_ratio = odd_nuls as f64 / count;
    if even_ratio >= 0.3 && even_ratio > odd_ratio * 2.0 {
        return Some((TextEncodingKind::Utf16BE, 2));
    }
    if odd_ratio >= 0.3 && odd_ratio > even_ratio * 2.0 {
        return Some((TextEncodingKind::Utf16LE, 2));
    }
    if even_ratio >= 0.45
        && odd_ratio >= 0.45
        && let Some(first_non_zero) = bytes.iter().position(|&byte| byte != 0)
    {
        return Some((
            if first_non_zero % 4 == 0 { TextEncodingKind::Utf32LE } else { TextEncodingKind::Utf32BE },
            4,
        ));
    }
    None
}

fn normalize_line_endings(text: &str) -> String {
    let mut saw_lf = false;
    let mut saw_crlf = false;
    let mut saw_cr = false;
    let mut previous_was_cr = false;
    for scalar in text.chars() {
        if previous_was_cr {
            if scalar == '\n' {
                saw_crlf = true;
                previous_was_cr = false;
                continue;
            }
            saw_cr = true;
            previous_was_cr = false;
        }
        if scalar == '\r' {
            previous_was_cr = true;
        } else if scalar == '\n' {
            saw_lf = true;
        }
    }
    if previous_was_cr {
        saw_cr = true;
    }
    if saw_crlf && !saw_lf && !saw_cr {
        return upleft_swift_text::replacing_occurrences(text, "\r\n", "\n");
    }
    if saw_cr && !saw_lf && !saw_crlf {
        return upleft_swift_text::replacing_occurrences(text, "\r", "\n");
    }
    text.to_owned()
}
