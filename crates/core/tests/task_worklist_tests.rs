//! TaskWorklistTests.swift. TaskItems are built by hand: the worklist is a
//! pure derivation, so no parser is involved.

use upleft_core::task_worklist::{Segment, TaskWorklist};
use upleft_core::{HeadingNode, NSRange, TaskItem};

fn task(text: &str, checked: bool, heading: Option<isize>, indent: isize, mark: isize) -> TaskItem {
    TaskItem::new(
        checked,
        NSRange::new(mark, 1),
        NSRange::new(mark + 4, text.encode_utf16().count() as isize),
        text,
        heading,
        indent,
    )
}

/// `task(_:)` with Swift's defaults.
fn t(text: &str) -> TaskItem {
    task(text, false, None, 0, 0)
}

fn done(text: &str) -> TaskItem {
    task(text, true, None, 0, 0)
}

fn under(text: &str, heading: isize) -> TaskItem {
    task(text, false, Some(heading), 0, 0)
}

fn done_under(text: &str, heading: isize) -> TaskItem {
    task(text, true, Some(heading), 0, 0)
}

fn headings(titles: &[&str]) -> Vec<HeadingNode> {
    titles.iter().map(|title| HeadingNode::new(1, *title, NSRange::new(0, 1), NSRange::new(0, 1), NSRange::new(0, 1))).collect()
}

// MARK: Empty input

#[test]
fn empty_input_yields_an_empty_worklist() {
    let worklist = TaskWorklist::new(&[], &[]);
    assert!(worklist.sections.is_empty());
    assert_eq!(worklist.total_count, 0);
    assert_eq!(worklist.done_count, 0);
    assert!(worklist.up_next.is_none());
    assert!(worklist.segments.is_empty());
    assert_eq!(worklist.status_line, "");
    assert_eq!(worklist.status_report, "");
}

// MARK: Sections

#[test]
fn tasks_before_any_heading_land_in_the_document_section() {
    let worklist = TaskWorklist::new(&[task("alpha", false, None, 0, 3), task("beta", true, None, 0, 20)], &headings(&["Later"]));
    assert_eq!(worklist.sections.len(), 1);
    assert_eq!(worklist.sections[0].heading_index, None);
    assert_eq!(worklist.sections[0].title, "Document");
    assert_eq!(worklist.sections[0].open_count, 1);
    assert_eq!(worklist.sections[0].done_count, 1);
    // The range offsets survive the flattening into Entry.
    assert_eq!(worklist.sections[0].entries[0].mark_offset, 3);
    assert_eq!(worklist.sections[0].entries[0].content_offset, 7);
    assert_eq!(worklist.sections[0].entries[1].mark_offset, 20);
}

#[test]
fn sections_appear_in_order_of_their_first_task() {
    let worklist = TaskWorklist::new(
        &[
            t("preamble"),
            under("a", 0),
            under("b", 1),
            // Back-reference: appends to the existing section rather than
            // opening a new one, and the section order is untouched.
            under("c", 0),
            under("d", 1),
        ],
        &headings(&["Intro", "Later"]),
    );
    assert_eq!(worklist.sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Document", "Intro", "Later"]);
    assert_eq!(worklist.sections.iter().map(|s| s.heading_index).collect::<Vec<_>>(), [None, Some(0), Some(1)]);
    assert_eq!(worklist.sections[1].entries.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["a", "c"]);
    assert_eq!(worklist.sections[2].entries.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["b", "d"]);
    assert_eq!(worklist.total_count, 5);
    assert_eq!(worklist.done_count, 0);
}

#[test]
fn headings_without_tasks_get_no_section() {
    let worklist = TaskWorklist::new(&[under("only", 1)], &headings(&["Empty", "Full"]));
    assert_eq!(worklist.sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Full"]);
    assert_eq!(worklist.sections[0].heading_index, Some(1));
}

// MARK: Open / done partitioning

#[test]
fn open_and_done_partitions_keep_document_order() {
    let worklist = TaskWorklist::new(&[t("one"), done("two"), t("three"), done("four"), t("five")], &[]);
    let section = &worklist.sections[0];
    let texts = |entries: &[upleft_core::task_worklist::Entry]| entries.iter().map(|e| e.text.clone()).collect::<Vec<_>>();
    assert_eq!(texts(&section.entries), ["one", "two", "three", "four", "five"]);
    assert_eq!(texts(&section.open_entries), ["one", "three", "five"]);
    assert_eq!(texts(&section.done_entries), ["two", "four"]);
    assert_eq!(section.open_count, 3);
    assert_eq!(section.done_count, 2);
    // The partitions are views of the same tasks, not renumbered copies.
    assert_eq!(section.open_entries.iter().map(|e| e.task_index).collect::<Vec<_>>(), [0, 2, 4]);
    assert_eq!(section.done_entries.iter().map(|e| e.task_index).collect::<Vec<_>>(), [1, 3]);
}

// MARK: Up next

#[test]
fn up_next_is_the_first_open_task_in_document_order() {
    let worklist = TaskWorklist::new(
        &[done_under("finished", 0), done_under("also finished", 0), under("waiting", 1), under("later", 1)],
        &headings(&["Done", "Open"]),
    );
    // A fully-done section is skipped, not just a fully-done task.
    let up_next = worklist.up_next.as_ref().expect("an open task");
    assert_eq!(up_next.section_index, 1);
    assert_eq!(up_next.entry.text, "waiting");
    assert_eq!(up_next.entry.task_index, 2);
    assert_eq!(up_next.entry, worklist.sections[1].open_entries[0]);
}

#[test]
fn up_next_is_nil_when_everything_is_done() {
    let worklist = TaskWorklist::new(&[done("a"), done_under("b", 0)], &headings(&["H"]));
    assert!(worklist.up_next.is_none());
    assert_eq!(worklist.done_count, worklist.total_count);
}

// MARK: Segments

#[test]
fn segment_weights_sum_to_one_and_completion_is_per_section() {
    let worklist = TaskWorklist::new(
        &[under("a1", 0), done_under("a2", 0), under("a3", 0), done_under("b1", 1)],
        &headings(&["A", "B"]),
    );
    assert_eq!(
        worklist.segments,
        vec![Segment::new(0, "A", 3, 1, 3.0 / 4.0, 1.0 / 3.0), Segment::new(1, "B", 1, 1, 1.0 / 4.0, 1.0)]
    );
    let weight_sum = worklist.segments.iter().fold(0.0, |sum, segment| sum + segment.weight);
    assert!((weight_sum - 1.0f64).abs() < 1e-12);
}

// MARK: Status line

#[test]
fn status_line_covers_empty_partial_and_complete() {
    assert_eq!(TaskWorklist::new(&[], &[]).status_line, "");

    let all_done = TaskWorklist::new(&[done("a"), done("b")], &[]);
    assert_eq!(all_done.status_line, "All 2 tasks done");

    // The separator is space, U+00B7 middle dot, space.
    let partial = TaskWorklist::new(&[done("a"), t("b"), t("c")], &[]);
    assert_eq!(partial.status_line, "1 of 3 done \u{B7} next: b");
}

// MARK: Status report

#[test]
fn status_report_renders_two_sections_with_indentation() {
    let worklist = TaskWorklist::new(
        &[done_under("alpha", 0), task("beta", false, Some(0), 1, 0), under("gamma", 1)],
        &headings(&["Intro", "Later"]),
    );
    // Em dash (U+2014) after the bold summary; no trailing newline.
    assert_eq!(
        worklist.status_report,
        "**1 of 3 done** \u{2014} next: beta\n\n## Intro (1/2)\n- [x] alpha\n  - [ ] beta\n\n## Later (0/1)\n- [ ] gamma"
    );
}

#[test]
fn status_report_for_an_empty_worklist_is_empty() {
    assert_eq!(TaskWorklist::new(&[], &[]).status_report, "");
}

// MARK: Singular

#[test]
fn a_single_finished_task_uses_the_singular_everywhere() {
    let worklist = TaskWorklist::new(&[done("alpha")], &[]);
    assert_eq!(worklist.status_line, "1 task done");
    assert_eq!(worklist.status_report, "**1 task done**\n\n## Document (1/1)\n- [x] alpha");
    assert!(worklist.up_next.is_none());
}

// MARK: Value semantics

#[test]
fn worklists_with_equal_inputs_are_equal() {
    let tasks = [t("a"), done_under("b", 0)];
    let heads = headings(&["H"]);
    assert_eq!(TaskWorklist::new(&tasks, &heads), TaskWorklist::new(&tasks, &heads));
}

// MARK: Robustness

/// An out-of-range `headingIndex` must degrade to the "Document" section,
/// never trap.
#[test]
fn an_out_of_range_heading_index_falls_back_to_document() {
    let tasks = [
        under("orphan", 7), // no such heading
        done_under("valid", 0),
    ];
    let worklist = TaskWorklist::new(&tasks, &headings(&["Only"]));
    assert_eq!(worklist.total_count, 2);
    // The orphan joined "Document"; the valid task stayed under its heading.
    let titles: Vec<&str> = worklist.sections.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.contains(&"Document"));
    assert!(titles.contains(&"Only"));
    let document = worklist.sections.iter().find(|s| s.heading_index.is_none());
    assert_eq!(document.map(|d| d.entries.iter().map(|e| e.text.as_str()).collect::<Vec<_>>()), Some(vec!["orphan"]));
    assert_eq!(worklist.up_next.as_ref().map(|u| u.entry.text.as_str()), Some("orphan"));
}

#[test]
fn a_task_whose_heading_index_is_valid_survives() {
    let worklist = TaskWorklist::new(&[under("real", 1)], &headings(&["A", "B"]));
    assert_eq!(worklist.sections.first().map(|s| s.title.as_str()), Some("B"));
    assert_eq!(worklist.sections.first().and_then(|s| s.heading_index), Some(1));
}

// Extra (not in Swift): a negative heading index is out of range too, and the
// count line stays a pure meter.
#[test]
fn negative_heading_index_and_count_line() {
    let worklist = TaskWorklist::new(&[under("x", -1), t("y")], &headings(&["H"]));
    assert_eq!(worklist.sections.len(), 1);
    assert_eq!(worklist.sections[0].title, "Document");
    assert_eq!(worklist.count_line, "0 of 2 done");
    let all = TaskWorklist::new(&[done("x"), done("y"), done("z")], &[]);
    assert_eq!(all.count_line, "All 3 tasks done");
    assert_eq!(all.status_report, "**All 3 tasks done**\n\n## Document (3/3)\n- [x] x\n- [x] y\n- [x] z");
}
