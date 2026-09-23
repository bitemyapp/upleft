//! TidyTests.swift — Tidy Document (§9.1).

mod common;

use common::corpus;
use upleft_core::parser::MarkdownParser;
use upleft_core::tidy::TidyDocument;
use upleft_core::*;

fn tidied(text: &str, rules: &[TidyRule]) -> String {
    applied(&TidyDocument::plan_with(&MarkdownParser::parse(text), rules), text)
}

fn tidied_all(text: &str) -> String {
    tidied(text, &TidyRule::ALL_CASES)
}

// MARK: Individual rules

#[test]
fn collapses_skipped_heading_levels() {
    let out = tidied("# Title\n\n## Two\n\n#### Four\n\n##### Five\n", &[TidyRule::HeadingLevels]);
    assert_eq!(out, "# Title\n\n## Two\n\n### Four\n\n#### Five\n");
}

/// H1 is the document's title. Re-levelling it changes what the document
/// *is*, so it is never touched even when a jump would justify it.
#[test]
fn never_changes_h1() {
    assert_eq!(tidied("### Deep\n\n# Title\n", &[TidyRule::HeadingLevels]), "### Deep\n\n# Title\n");
    assert_eq!(tidied("# A\n\n# B\n", &[TidyRule::HeadingLevels]), "# A\n\n# B\n");
}

#[test]
fn leaves_a_document_that_starts_at_h2_alone() {
    assert_eq!(tidied("## A\n\n### B\n", &[TidyRule::HeadingLevels]), "## A\n\n### B\n");
}

#[test]
fn aligns_table_pipes_respecting_alignment_markers() {
    let out = tidied("|Name|Count|Notes|\n|:-|-:|:-:|\n|a|1|x|\n|bbbb|22|yy|\n", &[TidyRule::TablePipes]);
    assert_eq!(
        out,
        "| Name | Count | Notes |\n| :--- | ----: | :---: |\n| a    |     1 |   x   |\n| bbbb |    22 |  yy   |\n"
    );
}

#[test]
fn collapses_blank_line_runs() {
    assert_eq!(tidied("a\n\n\n\n\nb\n", &[TidyRule::BlankLines]), "a\n\nb\n");
    assert_eq!(tidied("a\n\nb\n", &[TidyRule::BlankLines]), "a\n\nb\n");
}

#[test]
fn blank_lines_inside_code_fences_are_content() {
    let text = "```\nx\n\n\n\ny\n```\n";
    assert_eq!(tidied(text, &[TidyRule::BlankLines]), text);
}

#[test]
fn adds_code_fence_languages_only_when_confident() {
    assert_eq!(
        tidied("```\ndef f(self):\n    import os\n```\n", &[TidyRule::CodeFenceLanguages]),
        "```python\ndef f(self):\n    import os\n```\n"
    );
    let vague = "```\njust some words\n```\n";
    assert_eq!(tidied(vague, &[TidyRule::CodeFenceLanguages]), vague);
    let tagged = "```text\ndef f(self):\n    import os\n```\n";
    assert_eq!(tidied(tagged, &[TidyRule::CodeFenceLanguages]), tagged);
}

#[test]
fn renumbers_ordered_lists() {
    assert_eq!(tidied("1. a\n1. b\n1. c\n", &[TidyRule::OrderedListNumbers]), "1. a\n2. b\n3. c\n");
    // A list that deliberately starts at 3 keeps its start.
    assert_eq!(tidied("3. a\n7. b\n", &[TidyRule::OrderedListNumbers]), "3. a\n4. b\n");
}

#[test]
fn normalises_list_markers() {
    assert_eq!(tidied("* a\n* b\n", &[TidyRule::ListMarkers]), "- a\n- b\n");
    assert_eq!(tidied("+ a\n+ b\n", &[TidyRule::ListMarkers]), "- a\n- b\n");
}

#[test]
fn trims_trailing_whitespace_but_keeps_hard_breaks() {
    // Exactly two spaces is a deliberate hard line break (§6.4).
    assert_eq!(tidied("line one  \nline two\n", &[TidyRule::TrailingWhitespace]), "line one  \nline two\n");
    assert_eq!(tidied("line one   \nline two\t\n", &[TidyRule::TrailingWhitespace]), "line one\nline two\n");
    assert_eq!(tidied("   \nx\n", &[TidyRule::TrailingWhitespace]), "\nx\n");
}

#[test]
fn trailing_whitespace_inside_code_is_preserved() {
    let text = "```\nx   \n```\n";
    assert_eq!(tidied(text, &[TidyRule::TrailingWhitespace]), text);
}

/// §9.1 regression: trailing-whitespace and blank-lines overlap on
/// whitespace-only blank runs. Previously one edit won silently, so the
/// collapse was dropped and the run kept every whitespace line.
#[test]
fn whitespace_only_blank_runs_collapse_without_overlap() {
    let source = "a\n   \n\t \nb\n";
    let rules = [TidyRule::TrailingWhitespace, TidyRule::BlankLines];
    let mut sorted = TidyDocument::plan_with(&MarkdownParser::parse(source), &rules);
    sorted.sort_by(|a, b| a.range.location.cmp(&b.range.location));
    for index in 1..sorted.len() {
        assert!(
            sorted[index - 1].range.upper_bound() <= sorted[index].range.location,
            "overlapping tidy edits would be dropped silently"
        );
    }
    let applied_text = applied(&TidyDocument::plan_with(&MarkdownParser::parse(source), &rules), source);
    assert_eq!(applied_text, "a\n\nb\n", "whitespace-only blank run not collapsed: {applied_text:?}");
}

#[test]
fn lone_blank_whitespace_line_still_trimmed_alone() {
    assert_eq!(tidied("   \nx\n", &[TidyRule::TrailingWhitespace]), "\nx\n");
    assert_eq!(tidied("   \nx\n", &[TidyRule::TrailingWhitespace, TidyRule::BlankLines]), "\nx\n");
}

// MARK: Invariants

/// A rule that normalises to a form it would then re-normalise produces a
/// document that never stops changing. Idempotence is the guard.
#[test]
fn plan_is_idempotent_across_the_corpus() {
    for (name, text) in corpus::ALL {
        let once = tidied_all(text);
        let second = TidyDocument::plan(&MarkdownParser::parse(&once));
        assert!(
            second.is_empty(),
            "{name}: second pass wanted {:?}",
            second.iter().map(|edit| edit.summary.as_str()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn each_rule_is_individually_idempotent() {
    for rule in TidyRule::ALL_CASES {
        for (name, text) in corpus::ALL {
            let once = tidied(text, &[rule]);
            let second = TidyDocument::plan_with(&MarkdownParser::parse(&once), &[rule]);
            assert!(
                second.is_empty(),
                "{} on {name}: {:?}",
                rule.raw_value(),
                second.iter().map(|edit| edit.summary.as_str()).collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn edits_never_overlap() {
    for (name, text) in corpus::ALL {
        let edits = TidyDocument::plan(&MarkdownParser::parse(text));
        for pair in edits.windows(2) {
            assert!(pair[0].range.upper_bound() <= pair[1].range.location, "{name}: overlapping tidy edits");
        }
    }
}

#[test]
fn every_edit_is_labelled() {
    let edits = TidyDocument::plan(&MarkdownParser::parse(corpus::ODD_SPACING));
    assert!(!edits.is_empty());
    for edit in &edits {
        assert!(!edit.summary.is_empty());
        assert!(edit.rule.is_some());
    }
}

#[test]
fn a_tidy_document_gets_no_edits() {
    let clean = "# Title\n\n## Section\n\nA paragraph.\n\n- one\n- two\n";
    assert!(TidyDocument::plan(&MarkdownParser::parse(clean)).is_empty());
}

#[test]
fn selected_rules_only_produce_their_own_edits() {
    let edits = TidyDocument::plan_with(&MarkdownParser::parse(corpus::KITCHEN_SINK), &[TidyRule::BlankLines]);
    assert!(edits.iter().all(|edit| edit.rule == Some(TidyRule::BlankLines)));
}

/// The whole point of §9.1: the document is still the same document.
#[test]
fn tidy_preserves_content() {
    let out = tidied_all(corpus::KITCHEN_SINK);
    let doc = MarkdownParser::parse(&out);
    let original = MarkdownParser::parse(corpus::KITCHEN_SINK);
    assert_eq!(
        doc.headings.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(),
        original.headings.iter().map(|h| h.title.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(doc.tasks.len(), original.tasks.len());
    assert_eq!(
        doc.front_matter.as_ref().and_then(|f| f.get("title")),
        original.front_matter.as_ref().and_then(|f| f.get("title"))
    );
}
