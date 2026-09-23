//! Port of `ClipboardSemanticHTML.swift`: a deliberately small, safe
//! Markdown-to-HTML projection for the system clipboard. Unsupported Markdown
//! and raw HTML remain escaped text.
//!
//! Swift's `String` operations are reproduced with `upleft_core::swift_text`:
//! prefix, drop and index arithmetic walk Characters, `trimmingCharacters`,
//! `replacingOccurrences` and `components(separatedBy:)` are Foundation's.
//! `Substring.range(of:)` is matched Character by Character, which agrees with
//! Foundation's non-literal search for every ASCII needle this file searches
//! for except next to a composed sequence that is not also one grapheme.

use objc2_foundation::{NSString, NSURL};
use upleft_core::swift_text::{self, CharSet};

struct ListEntry {
    indent: isize,
    kind: &'static str,
    text: String,
}

pub struct ClipboardSemanticHTML;

impl ClipboardSemanticHTML {
    pub fn render(markdown: &str) -> String {
        let normalized = swift_text::replacing_occurrences(
            &swift_text::replacing_occurrences(markdown, "\r\n", "\n"),
            "\r",
            "\n",
        );
        let lines = swift_text::components_separated_by(&normalized, "\n");
        let mut output: Vec<String> = Vec::new();
        let mut index = 0usize;
        let mut in_fence = false;
        let mut fence_language = String::new();
        let mut fence_lines: Vec<String> = Vec::new();
        let mut paragraph: Vec<String> = Vec::new();
        let mut list_entries: Vec<ListEntry> = Vec::new();

        fn flush_paragraph(paragraph: &mut Vec<String>, output: &mut Vec<String>) {
            if paragraph.is_empty() {
                return;
            }
            let text = paragraph.join("\n");
            output.push(format!("<p>{}</p>", inline(&text)));
            paragraph.clear();
        }
        fn flush_list(entries: &mut Vec<ListEntry>, output: &mut Vec<String>) {
            if entries.is_empty() {
                return;
            }
            output.push(list_html(entries));
            entries.clear();
        }
        fn fence(language: &str, lines: &[String]) -> String {
            let class = if language.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", escape_attribute(language))
            };
            format!("<pre><code{class}>{}</code></pre>", escape(&lines.join("\n")))
        }

        while index < lines.len() {
            let line = &lines[index];
            if in_fence {
                let trimmed = swift_text::trim_whitespaces(line);
                if swift_text::has_prefix(trimmed, "```") || swift_text::has_prefix(trimmed, "~~~") {
                    output.push(fence(&fence_language, &fence_lines));
                    in_fence = false;
                    fence_language.clear();
                    fence_lines.clear();
                } else {
                    fence_lines.push(line.clone());
                }
                index += 1;
                continue;
            }

            let trimmed = swift_text::trim_whitespaces(line);
            if swift_text::has_prefix(trimmed, "```") || swift_text::has_prefix(trimmed, "~~~") {
                flush_paragraph(&mut paragraph, &mut output);
                flush_list(&mut list_entries, &mut output);
                in_fence = true;
                fence_language = swift_text::trim_whitespaces(swift_text::drop_first(trimmed, 3)).to_owned();
                index += 1;
                continue;
            }
            if trimmed.is_empty() {
                flush_paragraph(&mut paragraph, &mut output);
                flush_list(&mut list_entries, &mut output);
                index += 1;
                continue;
            }

            if let Some((level, text)) = heading_parts(trimmed) {
                flush_paragraph(&mut paragraph, &mut output);
                flush_list(&mut list_entries, &mut output);
                output.push(format!("<h{level}>{}</h{level}>", inline(&text)));
                index += 1;
                continue;
            }

            if index + 1 < lines.len()
                && is_table_delimiter(&lines[index + 1])
                && let Some(header) = table_cells(line)
            {
                flush_paragraph(&mut paragraph, &mut output);
                flush_list(&mut list_entries, &mut output);
                let mut rows: Vec<Vec<String>> = vec![header];
                index += 2;
                while index < lines.len() {
                    let Some(row) = table_cells(&lines[index]) else { break };
                    rows.push(row);
                    index += 1;
                }
                output.push(table_html(&rows));
                continue;
            }

            if let Some(item) = list_part(line) {
                flush_paragraph(&mut paragraph, &mut output);
                list_entries.push(item);
                index += 1;
                continue;
            }

            flush_list(&mut list_entries, &mut output);
            paragraph.push(line.clone());
            index += 1;
        }
        if in_fence {
            output.push(fence(&fence_language, &fence_lines));
        }
        flush_paragraph(&mut paragraph, &mut output);
        flush_list(&mut list_entries, &mut output);
        output.join("\n")
    }
}

fn heading_parts(line: &str) -> Option<(usize, String)> {
    let hashes = swift_text::graphemes(line).take_while(|g| is(g, '#')).count();
    if !(1..=6).contains(&hashes) || !swift_text::first(swift_text::drop_first(line, hashes)).is_some_and(|g| is(g, ' ')) {
        return None;
    }
    Some((hashes, swift_text::trim_whitespaces(swift_text::drop_first(line, hashes)).to_owned()))
}

fn list_part(line: &str) -> Option<ListEntry> {
    let mut indent = 0isize;
    let mut consumed = 0usize;
    for g in swift_text::graphemes(line) {
        if is(g, ' ') {
            indent += 1;
        } else if is(g, '\t') {
            indent += 4;
        } else {
            break;
        }
        consumed += 1;
    }
    let content = swift_text::drop_first(line, consumed);
    if ["- ", "* ", "+ "].iter().any(|marker| swift_text::has_prefix(content, marker)) {
        return Some(ListEntry { indent, kind: "ul", text: swift_text::drop_first(content, 2).to_owned() });
    }
    let mut digits = 0usize;
    for g in swift_text::graphemes(content) {
        if !swift_text::is_number(g) {
            break;
        }
        digits += 1;
    }
    if digits == 0 || !swift_text::has_prefix(swift_text::drop_first(content, digits), ". ") {
        return None;
    }
    Some(ListEntry { indent, kind: "ol", text: swift_text::drop_first(content, digits + 2).to_owned() })
}

fn list_html(entries: &[ListEntry]) -> String {
    fn render_level(entries: &[ListEntry], index: &mut usize, indent: isize) -> String {
        let mut html = String::new();
        while *index < entries.len() && entries[*index].indent == indent {
            let kind = entries[*index].kind;
            html += &format!("<{kind}>");
            while *index < entries.len() && entries[*index].indent == indent && entries[*index].kind == kind {
                let entry = &entries[*index];
                *index += 1;
                html += &format!("<li>{}", inline(&entry.text));
                if *index < entries.len() && entries[*index].indent > indent {
                    let deeper = entries[*index].indent;
                    html += &render_level(entries, index, deeper);
                }
                html += "</li>";
            }
            html += &format!("</{kind}>");
        }
        html
    }
    let mut index = 0usize;
    render_level(entries, &mut index, entries.first().map_or(0, |entry| entry.indent))
}

fn table_cells(line: &str) -> Option<Vec<String>> {
    if !swift_text::contains_bridged(line, "|") {
        return None;
    }
    let mut cells: Vec<String> = swift_text::split(line, '|', usize::MAX, false)
        .into_iter()
        .map(|cell| swift_text::trim_whitespaces(cell).to_owned())
        .collect();
    let trimmed = swift_text::trim_whitespaces(line);
    if swift_text::has_prefix(trimmed, "|") && !cells.is_empty() {
        cells.remove(0);
    }
    if swift_text::has_suffix(trimmed, "|") && !cells.is_empty() {
        cells.pop();
    }
    if cells.len() > 1 { Some(cells) } else { None }
}

fn is_table_delimiter(line: &str) -> bool {
    let Some(cells) = table_cells(line) else { return false };
    if cells.is_empty() {
        return false;
    }
    cells.iter().all(|cell| {
        let value = swift_text::trim_whitespaces(cell);
        swift_text::count(value) >= 3 && swift_text::graphemes(value).all(|g| is(g, '-') || is(g, ':'))
    })
}

fn table_html(rows: &[Vec<String>]) -> String {
    let Some(header) = rows.first() else { return String::new() };
    let head: String = header.iter().map(|cell| format!("<th>{}</th>", inline(cell))).collect();
    let body: String = rows[1..]
        .iter()
        .map(|row| {
            let mut cells = row.clone();
            cells.extend(std::iter::repeat_n(String::new(), header.len().saturating_sub(row.len())));
            let cells: String = cells[..header.len()]
                .iter()
                .map(|cell| format!("<td>{}</td>", inline(cell)))
                .collect();
            format!("<tr>{cells}</tr>")
        })
        .collect();
    format!(
        "<table><thead><tr>{head}</tr></thead>{}</table>",
        if body.is_empty() { String::new() } else { format!("<tbody>{body}</tbody>") }
    )
}

/// The first grapheme index at or after `from` where `needle` (a sequence of
/// graphemes) starts, and the index just past it.
fn find_graphemes(characters: &[&str], from: usize, needle: &[&str]) -> Option<(usize, usize)> {
    if needle.is_empty() || from > characters.len() {
        return None;
    }
    (from..characters.len().saturating_sub(needle.len() - 1))
        .find(|&start| same(&characters[start..start + needle.len()], needle))
        .map(|start| (start, start + needle.len()))
}

fn has_prefix_at(characters: &[&str], index: usize, prefix: &[&str]) -> bool {
    characters.len() >= index + prefix.len() && same(&characters[index..index + prefix.len()], prefix)
}

/// `Character == Character` for a one-scalar ASCII right-hand side.
#[inline]
fn is(g: &str, c: char) -> bool {
    swift_text::char_is(g, c)
}

/// Character-wise equality of two runs whose right-hand side is ASCII.
fn same(a: &[&str], b: &[&str]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| swift_text::char_eq(x, y))
}

fn inline(input: &str) -> String {
    let characters: Vec<&str> = swift_text::graphemes(input).collect();
    let end = characters.len();
    let mut output = String::new();
    let mut index = 0usize;
    while index < end {
        let character = characters[index];
        if is(character, '<') {
            // Raw HTML is source text from the document, not trusted markup.
            output += "&lt;";
            index += 1;
            continue;
        }
        if is(character, '\\') {
            let next = index + 1;
            if next < end {
                output += &escape(characters[next]);
                index = next + 1;
                continue;
            }
        }
        if has_prefix_at(&characters, index, &["*", "*"]) || has_prefix_at(&characters, index, &["_", "_"]) {
            let marker = if is(characters[index], '*') { ["*", "*"] } else { ["_", "_"] };
            if let Some((found, after)) = find_graphemes(&characters, index + 2, &marker) {
                output += &format!("<strong>{}</strong>", inline(&characters[index + 2..found].concat()));
                index = after;
                continue;
            }
        }
        if has_prefix_at(&characters, index, &["~", "~"])
            && let Some((found, after)) = find_graphemes(&characters, index + 2, &["~", "~"])
        {
            output += &format!("<del>{}</del>", inline(&characters[index + 2..found].concat()));
            index = after;
            continue;
        }
        if is(character, '`')
            && let Some(close) = (index + 1..end).find(|&i| is(characters[i], '`'))
        {
            output += &format!("<code>{}</code>", escape(&characters[index + 1..close].concat()));
            index = close + 1;
            continue;
        }
        if has_prefix_at(&characters, index, &["!", "["])
            && let Some(close) = (index + 2..end).find(|&i| is(characters[i], ']'))
            && has_prefix_at(&characters, close + 1, &["("])
        {
            let destination_start = close + 1;
            if let Some(destination_end) = (destination_start..end).find(|&i| is(characters[i], ')')) {
                let alt = characters[index + 2..close].concat();
                let destination = characters[destination_start + 1..destination_end].concat();
                let source = escape_attribute(&destination);
                if !source.is_empty() {
                    output += &format!("<img src=\"{source}\" alt=\"{}\">", escape_attribute(&alt));
                } else {
                    output += &escape(&format!("![{alt}]({destination})"));
                }
                index = destination_end + 1;
                continue;
            }
        }
        if is(character, '[')
            && let Some(close) = (index + 1..end).find(|&i| is(characters[i], ']'))
            && has_prefix_at(&characters, close + 1, &["("])
        {
            let destination_start = close + 1;
            if let Some(destination_end) = (destination_start..end).find(|&i| is(characters[i], ')')) {
                let label = characters[index + 1..close].concat();
                let destination = characters[destination_start + 1..destination_end].concat();
                output += &format!("<a href=\"{}\">{}</a>", escape_attribute(&destination), inline(&label));
                index = destination_end + 1;
                continue;
            }
        }
        output += &escape(character);
        index += 1;
    }
    swift_text::replacing_occurrences(&output, "  \n", "<br>\n")
}

fn escape(value: &str) -> String {
    let value = swift_text::replacing_occurrences(value, "&", "&amp;");
    let value = swift_text::replacing_occurrences(&value, "<", "&lt;");
    let value = swift_text::replacing_occurrences(&value, ">", "&gt;");
    swift_text::replacing_occurrences(&value, "\"", "&quot;")
}

fn escape_attribute(value: &str) -> String {
    let trimmed = swift_text::trimming(value, CharSet::WhitespacesAndNewlines);
    // `URL(string:)`: nil for a string Foundation cannot make a URL of.
    let Some(url) = NSURL::URLWithString(&NSString::from_str(trimmed)) else {
        return String::new();
    };
    let allowed = match url.scheme() {
        None => true,
        Some(scheme) => {
            let scheme = swift_text::lowercased(&scheme.to_string());
            ["http", "https", "mailto", "file"].contains(&scheme.as_str())
        }
    };
    if !allowed {
        return String::new();
    }
    escape(trimmed)
}
