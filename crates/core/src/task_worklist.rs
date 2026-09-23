//! TaskWorklist.swift — a document's tasks, organised for the Tasks panel.
//!
//! Tasks grouped under the heading they belong to, split into open and done,
//! with progress numbers, a one-line status summary and a clipboard-ready
//! report. All of it is a pure function of `doc.tasks` and `doc.headings`:
//! one pass buckets the tasks into sections (`doc.tasks` is already in
//! document order, so appending preserves order everywhere), then a handful
//! of counts and the two string renders.

use std::collections::HashMap;

use crate::model::{HeadingNode, TaskItem};

/// `TaskWorklist.Entry`: a task flattened to what the panel renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Index into the source `[TaskItem]`.
    pub task_index: isize,
    /// `markRange.location` — the offset `Restructure.toggleTask` wants.
    pub mark_offset: isize,
    /// `contentRange.location`.
    pub content_offset: isize,
    pub text: String,
    pub is_checked: bool,
    pub indent_level: isize,
}

impl Entry {
    pub fn new(
        task_index: isize,
        mark_offset: isize,
        content_offset: isize,
        text: impl Into<String>,
        is_checked: bool,
        indent_level: isize,
    ) -> Entry {
        Entry { task_index, mark_offset, content_offset, text: text.into(), is_checked, indent_level }
    }
}

/// `TaskWorklist.Section`: the tasks that share a `headingIndex`. A section
/// exists only when at least one task falls under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub heading_index: Option<isize>,
    /// The heading's title, or "Document" when `heading_index` is `None`.
    pub title: String,
    /// Every task in the section, in document order.
    pub entries: Vec<Entry>,
    /// The unchecked tasks, stable in document order — shown first.
    pub open_entries: Vec<Entry>,
    /// The checked tasks, stable in document order.
    pub done_entries: Vec<Entry>,
    pub open_count: isize,
    pub done_count: isize,
}

impl Section {
    pub fn new(
        heading_index: Option<isize>,
        title: impl Into<String>,
        entries: Vec<Entry>,
        open_entries: Vec<Entry>,
        done_entries: Vec<Entry>,
        open_count: isize,
        done_count: isize,
    ) -> Section {
        Section { heading_index, title: title.into(), entries, open_entries, done_entries, open_count, done_count }
    }
}

/// `TaskWorklist.Segment`: one section's share of the progress bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub section_index: isize,
    pub title: String,
    pub task_count: isize,
    pub done_count: isize,
    /// `task_count / total_count`, or 0 when the worklist is empty.
    pub weight: f64,
    /// `done_count / task_count`, or 0 when the section has no tasks.
    pub completion: f64,
}

impl Segment {
    pub fn new(
        section_index: isize,
        title: impl Into<String>,
        task_count: isize,
        done_count: isize,
        weight: f64,
        completion: f64,
    ) -> Segment {
        Segment { section_index, title: title.into(), task_count, done_count, weight, completion }
    }
}

/// `upNext`'s `(sectionIndex: Int, entry: Entry)` tuple.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpNext {
    pub section_index: isize,
    pub entry: Entry,
}

/// A document's tasks, organised for the Tasks panel.
///
/// Swift writes `==` out by hand only because the `upNext` tuple blocks
/// synthesis; it compares every field, which is what the derive does.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskWorklist {
    /// Sections in the order their first task appears in the document.
    pub sections: Vec<Section>,
    pub total_count: isize,
    pub done_count: isize,
    /// The first open entry in document order — what "next" refers to.
    pub up_next: Option<UpNext>,
    pub segments: Vec<Segment>,
    /// One line for the status bar; empty when there are no tasks.
    pub status_line: String,
    /// Just the counts, for the panel's caption.
    pub count_line: String,
    /// Clipboard-ready Markdown summary; empty when there are no tasks.
    pub status_report: String,
}

impl TaskWorklist {
    /// `init(tasks:headings:)`. An out-of-range `headingIndex` (a task array
    /// captured a parse earlier than its headings) degrades to the "Document"
    /// section rather than trapping.
    pub fn new(tasks: &[TaskItem], headings: &[HeadingNode]) -> TaskWorklist {
        let mut sections: Vec<Section> = Vec::new();
        let mut bucket: HashMap<Option<isize>, usize> = HashMap::new();
        let mut up_next: Option<UpNext> = None;
        let mut done_count: isize = 0;

        for (task_index, task) in tasks.iter().enumerate() {
            let entry = Entry::new(
                task_index as isize,
                task.mark_range.location,
                task.content_range.location,
                task.text.clone(),
                task.is_checked,
                task.indent_level,
            );
            // Validate against the live `headings` array, not the task's word.
            let heading_index = task.heading_index.filter(|&i| i >= 0 && (i as usize) < headings.len());
            let section_index = match bucket.get(&heading_index) {
                Some(&existing) => existing,
                None => {
                    let index = sections.len();
                    bucket.insert(heading_index, index);
                    sections.push(Section::new(
                        heading_index,
                        heading_index.map_or_else(|| "Document".to_owned(), |i| headings[i as usize].title.clone()),
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                        0,
                        0,
                    ));
                    index
                }
            };
            let section = &mut sections[section_index];
            section.entries.push(entry.clone());
            if task.is_checked {
                section.done_entries.push(entry);
                section.done_count += 1;
                done_count += 1;
            } else {
                section.open_entries.push(entry.clone());
                section.open_count += 1;
                // The first open task met is the first in document order.
                if up_next.is_none() {
                    up_next = Some(UpNext { section_index: section_index as isize, entry });
                }
            }
        }

        let total = tasks.len() as isize;
        let segments: Vec<Segment> = sections
            .iter()
            .enumerate()
            .map(|(index, section)| {
                let task_count = section.entries.len() as isize;
                Segment::new(
                    index as isize,
                    section.title.clone(),
                    task_count,
                    section.done_count,
                    if tasks.is_empty() { 0.0 } else { task_count as f64 / total as f64 },
                    if task_count == 0 { 0.0 } else { section.done_count as f64 / task_count as f64 },
                )
            })
            .collect();

        let (status_line, count_line, status_report);
        if tasks.is_empty() {
            status_line = String::new();
            count_line = String::new();
            status_report = String::new();
        } else if done_count == total {
            let summary = if total == 1 { "1 task done".to_owned() } else { format!("All {total} tasks done") };
            let mut report = format!("**{summary}**");
            Self::append_sections(&mut report, &sections);
            status_line = summary.clone();
            count_line = summary;
            status_report = report;
        } else {
            // done_count < total_count means an open task exists, so up_next is set.
            let next = up_next.as_ref().map_or("", |u| u.entry.text.as_str());
            // U+00B7 middle dot for the one-liner, U+2014 em dash for Markdown.
            status_line = format!("{done_count} of {total} done \u{B7} next: {next}");
            count_line = format!("{done_count} of {total} done");
            let mut report = format!("**{done_count} of {total} done** \u{2014} next: {next}");
            Self::append_sections(&mut report, &sections);
            status_report = report;
        }

        TaskWorklist { sections, total_count: total, done_count, up_next, segments, status_line, count_line, status_report }
    }

    /// Every section below the report's first line: a blank line, the `##`
    /// heading with its done/total counts, then each task as a GFM checkbox
    /// indented two spaces per `indent_level`.
    fn append_sections(report: &mut String, sections: &[Section]) {
        for section in sections {
            report.push_str(&format!("\n\n## {} ({}/{})", section.title, section.done_count, section.entries.len()));
            for entry in &section.entries {
                report.push('\n');
                if entry.indent_level > 0 {
                    report.push_str(&"  ".repeat(entry.indent_level as usize));
                }
                report.push_str(if entry.is_checked { "- [x] " } else { "- [ ] " });
                report.push_str(&entry.text);
            }
        }
    }
}
