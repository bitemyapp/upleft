//! Restructure.swift — restructuring (§9.2).
//!
//! Promote/demote/move/convert/sort, plus the table operations §6.3 drives
//! from the pointer. Everything returns `[TextEdit]` so §9.1's accept/reject
//! sheet and plain-text undo work uniformly.
//!
//! Section boundaries land on line starts and a section owns the blank lines
//! that follow it, so a move is a pure cut-and-paste of whole lines with no
//! whitespace arithmetic at all (§14's `moveSection` trap).

use std::sync::Arc;

use crate::contracts::{ListConversion, ListSortOrder, TextEdit};
use crate::document_io::DocumentIO;
use crate::metrics::PlainText;
use crate::model::{BlockContent, HeadingNode, MDBlock, ParsedDocument, TableAlignment, TableData, TaskItem};
use crate::ns_range::NSRange;
use crate::swift_text::{self, ns::NSStringExt};
use crate::table_formatter::TableFormatter;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MoveDirection {
    Up,
    Down,
}

pub struct Restructure;

/// `String(s.prefix { … })` over Characters.
fn prefix_while(s: &str, mut predicate: impl FnMut(&str) -> bool) -> &str {
    let end = swift_text::first_index_where(s, |g| !predicate(g)).unwrap_or(s.len());
    &s[..end]
}

/// `ListSortOrder.rawValue` (the Swift enum is `String`-backed).
fn sort_order_raw_value(order: ListSortOrder) -> &'static str {
    match order {
        ListSortOrder::Alphabetical => "alphabetical",
        ListSortOrder::ReverseAlphabetical => "reverseAlphabetical",
        ListSortOrder::UncheckedFirst => "uncheckedFirst",
        ListSortOrder::CheckedFirst => "checkedFirst",
    }
}

#[inline]
fn contains_index<T>(items: &[T], index: isize) -> bool {
    index >= 0 && (index as usize) < items.len()
}

impl Restructure {
    // MARK: Headings

    pub fn promote_heading(doc: &ParsedDocument, heading_index: isize) -> Vec<TextEdit> {
        Self::relevel(doc, heading_index, -1)
    }

    pub fn demote_heading(doc: &ParsedDocument, heading_index: isize) -> Vec<TextEdit> {
        Self::relevel(doc, heading_index, 1)
    }

    /// Sets one heading subtree to an exact root level while preserving the
    /// relative depth of every descendant (the direct H1…H6 path).
    pub fn set_heading_level(doc: &ParsedDocument, heading_index: isize, level: isize) -> Vec<TextEdit> {
        if !(contains_index(&doc.headings, heading_index) && (1..=6).contains(&level)) {
            return Vec::new();
        }
        Self::relevel(doc, heading_index, level - doc.headings[heading_index as usize].level)
    }

    /// Converts an ATX or setext heading to ordinary body text without
    /// guessing marker widths: `contentRange` is parser-owned, so compact
    /// headings (`#Title`), extra marker spacing and closing hash runs never
    /// cost title characters.
    pub fn heading_to_body_text(doc: &ParsedDocument, heading_index: isize) -> Vec<TextEdit> {
        if !contains_index(&doc.headings, heading_index) {
            return Vec::new();
        }
        let heading = &doc.headings[heading_index as usize];
        let first_line = doc.range_of_line(doc.line_at(heading.range.location));

        if heading.range.upper_bound() > first_line.upper_bound() {
            let underline_number = doc.line_at(heading.range.upper_bound() - 1);
            let underline_line = doc.range_of_line(underline_number);
            if !(underline_line.location > first_line.location) {
                return Vec::new();
            }
            let removal_end = if underline_number < doc.line_starts.len() as isize {
                doc.line_starts[underline_number as usize]
            } else {
                doc.length
            };
            return vec![TextEdit::new(
                NSRange::new(underline_line.location, removal_end - underline_line.location),
                "",
                "Heading to body text",
                None,
            )];
        }

        // Anchor at the parser-owned block start: past the leading indent for
        // a plain ATX line, past the container marker inside a blockquote or
        // list item (which must survive the conversion).
        let marker_location = heading.range.location;
        if !(marker_location >= first_line.location
            && marker_location <= first_line.upper_bound()
            && heading.content_range.location >= marker_location
            && heading.content_range.location <= first_line.upper_bound())
        {
            return Vec::new();
        }
        let mut edits = vec![TextEdit::new(
            NSRange::new(marker_location, heading.content_range.location - marker_location),
            "",
            "Heading to body text",
            None,
        )];
        if heading.content_range.upper_bound() < first_line.upper_bound() {
            edits.push(TextEdit::new(
                NSRange::new(heading.content_range.upper_bound(), first_line.upper_bound() - heading.content_range.upper_bound()),
                "",
                "Remove closing heading marker",
                None,
            ));
        }
        edits.into_iter().filter(|edit| edit.range.length > 0).collect()
    }

    /// Moves the whole subtree: the heading and every heading beneath it
    /// shift by `delta`, clamped to 1…6. A no-op at the clamp rather than a
    /// partial move, so the subtree's shape is never flattened.
    fn relevel(doc: &ParsedDocument, heading_index: isize, delta: isize) -> Vec<TextEdit> {
        if !contains_index(&doc.headings, heading_index) {
            return Vec::new();
        }
        if delta == 0 {
            return Vec::new();
        }
        let index = heading_index as usize;
        let root = &doc.headings[index];
        let target = root.level + delta;
        if !(target >= 1 && target <= 6) {
            return Vec::new();
        }

        let subtree_end = doc.headings[index + 1..]
            .iter()
            .position(|heading| heading.level <= root.level)
            .map_or(doc.headings.len(), |p| index + 1 + p);
        let subtree = &doc.headings[index..subtree_end];
        if !subtree.iter().all(|heading| (1..=6).contains(&(heading.level + delta))) {
            return Vec::new();
        }

        let mut edits: Vec<TextEdit> = Vec::new();
        for heading in subtree {
            let level = heading.level + delta;
            if level == heading.level {
                continue;
            }
            let Some(edit) = Self::rewrite_level(doc, heading, level) else { continue };
            edits.push(edit);
        }
        edits
    }

    fn rewrite_level(doc: &ParsedDocument, heading: &HeadingNode, level: isize) -> Option<TextEdit> {
        let line = doc.range_of_line(doc.line_at(heading.range.location));

        // A setext heading's block spans its underline line; an ATX heading is
        // confined to its own source line. Only a genuine setext shape may
        // consume a second line (a heading nested in a container also fails
        // the "line starts with N hashes" probe and must never land here).
        let is_setext = heading.range.upper_bound() > line.upper_bound();

        if is_setext {
            // Setext can express only H1/H2, so normalize to ATX, keeping the
            // container prefix, the title's exact source bytes (not the plain
            // `heading.title`) and the original line ending.
            let title_line_number = doc.line_at(line.location);
            if !(title_line_number < doc.line_starts.len() as isize) {
                return None;
            }
            // The underline sits on the immediately following source line.
            // `lineStarts` is 0-based, so the 1-based number of a line indexes
            // the *following* line's start.
            let underline_start = doc.line_starts[title_line_number as usize];
            let removal_end = if title_line_number + 1 < doc.line_starts.len() as isize {
                doc.line_starts[(title_line_number + 1) as usize]
            } else {
                doc.length
            };
            // Only consume the second line when it really is an underline.
            let underline_raw = doc.substring(NSRange::new(underline_start, removal_end - underline_start));
            let underline_text = swift_text::trim_whitespaces_and_newlines(&underline_raw);
            if underline_text.is_empty()
                || !swift_text::all_satisfy(underline_text, |g| swift_text::char_is(g, '=') || swift_text::char_is(g, '-'))
            {
                return None;
            }
            let prefix_length = 0.max((heading.range.location - line.location).min(line.length));
            let prefix = doc.substring(NSRange::new(line.location, prefix_length));
            let separator = if line.upper_bound() < underline_start {
                doc.substring(NSRange::new(line.upper_bound(), underline_start - line.upper_bound()))
            } else {
                String::new()
            };
            let raw_title = doc.substring(heading.content_range);
            return Some(TextEdit::new(
                NSRange::new(line.location, removal_end - line.location),
                format!("{prefix}{} {raw_title}{separator}", swift_text::repeating("#", level)),
                format!("H{} → H{level}: {}", heading.level, heading.title),
                None,
            ));
        }

        // ATX: replace exactly the `#` run at the parser-owned block start.
        let ns_source = swift_text::ns::utf16(&doc.substring(line));
        let column_in_line = heading.range.location - line.location;
        if !(column_in_line >= 0 && column_in_line <= ns_source.as_slice().length()) {
            return None;
        }
        let from_column = ns_source.as_slice().substring_from(column_in_line);
        let hashes = prefix_while(&from_column, |g| swift_text::char_is(g, '#'));
        let hash_units = swift_text::utf16_count(hashes);
        if hash_units != heading.level {
            return None;
        }
        Some(TextEdit::new(
            NSRange::new(heading.range.location, hash_units),
            swift_text::repeating("#", level),
            format!("H{} → H{level}: {}", heading.level, heading.title),
            None,
        ))
    }

    // MARK: Moving

    /// Moves `heading_index`'s section so it starts where `target_index`'s
    /// does; `target_index == doc.headings.count` moves it to the end.
    ///
    /// The blank lines between two sections belong to the *join*, so the
    /// section is split into its content and its trailing blank run, the
    /// content moves, and each end of the move re-establishes the separator
    /// its position calls for.
    pub fn move_section(doc: &ParsedDocument, heading_index: isize, target_index: isize) -> Vec<TextEdit> {
        if !contains_index(&doc.headings, heading_index) {
            return Vec::new();
        }
        let section = doc.headings[heading_index as usize].section_range;
        let destination = if target_index < doc.headings.len() as isize {
            doc.headings[0.max(target_index) as usize].section_range.location
        } else {
            doc.length
        };
        if !(destination <= section.location || destination >= section.upper_bound()) {
            return Vec::new();
        }
        if destination == section.location {
            return Vec::new();
        }

        let text = doc.utf16.as_slice();
        let ending = DocumentIO::dominant_line_ending(&doc.text).raw_value();
        let title = &doc.headings[heading_index as usize].title;
        let split = SectionSplit::new(section, text);
        let leading_separator = Self::blank_separator(section.location, text);

        // Cut. A section that owns a trailing blank run takes it along; the
        // last section owns nothing, so the run before it — and the final
        // newline, if the file has none of its own — go too, measured in the
        // bytes the document actually has.
        let cut = if split.trailing_blanks > 0 || section.upper_bound() < doc.length {
            section
        } else {
            let unterminated_width = if split.is_terminated {
                0
            } else {
                Self::terminator(section.location, text).map_or(swift_text::utf16_count(ending), swift_text::utf16_count)
            };
            let extra = swift_text::utf16_count(&leading_separator) + unterminated_width;
            NSRange::new(0.max(section.location - extra), section.length + extra.min(section.location))
        };

        // Paste.
        let insertion = if destination >= doc.length {
            // Appending: the separator goes in front, since there is no
            // following section to carry one.
            let separator: &str = if split.trailing_blanks > 0 {
                &split.trailing_separator
            } else if leading_separator.is_empty() {
                ending
            } else {
                &leading_separator
            };
            format!("{separator}{}", split.core)
        } else {
            let mut separator = Self::blank_separator(destination, text);
            if separator.is_empty() {
                separator = leading_separator.clone();
            }
            split.terminated_core(ending) + &separator
        };

        vec![
            TextEdit::new(cut, "", format!("Move section: {title}"), None),
            TextEdit::new(NSRange::new(destination, 0), insertion, format!("Move section: {title}"), None),
        ]
    }

    /// The line terminator that ends exactly at `offset` (`\r\n`, `\n`, or a
    /// lone `\r`), or `None`.
    fn terminator(offset: isize, text: &[u16]) -> Option<&'static str> {
        if offset <= 0 {
            return None;
        }
        if offset >= 2 && text.character_at(offset - 2) == 0x0D && text.character_at(offset - 1) == 0x0A {
            return Some("\r\n");
        }
        if text.character_at(offset - 1) == 0x0A {
            return Some("\n");
        }
        if text.character_at(offset - 1) == 0x0D {
            return Some("\r");
        }
        None
    }

    /// The blank-line separator immediately before `offset`, a line start: the
    /// run of terminators there less the previous line's own, as the exact
    /// bytes found in the document.
    fn blank_separator(offset: isize, text: &[u16]) -> String {
        let mut terminators: Vec<&'static str> = Vec::new();
        let mut index = offset;
        while let Some(terminator) = Self::terminator(index, text) {
            terminators.push(terminator);
            index -= terminator.len() as isize;
        }
        // `terminators[0]` is the previous line's own terminator.
        terminators.iter().skip(1).rev().copied().collect()
    }

    /// Moves the block containing `offset` past its neighbouring sibling. The
    /// gap between the two stays exactly where it is, which keeps `⌥⌘↑`/`⌥⌘↓`
    /// from slowly eating a document's spacing.
    pub fn move_block(doc: &ParsedDocument, offset: isize, direction: MoveDirection) -> Vec<TextEdit> {
        let Some((parent, index)) = Self::movable_block(doc, offset) else { return Vec::new() };
        let siblings = &parent.children;
        let index = index as isize;
        let other_index = if direction == MoveDirection::Up { index - 1 } else { index + 1 };
        if !contains_index(siblings, other_index) {
            return Vec::new();
        }

        let first = &siblings[index.min(other_index) as usize];
        let second = &siblings[index.max(other_index) as usize];
        let text = doc.utf16.as_slice();
        let span = NSRange::new(first.range.location, second.range.upper_bound() - first.range.location);
        let gap = NSRange::new(first.range.upper_bound(), 0.max(second.range.location - first.range.upper_bound()));
        let replacement = text.substring(second.range) + &text.substring(gap) + &text.substring(first.range);
        if swift_text::str_eq(&replacement, &text.substring(span)) {
            return Vec::new();
        }
        vec![TextEdit::new(
            span,
            replacement,
            if direction == MoveDirection::Up { "Move block up" } else { "Move block down" },
            None,
        )]
    }

    /// The block a move should act on: the innermost list item if there is
    /// one, otherwise the top-level block.
    fn movable_block(doc: &ParsedDocument, offset: isize) -> Option<(Arc<MDBlock>, usize)> {
        // (parent, child)
        let mut chain: Vec<(&Arc<MDBlock>, &Arc<MDBlock>)> = Vec::new();
        fn descend<'a>(block: &'a Arc<MDBlock>, offset: isize, chain: &mut Vec<(&'a Arc<MDBlock>, &'a Arc<MDBlock>)>) {
            if let Some(child) = block.children.iter().find(|child| child.range.touches(offset)) {
                chain.push((block, child));
                descend(child, offset, chain);
            }
        }
        descend(&doc.root, offset, &mut chain);
        if chain.is_empty() {
            return None;
        }

        let chosen = chain
            .iter()
            .rev()
            .find(|(parent, _)| matches!(parent.content, BlockContent::List { .. } | BlockContent::Document))
            .copied()
            .unwrap_or(chain[0]);
        let index = chosen.0.children.iter().position(|child| Arc::ptr_eq(child, chosen.1))?;
        Some((chosen.0.clone(), index))
    }

    // MARK: Conversion

    /// Converts the lines touched by `range` between paragraph, bullet list,
    /// numbered list, task list and blockquote, line by line.
    pub fn convert(doc: &ParsedDocument, range: NSRange, conversion: ListConversion) -> Vec<TextEdit> {
        let first_line = doc.line_at(0.max(range.location)) - 1;
        let last_line = doc.line_at(range.location.max(range.upper_bound() - 1)) - 1;
        if first_line > last_line {
            return Vec::new();
        }

        let mut edits: Vec<TextEdit> = Vec::new();
        let mut ordinal: isize = 1;
        for line in first_line..=last_line {
            let line_range = doc.range_of_line(line + 1);
            let source = doc.substring(line_range);
            if swift_text::is_blank_line(&source) && conversion != ListConversion::Blockquote {
                continue;
            }

            let indent = swift_text::leading_indent(&source);
            let stripped = BlockMarker::strip(swift_text::drop_first(&source, swift_text::count(indent)));
            let replacement = match conversion {
                ListConversion::Paragraph => format!("{indent}{stripped}"),
                ListConversion::BulletList => format!("{indent}- {stripped}"),
                ListConversion::NumberedList => {
                    let out = format!("{indent}{ordinal}. {stripped}");
                    ordinal += 1;
                    out
                }
                ListConversion::TaskList => format!("{indent}- [ ] {stripped}"),
                ListConversion::Blockquote => {
                    if stripped.is_empty() {
                        format!("{indent}>")
                    } else {
                        format!("{indent}> {stripped}")
                    }
                }
            };
            if swift_text::str_eq(&replacement, &source) {
                continue;
            }
            edits.push(TextEdit::new(line_range, replacement, format!("Convert to {}", conversion.title()), None));
        }
        edits
    }

    // MARK: Sorting

    pub fn sort_list(doc: &ParsedDocument, offset: isize, order: ListSortOrder) -> Vec<TextEdit> {
        let Some(list) = Self::enclosing_list(&doc.root, offset) else { return Vec::new() };
        let BlockContent::List { ordered, start, .. } = list.content else { return Vec::new() };
        let items: Vec<&Arc<MDBlock>> =
            list.children.iter().filter(|child| matches!(child.content, BlockContent::ListItem { .. })).collect();
        if items.len() <= 1 {
            return Vec::new();
        }

        let text = doc.utf16.as_slice();
        let bodies: Vec<String> = items.iter().map(|item| text.substring(item.range)).collect();
        let separators: Vec<String> = items
            .iter()
            .zip(items.iter().skip(1))
            .map(|(previous, next)| {
                text.substring(NSRange::new(
                    previous.range.upper_bound(),
                    0.max(next.range.location - previous.range.upper_bound()),
                ))
            })
            .collect();

        // (text, checked)
        let keys: Vec<(String, Option<bool>)> = items
            .iter()
            .map(|item| {
                let checked = match &item.content {
                    BlockContent::ListItem { checkbox, .. } => checkbox.map(|b| b.is_checked),
                    _ => None,
                };
                (swift_text::lowercased(&PlainText::of(item, text)), checked)
            })
            .collect();

        let identity: Vec<usize> = (0..items.len()).collect();
        let mut order_ = identity.clone();
        match order {
            ListSortOrder::Alphabetical => order_.sort_by(|&a, &b| swift_text::str_cmp(&keys[a].0, &keys[b].0)),
            ListSortOrder::ReverseAlphabetical => order_.sort_by(|&a, &b| swift_text::str_cmp(&keys[b].0, &keys[a].0)),
            ListSortOrder::UncheckedFirst => {
                order_ = Self::stable_sort(&order_, |a, b| Self::rank(keys[a].1, true) < Self::rank(keys[b].1, true))
            }
            ListSortOrder::CheckedFirst => {
                order_ = Self::stable_sort(&order_, |a, b| Self::rank(keys[a].1, false) < Self::rank(keys[b].1, false))
            }
        }
        if order_ == identity {
            return Vec::new();
        }

        let mut result = String::new();
        for (position, &source) in order_.iter().enumerate() {
            let mut body = bodies[source].clone();
            // A sorted ordered list with its original numbers scrambled would
            // be worse than not sorting at all, so renumber as we go.
            if ordered {
                body = Self::renumbered(&body, start + position as isize);
            }
            result.push_str(&body);
            if position < separators.len() {
                result.push_str(&separators[position]);
            }
        }
        let span = NSRange::new(
            items[0].range.location,
            items[items.len() - 1].range.upper_bound() - items[0].range.location,
        );
        if swift_text::str_eq(&result, &text.substring(span)) {
            return Vec::new();
        }
        vec![TextEdit::new(span, result, format!("Sort list ({})", sort_order_raw_value(order)), None)]
    }

    fn rank(checked: Option<bool>, unchecked_first: bool) -> isize {
        let Some(checked) = checked else { return 2 };
        if unchecked_first {
            return if checked { 1 } else { 0 };
        }
        if checked { 0 } else { 1 }
    }

    fn stable_sort(indices: &[usize], less: impl Fn(usize, usize) -> bool) -> Vec<usize> {
        let mut enumerated: Vec<(usize, usize)> = indices.iter().copied().enumerate().collect();
        let before = |a: &(usize, usize), b: &(usize, usize)| less(a.1, b.1) || (!less(b.1, a.1) && a.0 < b.0);
        enumerated.sort_by(|a, b| {
            if before(a, b) {
                std::cmp::Ordering::Less
            } else if before(b, a) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        enumerated.into_iter().map(|(_, element)| element).collect()
    }

    fn renumbered(item: &str, number: isize) -> String {
        let indent = swift_text::leading_indent(item);
        let rest = swift_text::drop_first(item, swift_text::count(indent));
        let digits = prefix_while(rest, swift_text::is_number);
        if digits.is_empty() {
            return item.to_owned();
        }
        format!("{indent}{number}{}", swift_text::drop_first(rest, swift_text::count(digits)))
    }

    fn enclosing_list(block: &Arc<MDBlock>, offset: isize) -> Option<Arc<MDBlock>> {
        let mut found: Option<Arc<MDBlock>> = None;
        block.walk_pruning(&mut |candidate| {
            if !candidate.range.touches(offset) {
                return false;
            }
            if let BlockContent::List { .. } = candidate.content {
                found = Some(candidate.clone());
            }
            true
        });
        found
    }

    // MARK: Table of contents

    /// Markdown bullet list of anchor links, indented by heading depth.
    pub fn table_of_contents(doc: &ParsedDocument, max_level: isize) -> String {
        let headings: Vec<&HeadingNode> =
            doc.headings.iter().filter(|heading| heading.level <= max_level && !heading.title.is_empty()).collect();
        let Some(minimum) = headings.iter().map(|heading| heading.level).min() else { return String::new() };
        let ending = DocumentIO::dominant_line_ending(&doc.text).raw_value();
        headings
            .iter()
            .map(|heading| {
                let indent = swift_text::repeating("  ", heading.level - minimum);
                format!("{indent}- [{}](#{})", Self::escaped(&heading.title), heading.slug)
            })
            .collect::<Vec<String>>()
            .join(ending)
    }

    fn escaped(title: &str) -> String {
        swift_text::replacing_occurrences(&swift_text::replacing_occurrences(title, "[", "\\["), "]", "\\]")
    }

    // MARK: Tasks

    /// A one-character replacement, which keeps the edit trivial to undo and
    /// the click-to-toggle write cheap on a large document.
    pub fn toggle_task(doc: &ParsedDocument, offset: isize) -> Option<TextEdit> {
        let task = doc
            .tasks
            .iter()
            .find(|task| task.mark_range.touches(offset))
            .or_else(|| doc.tasks.iter().find(|task| Self::task_line(doc, task) == doc.line_at(offset)))?;
        Some(TextEdit::new(
            task.mark_range,
            if task.is_checked { " " } else { "x" },
            if task.is_checked { "Uncheck task" } else { "Check task" },
            None,
        ))
    }

    fn task_line(doc: &ParsedDocument, task: &TaskItem) -> isize {
        doc.line_at(task.mark_range.location)
    }

    // MARK: Tables (§6.3)

    pub fn realign_table(doc: &ParsedDocument, table_range: NSRange) -> Vec<TextEdit> {
        let Some((block, table)) = Self::table(doc, table_range) else { return Vec::new() };
        let text = doc.utf16.as_slice();
        let range = TableFormatter::source_range(table, block.range);
        let rendered = TableFormatter::render(&TableFormatter::model(table, text));
        if rendered.is_empty() || swift_text::str_eq(&rendered, &text.substring(range)) {
            return Vec::new();
        }
        vec![TextEdit::new(range, rendered, "Realign table", None)]
    }

    pub fn set_column_alignment(doc: &ParsedDocument, table_range: NSRange, column: isize, alignment: TableAlignment) -> Vec<TextEdit> {
        let Some((block, table)) = Self::table(doc, table_range) else { return Vec::new() };
        let text = doc.utf16.as_slice();
        let mut model = TableFormatter::model(table, text);
        let columns = model.column_count();
        if !(column >= 0 && column < columns) {
            return Vec::new();
        }
        while (model.alignments.len() as isize) < columns {
            model.alignments.push(TableAlignment::None);
        }
        if model.alignments[column as usize] == alignment {
            return Vec::new();
        }
        model.alignments[column as usize] = alignment;

        let range = TableFormatter::source_range(table, block.range);
        vec![TextEdit::new(
            range,
            TableFormatter::render(&model),
            format!("Align column {} {}", column + 1, alignment.raw_value()),
            None,
        )]
    }

    /// Inserts an empty row after `after_row`, an index into `TableData.rows`
    /// where 0 is the header. A negative index inserts as the first body row.
    pub fn insert_row(doc: &ParsedDocument, table_range: NSRange, after_row: isize) -> Vec<TextEdit> {
        let Some((block, table)) = Self::table(doc, table_range) else { return Vec::new() };
        let text = doc.utf16.as_slice();
        let mut model = TableFormatter::model(table, text);
        let position = (model.rows.len() as isize).min(1.max(after_row + 1));
        let columns = model.column_count();
        model.rows.insert(position as usize, vec![String::new(); columns as usize]);

        let range = TableFormatter::source_range(table, block.range);
        vec![TextEdit::new(range, TableFormatter::render(&model), "Insert table row", None)]
    }

    /// Deletes `row` (an index into `TableData.rows`). Refuses to delete the
    /// header, because a GFM table without one is not a table.
    pub fn delete_row(doc: &ParsedDocument, table_range: NSRange, row: isize) -> Vec<TextEdit> {
        let Some((block, table)) = Self::table(doc, table_range) else { return Vec::new() };
        let text = doc.utf16.as_slice();
        let mut model = TableFormatter::model(table, text);
        if !(row > 0 && row < model.rows.len() as isize) {
            return Vec::new();
        }
        model.rows.remove(row as usize);

        let range = TableFormatter::source_range(table, block.range);
        vec![TextEdit::new(range, TableFormatter::render(&model), "Delete table row", None)]
    }

    fn table(doc: &ParsedDocument, range: NSRange) -> Option<(&Arc<MDBlock>, &TableData)> {
        let mut result: Option<(&Arc<MDBlock>, &TableData)> = None;
        doc.root.walk(&mut |block| {
            if result.is_some() {
                return;
            }
            let BlockContent::Table(data) = &block.content else { return };
            let full = TableFormatter::source_range(data, block.range);
            if full.location < range.upper_bound().max(range.location + 1) && range.location < full.upper_bound() {
                result = Some((block, data));
            }
        });
        result
    }

    // MARK: Tasks

    /// Adds `- [ ] text` after the last task of the section `heading_index`
    /// names — after that task's whole *block*, so an anchor with nested
    /// children is never split from its family. With no matching task the
    /// new task starts its own list at the end of the document, behind exactly
    /// one blank line (none when the file is empty or already ends blank).
    pub fn insert_task(doc: &ParsedDocument, text: &str, heading_index: Option<isize>) -> Vec<TextEdit> {
        let trimmed = swift_text::trim_whitespaces_and_newlines(text);
        if trimmed.is_empty() {
            return Vec::new();
        }

        if let Some(anchor) = doc.tasks.iter().rev().find(|task| task.heading_index == heading_index) {
            let block = Self::task_block(doc, doc.line_at(anchor.mark_range.location));
            // A block that ran to EOF without a trailing newline has no line
            // start after it, so the separator goes in front instead.
            let needs_leading_newline = block.upper_bound() >= doc.length && !swift_text::has_suffix(&doc.text, "\n");
            return vec![TextEdit::new(
                NSRange::new(block.upper_bound(), 0),
                if needs_leading_newline { format!("\n- [ ] {trimmed}\n") } else { format!("- [ ] {trimmed}\n") },
                "Add task",
                None,
            )];
        }

        // No anchor: append at the end of the document, leaving exactly one
        // blank line between the new list and whatever precedes it.
        let replacement = if doc.length == 0 || swift_text::has_suffix(&doc.text, "\n\n") {
            format!("- [ ] {trimmed}\n")
        } else if swift_text::has_suffix(&doc.text, "\n") {
            format!("\n- [ ] {trimmed}\n")
        } else {
            format!("\n\n- [ ] {trimmed}\n")
        };
        vec![TextEdit::new(NSRange::new(doc.length, 0), replacement, "Add task", None)]
    }

    /// Moves `task_index` among its siblings — same section, same indent
    /// level, same parent — so it lands immediately before `target_index`, or
    /// after the last sibling when `target_index` is `None`. Crossing a
    /// section or a nesting level is a re-parenting and is refused.
    ///
    /// The result is two edits in original coordinates — cut the block, paste
    /// its text at the destination — that apply cleanly back to front. A block
    /// that ends the file without a `\n` borrows the newline before it, and a
    /// paste aimed at an unterminated EOF brings its own leading `\n`.
    pub fn move_task(doc: &ParsedDocument, task_index: isize, target_index: Option<isize>) -> Vec<TextEdit> {
        let tasks = &doc.tasks;
        if !contains_index(tasks, task_index) {
            return Vec::new();
        }
        if let Some(target_index) = target_index
            && target_index == task_index
        {
            return Vec::new();
        }

        let source = &tasks[task_index as usize];
        let source_parent = Self::task_parent(task_index, tasks);
        let is_sibling = |index: isize| -> bool {
            let task = &tasks[index as usize];
            task.heading_index == source.heading_index
                && task.indent_level == source.indent_level
                && Self::task_parent(index, tasks) == source_parent
        };

        let insertion = if let Some(target_index) = target_index {
            if !(contains_index(tasks, target_index) && is_sibling(target_index)) {
                return Vec::new();
            }
            doc.line_starts[(doc.line_at(tasks[target_index as usize].mark_range.location) - 1) as usize]
        } else {
            // After the last sibling's block — unless the source already is
            // that sibling, which is where the move would put it.
            let Some(last) = (0..tasks.len() as isize).filter(|&index| is_sibling(index)).last() else { return Vec::new() };
            if last == task_index {
                return Vec::new();
            }
            Self::task_block(doc, doc.line_at(tasks[last as usize].mark_range.location)).upper_bound()
        };

        let block = Self::task_block(doc, doc.line_at(source.mark_range.location));
        if insertion == block.location {
            return Vec::new(); // already in position
        }

        let mut cut = block;
        let mut paste = doc.substring(block);
        if swift_text::has_suffix(&paste, "\n") {
            // Landing at an EOF with no trailing newline: the separator goes
            // in front and the block leaves its own terminator where it was.
            if insertion >= doc.length && !swift_text::has_suffix(&doc.text, "\n") {
                paste = format!("\n{}", swift_text::drop_last(&paste, 1));
            }
        } else {
            // The block ends the file without a newline: cut the separator
            // before it too, and restore the terminator on the pasted copy.
            if block.location > 0 {
                cut = NSRange::new(block.location - 1, block.length + 1);
            }
            paste.push('\n');
        }

        vec![
            TextEdit::new(cut, "", "Move task", None),
            TextEdit::new(NSRange::new(insertion, 0), paste, "Move task", None),
        ]
    }

    /// The block a task owns for insertion and moving: its own line plus every
    /// following non-blank line indented deeper than it. The range spans whole
    /// lines, trailing newline included.
    fn task_block(doc: &ParsedDocument, line: isize) -> NSRange {
        let start = doc.line_starts[(line - 1) as usize];
        let indent = swift_text::count(swift_text::leading_indent(&doc.substring(doc.range_of_line(line))));
        let mut next = line + 1;
        while next <= doc.line_starts.len() as isize {
            let source = doc.substring(doc.range_of_line(next));
            if swift_text::is_blank_line(&source) || swift_text::count(swift_text::leading_indent(&source)) <= indent {
                break;
            }
            next += 1;
        }
        let end = if next <= doc.line_starts.len() as isize { doc.line_starts[(next - 1) as usize] } else { doc.length };
        NSRange::new(start, end - start)
    }

    /// A task's parent for sibling tests: the nearest preceding task in
    /// document order that sits shallower in the same section.
    fn task_parent(index: isize, tasks: &[TaskItem]) -> Option<isize> {
        let task = &tasks[index as usize];
        let mut candidate = index - 1;
        while candidate >= 0 {
            let preceding = &tasks[candidate as usize];
            if preceding.heading_index == task.heading_index && preceding.indent_level < task.indent_level {
                return Some(candidate);
            }
            candidate -= 1;
        }
        None
    }
}

/// A section's content, its terminator and the blank run that trails it.
/// Terminators are recognised by width, so a move through a CRLF,
/// classic-Mac, or mixed file keeps the bytes it found.
struct SectionSplit {
    core: String,
    is_terminated: bool,
    trailing_blanks: isize,
    /// The exact terminator bytes of the trailing blank run, in document
    /// order.
    trailing_separator: String,
}

impl SectionSplit {
    fn new(section: NSRange, text: &[u16]) -> SectionSplit {
        let mut end = section.upper_bound();
        let mut stripped: Vec<&'static str> = Vec::new();
        while let Some(terminator) = Restructure::terminator(end, text)
            && end - terminator.len() as isize >= section.location
        {
            end -= terminator.len() as isize;
            stripped.push(terminator);
        }
        let is_terminated = !stripped.is_empty();
        // `stripped` is in reverse document order; its last element is the
        // terminator directly after the content.
        let core = if let Some(own) = stripped.last() {
            text.substring(NSRange::new(section.location, end + own.len() as isize - section.location))
        } else {
            text.substring(NSRange::new(section.location, end - section.location))
        };
        let own_count = if is_terminated { 1 } else { 0 };
        let trailing_blanks = 0.max(stripped.len() as isize - own_count);
        let trailing_separator: String = stripped[..stripped.len() - own_count as usize].iter().rev().copied().collect();
        SectionSplit { core, is_terminated, trailing_blanks, trailing_separator }
    }

    /// The content with a line terminator, for pasting somewhere that is not
    /// the end of the document.
    fn terminated_core(&self, ending: &str) -> String {
        if self.is_terminated {
            return self.core.clone();
        }
        if swift_text::has_suffix(&self.core, "\n") || swift_text::has_suffix(&self.core, "\r") {
            self.core.clone()
        } else {
            self.core.clone() + ending
        }
    }
}

// MARK: - Block markers

pub struct BlockMarker;

impl BlockMarker {
    /// Removes a leading block marker — `#`, `>`, `-`, `1.`, `- [ ]` —
    /// leaving the line's content. Used by conversion and list continuation.
    pub fn strip(line: &str) -> String {
        use swift_text::{char_is, count, drop_first, first, has_prefix};
        let mut rest: &str = line;
        let mut changed = true;
        while changed {
            changed = false;
            if has_prefix(rest, ">") {
                rest = drop_first(rest, 1);
                if has_prefix(rest, " ") {
                    rest = drop_first(rest, 1);
                }
                changed = true;
                continue;
            }
            let hashes = prefix_while(rest, |g| char_is(g, '#'));
            let hash_count = count(hashes);
            if !hashes.is_empty() && hash_count <= 6 && has_prefix(drop_first(rest, hash_count), " ") {
                rest = drop_first(rest, hash_count + 1);
                changed = true;
                continue;
            }
            if let Some(bullet) = first(rest)
                && (char_is(bullet, '-') || char_is(bullet, '*') || char_is(bullet, '+'))
                && has_prefix(drop_first(rest, 1), " ")
            {
                rest = drop_first(rest, 2);
                changed = true;
            } else {
                let digits = prefix_while(rest, swift_text::is_number);
                if !digits.is_empty() {
                    let after = drop_first(rest, count(digits));
                    if let Some(punctuation) = first(after)
                        && (char_is(punctuation, '.') || char_is(punctuation, ')'))
                        && has_prefix(drop_first(after, 1), " ")
                    {
                        rest = drop_first(after, 2);
                        changed = true;
                    }
                }
            }
            if changed {
                continue;
            }
            // A task marker only ever follows a bullet, which the branch above
            // has already consumed by the time we get here.
            break;
        }
        if has_prefix(rest, "[ ] ") || has_prefix(rest, "[x] ") || has_prefix(rest, "[X] ") {
            rest = drop_first(rest, 4);
        }
        rest.to_owned()
    }
}
