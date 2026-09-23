//! Derived.swift — the line-level source scan, the derived outline / task /
//! footnote structures, and heading slugs.

use std::collections::HashMap;
use std::sync::Arc;

use crate::metrics::{Metrics, PlainText};
use crate::model::{BlockContent, BlockRef, Checkbox, HeadingNode, LinkReference, MDBlock, TaskItem};
use crate::ns_range::NSRange;
use crate::source_positions::SourceMap;
use crate::swift_text::{self, CharSet, ns::NSStringExt};

// MARK: - Line-level source scan

/// Recovers link reference definitions and footnote definitions from the
/// source, with fence tracking so a definition inside a code block is ignored.
#[derive(Clone, Debug, Default)]
pub struct SourceScanner {
    pub footnote_definitions: Vec<FootnoteDefinition>,
    pub link_references: HashMap<String, LinkReference>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FootnoteDefinition {
    pub identifier: String,
    /// `[^id]: ` including the trailing space.
    pub marker_range: NSRange,
    /// The definition line plus any indented continuation lines.
    pub range: NSRange,
}

impl SourceScanner {
    pub fn new(map: &SourceMap) -> SourceScanner {
        let mut scanner = SourceScanner::default();
        let text = map.text.as_slice();
        let mut line: isize = 0;
        // The open fence's character and run length.
        let mut fence: Option<(u16, isize)> = None;
        while line < map.line_count() {
            // `defer { line += 1 }`
            let current = line;
            line += 1;

            let line_range = map.content_range_of_line(current);
            let start = line_range.location;
            let end = line_range.upper_bound();

            let mut trimmed_start = start;
            let mut indent_columns: isize = 0;
            while trimmed_start < end {
                let c = text.character_at(trimmed_start);
                if c == 0x20 {
                    trimmed_start += 1;
                    indent_columns += 1;
                } else if c == 0x09 {
                    trimmed_start += 1;
                    indent_columns += 4 - (indent_columns % 4);
                } else {
                    break;
                }
            }
            let trimmed_length = end - trimmed_start;

            if let Some(open) = fence {
                if Self::is_closing_fence_line(text, trimmed_start, trimmed_length, open) {
                    fence = None;
                }
                continue;
            }
            if let Some(backticks) = Self::fence_run(text, trimmed_start, trimmed_length, 0x60)
                && backticks >= 3
            {
                fence = Some((0x60, backticks));
                continue;
            }
            if let Some(tildes) = Self::fence_run(text, trimmed_start, trimmed_length, 0x7E)
                && tildes >= 3
            {
                fence = Some((0x7E, tildes));
                continue;
            }
            if !(indent_columns < 4 && trimmed_length > 0 && text.character_at(trimmed_start) == 0x5B) {
                continue;
            }

            let Some(close) = Self::closing_bracket(text, trimmed_start, end) else { continue };
            if !(close + 1 < end && text.character_at(close + 1) == 0x3A) {
                continue;
            }

            let label_start = trimmed_start + 1;
            if !(close > label_start) {
                continue;
            }
            let label = text.substring(NSRange::new(label_start, close - label_start));
            if label.is_empty() {
                continue;
            }
            let marker_length = (close - trimmed_start) + 2;
            let indent = trimmed_start - start;
            let body_start = line_range.location + indent + marker_length;
            let clamped = body_start.min(end);
            let body = text.substring(NSRange::new(clamped, 0.max(end - clamped)));

            if swift_text::has_prefix(&label, "^") {
                let identifier = swift_text::drop_first(&label, 1).to_owned();
                if identifier.is_empty() {
                    continue;
                }
                let mut last = current;
                while last + 1 < map.line_count() && Self::is_indented_continuation(map, last + 1) {
                    last += 1;
                }
                let end_ = map.content_range_of_line(last).upper_bound();
                let leading_space = if swift_text::has_prefix(&body, " ") { 1 } else { 0 };
                scanner.footnote_definitions.push(FootnoteDefinition {
                    identifier,
                    marker_range: NSRange::new(line_range.location, body_start + leading_space - line_range.location),
                    range: NSRange::new(line_range.location, 0.max(end_ - line_range.location)),
                });
                // `line = last`, then the deferred increment.
                line = last + 1;
                continue;
            }

            let (destination, title) = Self::destination_and_title(&body);
            if destination.is_empty() {
                continue;
            }
            swift_text::dict_insert(
                &mut scanner.link_references,
                swift_text::lowercased(&label),
                LinkReference::new(label, destination, title, line_range),
            );
        }
        scanner
    }

    fn fence_run(text: &[u16], offset: isize, length: isize, character: u16) -> Option<isize> {
        let mut run = 0;
        while run < length && text.character_at(offset + run) == character {
            run += 1;
        }
        if run > 0 { Some(run) } else { None }
    }

    fn is_closing_fence_line(text: &[u16], offset: isize, length: isize, fence: (u16, isize)) -> bool {
        let Some(run) = Self::fence_run(text, offset, length, fence.0) else { return false };
        if !(run >= 3 && run >= fence.1) {
            return false;
        }
        let mut index = offset + run;
        let end = offset + length;
        while index < end {
            let c = text.character_at(index);
            if c != 0x20 && c != 0x09 {
                return false;
            }
            index += 1;
        }
        true
    }

    fn closing_bracket(text: &[u16], start: isize, end: isize) -> Option<isize> {
        let mut depth = 0;
        let mut index = start;
        while index < end {
            match text.character_at(index) {
                0x5B => depth += 1,
                0x5D => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                0x5C => index += 1,
                _ => {}
            }
            index += 1;
        }
        None
    }

    /// Non-blank and indented at least four columns.
    fn is_indented_continuation(map: &SourceMap, line: isize) -> bool {
        let range = map.content_range_of_line(line);
        let text = map.text.as_slice();
        let mut columns = 0;
        let mut saw_content = false;
        let mut index = range.location;
        let end = range.upper_bound();
        while index < end {
            let c = text.character_at(index);
            if c == 0x20 {
                columns += 1;
            } else if c == 0x09 {
                columns += 4 - (columns % 4);
            } else {
                saw_content = true;
                break;
            }
            index += 1;
        }
        if !saw_content {
            return false;
        }
        columns >= 4
    }

    fn destination_and_title(body: &str) -> (String, Option<String>) {
        let angle = CharSet::Chars("<>");
        let trimmed = swift_text::trim_whitespaces(body);
        let Some(space) = swift_text::first_index_where(trimmed, |g| g == " " || g == "\t") else {
            return (swift_text::trimming(trimmed, angle).to_owned(), None);
        };
        let destination = swift_text::trimming(&trimmed[..space], angle).to_owned();
        let mut title = swift_text::trim_whitespaces(&trimmed[space..]).to_owned();
        if swift_text::count(&title) >= 2
            && let (Some(first), Some(last)) = (swift_text::first(&title), swift_text::last(&title))
            && ((swift_text::char_is(first, '"') && swift_text::char_is(last, '"'))
                || (swift_text::char_is(first, '\'') && swift_text::char_is(last, '\''))
                || (swift_text::char_is(first, '(') && swift_text::char_is(last, ')')))
        {
            title = swift_text::drop_last(swift_text::drop_first(&title, 1), 1).to_owned();
        }
        let title = if title.is_empty() { None } else { Some(title) };
        (destination, title)
    }
}

// MARK: - Derived structures

/// Outline, task list and footnote index, computed in one walk.
#[derive(Clone, Debug, Default)]
pub struct DerivedStructures {
    pub headings: Vec<HeadingNode>,
    pub tasks: Vec<TaskItem>,
    pub footnotes: HashMap<String, BlockRef>,
}

impl DerivedStructures {
    pub fn new(root: &Arc<MDBlock>, map: &SourceMap) -> DerivedStructures {
        let mut derived = DerivedStructures::default();
        let mut heading_blocks: Vec<BlockRef> = Vec::new();
        let mut task_items: Vec<(BlockRef, isize)> = Vec::new();

        fn walk(
            block: &BlockRef,
            list_depth: isize,
            heading_blocks: &mut Vec<BlockRef>,
            task_items: &mut Vec<(BlockRef, isize)>,
            footnotes: &mut HashMap<String, BlockRef>,
        ) {
            match &block.content {
                BlockContent::Heading { .. } => heading_blocks.push(block.clone()),
                BlockContent::ListItem { checkbox: Some(_), .. } => task_items.push((block.clone(), 0.max(list_depth - 1))),
                BlockContent::FootnoteDefinition { identifier } => {
                    swift_text::dict_insert(footnotes, identifier.clone(), block.clone());
                }
                _ => {}
            }
            let next_depth = if block.content.is_list() { list_depth + 1 } else { list_depth };
            for child in &block.children {
                walk(child, next_depth, heading_blocks, task_items, footnotes);
            }
        }
        for child in &root.children {
            walk(child, 0, &mut heading_blocks, &mut task_items, &mut derived.footnotes);
        }

        derived.headings = Self::outline(&heading_blocks, root, map);
        let text = map.text.as_slice();
        derived.tasks = task_items
            .iter()
            .map(|(block, indent)| {
                let checkbox = match &block.content {
                    BlockContent::ListItem { checkbox: Some(box_), .. } => *box_,
                    _ => Checkbox::new(false, block.range),
                };
                TaskItem::new(
                    checkbox.is_checked,
                    checkbox.mark_range,
                    block.content_range,
                    Self::task_label(block, text),
                    Self::heading_index(block.range.location, &derived.headings),
                    *indent,
                )
            })
            .collect();
        derived
    }

    fn heading_index(location: isize, headings: &[HeadingNode]) -> Option<isize> {
        if headings.is_empty() {
            return None;
        }
        let (mut lo, mut hi, mut best) = (0isize, headings.len() as isize - 1, -1isize);
        while lo <= hi {
            let mid = (lo + hi) / 2;
            if headings[mid as usize].range.location < location {
                best = mid;
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        if best >= 0 { Some(best) } else { None }
    }

    /// A task's label is its own source line, from just after the `[ ]`
    /// marker to that line's terminator.
    fn task_label(block: &MDBlock, text: &[u16]) -> String {
        let line = text.line_range_for(NSRange::new(block.range.location, 0));
        let start = line.location.max(block.content_range.location).min(line.upper_bound());
        if !(line.upper_bound() > start) {
            return String::new();
        }
        let label = text.substring(NSRange::new(start, line.upper_bound() - start));
        Self::plain_inline_text(&label)
    }

    /// Strips the light markdown that commonly decorates a task label.
    fn plain_inline_text(source: &str) -> String {
        let trimmed = swift_text::trim_whitespaces_and_newlines(source);
        let mut text = swift_text::replacing_occurrences(trimmed, "**", "");
        text = swift_text::replacing_occurrences(&text, "__", "");
        text = swift_text::replacing_occurrences(&text, "*", "");
        text = swift_text::replacing_occurrences(&text, "`", "");
        swift_text::trim_whitespaces(&text).to_owned()
    }

    fn outline(blocks: &[BlockRef], root: &Arc<MDBlock>, map: &SourceMap) -> Vec<HeadingNode> {
        let mut nodes: Vec<HeadingNode> = Vec::new();
        let mut slugs: HashMap<String, isize> = HashMap::new();
        if blocks.is_empty() {
            return Vec::new();
        }
        let text = map.text.as_slice();

        // A section runs until the next heading at the same level or above,
        // found for every heading with a monotonic stack.
        let mut section_ends: Vec<isize> = vec![map.length; blocks.len()];
        let mut stack: Vec<(isize, isize)> = Vec::new(); // (level, lineStart)
        for index in (0..blocks.len()).rev() {
            let BlockContent::Heading { level } = blocks[index].content else { continue };
            while let Some(&(top_level, _)) = stack.last()
                && top_level > level
            {
                stack.pop();
            }
            section_ends[index] = stack.last().map_or(map.length, |&(_, line_start)| line_start);
            stack.push((level, text.line_start_before(blocks[index].range.location)));
        }

        // Each heading's own-prose word count, in one tree walk.
        let mut spans: Vec<NSRange> = Vec::with_capacity(blocks.len());
        for (index, block) in blocks.iter().enumerate() {
            let own_end = if index + 1 < blocks.len() {
                text.line_start_before(blocks[index + 1].range.location)
            } else {
                map.length
            };
            spans.push(NSRange::new(block.range.upper_bound(), 0.max(own_end - block.range.upper_bound())));
        }
        let word_counts: Vec<isize> =
            PlainText::prose_per_section(root, &spans, text).iter().map(|prose| Metrics::word_count(prose)).collect();

        for (index, block) in blocks.iter().enumerate() {
            let BlockContent::Heading { level } = block.content else { continue };
            let line_start = text.line_start_before(block.range.location);

            let title = swift_text::trim_whitespaces_and_newlines(&PlainText::of(block, text)).to_owned();
            let base = Slug::make(&title);
            let count = *swift_text::dict_get(&slugs, &base).unwrap_or(&0);
            swift_text::dict_insert(&mut slugs, base.clone(), count + 1);

            let mut node = HeadingNode::new(
                level,
                title,
                block.range,
                block.content_range,
                NSRange::new(line_start, 0.max(section_ends[index] - line_start)),
            );
            node.slug = if count == 0 { base } else { format!("{base}-{count}") };
            node.word_count = word_counts[index];
            nodes.push(node);
        }

        // Parent/child links, resolved once the levels are all known.
        let mut parent_stack: Vec<usize> = Vec::new();
        for index in 0..nodes.len() {
            while let Some(&top) = parent_stack.last()
                && nodes[top].level >= nodes[index].level
            {
                parent_stack.pop();
            }
            if let Some(&parent) = parent_stack.last() {
                nodes[index].parent_index = Some(parent as isize);
                nodes[parent].child_indices.push(index as isize);
            }
            parent_stack.push(index);
        }
        nodes
    }
}

// MARK: - Slugs

pub struct Slug;

impl Slug {
    /// GitHub's anchor rules: lowercase, drop everything that isn't a letter,
    /// number, space or hyphen, then spaces become hyphens.
    pub fn make(title: &str) -> String {
        let lowered = swift_text::lowercased(title);
        let mut out = String::with_capacity(lowered.len());
        for character in swift_text::graphemes(&lowered) {
            if swift_text::is_letter(character) || swift_text::is_number(character) {
                out.push_str(character);
            } else if character == " " || character == "-" || character == "_" {
                out.push('-');
            }
        }
        let mut start = 0;
        let mut end = out.len();
        {
            let mut it = swift_text::graphemes(&out);
            while start < end {
                match it.next() {
                    Some(g) if g == "-" => start += 1,
                    _ => break,
                }
            }
        }
        {
            let mut it = swift_text::graphemes(&out[start..]);
            while end > start {
                match it.next_back() {
                    Some(g) if g == "-" => end -= 1,
                    _ => break,
                }
            }
        }
        out[start..end].to_owned()
    }
}
