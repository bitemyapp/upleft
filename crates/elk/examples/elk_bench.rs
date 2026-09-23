//! `cargo run --release -p upleft-elk --example elk_bench -- <iterations> <graph.json>...`
//!
//! The Rust side of `tools/elk-bench` (Swift): parses each graph once, then
//! times `Elk::new().layout(&graph)` — import, layout, export — and prints
//! name, min and median in µs.

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let iterations: usize = args[0].parse().expect("iterations");
    for path in &args[1..] {
        let text = std::fs::read_to_string(path).expect("read graph");
        let graph: serde_json::Value = serde_json::from_str(&text).expect("parse graph");
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            let result = upleft_elk::bridge::elk::Elk::new().layout(&graph).expect("layout");
            let elapsed = start.elapsed();
            assert!(result.get("width").is_some());
            samples.push(elapsed.as_secs_f64() * 1e6);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let name = std::path::Path::new(path).file_name().unwrap().to_string_lossy();
        println!("{name}\t{:.0}\t{:.0}", samples[0], samples[samples.len() / 2]);
    }
}
