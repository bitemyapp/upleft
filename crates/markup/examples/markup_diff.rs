//! Differential check of the pulldown-cmark adapter against the cmark-gfm
//! converter it replaced.
//!
//! Both trees are dumped in the conformance runner's markup-dump format
//! (`crates/conformance/src/dump/markup.rs`, mirroring the Swift oracle's
//! `MarkupDump.swift`) and compared node by node. Differences are counted by
//! category.
//!
//! ```sh
//! cargo run --release -p upleft-markup --features cmark-oracle --example markup_diff -- \
//!     [--corpus] [--spec] [--mutations N] [--random N] [--show K] [--category TEXT] [FILE...]
//! ```
//!
//! With no source flags and no files it checks the corpus, the spec examples
//! and 2,000 mutated and 2,000 random documents.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use upleft_markup::parser::cmark_oracle;
use upleft_markup::{Checkbox, ColumnAlignment, Document, Markup, MarkupData, ParseOptions, SourceRange};

// MARK: The markup dump (crates/conformance/src/dump/markup.rs)

fn range(range: Option<SourceRange>) -> Value {
    match range {
        None => Value::Null,
        Some(range) => Value::Array(vec![
            range.lower_bound.line.into(),
            range.lower_bound.column.into(),
            range.upper_bound.line.into(),
            range.upper_bound.column.into(),
        ]),
    }
}

fn string(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |value| value.into())
}

fn markup(markup: Markup<'_>) -> Value {
    let data = markup.data();
    let mut object = Map::new();
    object.insert("kind".into(), data.type_name().into());
    object.insert("range".into(), range(markup.range()));
    match data {
        MarkupData::CodeBlock { code, language } => {
            object.insert("code".into(), code.into());
            object.insert("language".into(), string(language));
        }
        MarkupData::HtmlBlock { raw_html } => {
            object.insert("rawHTML".into(), raw_html.into());
        }
        MarkupData::Heading { level } => {
            object.insert("level".into(), level.into());
        }
        MarkupData::ListItem { checkbox } => {
            object.insert(
                "checkbox".into(),
                match checkbox {
                    None => Value::Null,
                    Some(Checkbox::Checked) => "checked".into(),
                    Some(Checkbox::Unchecked) => "unchecked".into(),
                },
            );
        }
        MarkupData::OrderedList { start_index } => {
            object.insert("startIndex".into(), start_index.into());
        }
        MarkupData::Table { column_alignments } => {
            object.insert(
                "columnAlignments".into(),
                Value::Array(
                    column_alignments
                        .iter()
                        .map(|alignment| match alignment {
                            None => Value::Null,
                            Some(ColumnAlignment::Left) => "left".into(),
                            Some(ColumnAlignment::Center) => "center".into(),
                            Some(ColumnAlignment::Right) => "right".into(),
                        })
                        .collect(),
                ),
            );
            object.insert(
                "maxColumnCount".into(),
                markup.max_column_count().expect("a table has a column count").into(),
            );
        }
        MarkupData::TableCell { colspan, rowspan } => {
            object.insert("colspan".into(), colspan.into());
            object.insert("rowspan".into(), rowspan.into());
        }
        MarkupData::Link { destination, title } => {
            object.insert("destination".into(), string(destination));
            object.insert("title".into(), string(title));
            object.insert(
                "isAutolink".into(),
                markup.is_autolink().expect("a link has isAutolink").into(),
            );
        }
        MarkupData::Image { source, title } => {
            object.insert("source".into(), string(source));
            object.insert("title".into(), string(title));
        }
        MarkupData::InlineCode { code } => {
            object.insert("code".into(), code.into());
        }
        MarkupData::InlineHtml { raw_html } => {
            object.insert("rawHTML".into(), raw_html.into());
        }
        MarkupData::Text { string } => {
            object.insert("string".into(), string.into());
        }
        MarkupData::CustomInline { text } => {
            object.insert("text".into(), text.into());
        }
        MarkupData::SymbolLink { destination } => {
            object.insert("destination".into(), string(destination));
        }
        MarkupData::InlineAttributes { attributes } => {
            object.insert("attributes".into(), attributes.into());
        }
        _ => {}
    }
    if let Some(plain_text) = markup.plain_text() {
        object.insert("plainText".into(), plain_text.into());
    }
    object.insert("indexInParent".into(), markup.index_in_parent().into());
    object.insert(
        "children".into(),
        Value::Array(markup.children().map(self::markup).collect()),
    );
    Value::Object(object)
}

fn dump(document: &Document) -> Value {
    markup(document.root())
}

// MARK: Comparison

#[derive(Default)]
struct Tally {
    /// Documents whose trees differ, by category (each category counted once
    /// per document).
    documents: BTreeMap<String, usize>,
    /// Differing nodes, by category.
    nodes: BTreeMap<String, usize>,
    /// Up to a few examples per category.
    examples: BTreeMap<String, Vec<String>>,
}

fn kind(value: &Value) -> &str {
    value["kind"].as_str().unwrap_or("?")
}

fn children(value: &Value) -> &[Value] {
    value["children"].as_array().map_or(&[], Vec::as_slice)
}

fn short(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 90 {
        format!("{}…", text.chars().take(90).collect::<String>())
    } else {
        text
    }
}

/// Collects every difference between two trees: node-level differences where
/// the shapes agree, and one shape difference where they don't.
fn compare(reference: &Value, candidate: &Value, path: &str, found: &mut Vec<(String, String)>) {
    if kind(reference) != kind(candidate) {
        found.push((
            format!("shape: {} became {}", kind(reference), kind(candidate)),
            format!("{path}: cmark {} | pulldown {}", short(reference), short(candidate)),
        ));
        return;
    }
    let node_kind = kind(reference);
    let (Some(a), Some(b)) = (reference.as_object(), candidate.as_object()) else {
        return;
    };
    let reference_range = &a["range"];
    let candidate_range = &b["range"];
    if reference_range != candidate_range {
        let category = match (reference_range, candidate_range) {
            (Value::Null, _) => format!("range: {node_kind} has none in cmark"),
            (_, Value::Null) => format!("range: {node_kind} has none in pulldown"),
            (Value::Array(x), Value::Array(y)) => {
                let start = x[0] != y[0] || x[1] != y[1];
                let end = x[2] != y[2] || x[3] != y[3];
                let which = match (start, end) {
                    (true, true) => "start and end",
                    (true, false) => "start",
                    _ => "end",
                };
                let lines = if (start && x[0] != y[0]) || (end && x[2] != y[2]) {
                    "line"
                } else {
                    "column"
                };
                format!("range: {node_kind} {which} ({lines})")
            }
            _ => "range: malformed".into(),
        };
        found.push((
            category,
            format!("{path}: cmark {reference_range} | pulldown {candidate_range} {}", short(candidate)),
        ));
    }
    for (key, value) in a {
        if matches!(key.as_str(), "kind" | "range" | "children" | "indexInParent") {
            continue;
        }
        if b.get(key) != Some(value) {
            found.push((
                format!("property: {node_kind}.{key}"),
                format!(
                    "{path}: cmark {} | pulldown {}",
                    short(value),
                    short(b.get(key).unwrap_or(&Value::Null))
                ),
            ));
        }
    }
    let x = children(reference);
    let y = children(candidate);
    let same_kinds = x.len() == y.len() && x.iter().zip(y).all(|(p, q)| kind(p) == kind(q));
    if !same_kinds {
        let first = x
            .iter()
            .zip(y)
            .position(|(p, q)| kind(p) != kind(q))
            .unwrap_or(x.len().min(y.len()));
        let describe = |list: &[Value]| {
            list.get(first).map_or("nothing".to_owned(), |value| kind(value).to_owned())
        };
        found.push((
            format!(
                "shape: in {node_kind}, {} became {}",
                describe(x),
                describe(y)
            ),
            format!(
                "{path}/{first}: cmark [{}] | pulldown [{}]",
                x.iter().map(kind).collect::<Vec<_>>().join(","),
                y.iter().map(kind).collect::<Vec<_>>().join(",")
            ),
        ));
        return;
    }
    for (index, (p, q)) in x.iter().zip(y).enumerate() {
        compare(p, q, &format!("{path}/{index}"), found);
    }
}

// MARK: Inputs

fn corpus_files(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            corpus_files(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "md") {
            into.push(path);
        }
    }
}

fn read(path: &Path) -> Option<String> {
    let mut bytes = std::fs::read(path).ok()?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    String::from_utf8(bytes).ok()
}

/// A small deterministic generator (SplitMix64).
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const FRAGMENTS: &[&str] = &[
    "*", "**", "_", "__", "~", "~~", "`", "``", "[", "]", "(", ")", "![", "<", ">", "&amp;", "&#42;",
    "\\", "\\*", "|", "- ", "* ", "1. ", "> ", "# ", "## ", "```", "~~~", "    ", "\t", "  \n", "\n",
    "\n\n", "[x]", "[ ]", "<div>", "</div>", "<!-- ", " -->", "<span>", "http://x.y", "<http://a.b>",
    "[a]", "[a]: /url", "\"t\"", "^", "  ", "---", "===", ":--", "--:", "é", "日本", "😀", "a", "word ",
];

const BLOCKS: &[&str] = &[
    "Plain paragraph with **bold**, *em*, `code` and a [link](http://x.y \"t\").",
    "Second line of text\ncontinues here  \nwith a hard break.",
    "# Heading *one*",
    "## Heading two ##",
    "Setext\n===",
    "Setext two\n---",
    "- item one\n- item two\n  - nested\n- item three",
    "1. first\n2. second\n\n   more\n3. third",
    "- [ ] task\n- [x] done\n* [X] big",
    "> quote\n> more *quote*\nlazy line",
    "> - quoted list\n>   continued\n> - [ ] quoted task",
    "```swift\nlet x = 1\n```",
    "~~~\nunclosed fence",
    "    indented code\n\n    more code",
    "<div>\nhtml block\n</div>",
    "<!-- comment\nspanning -->",
    "| a | b |\n|---|:-:|\n| c | d |\n| e |",
    "Intro text\n| x | y |\n|--|--|\n| 1 | 2 |",
    "| ^ | colspan ||\n|---|---|---|\n| a | ^ | b |",
    "***",
    "text with ~~strike~~ and ~single~ and trailing ~",
    "[ref]: http://example.com \"Title\"\n[ref] and [other][ref]",
    "a <span class=\"x\">inline html</span> and <a@b.co>",
    "***strong em*** and **x*** and *__mixed__*",
    "Entity &copy; and escapes \\* \\_ \\` and \\\\",
    "   - indented item\n     cont\n  1) paren",
    "- a\n\n  b\n\n- c\n\n\n",
    "* list\n\n      indented code in item\n",
    "`code\nspanning` lines and <a\nhref=\"x\">",
    "![image *alt*](src.png) ![ref][ref]",
    "^[attributed *text*](key: 'value') and ^[plain](x: 1) and ^[link](http://x.y)",
    "Here are the results:\n| Stage | Time |\n|---|--:|\n| parse | `1.2 ms` |\n| `a\\|b` | **bold** |",
    "- [ ] todo with `code` and [link](x)\n- [x] done ~~struck~~\n  - nested *em*",
    "> [!NOTE]\n> A callout with **bold** and a list:\n> - one\n> - two",
    "1. Step one:\n   ```sh\n   cargo build\n   ```\n2. Step two\n\n   Details.",
    "Text with a footnote[^1] and <https://auto.link> and trailing  \nbreak\\\nand more.",
    "#### Heading with `code` and trailing hashes ####",
    "Paragraph\n***\n\n---\nnot a heading\n\n___",
];

/// A corpus document with a few random edits.
fn mutate(random: &mut Random, source: &str) -> String {
    let mut text: Vec<char> = source.chars().collect();
    let edits = 1 + random.below(6);
    for _ in 0..edits {
        if text.is_empty() {
            text.extend(random.pick(FRAGMENTS).chars());
            continue;
        }
        let at = random.below(text.len() + 1);
        match random.below(5) {
            0 | 1 => {
                let fragment: Vec<char> = random.pick(FRAGMENTS).chars().collect();
                text.splice(at..at, fragment);
            }
            2 => {
                let end = (at + 1 + random.below(12)).min(text.len());
                if at < end {
                    text.drain(at..end);
                }
            }
            3 => {
                // Indent or dedent a line.
                let line_start = text[..at.min(text.len())]
                    .iter()
                    .rposition(|&c| c == '\n')
                    .map_or(0, |i| i + 1);
                if random.below(2) == 0 {
                    text.splice(line_start..line_start, "  ".chars());
                } else if text.get(line_start) == Some(&' ') {
                    text.remove(line_start);
                }
            }
            _ => {
                let block: Vec<char> = format!("\n{}\n", random.pick(BLOCKS)).chars().collect();
                text.splice(at..at, block);
            }
        }
    }
    text.into_iter().collect()
}

/// A document built from random blocks, inline fragments and nesting.
fn generate(random: &mut Random) -> String {
    let mut text = String::new();
    for _ in 0..1 + random.below(6) {
        let prefix = match random.below(6) {
            0 => "> ",
            1 => "- ",
            2 => "1. ",
            _ => "",
        };
        let block = if random.below(3) == 0 {
            let mut line = String::new();
            for _ in 0..1 + random.below(8) {
                line.push_str(random.pick(FRAGMENTS));
            }
            line
        } else {
            (*random.pick(BLOCKS)).to_owned()
        };
        for (index, line) in block.split('\n').enumerate() {
            if index == 0 || random.below(3) != 0 {
                text.push_str(prefix);
            } else if !prefix.is_empty() && random.below(2) == 0 {
                text.push_str(&" ".repeat(prefix.len()));
            }
            text.push_str(line);
            text.push('\n');
        }
        if random.below(3) != 0 {
            text.push('\n');
        }
    }
    text
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut use_corpus = false;
    let mut use_spec = false;
    let mut mutations = 0;
    let mut random_documents = 0;
    let mut show = 3;
    let mut category_filter: Option<String> = None;
    let mut print_source: Option<String> = None;
    let mut files = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--corpus" => use_corpus = true,
            "--spec" => use_spec = true,
            "--mutations" => {
                index += 1;
                mutations = arguments[index].parse().expect("a count");
            }
            "--random" => {
                index += 1;
                random_documents = arguments[index].parse().expect("a count");
            }
            "--show" => {
                index += 1;
                show = arguments[index].parse().expect("a count");
            }
            "--category" => {
                index += 1;
                category_filter = Some(arguments[index].clone());
            }
            "--source" => {
                index += 1;
                print_source = Some(arguments[index].clone());
            }
            other => files.push(PathBuf::from(other)),
        }
        index += 1;
    }
    let default = files.is_empty() && !use_corpus && !use_spec && mutations == 0 && random_documents == 0;
    if default {
        use_corpus = true;
        use_spec = true;
        mutations = 2000;
        random_documents = 2000;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut corpus = Vec::new();
    corpus_files(&root, &mut corpus);
    corpus.sort();
    let (spec, rest): (Vec<PathBuf>, Vec<PathBuf>) = corpus
        .into_iter()
        .partition(|path| path.to_string_lossy().contains("/generated/spec/"));

    let mut inputs: Vec<(String, String, &'static str)> = Vec::new();
    for path in &files {
        if let Some(text) = read(path) {
            inputs.push((path.display().to_string(), text, "files"));
        }
    }
    if use_corpus {
        for path in &rest {
            if let Some(text) = read(path) {
                inputs.push((path.display().to_string(), text, "corpus"));
            }
        }
    }
    if use_spec {
        for path in &spec {
            if let Some(text) = read(path) {
                inputs.push((path.display().to_string(), text, "spec"));
            }
        }
    }
    let seeds: Vec<String> = rest.iter().chain(&spec).filter_map(|path| read(path)).collect();
    let mut random = Random(0x5EED_CAFE_F00D);
    for number in 0..mutations {
        let source = random.pick(&seeds).clone();
        // Long documents make mutations slow to compare and hard to read.
        let source: String = source.chars().take(4000).collect();
        inputs.push((format!("mutation-{number}"), mutate(&mut random, &source), "mutations"));
    }
    for number in 0..random_documents {
        inputs.push((format!("random-{number}"), generate(&mut random), "random"));
    }

    if let Some(wanted) = &print_source {
        for (name, text, _) in &inputs {
            if name == wanted {
                print!("{text}");
            }
        }
        return;
    }

    let mut by_source: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut tally = Tally::default();
    for (name, text, source) in &inputs {
        let reference = dump(&cmark_oracle::parse(text, ParseOptions::DISABLE_SMART_OPTS));
        let candidate = dump(&Document::parse(text, ParseOptions::DISABLE_SMART_OPTS));
        let entry = by_source.entry(source).or_default();
        entry.0 += 1;
        if reference == candidate {
            entry.1 += 1;
            continue;
        }
        let mut found = Vec::new();
        compare(&reference, &candidate, "", &mut found);
        let mut seen = std::collections::BTreeSet::new();
        for (category, example) in found {
            *tally.nodes.entry(category.clone()).or_default() += 1;
            if seen.insert(category.clone()) {
                *tally.documents.entry(category.clone()).or_default() += 1;
                let examples = tally.examples.entry(category).or_default();
                if examples.len() < show {
                    examples.push(format!("{name} {example}"));
                }
            }
        }
    }

    println!("identical trees:");
    for (source, (total, identical)) in &by_source {
        println!("  {source:<10} {identical:>6}/{total}");
    }
    let mut categories: Vec<_> = tally.documents.iter().collect();
    categories.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("\ndifferences (documents, nodes, category):");
    for (category, documents) in categories {
        if category_filter
            .as_deref()
            .is_some_and(|filter| !category.contains(filter))
        {
            continue;
        }
        println!("{documents:>6} {:>7}  {category}", tally.nodes[category]);
        for example in &tally.examples[category] {
            println!("               {example}");
        }
    }
}
