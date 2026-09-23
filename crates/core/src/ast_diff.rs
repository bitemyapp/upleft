//! ASTDiff.swift — which blocks to re-decorate after a reparse (§3.5).
//!
//! "Reparse the whole document on every edit, diff the resulting AST against
//! the previous one by subtree hash, and re-decorate only the changed
//! blocks." Sibling lists are matched by hash with Myers first, so inserting a
//! line near the top of a long document does not dirty everything after it;
//! only the blocks left unmatched are compared pairwise and recursed into.

use std::sync::Arc;

use crate::contracts::DirtySet;
use crate::hashing::FNV;
use crate::model::{BlockContent, MDBlock, ParsedDocument};
use crate::myers::{Myers, Step};
use crate::ns_range::NSRange;
use crate::swift_text;

pub struct ASTDiff;

impl ASTDiff {
    pub fn dirty_set(old: Option<&ParsedDocument>, new: &ParsedDocument) -> DirtySet {
        let Some(old) = old else {
            return DirtySet::wholesale();
        };
        if swift_text::str_eq(&old.text, &new.text) {
            return DirtySet::none();
        }

        let old_top = &old.root.children;
        let new_top = &new.root.children;
        // A structural rewrite is cheaper to redecorate wholesale than to
        // reconcile.
        let larger = old_top.len().max(new_top.len()) as isize;
        if larger > 0 && (old_top.len() as isize - new_top.len() as isize).abs() * 2 > larger {
            return DirtySet::wholesale();
        }

        let mut ranges = Vec::new();
        if !Self::reconcile(old_top, new_top, &mut ranges, &old.utf16, &new.utf16) {
            return DirtySet::wholesale();
        }
        DirtySet::new(Self::coalesce(ranges), false)
    }

    /// Returns false when the diff gave up and the caller should go
    /// wholesale.
    fn reconcile(
        old: &[Arc<MDBlock>],
        new: &[Arc<MDBlock>],
        ranges: &mut Vec<NSRange>,
        old_text: &[u16],
        new_text: &[u16],
    ) -> bool {
        let old_hashes: Vec<u64> = old.iter().map(|block| block.subtree_hash).collect();
        let new_hashes: Vec<u64> = new.iter().map(|block| block.subtree_hash).collect();
        let Some(script) = Myers::diff_default(&old_hashes, &new_hashes) else {
            return false;
        };

        // Collect the unmatched runs between anchors, then pair them up.
        let mut old_run: Vec<&MDBlock> = Vec::new();
        let mut new_run: Vec<&MDBlock> = Vec::new();

        for step in script {
            match step {
                Step::Equal { .. } => {
                    Self::pair(&old_run, &new_run, ranges, old_text, new_text);
                    old_run.clear();
                    new_run.clear();
                }
                Step::Delete { old_index } => old_run.push(&old[old_index as usize]),
                Step::Insert { new_index } => new_run.push(&new[new_index as usize]),
            }
        }
        Self::pair(&old_run, &new_run, ranges, old_text, new_text);
        true
    }

    /// Pairs a run of changed old blocks against a run of changed new blocks.
    /// Same-kind pairs recurse so an edit inside one list item dirties that
    /// item rather than the list; everything else is dirty in full. When the
    /// container's own bytes (markers, blank quote lines) changed underneath
    /// unchanged children, the container itself is dirtied too.
    fn pair(old: &[&MDBlock], new: &[&MDBlock], ranges: &mut Vec<NSRange>, old_text: &[u16], new_text: &[u16]) {
        for (index, &after) in new.iter().enumerate() {
            if index >= old.len() {
                ranges.push(after.range);
                continue;
            }
            let before = old[index];
            let same_kind = discriminator(&before.content) == discriminator(&after.content);
            let same_container_semantics = Self::container_semantics_match(&before.content, &after.content);
            if same_kind && same_container_semantics && !before.children.is_empty() && !after.children.is_empty() {
                let mut nested = Vec::new();
                if Self::reconcile(&before.children, &after.children, &mut nested, old_text, new_text) {
                    if framework_hash(before, old_text) != framework_hash(after, new_text) {
                        ranges.push(after.range);
                    }
                    ranges.extend(nested);
                    continue;
                }
            }
            ranges.push(after.range);
        }
    }

    /// Child hashes do not describe a container's own marker metadata. A task
    /// toggle, list start change, or callout-kind edit can leave every child
    /// byte unchanged while still requiring the parent decoration to refresh.
    fn container_semantics_match(old: &BlockContent, new: &BlockContent) -> bool {
        match (old, new) {
            (
                BlockContent::Callout { kind: old_kind, title: old_title },
                BlockContent::Callout { kind: new_kind, title: new_title },
            ) => old_kind == new_kind && optional_string_eq(old_title.as_deref(), new_title.as_deref()),
            (
                BlockContent::List { ordered: old_ordered, start: old_start, tight: old_tight, marker: old_marker },
                BlockContent::List { ordered: new_ordered, start: new_start, tight: new_tight, marker: new_marker },
            ) => {
                old_ordered == new_ordered
                    && old_start == new_start
                    && old_tight == new_tight
                    && old_marker == new_marker
            }
            (
                BlockContent::ListItem { ordinal: old_ordinal, checkbox: old_box },
                BlockContent::ListItem { ordinal: new_ordinal, checkbox: new_box },
            ) => {
                old_ordinal == new_ordinal
                    && old_box.map(|b| b.is_checked) == new_box.map(|b| b.is_checked)
            }
            (
                BlockContent::FootnoteDefinition { identifier: old_id },
                BlockContent::FootnoteDefinition { identifier: new_id },
            ) => swift_text::str_eq(old_id, new_id),
            _ => true,
        }
    }

    fn coalesce(ranges: Vec<NSRange>) -> Vec<NSRange> {
        if ranges.len() <= 1 {
            return ranges;
        }
        let mut sorted = ranges;
        sorted.sort_by_key(|range| range.location); // stable, like Swift's `sorted`
        let mut out: Vec<NSRange> = Vec::with_capacity(sorted.len());
        out.push(sorted[0]);
        for &range in &sorted[1..] {
            let last = out.len() - 1;
            if range.location <= out[last].upper_bound() {
                out[last] = out[last].union(range);
            } else {
                out.push(range);
            }
        }
        out
    }
}

/// `String? == String?`.
fn optional_string_eq(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => swift_text::str_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

// Parser.swift's `BlockIdentifier.discriminator` and
// `SubtreeHasher.frameworkHash`, which live with the parser in Downright. They
// are private here until the parser port provides them.

/// `BlockIdentifier.discriminator(_:)`.
fn discriminator(content: &BlockContent) -> isize {
    match content {
        BlockContent::Document => 0,
        BlockContent::Heading { level } => 100 + level,
        BlockContent::Paragraph => 2,
        BlockContent::BlockQuote => 3,
        BlockContent::Callout { .. } => 4,
        BlockContent::List { .. } => 5,
        BlockContent::ListItem { .. } => 6,
        BlockContent::CodeBlock { .. } => 7,
        BlockContent::Mermaid { .. } => 8,
        BlockContent::MathBlock { .. } => 9,
        BlockContent::Table(_) => 10,
        BlockContent::ThematicBreak => 11,
        BlockContent::HtmlBlock => 12,
        BlockContent::FrontMatter(_) => 13,
        BlockContent::FootnoteDefinition { .. } => 14,
    }
}

/// `SubtreeHasher.frameworkHash(_:in:)`: a container's own bytes (kind + the
/// gaps between children), ignoring the children's hashes.
fn framework_hash(block: &MDBlock, text: &[u16]) -> u64 {
    let length = text.len() as isize;
    let mut h = FNV::combine_u64(FNV::OFFSET_BASIS, discriminator(&block.content) as u64);
    let full = clamp(block.range, length);
    let mut scan = full.location;
    for child in &block.children {
        let child_range = clamp(child.range, length);
        if child_range.location > scan {
            h = FNV::combine_range(h, text, NSRange::new(scan, child_range.location - scan));
        }
        if child_range.upper_bound() > scan {
            scan = child_range.upper_bound();
        }
    }
    if full.upper_bound() > scan {
        h = FNV::combine_range(h, text, NSRange::new(scan, full.upper_bound() - scan));
    }
    h
}

/// `SubtreeHasher.clamp(_:to:)`.
fn clamp(range: NSRange, length: isize) -> NSRange {
    let location = 0.max(range.location.min(length));
    NSRange::new(location, 0.max(range.length.min(length - location)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn document(text: &str, children: Vec<Arc<MDBlock>>) -> ParsedDocument {
        let length = swift_text::utf16_count(text);
        let root = MDBlock::new(BlockContent::Document, NSRange::new(0, length), NSRange::new(0, length))
            .with_children(children)
            .into_ref();
        ParsedDocument::new(
            text.to_owned(),
            length,
            root,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            HashMap::new(),
            HashMap::new(),
            vec![0],
        )
    }

    fn paragraph(location: isize, length: isize, hash: u64) -> Arc<MDBlock> {
        let mut block = MDBlock::new(BlockContent::Paragraph, NSRange::new(location, length), NSRange::new(location, length));
        block.subtree_hash = hash;
        block.into_ref()
    }

    #[test]
    fn hand_built_trees_dirty_only_the_changed_block() {
        // "a\n\nb\n\nc\n" → "a\n\nB\n\nc\n": hashes stand in for the parser's.
        let old = document("a\n\nb\n\nc\n", vec![paragraph(0, 1, 1), paragraph(3, 1, 2), paragraph(6, 1, 3)]);
        let new = document("a\n\nB\n\nc\n", vec![paragraph(0, 1, 1), paragraph(3, 1, 20), paragraph(6, 1, 3)]);
        let dirty = ASTDiff::dirty_set(Some(&old), &new);
        assert!(!dirty.is_wholesale);
        assert_eq!(dirty.ranges, vec![NSRange::new(3, 1)]);
        assert!(ASTDiff::dirty_set(None, &new).is_wholesale);
        assert!(ASTDiff::dirty_set(Some(&new), &new).is_empty());
    }

    #[test]
    fn canonically_equal_text_is_clean() {
        let old = document("é\n", vec![paragraph(0, 1, 1)]);
        let new = document("e\u{301}\n", vec![paragraph(0, 2, 2)]);
        assert!(ASTDiff::dirty_set(Some(&old), &new).is_empty());
    }

    #[test]
    fn coalesce_merges_touching_ranges_in_order() {
        let merged = ASTDiff::coalesce(vec![NSRange::new(10, 5), NSRange::new(0, 3), NSRange::new(3, 2), NSRange::new(20, 1)]);
        assert_eq!(merged, vec![NSRange::new(0, 5), NSRange::new(10, 5), NSRange::new(20, 1)]);
    }
}
