//! Corpus.swift — documents that have historically broken markdown tooling,
//! shared by the ported MarkdownCore tests.
#![allow(dead_code)]

pub const KITCHEN_SINK: &str = "---\ntitle: Release Plan\ntags: [alpha, beta]\nowner: \"Ada Lovelace\"\n---\n\n# Release Plan\n\nIntro paragraph with **bold**, *italic*, `code`, ~~strike~~ and a [link](docs/plan.md).\nSee src/auth/session.ts:42 for the handler.\n\n## Phase one\n\n> [!NOTE] Read this first\n> The plan changed on Tuesday.\n\n- [ ] Wire the parser\n- [x] Land the model\n    - nested detail\n    - more detail\n\n1. First\n2. Second\n3. Third\n\n```swift\nfunc greet() -> String { \"hi\" }\n```\n\n| Name | Count | Notes |\n|:-----|------:|:-----:|\n| a | 1 | x |\n| bb | 22 | yy |\n\n### Phase one details\n\nInline math $x^2 + y^2$ and a display block:\n\n$$\ne^{i\\pi} + 1 = 0\n$$\n\nSee [[Design Notes|the notes]] for more.\n\n## Phase two\n\nSome prose.  A second sentence lives here.\n\n---\n\nFinal paragraph.[^1]\n\n[^1]: A footnote body.";

pub const NESTED_LISTS: &str = "- top\n  - second\n    - third\n      continued line\n  - back to second\n- another top\n\n1. one\n   1. one-a\n   2. one-b\n2. two";

pub const HTML_AND_CODE: &str = "<div class=\"warning\">\n  <p>Raw HTML block</p>\n</div>\n\nText after html.\n\n    indented code block\n    second line\n\n~~~python\ndef main():\n    pass\n~~~";

pub const ODD_SPACING: &str = "para one\n\n\n\n\npara two   \n\nline with break  \nnext line\n\n\n";

pub const TABS: &str = "\t- tab indented item\n\t\t- deeper\n\nnormal\n";

pub const NO_TRAILING_NEWLINE: &str = "# Title\n\nBody without a trailing newline.";

pub const CRLF: &str = "# Title\r\n\r\nA paragraph.\r\n\r\n- item\r\n- item two\r\n";

pub const MIXED_ENDINGS: &str = "line one\nline two\r\nline three\rline four\n";

pub const UNICODE: &str = "# Überschrift mit Ümlauten\n\nEin Absatz mit *Betonung* und `Código` und 日本語のテキスト.\n\nEmoji 🎉 in a **bold** run, then more text.\n\n| Spalte | Wert |\n| --- | --- |\n| café | 1 |\n| 日本 | 22 |";

pub const SHELL_SNIPPETS: &str = "Run `echo $PATH` to see it.\n\nIt costs $5 and $10 respectively, or $100 total.\n\n```bash\necho $HOME\nx=$(pwd)\n```\n\nUse $VAR and $(cmd) freely.";

/// Everything above, for sweeps that want maximum surface.
pub const ALL: &[(&str, &str)] = &[
    ("kitchenSink", KITCHEN_SINK),
    ("nestedLists", NESTED_LISTS),
    ("htmlAndCode", HTML_AND_CODE),
    ("oddSpacing", ODD_SPACING),
    ("tabs", TABS),
    ("noTrailingNewline", NO_TRAILING_NEWLINE),
    ("crlf", CRLF),
    ("mixedEndings", MIXED_ENDINGS),
    ("unicode", UNICODE),
    ("shellSnippets", SHELL_SNIPPETS),
];

/// A synthetic block document for the AST-diff locality test.
pub fn many_blocks(count: usize) -> String {
    (0..count)
        .map(|index| if index % 5 == 0 { format!("## Section {index}") } else { format!("Paragraph number {index} with some words in it.") })
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}
