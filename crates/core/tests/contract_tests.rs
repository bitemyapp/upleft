//! ContractTests.swift — properties the render and app layers rely on but
//! that no single feature test would otherwise pin down.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use common::corpus;
use upleft_core::ast_diff::ASTDiff;
use upleft_core::list_editing::ListEditing;
use upleft_core::metrics::Metrics;
use upleft_core::model::{BlockContent, BlockIdentity, InlineKind, MDBlock};
use upleft_core::parser::MarkdownParser;
use upleft_core::restructure::Restructure;
use upleft_core::structural_zoom::StructuralZoom;
use upleft_core::swift_text::{self, ns::NSStringExt};
use upleft_core::tidy::TidyDocument;
use upleft_core::{ParseOptions, ZoomLevel};

#[test]
fn parse_options_disable_their_passes() {
    let text = "---\na: b\n---\n\n$x$ and [[W]] and src/a.ts and\n\n```mermaid\ngraph TD;\n```\n";
    let off = ParseOptions {
        detect_front_matter: false,
        detect_math: false,
        detect_callouts: false,
        detect_wikilinks: false,
        detect_path_tokens: false,
        detect_mermaid: false,
        ..ParseOptions::DEFAULT
    };
    let doc = MarkdownParser::parse_with(text, off);
    assert!(doc.front_matter.is_none());
    assert!(doc.path_tokens.is_empty());
    let mut kinds: Vec<&str> = Vec::new();
    doc.root.walk(&mut |block| {
        if matches!(block.content, BlockContent::Mermaid { .. }) {
            kinds.push("mermaid");
        }
        if matches!(block.content, BlockContent::MathBlock { .. }) {
            kinds.push("math");
        }
        for span in &block.inlines {
            span.walk(&mut |inline| {
                if matches!(inline.kind, InlineKind::InlineMath { .. }) {
                    kinds.push("inlineMath");
                }
                if matches!(inline.kind, InlineKind::Wikilink { .. }) {
                    kinds.push("wikilink");
                }
            });
        }
    });
    assert!(kinds.is_empty(), "disabled passes still ran: {kinds:?}");
}

#[test]
fn extension_pass_limit_skips_optional_passes() {
    let text = "Math $x^2$ and src/a.ts here.\n";
    let limited = MarkdownParser::parse_with(text, ParseOptions { extension_pass_limit: 4, ..ParseOptions::DEFAULT });
    assert!(limited.path_tokens.is_empty());
    // The block structure is still parsed; only the extension passes stop.
    assert_eq!(limited.root.children.len(), 1);
}

#[test]
fn path_tokens_are_in_document_order() {
    let doc = MarkdownParser::parse(corpus::KITCHEN_SINK);
    for pair in doc.path_tokens.windows(2) {
        assert!(pair[0].range.location < pair[1].range.location);
    }
}

#[test]
fn line_starts_agree_with_the_document_helpers() {
    for (name, text) in corpus::ALL {
        let doc = MarkdownParser::parse(text);
        if !(doc.length > 0) {
            continue;
        }
        for (index, &start) in doc.line_starts.iter().enumerate() {
            let index = index as isize;
            assert_eq!(doc.line_at(start), index + 1, "{name} line {index}");
            let range = doc.range_of_line(index + 1);
            assert_eq!(range.location, start);
            assert!(!swift_text::contains(&doc.substring(range), "\n"));
        }
    }
}

#[test]
fn block_lookup_finds_the_deepest_block() {
    let doc = MarkdownParser::parse("# H\n\n- item **bold**\n");
    let offset = doc.utf16.as_slice().range_of_literal(&swift_text::ns::utf16("bold"), upleft_core::NSRange::new(0, doc.length)).location;
    let block = doc.root.block_at(offset);
    assert!(block.is_some());
    assert!(matches!(block.unwrap().content, BlockContent::Paragraph), "expected the paragraph inside the item");
}

/// `block_at` binary searches its children, which is only sound while
/// siblings stay in source order and never overlap. Checks both the
/// invariant and the answer at every offset against a linear scan.
#[test]
fn block_lookup_matches_a_linear_scan_at_every_offset() {
    fn linear(block: &Arc<MDBlock>, offset: isize) -> Option<Arc<MDBlock>> {
        if !block.range.touches(offset) {
            return None;
        }
        for child in &block.children {
            if let Some(hit) = linear(child, offset) {
                return Some(hit);
            }
        }
        Some(block.clone())
    }
    fn check_ordering(block: &MDBlock) {
        for pair in block.children.windows(2) {
            assert!(pair[0].range.location <= pair[1].range.location);
            assert!(pair[0].range.upper_bound() <= pair[1].range.location);
        }
        for child in &block.children {
            check_ordering(child);
        }
    }

    for (_, text) in corpus::ALL {
        let doc = MarkdownParser::parse(text);
        check_ordering(&doc.root);
        for offset in 0..=doc.length {
            let fast = doc.root.block_at(offset);
            let slow = linear(&doc.root, offset);
            assert_eq!(fast.map(|b| Arc::as_ptr(&b)), slow.map(|b| Arc::as_ptr(&b)));
        }
    }
}

#[test]
fn subtree_hashes_are_assigned_everywhere() {
    let doc = MarkdownParser::parse(corpus::KITCHEN_SINK);
    doc.root.walk(&mut |block| assert_ne!(block.subtree_hash, 0));
}

#[test]
fn identities_are_unique_among_siblings() {
    let doc = MarkdownParser::parse(corpus::KITCHEN_SINK);
    fn check(children: &[Arc<MDBlock>]) {
        let mut seen: HashSet<BlockIdentity> = HashSet::new();
        for child in children {
            assert!(seen.insert(child.identity), "duplicate identity {:?}", child.identity);
            check(&child.children);
        }
    }
    check(&doc.root.children);
}

/// A coarse quadratic-behaviour probe, not the §12 benchmark.
#[test]
fn parses_a_hundred_kilobytes_quickly() {
    let unit = format!("{}\n\n", corpus::KITCHEN_SINK);
    let mut text = String::new();
    while text.len() < 100_000 {
        text.push_str(&unit);
    }
    let start = std::time::Instant::now();
    let doc = MarkdownParser::parse(&text);
    let elapsed = start.elapsed().as_secs_f64();
    assert!(doc.length > 0);
    assert!(elapsed < 5.0, "100KB parse took {elapsed}s");
}

#[test]
fn parsing_is_deterministic() {
    let a = MarkdownParser::parse(corpus::KITCHEN_SINK);
    let b = MarkdownParser::parse(corpus::KITCHEN_SINK);
    assert_eq!(a.root.subtree_hash, b.root.subtree_hash);
    let slugs = |doc: &upleft_core::ParsedDocument| doc.headings.iter().map(|h| h.slug.clone()).collect::<Vec<_>>();
    assert_eq!(slugs(&a), slugs(&b));
}

#[test]
fn degenerate_inputs_do_not_crash() {
    for text in ["", "\n", "\n\n\n", " ", "\t", "#", "```", "|", "> ", "- ", "[", "$", "\u{FEFF}"] {
        let doc = MarkdownParser::parse(text);
        assert_eq!(doc.length, swift_text::utf16_count(text));
        let _ = TidyDocument::plan(&doc);
        let _ = StructuralZoom::plan(&doc, ZoomLevel::Skeleton);
        let _ = Metrics::metrics_for(text);
        let _ = Restructure::table_of_contents(&doc, 6);
        let _ = ListEditing::continuation(&doc, 0);
        let _ = ASTDiff::dirty_set(None, &doc);
    }
}
