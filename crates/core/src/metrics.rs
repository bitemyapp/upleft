//! Metrics.swift — plain-text extraction and reading metadata (§9.6).
//!
//! "Words" means words a human reads: markers, code, math, raw HTML and front
//! matter are excluded. Sentence counting uses NaturalLanguage's
//! `NLTokenizer`, as Downright does, through objc2.

use std::sync::Arc;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_foundation::{NSRange as FRange, NSString};
use objc2_natural_language::{NLTokenUnit, NLTokenizer};

use crate::contracts::{ParseOptions, ReadingMetrics};
use crate::model::{BlockContent, InlineKind, InlineSpan, MDBlock, ParsedDocument};
use crate::ns_range::NSRange;
use crate::parser::MarkdownParser;
use crate::swift_text::{self, ns::NSStringExt};

// MARK: - Plain text extraction

pub struct PlainText;

impl PlainText {
    /// Readable text of one block, markers removed.
    pub fn of(block: &MDBlock, text: &[u16]) -> String {
        let mut out = String::new();
        Self::append_block(block, text, &mut out);
        out
    }

    pub fn of_spans(spans: &[InlineSpan], text: &[u16]) -> String {
        let mut out = String::new();
        Self::append_spans(spans, text, &mut out);
        out
    }

    fn append_block(block: &MDBlock, text: &[u16], out: &mut String) {
        match &block.content {
            BlockContent::CodeBlock { .. }
            | BlockContent::Mermaid { .. }
            | BlockContent::MathBlock { .. }
            | BlockContent::HtmlBlock
            | BlockContent::FrontMatter(_)
            | BlockContent::ThematicBreak => return,
            BlockContent::Table(table) => {
                for (r_idx, row) in table.rows.iter().enumerate() {
                    if r_idx > 0 && !out.is_empty() {
                        out.push(' ');
                    }
                    for (c_idx, cell) in row.cells.iter().enumerate() {
                        if c_idx > 0 && !out.is_empty() {
                            out.push(' ');
                        }
                        Self::append_spans(&cell.inlines, text, out);
                    }
                }
                return;
            }
            _ => {}
        }
        if !block.inlines.is_empty() {
            Self::append_spans(&block.inlines, text, out);
            return;
        }
        let count = block.children.len();
        for (index, child) in block.children.iter().enumerate() {
            let before = out.len();
            Self::append_block(child, text, out);
            // `out.count > beforeCount`: the Character count grew. Appended
            // text that is only combining marks joins the last Character and
            // leaves the count unchanged.
            if character_count_grew(out, before) && index < count - 1 {
                out.push(' ');
            }
        }
    }

    fn append_spans(spans: &[InlineSpan], text: &[u16], out: &mut String) {
        for span in spans {
            match &span.kind {
                InlineKind::Text | InlineKind::PathToken(_) => out.push_str(&text.substring(span.range)),
                InlineKind::InlineCode => out.push_str(&text.substring(span.content_range)),
                InlineKind::SoftBreak | InlineKind::LineBreak => out.push(' '),
                InlineKind::InlineHTML | InlineKind::InlineMath { .. } | InlineKind::FootnoteReference { .. } => continue,
                InlineKind::Wikilink { target, label } => out.push_str(label.as_deref().unwrap_or(target)),
                _ => Self::append_spans(&span.children, text, out),
            }
        }
    }

    /// Prose within `range`, used for read time and word counts.
    pub fn prose(root: &Arc<MDBlock>, range: NSRange, text: &[u16]) -> String {
        let mut pieces: Vec<String> = Vec::new();
        root.walk_pruning(&mut |block| {
            if !(block.range.upper_bound() > range.location && block.range.location < range.upper_bound()) {
                return false;
            }
            match block.content {
                BlockContent::Document
                | BlockContent::BlockQuote
                | BlockContent::Callout { .. }
                | BlockContent::List { .. }
                | BlockContent::ListItem { .. } => true,
                _ => {
                    let piece = Self::of(block, text);
                    if !piece.is_empty() {
                        pieces.push(piece);
                    }
                    false
                }
            }
        });
        pieces.join("\n")
    }

    /// Prose per section span, computed in a single tree walk. `spans` must be
    /// ordered by location and non-overlapping.
    pub fn prose_per_section(root: &Arc<MDBlock>, spans: &[NSRange], text: &[u16]) -> Vec<String> {
        if spans.is_empty() {
            return Vec::new();
        }
        let mut buffers = vec![String::new(); spans.len()];

        root.walk_pruning(&mut |block| match block.content {
            BlockContent::Document
            | BlockContent::BlockQuote
            | BlockContent::Callout { .. }
            | BlockContent::List { .. }
            | BlockContent::ListItem { .. } => true,
            _ => {
                let location = block.range.location;
                let upper = block.range.upper_bound();
                let (mut lo, mut hi, mut best) = (0isize, spans.len() as isize - 1, -1isize);
                while lo <= hi {
                    let mid = (lo + hi) / 2;
                    if spans[mid as usize].location <= location {
                        best = mid;
                        lo = mid + 1;
                    } else {
                        hi = mid - 1;
                    }
                }
                if best >= 0 {
                    let span = spans[best as usize];
                    if upper > span.location && location < span.upper_bound() {
                        let piece = Self::of(block, text);
                        if !piece.is_empty() {
                            let buffer = &mut buffers[best as usize];
                            if buffer.is_empty() {
                                *buffer = piece;
                            } else {
                                buffer.push('\n');
                                buffer.push_str(&piece);
                            }
                        }
                    }
                }
                false
            }
        });
        buffers
    }
}

/// Whether `out` has more Characters than its first `before` bytes had.
fn character_count_grew(out: &str, before: usize) -> bool {
    if out.len() == before {
        return false;
    }
    if before == 0 {
        return true;
    }
    let head = &out[..before];
    let tail_start = match swift_text::last(head) {
        Some(last) => before - last.len(),
        None => 0,
    };
    let tail = &out[tail_start..];
    // Appending can only merge into the previous last Character; it grew iff
    // the tail from that Character on is now more than one Character.
    let mut graphemes = swift_text::graphemes(tail);
    graphemes.next();
    graphemes.next().is_some()
}

// MARK: - Sentence tokenizer

/// `NLTokenizer(unit: .sentence)`.
pub struct SentenceTokenizer {
    tokenizer: Retained<NLTokenizer>,
}

impl Default for SentenceTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl SentenceTokenizer {
    pub fn new() -> SentenceTokenizer {
        let tokenizer = unsafe { NLTokenizer::initWithUnit(NLTokenizer::alloc(), NLTokenUnit::Sentence) };
        SentenceTokenizer { tokenizer }
    }

    /// Sets `string` and enumerates the sentence ranges (UTF-16) over the
    /// whole string; `visit` returns `false` to stop.
    pub fn enumerate(&self, source: &str, visit: impl FnMut(NSRange) -> bool) {
        let visit = std::cell::RefCell::new(visit);
        objc2::rc::autoreleasepool(|_| {
            let string = NSString::from_str(source);
            let length = string.length();
            unsafe { self.tokenizer.setString(Some(&string)) };
            let block = block2::StackBlock::new(
                |range: FRange, _attributes: objc2_natural_language::NLTokenizerAttributes, stop: std::ptr::NonNull<objc2::runtime::Bool>| {
                    if !(visit.borrow_mut())(NSRange::new(range.location as isize, range.length as isize)) {
                        unsafe { *stop.as_ptr() = objc2::runtime::Bool::YES };
                    }
                },
            );
            unsafe { self.tokenizer.enumerateTokensInRange_usingBlock(FRange::new(0, length), &block) };
        });
    }
}

// MARK: - Reading metadata (§9.6)

pub struct Metrics;

impl Metrics {
    /// The median silent-reading rate for prose.
    pub const WORDS_PER_MINUTE: f64 = 238.0;

    pub fn metrics_for(text: &str) -> ReadingMetrics {
        if text.is_empty() {
            return ReadingMetrics::ZERO;
        }
        let document = MarkdownParser::parse_with(text, ParseOptions::STRUCTURE_ONLY);
        Self::metrics_of(&PlainText::prose(&document.root, NSRange::new(0, document.length), &document.utf16))
    }

    /// Parallel to `doc.headings`; each entry covers that section's own prose.
    pub fn section_metrics(doc: &ParsedDocument) -> Vec<ReadingMetrics> {
        let headings = &doc.headings;
        if headings.is_empty() {
            return Vec::new();
        }
        let mut spans: Vec<NSRange> = Vec::with_capacity(headings.len());
        for (index, heading) in headings.iter().enumerate() {
            let end = if index + 1 < headings.len() { headings[index + 1].range.location } else { doc.length };
            let start = heading.range.upper_bound();
            spans.push(NSRange::new(start, 0.max(end - start)));
        }
        PlainText::prose_per_section(&doc.root, &spans, &doc.utf16).iter().map(|prose| Self::metrics_of(prose)).collect()
    }

    /// Readable word count for the whole document.
    pub fn document_word_count(doc: &ParsedDocument) -> isize {
        Self::word_count(&PlainText::prose(&doc.root, NSRange::new(0, doc.length), &doc.utf16))
    }

    /// First sentence of the prose in `range`, as a source range (§5.2).
    pub fn first_sentence_range(doc: &ParsedDocument, range: NSRange) -> Option<NSRange> {
        Self::first_sentence_range_with(doc, range, &doc.utf16, &SentenceTokenizer::new())
    }

    /// The same lookup with the per-call costs hoisted out.
    pub fn first_sentence_range_with(
        doc: &ParsedDocument,
        range: NSRange,
        text: &[u16],
        tokenizer: &SentenceTokenizer,
    ) -> Option<NSRange> {
        if !(range.length > 0 && range.upper_bound() <= doc.length) {
            return None;
        }
        let paragraph = Self::first_prose_block(&doc.root, range)?;
        let lower = range.location.max(paragraph.content_range.location);
        let bounds = NSRange::new(lower, range.upper_bound().min(paragraph.content_range.upper_bound()) - lower);
        if !(bounds.length > 0) {
            return None;
        }
        let source = text.substring(bounds);
        let mut result: Option<NSRange> = None;
        tokenizer.enumerate(&source, |token| {
            result = Some(NSRange::new(bounds.location + token.location, token.length));
            false
        });
        result
    }

    fn first_prose_block(root: &Arc<MDBlock>, range: NSRange) -> Option<Arc<MDBlock>> {
        let mut found: Option<Arc<MDBlock>> = None;
        root.walk_pruning(&mut |block| {
            if found.is_some() {
                return false;
            }
            if !(block.range.upper_bound() > range.location && block.range.location < range.upper_bound()) {
                return false;
            }
            match block.content {
                BlockContent::Paragraph => {
                    found = Some(block.clone());
                    false
                }
                BlockContent::Document
                | BlockContent::BlockQuote
                | BlockContent::Callout { .. }
                | BlockContent::List { .. }
                | BlockContent::ListItem { .. } => true,
                _ => false,
            }
        });
        found
    }

    /// `metrics(of:)`.
    pub fn metrics_of(prose: &str) -> ReadingMetrics {
        let words = Self::word_count(prose);
        ReadingMetrics::new(
            words,
            swift_text::count(prose) as isize,
            Self::sentence_count(prose),
            words as f64 / Self::WORDS_PER_MINUTE,
        )
    }

    pub fn word_count(prose: &str) -> isize {
        let mut count = 0isize;
        let mut in_word = false;
        for scalar in prose.chars() {
            let v = scalar as u32;
            let is_word = if v < 128 {
                (0x30..=0x39).contains(&v) || (0x41..=0x5A).contains(&v) || (0x61..=0x7A).contains(&v) || v == 0x27 || v == 0x2D
            } else {
                scalar == '\u{2019}' || swift_text::scalar_is_alphabetic(scalar) || swift_text::scalar_is_numeric(scalar)
            };
            if is_word {
                if !in_word {
                    count += 1;
                    in_word = true;
                }
            } else {
                in_word = false;
            }
        }
        count
    }

    pub fn sentence_count(prose: &str) -> isize {
        if prose.is_empty() {
            return 0;
        }
        let tokenizer = SentenceTokenizer::new();
        let mut count = 0isize;
        tokenizer.enumerate(prose, |_| {
            count += 1;
            true
        });
        count
    }
}
