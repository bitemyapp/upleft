//! `upleft-bench` — Downright's `drbench` (`Sources/drbench/main.swift`),
//! stage for stage and line for line, run against Upleft.
//!
//!   cargo run --release -p upleft-bench
//!
//! The corpus generator, the stages, the sample counts, the percentile rule
//! and the output format are the Swift's, so the two reports compare line by
//! line. Stages whose layer is not ported yet are omitted rather than faked.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use objc2::rc::Retained;
use objc2::{AnyThread, msg_send};
use objc2_app_kit::{NSAppearance, NSTextStorage};
use objc2_foundation::{NSRange as FoundationRange, NSString};
use upleft_core::ast_diff::ASTDiff;
use upleft_core::metrics::Metrics;
use upleft_core::parser::MarkdownParser;
use upleft_core::structural_zoom::StructuralZoom;
use upleft_core::text_diff::TextDiff;
use upleft_core::tidy::TidyDocument;
use upleft_core::{DirtySet, ParseOptions, ZoomLevel};
use upleft_markup::Document;
use upleft_markup::parser::parse_options::ParseOptions as MarkupParseOptions;
use upleft_render::engine::decoration_engine::DecorationEngine;
use upleft_render::engine::display_map::{DisplayMap, ParagraphIndex};
use upleft_render::render_contracts::Theme;
use upleft_render::syntax::builtin_syntax_highlighter::BuiltinSyntaxHighlighter;
use upleft_render::syntax::syntax_contracts::SyntaxHighlighter;
use upleft_render::theme::style_sheet::StyleSheet;

// MARK: - Harness

/// Set when a measured p95 misses its budget.
static BUDGET_VIOLATED: AtomicBool = AtomicBool::new(false);

/// Nearest-rank percentile over an ascending sample array.
fn percentile(ascending: &[f64], p: f64) -> f64 {
    assert!(!ascending.is_empty());
    let rank = (p * ascending.len() as f64).ceil() as isize;
    ascending[(ascending.len() as isize - 1).min(0.max(rank - 1)) as usize]
}

fn measure(label: &str, budget: Option<f64>, runs: usize, mut body: impl FnMut()) -> f64 {
    // One warm-up so first-call lazy initialisation isn't charged to the p50.
    body();
    let mut samples: Vec<f64> = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        body();
        samples.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    let p50 = percentile(&samples, 0.50);
    let p95 = percentile(&samples, 0.95);

    // `String(format: "  %-44@  …")`: Foundation ignores the width for `%@`,
    // so the label is not padded.
    let mut line = format!("  {label}  p50 {p50:8.3} ms   p95 {p95:8.3} ms");
    line += &format!("   max {:8.3} ms (n={})", samples.last().unwrap(), samples.len());
    if let Some(budget) = budget {
        line += &if p95 <= budget {
            format!("   ✓ under {budget:.0} ms")
        } else {
            format!("   ✗ BUDGET {budget:.0} ms")
        };
        if p95 > budget {
            BUDGET_VIOLATED.store(true, Ordering::Relaxed);
        }
    }
    println!("{line}");
    p95
}

// MARK: - Corpus

/// A document shaped like agent output: headings every few lines, task lists,
/// fenced code, tables, inline paths.
fn agent_document(target_lines: usize) -> String {
    let mut out = String::new();
    let mut line_count = 0usize;
    let mut index = 0usize;
    while line_count < target_lines {
        index += 1;
        let block = format!(
            "## Section {index}\n\nA paragraph with **bold**, `code`, a [link](https://example.com), and a\npath reference `src/module{index}/file.ts:{index}` that resolves.\n\n- [ ] first task for section {index}\n- [x] second task\n- a plain item\n"
        );
        out += &block;
        line_count += block.bytes().filter(|&b| b == b'\n').count();
        if index.is_multiple_of(7) {
            out += &format!(
                "```swift\nlet value{index} = {index}\nfunc compute{index}() -> Int {{ value{index} * 2 }}\n```\n\n"
            );
            line_count += 6;
        }
        if index.is_multiple_of(11) {
            out += &format!("| column | value |\n|---|--:|\n| a | {index} |\n| b | {} |\n\n", index * 2);
            line_count += 6;
        }
        if index.is_multiple_of(13) {
            out += "> [!NOTE]\n> A callout, because agents emit these constantly.\n\n";
            line_count += 3;
        }
    }
    out
}

fn text_storage(text: &str) -> Retained<NSTextStorage> {
    let string = NSString::from_str(text);
    // SAFETY: `initWithString:` on a freshly allocated NSTextStorage.
    unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*string] }
}

fn main() {
    let document5k = agent_document(5_000);
    // Generate past the 100 KB target and truncate to it (every character is
    // ASCII, so bytes are Characters).
    let mut document100k = agent_document(6_000);
    document100k.truncate(100_000);

    println!(
        "\nUpleft performance budget (§12)\n  build: {}\n  corpus: {} chars, {} lines\n  cold-open corpus: {} chars\n",
        if cfg!(debug_assertions) {
            "DEBUG — numbers are not the product promise, rebuild with --release"
        } else {
            "release"
        },
        document5k.len(),
        document5k.bytes().filter(|&b| b == b'\n').count(),
        document100k.len()
    );

    println!("Parse (§3.5 — full reparse on every edit)");
    measure("cmark alone", None, 15, || {
        std::hint::black_box(Document::parse(&document5k, MarkupParseOptions::DISABLE_SMART_OPTS));
    });
    measure("MarkdownParser.parse, all passes", None, 15, || {
        std::hint::black_box(MarkdownParser::parse(&document5k));
    });
    measure("  … extension passes off", None, 15, || {
        std::hint::black_box(MarkdownParser::parse_with(
            &document5k,
            ParseOptions {
                detect_front_matter: false,
                detect_math: false,
                detect_callouts: false,
                detect_wikilinks: false,
                detect_path_tokens: false,
                detect_mermaid: false,
                ..ParseOptions::DEFAULT
            },
        ));
    });
    type Variant = (&'static str, fn(&mut ParseOptions));
    let variants: [Variant; 4] = [
        ("  … without path tokens", |o| o.detect_path_tokens = false),
        ("  … without math", |o| o.detect_math = false),
        ("  … without wikilinks", |o| o.detect_wikilinks = false),
        ("  … without callouts", |o| o.detect_callouts = false),
    ];
    for (name, mutate) in variants {
        let mut options = ParseOptions::DEFAULT;
        mutate(&mut options);
        measure(name, None, 15, || {
            std::hint::black_box(MarkdownParser::parse_with(&document5k, options));
        });
    }

    println!("\nDiff");
    let baseline = MarkdownParser::parse(&document5k);
    let mut edited_text = document5k.clone();
    edited_text.insert(edited_text.len() / 2, 'x');
    let edited = MarkdownParser::parse(&edited_text);
    measure("ASTDiff.dirtySet, one-character edit", None, 25, || {
        std::hint::black_box(ASTDiff::dirty_set(Some(&baseline), &edited));
    });
    measure("TextDiff.hunks, external rewrite", None, 10, || {
        std::hint::black_box(TextDiff::hunks(&document5k, &edited_text));
    });

    println!("\nDecorate (§12 — keystroke → updated render, budget 8 ms p95)");
    let style_sheet = StyleSheet::new(Theme::fallback(), &NSAppearance::currentDrawingAppearance(), None);
    let mut engine = DecorationEngine::new(style_sheet);
    let storage = NSTextStorage::new();
    storage.replaceCharactersInRange_withString(FoundationRange::new(0, 0), &NSString::from_str(&document5k));
    let dirty = ASTDiff::dirty_set(Some(&baseline), &edited);
    measure("incremental, one dirty block", Some(8.0), 25, || {
        engine.decorate(&storage, &baseline, &dirty);
    });
    measure("wholesale (mode switch)", None, 5, || {
        engine.decorate(&storage, &baseline, &DirtySet::wholesale());
    });

    println!("\nSynchronous typing response (§12 — main-thread budget 8 ms p95)");
    let response_storage = text_storage(&document5k);
    let mut response_caret = response_storage.length() as isize / 2;
    let x = NSString::from_str("x");
    let typing_p95 = measure("edit + paragraph map", Some(8.0), 100, || {
        response_storage.replaceCharactersInRange_withString(FoundationRange::new(response_caret as usize, 0), &x);
        let paragraphs = ParagraphIndex::from_text(&response_storage.string());
        let map = DisplayMap::with_hidden(paragraphs, &[]);
        std::hint::black_box(map.text_kit_offset_for_source(response_caret));
        response_caret += 1;
    });

    println!("\nSemantic convergence (end to end; outside the typing budget)");
    let convergence_storage = text_storage(&edited_text);
    let convergence_p95 = measure("worker pipeline", Some(100.0), 60, || {
        let fresh = MarkdownParser::parse(&edited_text);
        let set = ASTDiff::dirty_set(Some(&baseline), &fresh);
        engine.decorate(&convergence_storage, &fresh, &set);
    });

    println!("  … phase breakdown (same corpus, not separately budgeted)");
    measure("  parse phase", None, 15, || {
        std::hint::black_box(MarkdownParser::parse(&edited_text));
    });
    measure("  diff phase", None, 15, || {
        std::hint::black_box(ASTDiff::dirty_set(Some(&baseline), &edited));
    });
    measure("  decoration phase", None, 15, || {
        engine.decorate(&convergence_storage, &edited, &dirty);
    });

    println!("\nCold open (§12 — first rendered pixel under 250 ms for 100 KB)");
    measure("parse 100 KB", Some(250.0), 30, || {
        std::hint::black_box(MarkdownParser::parse(&document100k));
    });

    println!("\nOther");
    measure("StructuralZoom.plan, skeleton", None, 10, || {
        std::hint::black_box(StructuralZoom::plan(&baseline, ZoomLevel::Skeleton));
    });
    measure("Metrics.metrics", None, 10, || {
        std::hint::black_box(Metrics::metrics_for(&document5k));
    });
    measure("TidyDocument.plan", None, 10, || {
        std::hint::black_box(TidyDocument::plan(&baseline));
    });
    let code: Vec<u16> = "func f() -> Int { let x = \"s\" // c\n return 1 }\n"
        .repeat(200)
        .encode_utf16()
        .collect();
    measure("syntax highlight 10 KB Swift", None, 20, || {
        std::hint::black_box(BuiltinSyntaxHighlighter::shared().highlight(&code, Some("swift")));
    });

    println!(
        "\nTyping response p95: {typing_p95:.2} ms against an 8 ms budget.\nEnd-to-end semantic convergence p95: {convergence_p95:.2} ms against a 100 ms budget."
    );

    if BUDGET_VIOLATED.load(Ordering::Relaxed) && !cfg!(debug_assertions) {
        println!("\n❌ One or more budgets were missed. The performance budget is the product promise (§12).");
        std::process::exit(1);
    }
}
