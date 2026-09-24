//! `cargo bench -p upleft-markup`: times `Document::parse` over drbench's
//! 5,000-line agent document the way drbench's "cmark alone" case times
//! swift-markdown's `Document(parsing:options: [.disableSmartOpts])` (one
//! warm-up, then nearest-rank percentiles; the document is dropped inside
//! the timed region, as Swift releases it there). The Swift number comes from
//! `downright-oracle markup-bench <file> <out.json>`.
//!
//!   cargo bench -p upleft-markup [-- <file.md> [runs]]
//!
//! With `--features cmark-oracle` it also times the cmark-gfm converter the
//! pulldown-cmark adapter replaced, on the same document.

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use upleft_markup::{Document, ParseOptions};

/// Nearest-rank percentile over ascending samples (drbench's `percentile`).
fn percentile(ascending: &[f64], p: f64) -> f64 {
    assert!(!ascending.is_empty());
    let rank = (p * ascending.len() as f64).ceil() as usize;
    ascending[usize::min(ascending.len() - 1, rank.saturating_sub(1))]
}

fn main() {
    let arguments: Vec<String> = std::env::args()
        .skip(1)
        .filter(|argument| argument != "--bench")
        .collect();
    let path = arguments.first().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/generated/agent/agent-5000.md")
    });
    let runs: usize = arguments
        .get(1)
        .map_or(200, |runs| runs.parse().expect("runs is a number"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error} (run `just corpus` first)", path.display()));

    let samples = time(runs, || Document::parse(black_box(&text), ParseOptions::DISABLE_SMART_OPTS));
    println!(
        "upleft-markup Document::parse  {} ({} bytes, {} lines)\n  p50 {:.3} ms  p95 {:.3} ms  min {:.3} ms  (n={runs})",
        path.display(),
        text.len(),
        text.matches('\n').count(),
        percentile(&samples, 0.50),
        percentile(&samples, 0.95),
        samples[0],
    );
    #[cfg(feature = "cmark-oracle")]
    {
        let samples = time(runs, || {
            upleft_markup::parser::cmark_oracle::parse(black_box(&text), ParseOptions::DISABLE_SMART_OPTS)
        });
        println!(
            "cmark-gfm converter (oracle)\n  p50 {:.3} ms  p95 {:.3} ms  min {:.3} ms  (n={runs})",
            percentile(&samples, 0.50),
            percentile(&samples, 0.95),
            samples[0],
        );
    }
}

/// One warm-up so first-call initialisation isn't charged to the p50, then
/// `runs` timed parses, ascending.
fn time(runs: usize, parse: impl Fn() -> Document) -> Vec<f64> {
    drop(black_box(parse()));
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        let document = parse();
        drop(black_box(document));
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    samples
}
