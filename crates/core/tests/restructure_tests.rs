//! RestructureTests.swift — restructuring (§9.2) and the §6.3 table
//! operations.

use std::collections::HashSet;

use upleft_core::parser::MarkdownParser;
use upleft_core::restructure::{MoveDirection, Restructure};
use upleft_core::swift_text;
use upleft_core::*;

fn apply(text: &str, edits: &[TextEdit]) -> String {
    applied(edits, text)
}

/// `(text as NSString).length`.
fn ns_length(text: &str) -> isize {
    swift_text::utf16_count(text)
}

// MARK: Promote / demote

#[test]
fn promote_moves_the_whole_subtree() {
    let text = "# Top\n\n## Section\n\n### Sub\n\n#### Deep\n\n## Other\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::promote_heading(&doc, 1));
    assert_eq!(out, "# Top\n\n# Section\n\n## Sub\n\n### Deep\n\n## Other\n");
}

#[test]
fn demote_moves_the_whole_subtree() {
    let text = "# Top\n\n## Section\n\n### Sub\n\n## Other\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::demote_heading(&doc, 1));
    assert_eq!(out, "# Top\n\n### Section\n\n#### Sub\n\n## Other\n");
}

#[test]
fn promote_demote_round_trips() {
    let text = "# Top\n\n## Section\n\n### Sub\n\n## Other\n";
    let doc = MarkdownParser::parse(text);
    let demoted = apply(text, &Restructure::demote_heading(&doc, 1));
    let back = apply(&demoted, &Restructure::promote_heading(&MarkdownParser::parse(&demoted), 1));
    assert_eq!(back, text);
}

#[test]
fn sets_an_exact_heading_level_and_moves_its_subtree() {
    let text = "# Top\n\n## Section\n\n### Child\n\n## Other\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::set_heading_level(&doc, 1, 4));
    assert_eq!(out, "# Top\n\n#### Section\n\n##### Child\n\n## Other\n");
    assert!(Restructure::set_heading_level(&doc, 1, 2).is_empty());
    assert!(Restructure::set_heading_level(&doc, 1, 0).is_empty());
}

#[test]
fn heading_picker_converts_setext_across_all_levels() {
    let text = "Title\n=====\n\nBody\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::set_heading_level(&doc, 0, 4));
    assert_eq!(out, "#### Title\n\nBody\n");

    let crlf = "Title\r\n-----\r\n\r\nBody\r\n";
    let crlf_doc = MarkdownParser::parse(crlf);
    let promoted = apply(crlf, &Restructure::set_heading_level(&crlf_doc, 0, 1));
    assert_eq!(promoted, "# Title\r\n\r\nBody\r\n");
}

#[test]
fn heading_to_body_preserves_titles_and_removes_closing_markers() {
    let compact = "#   Title\n";
    assert_eq!(apply(compact, &Restructure::heading_to_body_text(&MarkdownParser::parse(compact), 0)), "Title\n");

    let closed = "  ###   Title ###\n";
    assert_eq!(apply(closed, &Restructure::heading_to_body_text(&MarkdownParser::parse(closed), 0)), "  Title\n");
}

#[test]
fn heading_to_body_supports_setext() {
    let text = "Title\n=====\n\nBody\n";
    assert_eq!(apply(text, &Restructure::heading_to_body_text(&MarkdownParser::parse(text), 0)), "Title\n\nBody\n");
}

// MARK: Headings inside containers (blockquote markers, list items)

/// Regression: a heading nested in a blockquote or list item does not begin
/// with `#` after its leading whitespace, so it used to take the
/// setext-normalization branch, which consumed the *following* line as an
/// underline and silently deleted the quoted body.
#[test]
fn demote_inside_blockquote_keeps_the_quoted_body() {
    let text = "# Top\n> ## Deep\n> hidden secret\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::demote_heading(&doc, 1));
    assert_eq!(out, "# Top\n> ### Deep\n> hidden secret\n");
}

#[test]
fn promote_inside_list_item_keeps_the_marker() {
    let text = "- intro\n- ## Deep\n- tail\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::promote_heading(&doc, 0));
    assert_eq!(out, "- intro\n- # Deep\n- tail\n");
}

/// A setext heading inside a list item normalizes to ATX while keeping the
/// item marker and consuming exactly its own underline line.
#[test]
fn setext_inside_list_item_normalizes_without_stray_underline() {
    let text = "- Title\n  =====\n- tail\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::set_heading_level(&doc, 0, 2));
    assert_eq!(out, "- ## Title\n- tail\n");
}

/// The old setext branch bailed out entirely when the heading was the
/// document's final line, so demote silently did nothing.
#[test]
fn rewrite_level_works_without_a_trailing_newline() {
    for text in ["## Deep", "# Top\n## Deep"] {
        let doc = MarkdownParser::parse(text);
        let out = apply(text, &Restructure::demote_heading(&doc, doc.headings.len() as isize - 1));
        assert_eq!(out, swift_text::replacing_occurrences(text, "## Deep", "### Deep"));
    }
}

#[test]
fn heading_to_body_text_keeps_container_markers() {
    let text = "> # Title\n> body\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::heading_to_body_text(&doc, 0));
    assert_eq!(out, "> Title\n> body\n");
}

#[test]
fn clamps_at_the_ends() {
    let top = MarkdownParser::parse("# A\n\n## B\n");
    assert!(Restructure::promote_heading(&top, 0).is_empty());

    let deep = MarkdownParser::parse("###### A\n");
    assert!(Restructure::demote_heading(&deep, 0).is_empty());

    // A subtree that would push a descendant past H6 is refused whole,
    // rather than flattened.
    let nested = MarkdownParser::parse("##### A\n\n###### B\n");
    let out = apply("##### A\n\n###### B\n", &Restructure::demote_heading(&nested, 0));
    assert_eq!(out, "##### A\n\n###### B\n");
}

// MARK: Move section — §14 calls this out as harder than it looks

#[test]
fn move_section_preserves_blank_line_structure() {
    let text = "# Doc\n\n## A\n\nAlpha body.\n\n## B\n\nBeta body.\n\n## C\n\nGamma body.\n";
    let doc = MarkdownParser::parse(text);
    // Move C before A.
    let out = apply(text, &Restructure::move_section(&doc, 3, 1));
    assert_eq!(out, "# Doc\n\n## C\n\nGamma body.\n\n## A\n\nAlpha body.\n\n## B\n\nBeta body.\n");
}

#[test]
fn move_section_carries_the_whole_subtree() {
    let text = "## A\n\nbody a\n\n### A1\n\nbody a1\n\n## B\n\nbody b\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::move_section(&doc, 0, 3));
    assert_eq!(out, "## B\n\nbody b\n\n## A\n\nbody a\n\n### A1\n\nbody a1\n");
}

#[test]
fn move_section_to_the_end_keeps_the_final_newline() {
    let text = "## A\n\nbody a\n\n## B\n\nbody b\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::move_section(&doc, 0, doc.headings.len() as isize));
    assert_eq!(out, "## B\n\nbody b\n\n## A\n\nbody a\n");
}

/// The awkward case: the last section has no trailing newline to carry, so it
/// borrows the one before it and the document's final-newline state is
/// preserved either way (§3.1).
#[test]
fn move_section_handles_a_missing_final_newline() {
    let text = "## A\n\nbody a\n\n## B\n\nbody b";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::move_section(&doc, 1, 0));
    assert_eq!(out, "## B\n\nbody b\n\n## A\n\nbody a");
    assert!(!swift_text::has_suffix(&out, "\n"));
}

#[test]
fn move_section_is_a_no_op_when_the_target_is_inside_it() {
    let text = "## A\n\nbody\n\n### A1\n\nsub\n\n## B\n\nb\n";
    let doc = MarkdownParser::parse(text);
    assert!(Restructure::move_section(&doc, 0, 1).is_empty());
    assert!(Restructure::move_section(&doc, 0, 0).is_empty());
}

/// A consistently CRLF document keeps CRLF through a move.
#[test]
fn move_section_keeps_consistent_crlf_endings() {
    let text = "## A\r\n\r\nbody a\r\n\r\n## B\r\n\r\nbody b\r\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::move_section(&doc, 1, 0));
    assert_eq!(out, "## B\r\n\r\nbody b\r\n\r\n## A\r\n\r\nbody a\r\n");
    assert!(!swift_text::contains(&out, "\r\r"), "no stray carriage returns");
}

/// Regression: in a mixed-ending file the old width-blind walk could not see
/// the `\r\n` blank line before the last section, so a move glued the two
/// sections together. The separator's own bytes must survive the move.
#[test]
fn move_section_preserves_mixed_ending_separators() {
    let text = "## A\n\nbody a\r\n\r\n## B\n\nbody b\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::move_section(&doc, 1, 0));
    let escaped = swift_text::replacing_occurrences(&swift_text::replacing_occurrences(&out, "\r", "\\r"), "\n", "\\n");
    // B keeps its own terminator, the `\r\n` blank separator it had in life
    // lands between it and A, and A keeps its own CRLF terminator.
    assert_eq!(escaped, "## B\\n\\nbody b\\n\\r\\n## A\\n\\nbody a\\r\\n");
}

#[test]
fn move_section_survives_an_arbitrary_permutation() {
    let mut text = String::from("# Doc\n\n## A\n\na\n\n## B\n\nb\n\n## C\n\nc\n\n## D\n\nd\n");
    for (from, to) in [(4isize, 1isize), (1, 4), (2, 1), (3, 2)] {
        let doc = MarkdownParser::parse(&text);
        if !(from >= 0 && (from as usize) < doc.headings.len()) {
            continue;
        }
        text = apply(&text, &Restructure::move_section(&doc, from, to));
        let reparsed = MarkdownParser::parse(&text);
        // No section ever loses or gains a blank line separator.
        assert!(!swift_text::contains(&text, "\n\n\n"), "grew a blank line: {text:?}");
        assert_eq!(reparsed.headings.len(), 5);
        let titles: HashSet<&str> = reparsed.headings.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, HashSet::from(["Doc", "A", "B", "C", "D"]));
    }
}

/// A classic-Mac file (lone `\r` terminators) must stay all-`\r` through a
/// section move.
#[test]
fn move_section_keeps_lone_cr_line_endings() {
    let text = "# One\r\r# Two\r\r# Three\r";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::move_section(&doc, 2, 0));
    assert_eq!(out, "# Three\r\r# One\r\r# Two\r");
}

#[test]
fn move_section_keeps_crlf_line_endings() {
    let text = "# One\r\n\r\n# Two\r\n\r\n# Three\r\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::move_section(&doc, 2, 0));
    assert_eq!(out, "# Three\r\n\r\n# One\r\n\r\n# Two\r\n");
}

// MARK: Move block

#[test]
fn move_block_swaps_siblings_and_keeps_the_gap() {
    let text = "First para.\n\nSecond para.\n\nThird para.\n";
    let doc = MarkdownParser::parse(text);
    let down = apply(text, &Restructure::move_block(&doc, 2, MoveDirection::Down));
    assert_eq!(down, "Second para.\n\nFirst para.\n\nThird para.\n");

    let up = apply(text, &Restructure::move_block(&doc, 16, MoveDirection::Up));
    assert_eq!(up, "Second para.\n\nFirst para.\n\nThird para.\n");
}

#[test]
fn move_block_acts_on_the_list_item_not_the_paragraph_inside_it() {
    let text = "- one\n- two\n- three\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::move_block(&doc, 8, MoveDirection::Up));
    assert_eq!(out, "- two\n- one\n- three\n");
}

#[test]
fn move_block_at_the_boundary_is_a_no_op() {
    let text = "a\n\nb\n";
    let doc = MarkdownParser::parse(text);
    assert!(Restructure::move_block(&doc, 0, MoveDirection::Up).is_empty());
    assert!(Restructure::move_block(&doc, 3, MoveDirection::Down).is_empty());
}

// MARK: Conversion

#[test]
fn converts_between_every_form() {
    let text = "alpha\nbeta\n";
    let doc = MarkdownParser::parse(text);
    let all = NSRange::new(0, ns_length(text));
    assert_eq!(apply(text, &Restructure::convert(&doc, all, ListConversion::BulletList)), "- alpha\n- beta\n");
    assert_eq!(apply(text, &Restructure::convert(&doc, all, ListConversion::NumberedList)), "1. alpha\n2. beta\n");
    assert_eq!(apply(text, &Restructure::convert(&doc, all, ListConversion::TaskList)), "- [ ] alpha\n- [ ] beta\n");
    assert_eq!(apply(text, &Restructure::convert(&doc, all, ListConversion::Blockquote)), "> alpha\n> beta\n");
}

#[test]
fn conversion_round_trips_back_to_paragraph() {
    let text = "alpha\nbeta\n";
    for form in [ListConversion::BulletList, ListConversion::NumberedList, ListConversion::TaskList, ListConversion::Blockquote] {
        let doc = MarkdownParser::parse(text);
        let all = NSRange::new(0, ns_length(text));
        let converted = apply(text, &Restructure::convert(&doc, all, form));
        let converted_doc = MarkdownParser::parse(&converted);
        let back = apply(
            &converted,
            &Restructure::convert(&converted_doc, NSRange::new(0, ns_length(&converted)), ListConversion::Paragraph),
        );
        assert_eq!(back, text, "{} did not round trip: {converted:?}", form.raw_value());
    }
}

#[test]
fn conversion_keeps_indentation() {
    let text = "  alpha\n";
    let doc = MarkdownParser::parse(text);
    let all = NSRange::new(0, ns_length(text));
    assert_eq!(apply(text, &Restructure::convert(&doc, all, ListConversion::BulletList)), "  - alpha\n");
}

// MARK: Sorting

#[test]
fn sorts_alphabetically() {
    let text = "- charlie\n- alpha\n- bravo\n";
    let doc = MarkdownParser::parse(text);
    assert_eq!(apply(text, &Restructure::sort_list(&doc, 2, ListSortOrder::Alphabetical)), "- alpha\n- bravo\n- charlie\n");
    assert_eq!(
        apply(text, &Restructure::sort_list(&doc, 2, ListSortOrder::ReverseAlphabetical)),
        "- charlie\n- bravo\n- alpha\n"
    );
}

#[test]
fn sorts_by_checkbox_state() {
    let text = "- [x] done\n- [ ] todo\n- [x] also done\n- [ ] later\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::sort_list(&doc, 2, ListSortOrder::UncheckedFirst));
    assert_eq!(out, "- [ ] todo\n- [ ] later\n- [x] done\n- [x] also done\n");
}

#[test]
fn sorting_an_ordered_list_renumbers() {
    let text = "1. charlie\n2. alpha\n3. bravo\n";
    let doc = MarkdownParser::parse(text);
    assert_eq!(apply(text, &Restructure::sort_list(&doc, 3, ListSortOrder::Alphabetical)), "1. alpha\n2. bravo\n3. charlie\n");
}

// MARK: Table of contents

#[test]
fn generates_an_indented_table_of_contents() {
    let doc = MarkdownParser::parse("# Doc\n\n## A\n\n### A1\n\n## B\n\n#### Deep\n");
    assert_eq!(Restructure::table_of_contents(&doc, 3), "- [Doc](#doc)\n  - [A](#a)\n    - [A1](#a1)\n  - [B](#b)");
    assert_eq!(Restructure::table_of_contents(&doc, 1), "- [Doc](#doc)");
}

// MARK: Tasks

#[test]
fn toggle_task_is_a_one_character_edit() {
    let text = "- [ ] alpha\n- [x] beta\n";
    let doc = MarkdownParser::parse(text);
    let check = Restructure::toggle_task(&doc, doc.tasks[0].mark_range.location);
    assert_eq!(check.as_ref().map(|edit| edit.range.length), Some(1));
    assert_eq!(check.as_ref().map(|edit| edit.replacement.as_str()), Some("x"));
    assert_eq!(apply(text, &[check.unwrap()]), "- [x] alpha\n- [x] beta\n");

    let uncheck = Restructure::toggle_task(&doc, doc.tasks[1].mark_range.location);
    assert_eq!(uncheck.as_ref().map(|edit| edit.replacement.as_str()), Some(" "));
    assert_eq!(apply(text, &[uncheck.unwrap()]), "- [ ] alpha\n- [ ] beta\n");
}

#[test]
fn toggle_task_outside_a_task_is_nil() {
    let doc = MarkdownParser::parse("Just a paragraph.\n");
    assert!(Restructure::toggle_task(&doc, 3).is_none());
}

// MARK: Tasks — insert

#[test]
fn insert_task_goes_after_the_last_task_of_the_section() {
    let text = "# A\n\n- [ ] one\n- [ ] two\n\n# B\n\n- [ ] three\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::insert_task(&doc, "new", Some(0)));
    assert_eq!(out, "# A\n\n- [ ] one\n- [ ] two\n- [ ] new\n\n# B\n\n- [ ] three\n");
}

#[test]
fn insert_task_lands_after_the_whole_child_block() {
    // The anchor is the last matching task, and its block extends over nested
    // children, so the new line never splits a family.
    let text = "# S\n\n- [ ] a\n- [ ] b\n  - [ ] b1\n  - [ ] b2\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::insert_task(&doc, "new", Some(0)));
    assert_eq!(out, "# S\n\n- [ ] a\n- [ ] b\n  - [ ] b1\n  - [ ] b2\n- [ ] new\n");

    // Non-task children indented under the anchor ride along too.
    let continuation = "# S\n\n- [ ] a\n  - plain child\n\n# T\n\n- [ ] t\n";
    let out2 = apply(continuation, &Restructure::insert_task(&MarkdownParser::parse(continuation), "new", Some(0)));
    assert_eq!(out2, "# S\n\n- [ ] a\n  - plain child\n- [ ] new\n\n# T\n\n- [ ] t\n");
}

#[test]
fn insert_task_into_headingless_document() {
    let text = "- [ ] one\n- [ ] two\n";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::insert_task(&doc, "new", None));
    assert_eq!(out, "- [ ] one\n- [ ] two\n- [ ] new\n");
}

#[test]
fn insert_task_into_empty_document() {
    let doc = MarkdownParser::parse("");
    assert_eq!(apply("", &Restructure::insert_task(&doc, "new", None)), "- [ ] new\n");
}

#[test]
fn insert_task_into_document_without_trailing_newline() {
    let text = "- [ ] one";
    let doc = MarkdownParser::parse(text);
    let out = apply(text, &Restructure::insert_task(&doc, "new", None));
    assert_eq!(out, "- [ ] one\n- [ ] new\n");
}

#[test]
fn insert_task_without_a_matching_section_appends_with_separation() {
    let text = "# A\n\n- [ ] x\n";
    let doc = MarkdownParser::parse(text);
    assert_eq!(apply(text, &Restructure::insert_task(&doc, "new", Some(1))), "# A\n\n- [ ] x\n\n- [ ] new\n");

    // Already blank at EOF: no extra separation.
    let blank = "# A\n\n- [ ] x\n\n";
    assert_eq!(
        apply(blank, &Restructure::insert_task(&MarkdownParser::parse(blank), "new", Some(1))),
        "# A\n\n- [ ] x\n\n- [ ] new\n"
    );

    // No trailing newline at all: the separator has to create the blank line.
    let prose = "Just prose.";
    assert_eq!(
        apply(prose, &Restructure::insert_task(&MarkdownParser::parse(prose), "new", None)),
        "Just prose.\n\n- [ ] new\n"
    );
}

#[test]
fn insert_task_rejects_whitespace_only_text() {
    let doc = MarkdownParser::parse("- [ ] one\n");
    assert!(Restructure::insert_task(&doc, "  \n\t ", None).is_empty());
}

// MARK: Tasks — move

#[test]
fn move_task_down_one_sibling() {
    let text = "- [ ] a\n- [ ] b\n- [ ] c\n";
    let doc = MarkdownParser::parse(text);
    assert_eq!(apply(text, &Restructure::move_task(&doc, 0, Some(2))), "- [ ] b\n- [ ] a\n- [ ] c\n");
}

#[test]
fn move_task_to_the_end() {
    let text = "- [ ] a\n- [ ] b\n- [ ] c\n";
    let doc = MarkdownParser::parse(text);
    assert_eq!(apply(text, &Restructure::move_task(&doc, 0, None)), "- [ ] b\n- [ ] c\n- [ ] a\n");
}

#[test]
fn move_task_up() {
    let text = "- [ ] a\n- [ ] b\n- [ ] c\n";
    let doc = MarkdownParser::parse(text);
    assert_eq!(apply(text, &Restructure::move_task(&doc, 2, Some(0))), "- [ ] c\n- [ ] a\n- [ ] b\n");
}

#[test]
fn move_task_carries_its_children() {
    let text = "- [ ] a\n  - [ ] a1\n- [ ] b\n";
    let doc = MarkdownParser::parse(text);
    // Lifting b over a leaves a's child attached to a.
    assert_eq!(apply(text, &Restructure::move_task(&doc, 2, Some(0))), "- [ ] b\n- [ ] a\n  - [ ] a1\n");
    // Moving a to the end carries a1 along with it.
    assert_eq!(apply(text, &Restructure::move_task(&doc, 0, None)), "- [ ] b\n- [ ] a\n  - [ ] a1\n");
}

#[test]
fn move_task_treats_blank_line_split_lists_as_one_sibling_group() {
    let text = "- [ ] a\n\n- [ ] b\n";
    let doc = MarkdownParser::parse(text);
    // The blank line is the join's, not either task's — it stays put.
    assert_eq!(apply(text, &Restructure::move_task(&doc, 1, Some(0))), "- [ ] b\n- [ ] a\n\n");
}

#[test]
fn move_task_refuses_another_section() {
    let text = "# A\n\n- [ ] a\n\n# B\n\n- [ ] b\n";
    let doc = MarkdownParser::parse(text);
    assert!(Restructure::move_task(&doc, 1, Some(0)).is_empty());
    assert!(Restructure::move_task(&doc, 0, Some(1)).is_empty());
}

#[test]
fn move_task_refuses_another_indent_level() {
    let text = "- [ ] a\n  - [ ] a1\n- [ ] b\n";
    let doc = MarkdownParser::parse(text);
    // Child before its parent, and parent before its child: re-parenting,
    // not reordering.
    assert!(Restructure::move_task(&doc, 1, Some(0)).is_empty());
    assert!(Restructure::move_task(&doc, 0, Some(1)).is_empty());
}

#[test]
fn move_task_already_in_position_is_a_no_op() {
    let text = "- [ ] a\n- [ ] b\n";
    let doc = MarkdownParser::parse(text);
    assert!(Restructure::move_task(&doc, 0, Some(0)).is_empty());
    assert!(Restructure::move_task(&doc, 1, None).is_empty());
}

#[test]
fn move_task_preserves_a_missing_final_newline() {
    let text = "- [ ] a\n- [ ] b";
    let doc = MarkdownParser::parse(text);
    // Last to first: the cut borrows the newline before the block.
    assert_eq!(apply(text, &Restructure::move_task(&doc, 1, Some(0))), "- [ ] b\n- [ ] a");
    // First to last: the paste brings its own leading separator.
    assert_eq!(apply(text, &Restructure::move_task(&doc, 0, None)), "- [ ] b\n- [ ] a");
}

// MARK: Tables (§6.3)

const TABLE: &str = "| a | bb |\n| --- | --- |\n| 1 | 2 |\n| 333 | 4 |\n";

#[test]
fn realigns_table_source() {
    let doc = MarkdownParser::parse(TABLE);
    let range = doc.root.children[0].range;
    let out = apply(TABLE, &Restructure::realign_table(&doc, range));
    assert_eq!(out, "| a   | bb  |\n| --- | --- |\n| 1   | 2   |\n| 333 | 4   |\n");
}

#[test]
fn sets_column_alignment() {
    let doc = MarkdownParser::parse(TABLE);
    let range = doc.root.children[0].range;
    let out = apply(TABLE, &Restructure::set_column_alignment(&doc, range, 1, TableAlignment::Right));
    // The alignment colon lives inside the column's existing width rather than
    // widening it, so realigning twice is a fixed point.
    assert!(swift_text::contains(&out, "| --- | --: |"));
    assert!(swift_text::contains(&out, "| a   |  bb |"));
}

#[test]
fn inserts_and_deletes_rows() {
    let doc = MarkdownParser::parse(TABLE);
    let range = doc.root.children[0].range;

    let inserted = apply(TABLE, &Restructure::insert_row(&doc, range, 1));
    assert_eq!(inserted, "| a   | bb  |\n| --- | --- |\n| 1   | 2   |\n|     |     |\n| 333 | 4   |\n");

    let deleted = apply(TABLE, &Restructure::delete_row(&doc, range, 1));
    assert_eq!(deleted, "| a   | bb  |\n| --- | --- |\n| 333 | 4   |\n");

    // A GFM table without a header is not a table.
    assert!(Restructure::delete_row(&doc, range, 0).is_empty());
}
