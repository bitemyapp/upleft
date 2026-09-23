//! Port of `ClipboardSemanticHTML.swift`: a deliberately small, safe
//! Markdown-to-HTML projection for the system clipboard.
//!
//! Swift walks `Character`s (extended grapheme clusters); this port does the
//! same through `upleft_core::swift_text`. `hasPrefix` and `range(of:)` are
//! only ever asked about ASCII markers here, so a byte search agrees with
//! Swift's canonical-equivalence comparison except where a marker is followed
//! by a combining mark that joins its grapheme — then Swift's `Character`
//! walk keeps the cluster whole and so does this one (unverified against the
//! oracle; the clipboard is not a conformance output).

use upleft_core::swift_text::{self, graphemes};

pub struct ClipboardSemanticHTML;

struct ListEntry {
    indent: usize,
    kind: &'static str,
    text: String,
}

impl ClipboardSemanticHTML {
    pub fn render(markdown: &str) -> String {
        let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalized.split('\n').collect();
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
        fn fence_html(language: &str, lines: &[String]) -> String {
            let class = if language.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", escape_attribute(language))
            };
            format!("<pre><code{class}>{}</code></pre>", escape(&lines.join("\n")))
        }

        while index < lines.len() {
            let line = lines[index];
            if in_fence {
                let trimmed = swift_text::trim_whitespaces(line);
                if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                    output.push(fence_html(&fence_language, &fence_lines));
                    in_fence = false;
                    fence_language.clear();
                    fence_lines.clear();
                } else {
                    fence_lines.push(line.to_owned());
                }
                index += 1;
                continue;
            }

            let trimmed = swift_text::trim_whitespaces(line);
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
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
                && is_table_delimiter(lines[index + 1])
                && let Some(header) = table_cells(line)
            {
                flush_paragraph(&mut paragraph, &mut output);
                flush_list(&mut list_entries, &mut output);
                let mut rows = vec![header];
                index += 2;
                while index < lines.len() {
                    let Some(row) = table_cells(lines[index]) else { break };
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
            paragraph.push(line.to_owned());
            index += 1;
        }
        if in_fence {
            output.push(fence_html(&fence_language, &fence_lines));
        }
        flush_paragraph(&mut paragraph, &mut output);
        flush_list(&mut list_entries, &mut output);
        output.join("\n")
    }
}

fn heading_parts(line: &str) -> Option<(usize, String)> {
    let hashes = graphemes(line).take_while(|g| *g == "#").count();
    if !(1..=6).contains(&hashes) || swift_text::first(swift_text::drop_first(line, hashes)) != Some(" ") {
        return None;
    }
    Some((hashes, swift_text::trim_whitespaces(swift_text::drop_first(line, hashes)).to_owned()))
}

fn list_part(line: &str) -> Option<ListEntry> {
    let leading: Vec<&str> = graphemes(line).take_while(|g| *g == " " || *g == "\t").collect();
    let indent = leading.iter().map(|g| if *g == "\t" { 4 } else { 1 }).sum();
    let content = swift_text::drop_first(line, leading.len());
    if ["- ", "* ", "+ "].iter().any(|marker| has_prefix(content, marker)) {
        return Some(ListEntry { indent, kind: "ul", text: swift_text::drop_first(content, 2).to_owned() });
    }
    let digits = graphemes(content).take_while(|g| swift_text::is_number(g)).count();
    if digits == 0 || !has_prefix(swift_text::drop_first(content, digits), ". ") {
        return None;
    }
    Some(ListEntry { indent, kind: "ol", text: swift_text::drop_first(content, digits + 2).to_owned() })
}

/// `String.hasPrefix` for an ASCII prefix: the prefix's graphemes must be the
/// string's leading graphemes.
fn has_prefix(s: &str, prefix: &str) -> bool {
    let count = graphemes(prefix).count();
    swift_text::prefix(s, count) == prefix
}

fn list_html(entries: &[ListEntry]) -> String {
    fn render_level(entries: &[ListEntry], index: &mut usize, indent: usize) -> String {
        let mut html = String::new();
        while *index < entries.len() && entries[*index].indent == indent {
            let kind = entries[*index].kind;
            html += &format!("<{kind}>");
            while *index < entries.len() && entries[*index].indent == indent && entries[*index].kind == kind {
                let text = inline(&entries[*index].text);
                *index += 1;
                html += &format!("<li>{text}");
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
    let mut index = 0;
    render_level(entries, &mut index, entries.first().map_or(0, |entry| entry.indent))
}

fn table_cells(line: &str) -> Option<Vec<String>> {
    if !line.contains('|') {
        return None;
    }
    let mut cells: Vec<String> = split_graphemes(line, "|").into_iter().map(|cell| swift_text::trim_whitespaces(&cell).to_owned()).collect();
    let trimmed = swift_text::trim_whitespaces(line);
    if has_prefix(trimmed, "|") && !cells.is_empty() {
        cells.remove(0);
    }
    if swift_text::last(trimmed) == Some("|") && !cells.is_empty() {
        cells.pop();
    }
    if cells.len() > 1 { Some(cells) } else { None }
}

/// `split(separator:omittingEmptySubsequences: false)` on `Character`s.
fn split_graphemes(line: &str, separator: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    for g in graphemes(line) {
        if g == separator {
            parts.push(String::new());
        } else {
            parts.last_mut().expect("at least one part").push_str(g);
        }
    }
    parts
}

fn is_table_delimiter(line: &str) -> bool {
    let Some(cells) = table_cells(line) else { return false };
    if cells.is_empty() {
        return false;
    }
    cells.iter().all(|cell| {
        let value = swift_text::trim_whitespaces(cell);
        swift_text::count(value) >= 3 && graphemes(value).all(|g| g == "-" || g == ":")
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
            let cells: String = cells.iter().take(header.len()).map(|cell| format!("<td>{}</td>", inline(cell))).collect();
            format!("<tr>{cells}</tr>")
        })
        .collect();
    let body = if body.is_empty() { String::new() } else { format!("<tbody>{body}</tbody>") };
    format!("<table><thead><tr>{head}</tr></thead>{body}</table>")
}

/// Grapheme-indexed view of a string, standing in for `String.Index` walks.
struct Clusters<'a> {
    items: Vec<&'a str>,
}

impl<'a> Clusters<'a> {
    fn new(s: &'a str) -> Clusters<'a> {
        Clusters { items: graphemes(s).collect() }
    }

    fn has_prefix_at(&self, index: usize, prefix: &str) -> bool {
        let wanted: Vec<&str> = graphemes(prefix).collect();
        index + wanted.len() <= self.items.len() && self.items[index..index + wanted.len()] == wanted[..]
    }

    fn find(&self, from: usize, needle: &str) -> Option<usize> {
        let wanted: Vec<&str> = graphemes(needle).collect();
        (from..self.items.len()).find(|&start| {
            start + wanted.len() <= self.items.len() && self.items[start..start + wanted.len()] == wanted[..]
        })
    }

    fn slice(&self, start: usize, end: usize) -> String {
        self.items[start..end].concat()
    }
}

fn inline(input: &str) -> String {
    let clusters = Clusters::new(input);
    let count = clusters.items.len();
    let mut output = String::new();
    let mut index = 0usize;
    while index < count {
        let current = clusters.items[index];
        if current == "<" {
            output += "&lt;";
            index += 1;
            continue;
        }
        if current == "\\" && index + 1 < count {
            output += &escape(clusters.items[index + 1]);
            index += 2;
            continue;
        }
        if clusters.has_prefix_at(index, "**") || clusters.has_prefix_at(index, "__") {
            let marker = clusters.slice(index, index + 2);
            if let Some(end) = clusters.find(index + 2, &marker) {
                output += &format!("<strong>{}</strong>", inline(&clusters.slice(index + 2, end)));
                index = end + 2;
                continue;
            }
        }
        if clusters.has_prefix_at(index, "~~")
            && let Some(end) = clusters.find(index + 2, "~~")
        {
            output += &format!("<del>{}</del>", inline(&clusters.slice(index + 2, end)));
            index = end + 2;
            continue;
        }
        if current == "`"
            && let Some(end) = clusters.find(index + 1, "`")
        {
            output += &format!("<code>{}</code>", escape(&clusters.slice(index + 1, end)));
            index = end + 1;
            continue;
        }
        if clusters.has_prefix_at(index, "![")
            && let Some(close) = clusters.find(index + 2, "]")
            && clusters.has_prefix_at(close + 1, "(")
        {
            let destination_start = close + 1;
            if let Some(destination_end) = clusters.find(destination_start, ")") {
                let alt = clusters.slice(index + 2, close);
                let destination = clusters.slice(destination_start + 1, destination_end);
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
        if current == "["
            && let Some(close) = clusters.find(index + 1, "]")
            && clusters.has_prefix_at(close + 1, "(")
        {
            let destination_start = close + 1;
            if let Some(destination_end) = clusters.find(destination_start, ")") {
                let label = clusters.slice(index + 1, close);
                let destination = clusters.slice(destination_start + 1, destination_end);
                output += &format!("<a href=\"{}\">{}</a>", escape_attribute(&destination), inline(&label));
                index = destination_end + 1;
                continue;
            }
        }
        output += &escape(current);
        index += 1;
    }
    output.replace("  \n", "<br>\n")
}

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn escape_attribute(value: &str) -> String {
    let trimmed = swift_text::trim_whitespaces_and_newlines(value);
    let Some(url) = objc2_foundation::NSURL::URLWithString(&objc2_foundation::NSString::from_str(trimmed)) else {
        return String::new();
    };
    let scheme = url.scheme().map(|scheme| scheme.to_string());
    let allowed = match &scheme {
        None => true,
        Some(scheme) => ["http", "https", "mailto", "file"].contains(&swift_text::lowercased(scheme).as_str()),
    };
    if !allowed {
        return String::new();
    }
    escape(trimmed)
}
