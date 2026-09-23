//! DiffTests.swift — ASTDiff, TextDiff and Myers.

mod common;

mod ast_diff_tests {
    use std::collections::HashSet;

    use super::common::corpus;
    use upleft_core::ast_diff::ASTDiff;
    use upleft_core::parser::MarkdownParser;
    use upleft_core::swift_text;

    #[test]
    fn first_parse_is_wholesale() {
        let doc = MarkdownParser::parse("# A\n");
        assert!(ASTDiff::dirty_set(None, &doc).is_wholesale);
    }

    #[test]
    fn identical_text_is_clean() {
        let text = corpus::KITCHEN_SINK;
        let a = MarkdownParser::parse(text);
        let b = MarkdownParser::parse(text);
        assert!(ASTDiff::dirty_set(Some(&a), &b).is_empty());
    }

    /// §3.5's whole premise: editing one paragraph of a 200-block document
    /// must dirty exactly that paragraph.
    #[test]
    fn editing_one_paragraph_dirties_one_block() {
        let original = corpus::many_blocks(200);
        let edited = swift_text::replacing_occurrences(
            &original,
            "Paragraph number 101 with some words in it.",
            "Paragraph number 101 with different words in it.",
        );
        assert_ne!(original, edited);

        let old = MarkdownParser::parse(&original);
        let new = MarkdownParser::parse(&edited);
        let dirty = ASTDiff::dirty_set(Some(&old), &new);

        assert!(!dirty.is_wholesale);
        assert_eq!(dirty.ranges.len(), 1);
        assert_eq!(new.substring(dirty.ranges[0]), "Paragraph number 101 with different words in it.");
    }

    /// An insertion near the top must not dirty everything after it.
    #[test]
    fn insertion_near_the_top_stays_local() {
        let original = corpus::many_blocks(200);
        let edited = swift_text::replacing_occurrences(
            &original,
            "Paragraph number 1 with some words in it.\n",
            "Paragraph number 1 with some words in it.\n\nA brand new paragraph.\n",
        );
        let old = MarkdownParser::parse(&original);
        let new = MarkdownParser::parse(&edited);
        let dirty = ASTDiff::dirty_set(Some(&old), &new);

        assert!(!dirty.is_wholesale);
        assert_eq!(dirty.ranges.len(), 1);
        assert_eq!(new.substring(dirty.ranges[0]), "A brand new paragraph.");
    }

    #[test]
    fn edit_inside_a_list_item_dirties_the_item_not_the_list() {
        let old = MarkdownParser::parse("# H\n\n- one\n- two\n- three\n");
        let new = MarkdownParser::parse("# H\n\n- one\n- TWO\n- three\n");
        let dirty = ASTDiff::dirty_set(Some(&old), &new);
        assert!(!dirty.is_wholesale);
        assert_eq!(dirty.ranges.len(), 1);
        assert_eq!(new.substring(dirty.ranges[0]), "TWO");
    }

    #[test]
    fn toggling_task_marker_dirties_the_list_item() {
        let old = MarkdownParser::parse("- [ ] Ship the fix\n");
        let new = MarkdownParser::parse("- [x] Ship the fix\n");
        let dirty = ASTDiff::dirty_set(Some(&old), &new);

        assert!(!dirty.is_wholesale);
        assert_eq!(dirty.ranges.len(), 1);
        assert_eq!(new.substring(dirty.ranges[0]), "- [x] Ship the fix");
    }

    /// The trailing `>` of a multi-line quote belongs to the blockquote's own
    /// bytes, so adding or removing it must dirty the quote even though every
    /// child paragraph is byte-identical.
    #[test]
    fn blank_quote_marker_edit_dirties_the_quote() {
        let old = MarkdownParser::parse("> a\n> b\n>\n");
        let new = MarkdownParser::parse("> a\n> b\n");
        let dirty = ASTDiff::dirty_set(Some(&old), &new);
        assert!(!dirty.is_wholesale);
        assert!(!dirty.is_empty());

        let reverse = ASTDiff::dirty_set(Some(&new), &old);
        assert!(!reverse.is_wholesale);
        assert!(!reverse.is_empty());
    }

    #[test]
    fn major_structural_change_goes_wholesale() {
        let old = MarkdownParser::parse(&corpus::many_blocks(40));
        let new = MarkdownParser::parse("# Completely different\n");
        assert!(ASTDiff::dirty_set(Some(&old), &new).is_wholesale);
    }

    #[test]
    fn dirty_ranges_are_ascending_and_disjoint() {
        let original = corpus::many_blocks(60);
        let edited = swift_text::replacing_occurrences(
            &swift_text::replacing_occurrences(&original, "Paragraph number 3 ", "Paragraph number three "),
            "Paragraph number 44 ",
            "Paragraph number forty-four ",
        );
        let dirty = ASTDiff::dirty_set(Some(&MarkdownParser::parse(&original)), &MarkdownParser::parse(&edited));
        assert_eq!(dirty.ranges.len(), 2);
        assert!(dirty.ranges[0].upper_bound() <= dirty.ranges[1].location);
    }

    #[test]
    fn subtree_hash_ignores_position_but_not_content() {
        let a = MarkdownParser::parse("# A\n\nalpha\n\nbeta\n");
        let b = MarkdownParser::parse("# A\n\nbeta\n\nalpha\n");
        // Same two paragraphs, swapped: their hashes must still match, which
        // is what lets the LCS pair them up.
        let a_hashes: HashSet<u64> = a.root.children.iter().map(|block| block.subtree_hash).collect();
        let b_hashes: HashSet<u64> = b.root.children.iter().map(|block| block.subtree_hash).collect();
        assert_eq!(a_hashes, b_hashes);

        let c = MarkdownParser::parse("# A\n\nalpha!\n\nbeta\n");
        let c_hashes: HashSet<u64> = c.root.children.iter().map(|block| block.subtree_hash).collect();
        assert_ne!(c_hashes, a_hashes);
    }

    #[test]
    fn container_reconciliation_with_shifted_offsets_does_not_dirty_unchanged_container() {
        let old = MarkdownParser::parse("# Title\n\n> Quote paragraph 1\n> Quote paragraph 2\n\nFooter");
        let new = MarkdownParser::parse(
            "# Title\n\nInserted preamble line that shifts all offsets downstream.\n\n> Quote paragraph 1\n> Quote paragraph 2\n\nFooter",
        );
        let dirty = ASTDiff::dirty_set(Some(&old), &new);
        assert!(!dirty.is_wholesale);
        assert_eq!(dirty.ranges.len(), 1);
        assert_eq!(new.substring(dirty.ranges[0]), "Inserted preamble line that shifts all offsets downstream.");
    }
}

mod text_diff_tests {
    use std::time::Instant;

    use super::common::corpus;
    use upleft_core::contracts::ChangeKind;
    use upleft_core::swift_text::{self, ns::NSStringExt, ns::utf16};
    use upleft_core::text_diff::TextDiff;

    #[test]
    fn identical_text_has_no_hunks() {
        assert!(TextDiff::hunks(corpus::KITCHEN_SINK, corpus::KITCHEN_SINK).is_empty());
    }

    #[test]
    fn pure_insertion_is_an_insert_hunk() {
        let hunks = TextDiff::hunks("a\nb\n", "a\nnew\nb\n");
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Inserted);
        assert_eq!(utf16("a\nnew\nb\n").substring(hunks[0].new_range), "new\n");
        assert_eq!(hunks[0].old_range.length, 0);
    }

    #[test]
    fn pure_deletion_is_a_delete_hunk() {
        let hunks = TextDiff::hunks("a\ngone\nb\n", "a\nb\n");
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Deleted);
        assert_eq!(utf16("a\ngone\nb\n").substring(hunks[0].old_range), "gone\n");
        assert_eq!(hunks[0].new_range.length, 0);
    }

    /// §8.1: the ranges must land on the changed words of the *new* text and
    /// nothing else.
    #[test]
    fn modified_hunks_carry_word_ranges_in_the_new_text() {
        let old = "The quick brown fox jumps over the lazy dog.\n";
        let new = "The quick red fox leaps over the lazy dog.\n";
        let hunks = TextDiff::hunks(old, new);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].kind, ChangeKind::Modified);

        let ns = utf16(new);
        let words: Vec<String> = hunks[0].word_ranges.iter().map(|&range| ns.substring(range)).collect();
        assert_eq!(words, vec!["red", "leaps"]);
        for range in &hunks[0].word_ranges {
            assert!(range.location >= hunks[0].new_range.location);
            assert!(range.upper_bound() <= hunks[0].new_range.upper_bound());
        }
    }

    #[test]
    fn adjacent_changed_words_merge_into_one_highlight() {
        let hunks = TextDiff::hunks("one two three four\n", "one alpha beta four\n");
        assert_eq!(hunks.len(), 1);
        let ns = utf16("one alpha beta four\n");
        let words: Vec<String> = hunks[0].word_ranges.iter().map(|&range| ns.substring(range)).collect();
        assert_eq!(words, vec!["alpha beta"]);
    }

    #[test]
    fn multiple_separated_edits_produce_separate_hunks() {
        let old = (1..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n") + "\n";
        let new = swift_text::replacing_occurrences(
            &swift_text::replacing_occurrences(&old, "line 3", "line three"),
            "line 17",
            "line seventeen",
        );
        let hunks = TextDiff::hunks(&old, &new);
        assert_eq!(hunks.len(), 2);
        assert!(hunks.iter().all(|hunk| hunk.kind == ChangeKind::Modified));
        assert!(hunks[0].new_range.upper_bound() <= hunks[1].new_range.location);
    }

    /// §12's keystroke budget: O(ND), not pathological, on 5k lines.
    #[test]
    fn handles_five_thousand_lines_quickly() {
        let old = (0..5000).map(|i| format!("line number {i} of the document")).collect::<Vec<_>>().join("\n") + "\n";
        let new = swift_text::replacing_occurrences(
            &old,
            "line number 2500 of the document",
            "line number 2500 of the rewritten document",
        );
        let start = Instant::now();
        let hunks = TextDiff::hunks(&old, &new);
        let elapsed = start.elapsed().as_secs_f64();
        assert_eq!(hunks.len(), 1);
        assert!(elapsed < 1.0, "5k-line diff took {elapsed}s");
    }

    #[test]
    fn unrelated_documents_degrade_to_one_hunk_rather_than_hanging() {
        let old = (0..3000).map(|i| format!("alpha {i}")).collect::<Vec<_>>().join("\n");
        let new = (0..3000).map(|i| format!("beta {i}")).collect::<Vec<_>>().join("\n");
        let start = Instant::now();
        let hunks = TextDiff::hunks(&old, &new);
        let elapsed = start.elapsed().as_secs_f64();
        assert!(!hunks.is_empty());
        assert!(elapsed < 2.0, "worst-case diff took {elapsed}s");
    }

    // Extra (not in DiffTests.swift): expectations recorded by running
    // Downright's TextDiff.swift itself.

    #[test]
    fn canonically_equal_texts_have_no_hunks() {
        assert!(TextDiff::hunks("caf\u{e9}\n", "cafe\u{301}\n").is_empty());
    }

    #[test]
    fn a_pure_deletion_anchors_on_the_next_character() {
        let hunks = TextDiff::hunks("a\ngone\nb\n", "a\nb\n");
        assert_eq!(TextDiff::anchor_range(&hunks[0], utf16("a\nb\n").length()), upleft_core::NSRange::new(2, 1));
        let tail = TextDiff::hunks("a\nb\n", "a\n");
        assert_eq!(TextDiff::anchor_range(&tail[0], 2), upleft_core::NSRange::new(1, 1));
        assert_eq!(TextDiff::anchor_range(&tail[0], 0), upleft_core::NSRange::new(0, 0));
    }
}

mod myers_tests {
    use std::time::Instant;

    use upleft_core::hashing::FNV;
    use upleft_core::myers::{Myers, Step};

    #[test]
    fn produces_a_minimal_script() {
        let script = Myers::diff_default(&[1, 2, 3, 4], &[1, 3, 4]);
        assert!(script.is_some());
        let (mut deletes, mut inserts, mut equals) = (0, 0, 0);
        for step in script.unwrap() {
            match step {
                Step::Delete { .. } => deletes += 1,
                Step::Insert { .. } => inserts += 1,
                Step::Equal { .. } => equals += 1,
            }
        }
        assert_eq!(deletes, 1);
        assert_eq!(inserts, 0);
        assert_eq!(equals, 3);
    }

    #[test]
    fn handles_empty_inputs() {
        assert_eq!(Myers::diff_default(&[], &[1, 2]).map(|s| s.len()), Some(2));
        assert_eq!(Myers::diff_default(&[1, 2], &[]).map(|s| s.len()), Some(2));
        assert_eq!(Myers::diff_default(&[], &[]).map(|s| s.is_empty()), Some(true));
    }

    #[test]
    fn gives_up_beyond_the_distance_cap() {
        assert_eq!(Myers::diff(&[1, 2, 3, 4], &[5, 6, 7, 8], 2), None);
    }

    /// Edge trimming: only the middle is worked; the assembled script must
    /// still be complete and in document order.
    #[test]
    fn trimmed_edges_still_produce_a_complete_script() {
        let old: [u64; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let new: [u64; 8] = [0, 1, 2, 99, 100, 7, 8, 9];
        let script = Myers::diff_default(&old, &new).expect("script");
        let deletes = script.iter().filter(|s| matches!(s, Step::Delete { .. })).count();
        let inserts = script.iter().filter(|s| matches!(s, Step::Insert { .. })).count();
        let equals = script.iter().filter(|s| matches!(s, Step::Equal { .. })).count();
        assert_eq!(deletes, 4);
        assert_eq!(inserts, 2);
        assert_eq!(equals, 6);
        // Replay the script: equal/delete walk old, equal/insert walk new,
        // and the equals must land on matching elements.
        let (mut oi, mut ni) = (0isize, 0isize);
        for step in script {
            match step {
                Step::Equal { old_index: o, new_index: n } => {
                    assert_eq!(old[o as usize], new[n as usize]);
                    assert_eq!(o, oi);
                    assert_eq!(n, ni);
                    oi += 1;
                    ni += 1;
                }
                Step::Delete { old_index: o } => {
                    assert_eq!(o, oi);
                    oi += 1;
                }
                Step::Insert { new_index: n } => {
                    assert_eq!(n, ni);
                    ni += 1;
                }
            }
        }
        assert_eq!(oi, old.len() as isize);
        assert_eq!(ni, new.len() as isize);
    }

    /// A large pair with no common lines must bail to `None` without building
    /// the O(D²) trace.
    #[test]
    fn large_disjoint_inputs_bail_without_building_the_trace() {
        let old: Vec<u64> = (0..4000).collect();
        let new: Vec<u64> = (0..4000).map(|i| i + 1_000_000).collect();
        let start = Instant::now();
        let result = Myers::diff_default(&old, &new);
        assert!(result.is_none());
        assert!(start.elapsed().as_secs_f64() < 5.0);
    }

    /// Reordering large shared content stays solvable despite the length.
    #[test]
    fn large_reorders_remain_diffable() {
        let base: Vec<String> = (0..2000).map(|i| format!("chunk {i}")).collect();
        let reversed_head: Vec<String> = base[..500].iter().rev().cloned().collect();
        let old: Vec<u64> = base.iter().chain(reversed_head.iter()).map(|s| FNV::hash_str(s)).collect();
        let new: Vec<u64> = reversed_head.iter().chain(base.iter()).map(|s| FNV::hash_str(s)).collect();
        let result = Myers::diff_default(&old, &new);
        assert!(result.is_some());
    }

    /// A mostly-disjoint pair whose total length sits just under the cap must
    /// bail to `None` instantly rather than spike memory.
    #[test]
    fn near_cap_disjoint_inputs_bail_without_building_the_trace() {
        let count = 2050u64; // n + m = 4100, within the widen-margin of the 4096 cap
        let old: Vec<u64> = (0..count).collect();
        // Share a single line so the overlap lower bound is far below the cap.
        let new: Vec<u64> = (1..count).map(|i| i + 1_000_000).chain([0]).collect();
        let start = Instant::now();
        let result = Myers::diff_default(&old, &new);
        assert!(result.is_none());
        assert!(start.elapsed().as_secs_f64() < 5.0);
    }
}
