//! The GFM table extension's row splitting (`row_from_string` in cmark-gfm's
//! `extensions/table.c`), for the pulldown-cmark adapter.
//!
//! pulldown-cmark decides which lines form a table and parses each cell's
//! inlines; the cell offsets, their columns, the column and row spans
//! (`CMARK_OPT_TABLE_SPANS`, which swift-markdown sets) and the text a cell's
//! inline positions are measured in come from this port.

use crate::nodes::tables::ColumnAlignment;

/// One cell of a row (`node_cell` plus `node_cell_data`).
#[derive(Clone, Debug)]
pub(crate) struct Cell {
    /// Offset of the cell in the row string, just after the `|` before it.
    pub(crate) start_offset: usize,
    /// Offset of the cell's last byte (or of the `|` after an empty cell).
    pub(crate) end_offset: usize,
    /// Bytes of whitespace between `start_offset` and the content.
    pub(crate) internal_offset: usize,
    /// Offset where the cell's content starts (after the pipe and spaces).
    pub(crate) content_start: usize,
    /// Offset where the matched content ends (exclusive).
    pub(crate) content_end: usize,
    pub(crate) colspan: u64,
    /// 0 for a row-span marker: content that is exactly `^` once trimmed
    /// and unescaped.
    pub(crate) rowspan: u64,
}

/// A parsed row (`table_row`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Row {
    pub(crate) cells: Vec<Cell>,
    pub(crate) paragraph_offset: usize,
}

fn is_spacechar(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | 0x0B | 0x0C)
}

fn is_cmark_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// `scan_table_cell_end`: `[|] spacechar*`.
fn cell_end(string: &[u8], offset: usize) -> usize {
    if offset >= string.len() || string[offset] != b'|' {
        return 0;
    }
    let mut end = offset + 1;
    while end < string.len() && is_spacechar(string[end]) {
        end += 1;
    }
    end - offset
}

/// `scan_table_cell`: the longest prefix matching
/// `(escaped_char | [^|\r\n])+`. A `|` continues the cell when the byte
/// before it is a backslash.
fn cell(string: &[u8], offset: usize) -> usize {
    // Any byte but a pipe or a line ending matches `[^|\r\n]`, including a
    // backslash, so the longest match runs through every `\|` (the
    // backslash, then `escaped_char`) and stops at the first other pipe.
    let mut end = offset;
    while end < string.len() {
        match string[end] {
            b'\r' | b'\n' => break,
            b'|' if !(end > offset && string[end - 1] == b'\\') => break,
            _ => end += 1,
        }
    }
    end - offset
}

/// `scan_table_row_end`: `spacechar* [\r]? [\n]`.
fn row_end(string: &[u8], offset: usize) -> usize {
    if offset >= string.len() {
        return 0;
    }
    let mut end = offset;
    while end < string.len() && is_spacechar(string[end]) {
        end += 1;
    }
    if end < string.len() && string[end] == b'\r' {
        end += 1;
    }
    if end < string.len() && string[end] == b'\n' {
        end + 1 - offset
    } else {
        0
    }
}

/// `unescape_pipes` then `cmark_strbuf_trim`, reduced to what the adapter
/// needs: whether the result is exactly `^`.
fn is_caret(string: &[u8]) -> bool {
    let mut unescaped = Vec::with_capacity(string.len());
    let mut index = 0;
    while index < string.len() {
        if string[index] == b'\\' && string.get(index + 1) == Some(&b'|') {
            index += 1;
        }
        unescaped.push(string[index]);
        index += 1;
    }
    let trimmed: &[u8] = {
        let mut start = 0;
        let mut end = unescaped.len();
        while start < end && is_cmark_space(unescaped[start]) {
            start += 1;
        }
        while end > start && is_cmark_space(unescaped[end - 1]) {
            end -= 1;
        }
        &unescaped[start..end]
    };
    trimmed == b"^"
}

/// `row_from_string` with `CMARK_OPT_TABLE_SPANS`. `string` must end with a
/// line ending, as cmark's line buffers do.
pub(crate) fn row_from_string(string: &[u8]) -> Option<Row> {
    let len = string.len();
    let mut row = Row::default();
    let mut expect_more_cells = true;

    let mut offset = cell_end(string, 0);
    while offset < len && expect_more_cells {
        let cell_matched = cell(string, offset);
        let pipe_matched = cell_end(string, offset + cell_matched);

        if cell_matched > 0 || pipe_matched > 0 {
            let mut start_offset = offset;
            let end_offset = if cell_matched > 0 {
                offset + cell_matched - 1
            } else {
                offset
            };
            let mut internal_offset = 0;
            while start_offset > row.paragraph_offset && string[start_offset - 1] != b'|' {
                start_offset -= 1;
                internal_offset += 1;
            }
            let content = &string[offset..offset + cell_matched];
            let empty = content.iter().all(|&byte| is_cmark_space(byte));
            let mut colspan = 1;
            // `append_row_cell` has already counted this cell, so even a first
            // cell can span.
            if empty && start_offset == end_offset {
                colspan = 0;
                if let Some(spanning) = row.cells.iter_mut().rev().find(|cell| cell.colspan > 0) {
                    spanning.colspan += 1;
                }
            }
            let is_rowspan_marker = is_caret(content);
            row.cells.push(Cell {
                start_offset,
                end_offset,
                internal_offset,
                content_start: offset,
                content_end: offset + cell_matched,
                colspan,
                rowspan: if is_rowspan_marker { 0 } else { 1 },
            });
        }

        offset += cell_matched + pipe_matched;

        if pipe_matched > 0 {
            expect_more_cells = true;
        } else {
            let end = row_end(string, offset);
            offset += end;
            if end > 0 && offset != len {
                row.paragraph_offset = offset;
                row.cells.clear();
                offset += cell_end(string, offset);
                expect_more_cells = true;
            } else {
                expect_more_cells = false;
            }
        }
    }

    if offset != len || row.cells.is_empty() {
        return None;
    }
    Some(row)
}

/// The column alignments of a delimiter row, as `try_opening_table_header`
/// reads them from each cell's first and last byte.
pub(crate) fn alignments(string: &[u8], row: &Row) -> Vec<Option<ColumnAlignment>> {
    row.cells
        .iter()
        .map(|cell| {
            let content = &string[cell.content_start..cell.content_end];
            let mut start = 0;
            let mut end = content.len();
            while start < end && is_cmark_space(content[start]) {
                start += 1;
            }
            while end > start && is_cmark_space(content[end - 1]) {
                end -= 1;
            }
            let trimmed = &content[start..end];
            let left = trimmed.first() == Some(&b':');
            let right = trimmed.last() == Some(&b':');
            match (left, right) {
                (true, true) => Some(ColumnAlignment::Center),
                (true, false) => Some(ColumnAlignment::Left),
                (false, true) => Some(ColumnAlignment::Right),
                (false, false) => None,
            }
        })
        .collect()
}
