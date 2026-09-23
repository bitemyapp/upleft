//! Port of `Tests/MarkdownRenderTests/SyntaxTests.swift`: syntax
//! highlighting (§11.3).

use std::collections::HashSet;

use upleft_render::syntax::builtin_syntax_highlighter::BuiltinSyntaxHighlighter;
use upleft_render::syntax::syntax_contracts::{SyntaxHighlighter, SyntaxRun, SyntaxToken};

use SyntaxToken as T;

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn highlight(code: &str, language: Option<&str>) -> Vec<SyntaxRun> {
    BuiltinSyntaxHighlighter::shared().highlight(&utf16(code), language)
}

/// `NSString.range(of:options: .literal, range:)`, occurrence by occurrence,
/// in UTF-16 units.
fn range_of(text: &str, occurrence: usize, code: &str) -> Option<(usize, usize)> {
    let haystack = utf16(code);
    let needle = utf16(text);
    let mut search_start = 0usize;
    let mut found = None;
    for _ in 0..=occurrence {
        if search_start > haystack.len() {
            return None;
        }
        let position = haystack[search_start..]
            .windows(needle.len())
            .position(|window| window == needle.as_slice())?;
        let location = search_start + position;
        found = Some((location, needle.len()));
        search_start = location + needle.len().max(1);
    }
    found
}

/// Runs must be ascending, non-overlapping, non-empty, and inside the input.
fn expect_run_invariants(runs: &[SyntaxRun], code: &str, language: &str) {
    let length = utf16(code).len();
    let mut previous_end = 0;
    for run in runs {
        assert!(
            run.range.location >= previous_end,
            "{language}: runs overlap or descend"
        );
        assert!(run.range.length > 0, "{language}: empty run");
        assert!(
            run.range.location + run.range.length <= length,
            "{language}: run past the end of the input"
        );
        previous_end = run.range.location + run.range.length;
    }
}

fn expect_token(
    text: &str,
    expected: SyntaxToken,
    occurrence: usize,
    code: &str,
    runs: &[SyntaxRun],
    language: &str,
) {
    let Some((location, length)) = range_of(text, occurrence, code) else {
        panic!(
            "{language}: the snippet has fewer than {} copies of {text}",
            occurrence + 1
        );
    };
    let covering = runs.iter().find(|run| {
        let start = run.range.location.max(location);
        let end = (run.range.location + run.range.length).min(location + length);
        end.saturating_sub(start) == length
    });
    let Some(run) = covering else {
        panic!(
            "{language}: no single run covers {text} at {{{location}, {length}}}; runs: {runs:?}"
        );
    };
    assert_eq!(run.token, expected, "{language}: {text}");
}

// MARK: - Language registry

#[test]
fn supported_languages_cover_the_spec_list() {
    let required = [
        "swift",
        "typescript",
        "javascript",
        "tsx",
        "jsx",
        "python",
        "rust",
        "go",
        "ruby",
        "java",
        "c",
        "cpp",
        "objc",
        "bash",
        "json",
        "yaml",
        "toml",
        "sql",
        "html",
        "css",
        "xml",
        "markdown",
        "diff",
        "plaintext",
    ];
    let supported: HashSet<&str> = BuiltinSyntaxHighlighter::supported_languages()
        .iter()
        .copied()
        .collect();
    for language in required {
        assert!(supported.contains(language), "missing language: {language}");
    }
    assert_eq!(
        supported.len(),
        BuiltinSyntaxHighlighter::supported_languages().len(),
        "duplicate canonical name"
    );
}

#[test]
fn aliases_resolve() {
    let expected = [
        ("ts", "typescript"),
        ("TS", "typescript"),
        ("js", "javascript"),
        ("sh", "bash"),
        ("zsh", "bash"),
        ("shell", "bash"),
        ("py", "python"),
        ("rs", "rust"),
        ("rb", "ruby"),
        ("yml", "yaml"),
        ("c++", "cpp"),
        ("objective-c", "objc"),
        ("golang", "go"),
        ("md", "markdown"),
        ("patch", "diff"),
        ("txt", "plaintext"),
        (" Swift ", "swift"),
    ];
    for (alias, canonical) in expected {
        assert_eq!(
            BuiltinSyntaxHighlighter::canonical_language(alias),
            Some(canonical),
            "alias {alias}"
        );
    }
    assert_eq!(
        BuiltinSyntaxHighlighter::canonical_language("brainfuck"),
        None
    );
    assert_eq!(BuiltinSyntaxHighlighter::canonical_language(""), None);
    assert!(BuiltinSyntaxHighlighter::shared().supports("TSX"));
    assert!(!BuiltinSyntaxHighlighter::shared().supports("cobol"));
}

/// Guessing a language colours the wrong words confidently, so an unknown one
/// produces nothing at all.
#[test]
fn unknown_languages_produce_no_runs() {
    assert!(highlight("let x = 1", None).is_empty());
    assert!(highlight("let x = 1", Some("cobol")).is_empty());
    assert!(highlight("plain words", Some("plaintext")).is_empty());
    assert!(highlight("", Some("swift")).is_empty());
}

// MARK: - Per-language classification

struct Snippet {
    language: &'static str,
    code: &'static str,
    expectations: &'static [(&'static str, SyntaxToken, usize)],
}

const SNIPPETS: &[Snippet] = &[
    Snippet {
        language: "swift",
        code: "// A greeting.\n@MainActor\nstruct Greeter {\n    let name = \"world // not a comment\"\n    func greet() -> Int { 42 }\n}",
        expectations: &[
            ("// A greeting.", T::Comment, 0),
            ("@MainActor", T::Attribute, 0),
            ("struct", T::Keyword, 0),
            ("Greeter", T::Type, 0),
            ("\"world // not a comment\"", T::String, 0),
            // Occurrence 1: "greet" also appears inside "// A greeting.".
            ("greet", T::Function, 1),
            ("Int", T::Type, 0),
            ("42", T::Number, 0),
        ],
    },
    Snippet {
        language: "typescript",
        code: "export interface User { id: number }\nconst greet = (u: User): string => `hi ${u.id}`;",
        expectations: &[
            ("export", T::Keyword, 0),
            ("interface", T::Keyword, 0),
            ("number", T::Type, 0),
            ("User", T::Type, 0),
            ("const", T::Keyword, 0),
            ("`hi ${u.id}`", T::String, 0),
        ],
    },
    Snippet {
        language: "javascript",
        code: "const MAX = 10;\nexport default function run(items) { return items.map(x => x * MAX); }",
        expectations: &[
            ("const", T::Keyword, 0),
            ("MAX", T::Constant, 0),
            ("10", T::Number, 0),
            ("run", T::Function, 0),
        ],
    },
    Snippet {
        language: "python",
        code: "import math\n\ndef area(r: float) -> float:\n    return math.pi * r ** 2  # circle",
        expectations: &[
            ("import", T::Keyword, 0),
            ("def", T::Keyword, 0),
            ("area", T::Function, 0),
            ("float", T::Type, 0),
            ("2", T::Number, 0),
            ("# circle", T::Comment, 0),
        ],
    },
    Snippet {
        language: "rust",
        code: "#[derive(Debug)]\npub struct Point { x: f64 }\n\nimpl Point {\n    pub fn origin() -> Self { Point { x: 0.0 } }\n}",
        expectations: &[
            ("#[derive(Debug)]", T::Attribute, 0),
            ("pub", T::Keyword, 0),
            ("struct", T::Keyword, 0),
            ("f64", T::Type, 0),
            ("origin", T::Function, 0),
            ("0.0", T::Number, 0),
        ],
    },
    Snippet {
        language: "go",
        code: "package main\n\nfunc Sum(xs []int) int {\n    raw := `line one\nline two`\n    _ = raw\n    return 0\n}",
        expectations: &[
            ("package", T::Keyword, 0),
            ("func", T::Keyword, 0),
            ("int", T::Type, 0),
            ("`line one\nline two`", T::String, 0),
            ("Sum", T::Function, 0),
        ],
    },
    Snippet {
        language: "ruby",
        code: "class Greeter\n  def greet(name)\n    @count += 1\n    puts \"hello #{name} # not a comment\"  # real comment\n  end\nend",
        expectations: &[
            ("class", T::Keyword, 0),
            ("Greeter", T::Type, 0),
            ("@count", T::Variable, 0),
            ("\"hello #{name} # not a comment\"", T::String, 0),
            ("# real comment", T::Comment, 0),
            ("greet", T::Function, 0),
        ],
    },
    Snippet {
        language: "java",
        code: "public class Main {\n    private static final int MAX_SIZE = 10;\n    @Override public String toString() { return \"x\"; }\n}",
        expectations: &[
            ("public", T::Keyword, 0),
            ("MAX_SIZE", T::Constant, 0),
            ("int", T::Type, 0),
            ("@Override", T::Attribute, 0),
            ("String", T::Type, 0),
            ("toString", T::Function, 0),
        ],
    },
    Snippet {
        language: "c",
        code: "#include <stdio.h>\n\nint main(void) {\n    printf(\"%d\\n\", 42);\n    return 0;\n}",
        expectations: &[
            ("#include", T::Attribute, 0),
            ("int", T::Type, 0),
            ("void", T::Type, 0),
            ("printf", T::Function, 0),
            ("42", T::Number, 0),
        ],
    },
    Snippet {
        language: "cpp",
        code: "auto s = R\"json({\"a\": 1})json\";\nconstexpr int kMax = 3;",
        expectations: &[
            ("R\"json({\"a\": 1})json\"", T::String, 0),
            ("constexpr", T::Keyword, 0),
            ("int", T::Type, 0),
            ("3", T::Number, 0),
        ],
    },
    Snippet {
        language: "objc",
        code: "@interface Greeter : NSObject\n@end\n\nNSString *greeting = @\"hi\";",
        expectations: &[
            ("@interface", T::Attribute, 0),
            ("NSObject", T::Type, 0),
            ("NSString", T::Type, 0),
            ("@\"hi\"", T::String, 0),
        ],
    },
    Snippet {
        language: "bash",
        code: "#!/usr/bin/env bash\nset -euo pipefail\nfor f in *.md; do\n  echo \"found $f\"\ndone",
        expectations: &[
            ("#!/usr/bin/env bash", T::Comment, 0),
            ("for", T::Keyword, 0),
            ("do", T::Keyword, 0),
            ("echo", T::Function, 0),
            ("\"found $f\"", T::String, 0),
        ],
    },
    Snippet {
        language: "json",
        code: "{\"name\": \"downright\", \"version\": 2, \"ok\": true, \"extra\": null}",
        expectations: &[
            ("\"name\"", T::Attribute, 0),
            ("\"downright\"", T::String, 0),
            ("2", T::Number, 0),
            ("true", T::Constant, 0),
            ("null", T::Constant, 0),
        ],
    },
    Snippet {
        language: "yaml",
        code: "name: downright  # a comment\nitems:\n  - one\n  - \"two: not a key\"",
        expectations: &[
            ("name", T::Attribute, 0),
            ("items", T::Attribute, 0),
            ("# a comment", T::Comment, 0),
            ("\"two: not a key\"", T::String, 0),
        ],
    },
    Snippet {
        language: "toml",
        code: "[package]\nname = \"downright\"\nedition = 2021",
        expectations: &[
            ("[package]", T::Type, 0),
            ("name", T::Attribute, 0),
            ("\"downright\"", T::String, 0),
            ("2021", T::Number, 0),
        ],
    },
    Snippet {
        language: "sql",
        code: "-- find people\nSELECT id, name FROM users WHERE name = 'O''Brien';",
        expectations: &[
            ("-- find people", T::Comment, 0),
            ("SELECT", T::Keyword, 0),
            ("FROM", T::Keyword, 0),
            ("'O''Brien'", T::String, 0),
        ],
    },
    Snippet {
        language: "css",
        code: "/* heading */\n.title { color: #ff0000; margin: -5px; }",
        expectations: &[
            ("/* heading */", T::Comment, 0),
            ("color", T::Attribute, 0),
            ("#ff0000", T::Constant, 0),
            ("5px", T::Number, 0),
        ],
    },
    Snippet {
        language: "html",
        code: "<!-- note -->\n<div class=\"a\">text &amp; more</div>",
        expectations: &[
            ("<!-- note -->", T::Comment, 0),
            ("div", T::Type, 0),
            ("class", T::Attribute, 0),
            ("\"a\"", T::String, 0),
            ("&amp;", T::Constant, 0),
        ],
    },
    Snippet {
        language: "xml",
        code: "<?xml version=\"1.0\"?>\n<root><item id=\"1\">v</item></root>",
        expectations: &[
            ("<?xml version=\"1.0\"?>", T::Attribute, 0),
            ("root", T::Type, 0),
            ("id", T::Attribute, 0),
            ("\"1\"", T::String, 0),
        ],
    },
    Snippet {
        language: "jsx",
        code: "export default function App() {\n  return <div className=\"app\">{title}</div>;\n}",
        expectations: &[
            ("export", T::Keyword, 0),
            ("function", T::Keyword, 0),
            ("App", T::Function, 0),
            ("\"app\"", T::String, 0),
        ],
    },
    Snippet {
        language: "tsx",
        code: "const Panel = (props: Props): JSX.Element => <section>{props.title}</section>;",
        expectations: &[
            ("const", T::Keyword, 0),
            ("Props", T::Type, 0),
            ("section", T::Plain, 0),
        ],
    },
    Snippet {
        language: "markdown",
        code: "# Title\n\nSome *emphasis* and `code`.\n\n- item [link](https://example.com)\n\n```swift\nlet x = 1\n```",
        expectations: &[
            ("# Title", T::Keyword, 0),
            ("`code`", T::String, 0),
            ("(https://example.com)", T::String, 0),
            ("```swift", T::Attribute, 0),
            ("let x = 1", T::String, 0),
        ],
    },
];

#[test]
fn language_snippets() {
    for snippet in SNIPPETS {
        let runs = highlight(snippet.code, Some(snippet.language));
        expect_run_invariants(&runs, snippet.code, snippet.language);
        assert!(!runs.is_empty(), "{} produced no runs", snippet.language);
        for (text, token, occurrence) in snippet.expectations {
            expect_token(
                text,
                *token,
                *occurrence,
                snippet.code,
                &runs,
                snippet.language,
            );
        }
    }
}

// MARK: - The cases that separate a lexer from a pile of regexes

#[test]
fn comment_markers_inside_strings() {
    let cases = [
        (
            "swift",
            r#"let s = "a // b /* c */ d""#,
            r#""a // b /* c */ d""#,
        ),
        ("python", "s = 'a # b'", "'a # b'"),
        (
            "bash",
            r#"echo "hash # inside string""#,
            r#""hash # inside string""#,
        ),
        ("sql", "SELECT 'a -- b'", "'a -- b'"),
    ];
    for (language, code, literal) in cases {
        let runs = highlight(code, Some(language));
        expect_token(literal, T::String, 0, code, &runs, language);
        assert!(
            !runs.iter().any(|run| run.token == T::Comment),
            "{language}: a comment marker inside a string opened a comment"
        );
    }
}

#[test]
fn quotes_inside_comments() {
    let code = "// he said \"hi and never closed it\nlet x = 1";
    let runs = highlight(code, Some("swift"));
    expect_token(
        "// he said \"hi and never closed it",
        T::Comment,
        0,
        code,
        &runs,
        "swift",
    );
    expect_token("let", T::Keyword, 0, code, &runs, "swift");
    assert!(
        !runs.iter().any(|run| run.token == T::String),
        "a quote inside a comment opened a string"
    );
}

#[test]
fn nested_block_comments() {
    let swift = "/* outer /* inner */ still outer */ let x = 1";
    let runs = highlight(swift, Some("swift"));
    expect_token(
        "/* outer /* inner */ still outer */",
        T::Comment,
        0,
        swift,
        &runs,
        "swift",
    );
    expect_token("let", T::Keyword, 0, swift, &runs, "swift");

    // C does not nest: the first `*/` closes it, and `int` is code again.
    let c = "/* outer /* inner */ int x;";
    let runs = highlight(c, Some("c"));
    expect_token("/* outer /* inner */", T::Comment, 0, c, &runs, "c");
    expect_token("int", T::Type, 0, c, &runs, "c");
}

#[test]
fn python_strings() {
    let code = "def f(x):\n    \"\"\"Doc with # not a comment and 'quotes'.\"\"\"\n    return f\"value {x}\"  # real comment";
    let runs = highlight(code, Some("python"));
    expect_token(
        "\"\"\"Doc with # not a comment and 'quotes'.\"\"\"",
        T::String,
        0,
        code,
        &runs,
        "python",
    );
    expect_token("f\"value {x}\"", T::String, 0, code, &runs, "python");
    expect_token("# real comment", T::Comment, 0, code, &runs, "python");
    expect_token("def", T::Keyword, 0, code, &runs, "python");
}

#[test]
fn raw_strings() {
    let swift = r##"let s = #"a "quoted" thing"# + "plain""##;
    let runs = highlight(swift, Some("swift"));
    expect_token(
        r##"#"a "quoted" thing"#"##,
        T::String,
        0,
        swift,
        &runs,
        "swift",
    );
    expect_token(r#""plain""#, T::String, 0, swift, &runs, "swift");

    let rust = r##"let s = r#"say "hi""#; let b = b"bytes";"##;
    let runs = highlight(rust, Some("rust"));
    expect_token(r##"r#"say "hi""#"##, T::String, 0, rust, &runs, "rust");
    expect_token(r#"b"bytes""#, T::String, 0, rust, &runs, "rust");
}

#[test]
fn rust_lifetimes() {
    let code = "fn take<'a>(s: &'a str) -> char { 'x' }";
    let runs = highlight(code, Some("rust"));
    expect_token("'a", T::Keyword, 0, code, &runs, "rust");
    expect_token("'a", T::Keyword, 1, code, &runs, "rust");
    expect_token("'x'", T::String, 0, code, &runs, "rust");
    expect_token("str", T::Type, 0, code, &runs, "rust");
}

#[test]
fn shell_hash_rules() {
    let code = "name=${USER#prefix}\necho $name  # trailing comment";
    let runs = highlight(code, Some("bash"));
    expect_token("${USER#prefix}", T::Variable, 0, code, &runs, "bash");
    expect_token("# trailing comment", T::Comment, 0, code, &runs, "bash");
    assert_eq!(runs.iter().filter(|run| run.token == T::Comment).count(), 1);
}

#[test]
fn diff_colouring() {
    let code = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n context stays plain\n-removed line\n+added line";
    let runs = highlight(code, Some("diff"));
    expect_run_invariants(&runs, code, "diff");
    for header in [
        "diff --git a/a.txt b/a.txt",
        "--- a/a.txt",
        "+++ b/a.txt",
        "@@ -1,3 +1,3 @@",
    ] {
        expect_token(header, T::DiffHeader, 0, code, &runs, "diff");
    }
    expect_token("-removed line", T::DiffRemoved, 0, code, &runs, "diff");
    expect_token("+added line", T::DiffAdded, 0, code, &runs, "diff");
    expect_token(" context stays plain", T::Plain, 0, code, &runs, "diff");
}

#[test]
fn multi_byte_characters() {
    let code = "let 🎉 = \"party 🎉 time\"";
    let runs = highlight(code, Some("swift"));
    expect_run_invariants(&runs, code, "swift");
    let units = utf16(code);
    for run in runs.iter().filter(|run| run.range.location > 0) {
        let previous = units[run.range.location - 1];
        assert!(
            !(0xD800..=0xDBFF).contains(&previous),
            "a run starts inside a surrogate pair"
        );
    }
}

// MARK: - Port-specific checks

/// The `&str` convenience agrees with the UTF-16 entry point.
#[test]
fn string_and_utf16_entry_points_agree() {
    for snippet in SNIPPETS {
        let direct = highlight(snippet.code, Some(snippet.language));
        let via_str =
            BuiltinSyntaxHighlighter::shared().highlight_str(snippet.code, Some(snippet.language));
        assert_eq!(direct, via_str, "{}", snippet.language);
    }
}

/// `SyntaxRunCache` returns exactly what the highlighter returns, and keeps
/// the least recently used quarter out when it overflows.
#[test]
fn syntax_run_cache_is_transparent() {
    use upleft_render::engine::syntax_run_cache::SyntaxRunCache;
    let cache = SyntaxRunCache::new(8);
    let highlighter = BuiltinSyntaxHighlighter::shared();
    for index in 0..20 {
        let code = utf16(&format!("let x{index} = {index}"));
        let runs = cache.runs(&code, Some("swift"), highlighter);
        assert_eq!(
            &*runs,
            highlighter.highlight(&code, Some("swift")).as_slice()
        );
        assert!(cache.len() <= 8);
    }
    // Canonically equivalent code of the same length hits the same entry.
    let composed = utf16("a\u{301}\u{316}");
    let reordered = utf16("a\u{316}\u{301}");
    let first = cache.runs(&composed, Some("swift"), highlighter);
    let second = cache.runs(&reordered, Some("swift"), highlighter);
    assert_eq!(first, second);
}
