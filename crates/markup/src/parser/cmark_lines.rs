//! cmark-gfm's view of the source lines, for the pulldown-cmark adapter.
//!
//! cmark reports positions in terms of the lines it processed: 1-based line
//! numbers, 1-based byte columns, and each line's length without its line
//! ending (`last_line_length`). Block containers strip their prefixes (`>`,
//! a list item's indentation) from each line before its text reaches a
//! paragraph, and inline positions are measured in that stripped text. This
//! module reproduces the line table and the prefix matching of
//! `check_open_blocks` (`S_find_first_nonspace`, `S_advance_offset`,
//! `parse_block_quote_prefix`, `parse_node_item_prefix` in `blocks.c`).

const TAB_STOP: usize = 4;

/// The lines cmark splits the source into (`S_parser_feed`): each ends at
/// `\n`, `\r\n` or `\r`, and a last line without an ending counts too.
pub(crate) struct SourceLines {
    /// Byte offset where each line starts.
    starts: Vec<usize>,
    /// Byte offset where each line's content ends (before its line ending).
    ends: Vec<usize>,
    /// `last_line_length` for each line: its length without the line ending,
    /// or less for an ATX heading's line, which `chop_trailing_hashtags`
    /// trims in place.
    lengths: Vec<u32>,
    /// The line of the last lookup: lookups mostly move forward a line at a
    /// time.
    cursor: std::cell::Cell<usize>,
}

impl SourceLines {
    pub(crate) fn new(bytes: &[u8]) -> SourceLines {
        let estimate = bytes.len() / 40 + 1;
        let mut starts = Vec::with_capacity(estimate);
        let mut ends = Vec::with_capacity(estimate);
        let mut start = 0;
        let mut position = 0;
        while let Some(found) = memchr_line_end(&bytes[position..]) {
            let end = position + found;
            starts.push(start);
            ends.push(end);
            position = if bytes[end] == b'\r' && bytes.get(end + 1) == Some(&b'\n') {
                end + 2
            } else {
                end + 1
            };
            start = position;
        }
        if start < bytes.len() {
            starts.push(start);
            ends.push(bytes.len());
        }
        let lengths = starts
            .iter()
            .zip(&ends)
            .map(|(start, end)| (end - start) as u32)
            .collect();
        SourceLines {
            starts,
            ends,
            lengths,
            cursor: std::cell::Cell::new(0),
        }
    }

    /// The number of lines cmark processed (`parser->line_number` at the end).
    pub(crate) fn count(&self) -> usize {
        self.starts.len()
    }

    /// The 0-based line containing `offset`. An offset inside a line ending
    /// belongs to that line; the end of the text belongs to the last line.
    pub(crate) fn line_of(&self, offset: usize) -> usize {
        let count = self.starts.len();
        let hint = self.cursor.get();
        if hint < count && self.starts[hint] <= offset {
            if hint + 1 == count || offset < self.starts[hint + 1] {
                return hint;
            }
            if hint + 2 == count || offset < self.starts[hint + 2] {
                self.cursor.set(hint + 1);
                return hint + 1;
            }
        }
        let line = match self.starts.binary_search(&offset) {
            Ok(line) => line,
            Err(0) => 0,
            Err(next) => next - 1,
        };
        self.cursor.set(line);
        line
    }

    pub(crate) fn start(&self, line: usize) -> usize {
        self.starts[line]
    }

    /// The end of the line's content, before its line ending.
    pub(crate) fn end(&self, line: usize) -> usize {
        self.ends[line]
    }

    /// cmark's `last_line_length` for the line: its length without the line
    /// ending.
    pub(crate) fn len(&self, line: usize) -> usize {
        self.lengths[line] as usize
    }

    /// The line's full length (`curline.size` without the line ending), which
    /// `finalize()` uses for a block ending on the line being processed.
    pub(crate) fn raw_len(&self, line: usize) -> usize {
        self.ends[line] - self.starts[line]
    }

    /// Records that cmark trimmed the line to `length` bytes.
    pub(crate) fn set_len(&mut self, line: usize, length: usize) {
        self.lengths[line] = length as u32;
    }

}

fn memchr_line_end(bytes: &[u8]) -> Option<usize> {
    memchr::memchr2(b'\n', b'\r', bytes)
}

/// A container that consumes a prefix of each line it continues on.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Prefix {
    BlockQuote,
    /// A list item: `marker_offset + padding` columns of indentation, except
    /// on the line with its marker (`first_line`), where the marker and its
    /// padding are the prefix.
    Item { width: usize, first_line: usize },
}

/// cmark's per-line scanning state: `offset` (bytes), `column` (tab-expanded)
/// and `partially_consumed_tab`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LineScan {
    pub(crate) offset: usize,
    pub(crate) column: usize,
    pub(crate) partially_consumed_tab: bool,
}

/// `S_find_first_nonspace`'s results.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FirstNonspace {
    pub(crate) offset: usize,
    pub(crate) indent: usize,
    pub(crate) blank: bool,
}

/// One line as cmark's block parser sees it: the content plus the `\n` cmark
/// appends, then a NUL.
pub(crate) struct Line<'a> {
    bytes: &'a [u8],
    index: usize,
    start: usize,
    end: usize,
}

impl<'a> Line<'a> {
    pub(crate) fn new(bytes: &'a [u8], lines: &SourceLines, line: usize) -> Line<'a> {
        Line {
            bytes,
            index: line,
            start: lines.start(line),
            end: lines.end(line),
        }
    }

    /// `peek_at(input, offset)`: the line's content, then `\n`, then NUL.
    pub(crate) fn peek(&self, offset: usize) -> u8 {
        if offset < self.end {
            self.bytes[offset]
        } else if offset == self.end {
            b'\n'
        } else {
            0
        }
    }

    pub(crate) fn scan(&self) -> LineScan {
        // `S_process_line` skips a byte order mark at the start of the first
        // line.
        let bom = self.index == 0 && self.bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        LineScan {
            offset: self.start + if bom { 3 } else { 0 },
            column: 0,
            partially_consumed_tab: false,
        }
    }

    /// `S_find_first_nonspace`.
    pub(crate) fn first_nonspace(&self, scan: &LineScan) -> FirstNonspace {
        let mut chars_to_tab = TAB_STOP - (scan.column % TAB_STOP);
        let mut offset = scan.offset;
        let mut column = scan.column;
        loop {
            match self.peek(offset) {
                b' ' => {
                    offset += 1;
                    column += 1;
                    chars_to_tab -= 1;
                    if chars_to_tab == 0 {
                        chars_to_tab = TAB_STOP;
                    }
                }
                b'\t' => {
                    offset += 1;
                    column += chars_to_tab;
                    chars_to_tab = TAB_STOP;
                }
                _ => break,
            }
        }
        let next = self.peek(offset);
        FirstNonspace {
            offset,
            indent: column - scan.column,
            blank: next == b'\n' || next == b'\r',
        }
    }

    /// `S_advance_offset`: `count` columns (or bytes when `columns` is false).
    pub(crate) fn advance(&self, scan: &mut LineScan, mut count: usize, columns: bool) {
        while count > 0 {
            let byte = self.peek(scan.offset);
            if byte == 0 {
                break;
            }
            if byte == b'\t' {
                let chars_to_tab = TAB_STOP - (scan.column % TAB_STOP);
                if columns {
                    scan.partially_consumed_tab = chars_to_tab > count;
                    let chars_to_advance = count.min(chars_to_tab);
                    scan.column += chars_to_advance;
                    if !scan.partially_consumed_tab {
                        scan.offset += 1;
                    }
                    count -= chars_to_advance;
                } else {
                    scan.partially_consumed_tab = false;
                    scan.column += chars_to_tab;
                    scan.offset += 1;
                    count -= 1;
                }
            } else {
                scan.partially_consumed_tab = false;
                scan.offset += 1;
                scan.column += 1;
                count -= 1;
            }
        }
    }

    /// Matches the prefixes of `containers`, outermost first, the way
    /// `check_open_blocks` continues open blocks on a non-blank line. Returns
    /// the scan state where matching stopped and whether every container
    /// matched.
    pub(crate) fn match_prefixes(&self, containers: &[Prefix]) -> (LineScan, bool) {
        let mut scan = self.scan();
        for container in containers {
            let first = self.first_nonspace(&scan);
            match *container {
                Prefix::BlockQuote => {
                    if first.indent <= 3 && self.peek(first.offset) == b'>' {
                        self.advance(&mut scan, first.indent + 1, true);
                        if matches!(self.peek(scan.offset), b' ' | b'\t') {
                            self.advance(&mut scan, 1, true);
                        }
                    } else {
                        return (scan, false);
                    }
                }
                Prefix::Item { first_line, .. } if first_line == self.index => {
                    // The line the item opened on: `open_new_blocks` consumed
                    // the marker and its padding.
                    scan = self.item_width(scan).1;
                }
                Prefix::Item { width, .. } => {
                    if first.indent >= width {
                        self.advance(&mut scan, width, true);
                    } else if first.blank {
                        let distance = first.offset - scan.offset;
                        self.advance(&mut scan, distance, false);
                    } else {
                        return (scan, false);
                    }
                }
            }
        }
        (scan, true)
    }

    /// The width (`marker_offset + padding`) of the list item whose marker
    /// starts at or after `scan` (`open_new_blocks`' list-item branch), and
    /// the scan state after the marker and its padding.
    pub(crate) fn item_width(&self, mut scan: LineScan) -> (usize, LineScan) {
        let first = self.first_nonspace(&scan);
        let marker_offset = first.indent;
        let mut position = first.offset;
        let byte = self.peek(position);
        if byte.is_ascii_digit() {
            let mut digits = 0;
            while digits < 9 && self.peek(position).is_ascii_digit() {
                position += 1;
                digits += 1;
            }
            position += 1; // `.` or `)`
        } else {
            position += 1; // `*`, `-` or `+`
        }
        let matched = position - first.offset;
        let distance = position - scan.offset;
        self.advance(&mut scan, distance, false);
        let saved = scan;
        while scan.column - saved.column <= 5 && matches!(self.peek(scan.offset), b' ' | b'\t') {
            self.advance(&mut scan, 1, true);
        }
        let spaces = scan.column - saved.column;
        let padding;
        if !(1..5).contains(&spaces) || matches!(self.peek(scan.offset), b'\n' | b'\r') {
            padding = matched + 1;
            scan = saved;
            if spaces > 0 {
                self.advance(&mut scan, 1, true);
            }
        } else {
            padding = matched + spaces;
        }
        (marker_offset + padding, scan)
    }
}
