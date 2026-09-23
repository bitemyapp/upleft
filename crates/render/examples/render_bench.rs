//! The render-foundation stages of drbench (`Sources/drbench/main.swift`),
//! measured the same way: one warm-up, then `runs` samples timed with the
//! uptime clock, nearest-rank p50/p95.
//!
//!   cargo run --release -p upleft-render --example render_bench

use std::time::Instant;

use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua};
use upleft_render::syntax::builtin_syntax_highlighter::BuiltinSyntaxHighlighter;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

/// Nearest-rank percentile over an ascending sample array.
fn percentile(ascending: &[f64], p: f64) -> f64 {
    assert!(!ascending.is_empty());
    let rank = (p * ascending.len() as f64).ceil() as i64;
    ascending[(ascending.len() as i64 - 1).min(0.max(rank - 1)) as usize]
}

fn measure(label: &str, runs: usize, mut body: impl FnMut()) -> f64 {
    // One warm-up so first-call lazy initialisation isn't charged to the p50.
    body();
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        body();
        samples.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }
    samples.sort_by(f64::total_cmp);
    let p50 = percentile(&samples, 0.50);
    let p95 = percentile(&samples, 0.95);
    println!(
        "  {label:<44}  p50 {p50:8.3} ms   p95 {p95:8.3} ms   max {:8.3} ms (n={})",
        samples.last().unwrap(),
        samples.len()
    );
    p95
}

fn main() {
    let highlighter = BuiltinSyntaxHighlighter::shared();
    measure("syntax highlight 10 KB Swift", 20, || {
        // As drbench does, the string is built inside the timed body.
        let code = "func f() -> Int { let x = \"s\" // c\n return 1 }\n".repeat(200);
        std::hint::black_box(highlighter.highlight_str(&code, Some("swift")));
    });
    let big = "func f() -> Int { let x = \"s\" // c\n return 1 }\n".repeat(20_000);
    measure("syntax highlight 1 MB Swift", 20, || {
        std::hint::black_box(highlighter.highlight_str(&big, Some("swift")));
    });

    // SAFETY: AppKit exports the appearance names as immutable globals.
    let (aqua, dark) = unsafe {
        (
            NSAppearance::appearanceNamed(NSAppearanceNameAqua).unwrap(),
            NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua).unwrap(),
        )
    };
    let themes = ThemeStore::shared().themes();
    let paper = themes.iter().find(|theme| theme.name == "Paper Light").unwrap().clone();
    measure("StyleSheet init Paper Light (aqua)", 50, || {
        std::hint::black_box(StyleSheet::new(paper.clone(), &aqua, None));
    });
    measure("StyleSheet init, 6 themes x 2 appearances", 20, || {
        for theme in &themes {
            for appearance in [&aqua, &dark] {
                std::hint::black_box(StyleSheet::new(theme.clone(), appearance, None));
            }
        }
    });
}
