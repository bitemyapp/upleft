//! The differential conformance check for copying formulas as TeX
//! (`upleft_render::view::math_copy_conformance`), headless: no view, no
//! window, no pasteboard. The view tests (`math_copy_*` in `view_tests`)
//! copy the same inputs through a hosted view and a private pasteboard.
//!
//! `cargo test -p upleft-render --test math_copy_conformance -- --nocapture`
//! prints the counts per property and category, and each failing input
//! minimized. `UPLEFT_MATH_COPY_DOCS` adds documents (paths separated by
//! `:`), `UPLEFT_MATH_COPY_RANDOM` sets the number of random documents.

use std::path::{Path, PathBuf};

use upleft_core::NSRange;
use upleft_render::view::math_copy_conformance::{Report, check_document, check_selection, minimize};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/generated")
}

fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir).map(|dir| dir.flatten().map(|entry| entry.path()).collect()).unwrap_or_default();
    paths.sort();
    paths
}

/// The `corpus/generated` formulas, bare.
fn corpus_formulas() -> Vec<String> {
    read_dir_sorted(&corpus().join("math"))
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "tex"))
        .filter_map(|path| std::fs::read_to_string(path).ok())
        // The first line names the kind; the formula follows.
        .map(|text| text.split_once('\n').map_or(text.as_str(), |(_, formula)| formula).trim().to_owned())
        .collect()
}

/// Formulas meant to break a copy: escapes, a literal `$`, comments,
/// braces, line breaks, Unicode, blanks, and what the guard rails reject.
const ADVERSARIAL: &[&str] = &[
    "x",
    "E = mc^2",
    "a+b",
    "\\$5",
    "\\text{cost \\$5}",
    "\\text{\\$}",
    "\\text{a $ b}",
    "a \\% b",
    "50\\%",
    "\\{x \\mid x > 0\\}",
    "{a}{b}",
    "a \\\\ b",
    "\\begin{matrix} a & b \\\\ c & d \\end{matrix}",
    "α + β = γ",
    "x² + ∞",
    "5 + x",
    "x + 5",
    "100",
    "1.5",
    "x$y",
    "a\\)b",
    "a\\]b",
    "\\mathop{lim}_{x}",
    "a_{1}^{2}",
    "\\frac{1}{2}",
    "x\\,dx",
    "a`b",
    "a*b*c",
    "a_b_c",
    "<a>&\"b\"",
    "\\sqrt[3]{x}",
    "(x)",
    "[x]",
    "\\left( x \\right)",
    "a\\",
];

/// The ways the dialect writes a formula, inline and in blocks.
#[derive(Clone, Copy, Debug)]
enum Form {
    Dollar,
    Paren,
    DollarDisplayInline,
    BracketInline,
    DollarBlock,
    DollarBlockOneLine,
    BracketBlock,
    Fence,
    TildeFence,
}

const FORMS: &[Form] = &[
    Form::Dollar,
    Form::Paren,
    Form::DollarDisplayInline,
    Form::BracketInline,
    Form::DollarBlock,
    Form::DollarBlockOneLine,
    Form::BracketBlock,
    Form::Fence,
    Form::TildeFence,
];

impl Form {
    fn is_block(self) -> bool {
        matches!(self, Form::DollarBlock | Form::DollarBlockOneLine | Form::BracketBlock | Form::Fence | Form::TildeFence)
    }

    fn write(self, latex: &str) -> String {
        match self {
            Form::Dollar => format!("${latex}$"),
            Form::Paren => format!("\\({latex}\\)"),
            Form::DollarDisplayInline => format!("$${latex}$$"),
            Form::BracketInline => format!("\\[{latex}\\]"),
            Form::DollarBlock => format!("$$\n{latex}\n$$"),
            Form::DollarBlockOneLine => format!("$${latex}$$"),
            Form::BracketBlock => format!("\\[\n{latex}\n\\]"),
            Form::Fence => format!("```math\n{latex}\n```"),
            Form::TildeFence => format!("~~~math\n{latex}\n~~~"),
        }
    }

    /// The formula as a block of its own, or inside a line of prose.
    fn snippet(self, latex: &str) -> String {
        if self.is_block() { self.write(latex) } else { format!("Some text {} more.", self.write(latex)) }
    }
}

/// `block` with `first` before its first line and `rest` before the others.
fn prefixed(block: &str, first: &str, rest: &str) -> String {
    block
        .split('\n')
        .enumerate()
        .map(|(i, line)| if i == 0 { format!("{first}{line}") } else if line.is_empty() { rest.trim_end().to_owned() } else { format!("{rest}{line}") })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every container a formula can sit in: (name, document, whether the
/// container takes a block).
fn containers(form: Form, latex: &str) -> Vec<(&'static str, String)> {
    let snippet = form.snippet(latex);
    let mut out = vec![
        ("paragraph", format!("Before.\n\n{snippet}\n\nAfter.\n")),
        ("list-tight", format!("- a\n{}\n- c\n", prefixed(&snippet, "- ", "  "))),
        ("list-loose", format!("- a\n\n{}\n\n- c\n", prefixed(&snippet, "- ", "  "))),
        ("list-ordered", format!("1. a\n2. {}\n", prefixed(&snippet, "", "   "))),
        ("quote", format!("{}\n", prefixed(&snippet, "> ", "> "))),
        ("quote-nested", format!("{}\n", prefixed(&snippet, "> > ", "> > "))),
        ("quote-in-list", format!("- item\n\n{}\n", prefixed(&snippet, "  > ", "  > "))),
        ("list-in-quote", format!("> - item\n{}\n", prefixed(&snippet, "> - ", ">   "))),
        ("list-nested", format!("- a\n  - b\n{}\n", prefixed(&snippet, "    - ", "      "))),
        ("footnote", format!("Text[^1].\n\n{}\n", prefixed(&snippet, "[^1]: ", "    "))),
        ("callout", format!("> [!NOTE]\n{}\n", prefixed(&snippet, "> ", "> "))),
    ];
    if !form.is_block() {
        out.push(("heading", format!("# Title {}\n\nBody.\n", form.write(latex))));
        out.push(("heading-setext", format!("Title {}\n===\n", form.write(latex))));
        out.push(("table-cell", format!("| a | b |\n|---|---|\n| {} | x |\n", form.write(latex))));
        out.push(("emphasis", format!("Some *text {} here* and **{}**.\n", form.write(latex), form.write(latex))));
        out.push(("link", format!("See [a {} b](https://example.com).\n", form.write(latex))));
    }
    out
}

/// Whitespace variants of a document.
fn whitespace_variants(text: &str) -> Vec<(&'static str, String)> {
    vec![
        ("crlf", text.replace('\n', "\r\n")),
        ("tabs", text.replace("\n", "\t\n").replacen("Some", "\tSome", 1)),
        ("trailing-spaces", text.replace('\n', "  \n")),
    ]
}

/// Whitespace inside the delimiters.
fn padded_formulas() -> Vec<String> {
    vec![
        " x ".to_owned(),
        "\tx\t".to_owned(),
        "  a + b".to_owned(),
        "a + b  ".to_owned(),
        "a\n+ b".to_owned(),
        "\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}".to_owned(),
        "  a = 1\n  b = 2".to_owned(),
        "".to_owned(),
        " ".to_owned(),
        "\n".to_owned(),
        "a\n\nb".to_owned(),
        "- x\n- y".to_owned(),
        "> x".to_owned(),
        "# x".to_owned(),
        "```\nx\n```".to_owned(),
        "$$ x $$".to_owned(),
        "x $$ y".to_owned(),
    ]
}

/// Formulas next to punctuation and digits, and neighbours.
fn neighbour_documents() -> Vec<String> {
    let mut out = Vec::new();
    for latex in ["x", "a+b", "\\alpha"] {
        for (before, after) in [
            ("(", ")"),
            ("", "."),
            ("", ","),
            ("5 ", ""),
            ("", " 5"),
            ("a", "b"),
            ("5", ""),
            ("", "5"),
            ("**5**", ""),
            ("", "**5**"),
            ("\\$", ""),
            ("", "\\$"),
            ("$5 and ", " and $10"),
            ("echo $PATH ", ""),
            ("`$x$` ", ""),
            ("\"", "\""),
            ("—", "—"),
        ] {
            for form in [Form::Dollar, Form::Paren, Form::DollarDisplayInline, Form::BracketInline] {
                out.push(format!("{before}{}{after}\n", form.write(latex)));
            }
        }
        // Two formulas with nothing between them.
        out.push(format!("\\({latex}\\)\\({latex}\\)\n"));
        out.push(format!("\\({latex}\\)$y$\n"));
        out.push(format!("${latex}$ ${latex}$\n"));
        out.push(format!("\\[{latex}\\]\\[{latex}\\]\n"));
        out.push(format!("Text\n```math\n{latex}\n```\nMore\n"));
        out.push(format!("$$\n{latex}\n$$\n$$\n{latex}\n$$\n"));
        out.push(format!("```math\n{latex}\n```\n```math\n{latex}\n```\n"));
        out.push(format!("Text $a$\n\n$$\n{latex}\n$$\n$b$ text\n"));
    }
    out
}

/// A xorshift generator, seeded.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const PROSE: &[&str] = &[
    "word", "text", " ", " ", " ", "\n", "\n\n", "5", "10", "$", "\\$", "*", "**", "_", "`", "- ", "> ", "1. ", "\t", "  \n",
    "\r\n", "|", ".", ",", "(", ")", "[", "]", "\\", "#", "$5", "echo $PATH", "é", "—", "[^1]", "[^1]: ", "    ", "```", "~~~",
];

/// A random document mixing prose and math.
fn random_document(random: &mut Random, formulas: &[String]) -> String {
    let mut text = String::new();
    let pieces = 1 + random.below(24);
    for _ in 0..pieces {
        if random.below(3) == 0 {
            let latex = if random.below(4) == 0 {
                const UNITS: &[&str] = &["a", "b", "1", " ", "\\", "$", "{", "}", "(", ")", "[", "]", "^", "_", "%", "&", "\n", "`", "*", "é", "\\alpha", "\\$"];
                (0..1 + random.below(8)).map(|_| *random.pick(UNITS)).collect::<String>()
            } else {
                random.pick(formulas).clone()
            };
            let form = *random.pick(FORMS);
            if form.is_block() && random.below(2) == 0 {
                text.push_str("\n\n");
            }
            text.push_str(&form.write(&latex));
            if form.is_block() && random.below(2) == 0 {
                text.push_str("\n\n");
            }
        } else {
            text.push_str(random.pick(PROSE));
        }
    }
    text
}

/// A corpus document with a few characters inserted, deleted or doubled.
fn mutated(random: &mut Random, text: &str) -> String {
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..1 + random.below(4) {
        if chars.is_empty() {
            break;
        }
        let at = random.below(chars.len());
        match random.below(3) {
            0 => {
                chars.remove(at);
            }
            1 => chars.insert(at, *random.pick(&['$', '\\', '\n', ' ', '5', '`', '>', '-', '{'])),
            _ => {
                let c = chars[at];
                chars.insert(at, c);
            }
        }
    }
    chars.into_iter().collect()
}

#[test]
fn copied_formulas_conform_to_the_dialect() {
    let started = std::time::Instant::now();
    let mut report = Report::default();

    // The corpus.
    let mut documents: Vec<(String, String)> = Vec::new();
    for dir in ["docs", "fixtures", "spec", "agent"] {
        for path in read_dir_sorted(&corpus().join(dir)) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                documents.push((format!("corpus-{dir}"), text));
            }
        }
    }
    if let Ok(extra) = std::env::var("UPLEFT_MATH_COPY_DOCS") {
        for path in extra.split(':').filter(|path| !path.is_empty()) {
            let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
            documents.push(("extra".to_owned(), text));
        }
    }
    let math_corpus: Vec<String> = documents
        .iter()
        .filter(|(_, text)| text.contains('$') || text.contains("\\(") || text.contains("\\[") || text.contains("math"))
        .map(|(_, text)| text.clone())
        .collect();
    for (category, text) in &documents {
        check_document(text, category, &mut report);
    }

    // Every form of every formula in every container, and whitespace.
    let mut formulas = corpus_formulas();
    formulas.extend(ADVERSARIAL.iter().map(|s| (*s).to_owned()));
    for latex in &formulas {
        for &form in FORMS {
            for (container, text) in containers(form, latex) {
                check_document(&text, &format!("container-{container}"), &mut report);
            }
            let text = format!("Before.\n\n{}\n\nAfter.\n", form.snippet(latex));
            for (variant, text) in whitespace_variants(&text) {
                check_document(&text, &format!("whitespace-{variant}"), &mut report);
            }
        }
    }
    for latex in padded_formulas() {
        for &form in FORMS {
            for (container, text) in containers(form, &latex) {
                if container == "paragraph" || container == "quote" || container == "list-loose" {
                    check_document(&text, "whitespace-inside", &mut report);
                }
            }
        }
    }
    for text in neighbour_documents() {
        check_document(&text, "neighbours", &mut report);
    }

    // `UPLEFT_MATH_COPY_DUMP=path` writes each distinct formula of the
    // corpus and the generated cases, as copied alone, one JSON string a
    // line, for an external TeX check.
    if let Ok(path) = std::env::var("UPLEFT_MATH_COPY_DUMP") {
        let mut seen = std::collections::BTreeSet::new();
        let mut texts: Vec<String> = documents.iter().map(|(_, text)| text.clone()).collect();
        for latex in &formulas {
            for &form in FORMS {
                texts.push(format!("Before.\n\n{}\n\nAfter.\n", form.snippet(latex)));
            }
        }
        for text in &texts {
            let document = upleft_core::parser::MarkdownParser::parse(text);
            let spans = upleft_render::view::math_copy::math_spans(&document);
            for span in &spans {
                seen.insert(upleft_render::view::math_copy_conformance::copy_source(&document, &spans, span.range));
            }
        }
        let lines: Vec<String> = seen.iter().map(|tex| serde_json::to_string(tex).expect("json")).collect();
        std::fs::write(&path, lines.join("\n") + "\n").expect("dump");
    }

    // Seeded random documents, and mutations of the corpus.
    let count: usize = std::env::var("UPLEFT_MATH_COPY_RANDOM").ok().and_then(|n| n.parse().ok()).unwrap_or(10_000);
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    for _ in 0..count {
        let text = random_document(&mut random, &formulas);
        check_document(&text, "random", &mut report);
        // Random selections, their ends anywhere, formulas and prose alike.
        let length = text.encode_utf16().count();
        if length > 1 {
            for _ in 0..2 {
                let a = random.below(length);
                let b = random.below(length);
                let selection = NSRange::new(a.min(b) as isize, (a.max(b) - a.min(b)).max(1) as isize);
                check_selection(&text, "random", selection, &mut report);
            }
        }
    }
    for _ in 0..count / 10 {
        if math_corpus.is_empty() {
            break;
        }
        let base = random.pick(&math_corpus).clone();
        let text = mutated(&mut random, &base);
        check_document(&text, "random-mutation", &mut report);
    }

    println!("math copy conformance, {:.1?}\n\n{}", started.elapsed(), report.summary());
    let mut seen = std::collections::BTreeSet::new();
    for failure in report.failures.iter().filter(|failure| !failure.property.starts_with("residual:")).chain(
        report.failures.iter().filter(|failure| failure.property.starts_with("residual:")),
    ) {
        println!("FAIL {} / {}\n{}", failure.property, failure.category, failure.detail);
        if seen.insert(failure.property.clone()) && failure.input.len() < 4000 {
            println!("minimized: {:?}", minimize(&failure.input, &failure.property));
        } else {
            println!("input: {:?}", failure.input.chars().take(400).collect::<String>());
        }
        println!();
    }
    assert!(report.failed() == 0, "{} failures", report.failed());
}
