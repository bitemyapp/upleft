//! Runs ELK-backed layouts in tests. With the `elk` feature the engine runs;
//! without it, ELK's answers come from Swift's own record of the same
//! diagram (corpus/mermaid-elk, made by tools/elk-capture), which also
//! checks that this port hands ELK the graphs Swift did.

#![allow(dead_code)]

use std::path::PathBuf;

use serde_json::Value;
use upleft_mermaid::mermaid::src_elk_instance::{elk_available, replay};

fn records() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/mermaid-elk")
}

/// The record whose source is `source` (trimmed), if any.
pub fn record_for(source: &str) -> Option<Value> {
    let wanted = upleft_mermaid::swift::trim_whitespaces_and_newlines(source);
    let mut entries: Vec<_> = std::fs::read_dir(records()).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.extension().is_none_or(|e| e != "elkrec") {
            continue;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        let record: Value = serde_json::from_str(&text).ok()?;
        if record["source"].as_str() == Some(wanted) {
            return Some(record);
        }
    }
    None
}

/// Runs `f` with ELK available: the engine, or `source`'s recorded answers.
/// Returns `f`'s result; panics when neither is available. Each call of `f`
/// that lays `source` out once consumes the record once, so `f` must lay it
/// out exactly `layouts` times.
pub fn with_elk<T>(source: &str, layouts: usize, f: impl FnOnce() -> T) -> T {
    if elk_available() {
        return f();
    }
    let record = record_for(source).unwrap_or_else(|| panic!("no ELK record for {source:?}; run tools/elk-capture.sh"));
    let calls = record["calls"].as_array().cloned().unwrap_or_default();
    let outputs: Vec<Option<Value>> = calls.iter().map(|c| (!c["output"].is_null()).then(|| c["output"].clone())).collect();
    let mut all = Vec::new();
    for _ in 0..layouts {
        all.extend(outputs.iter().cloned());
    }
    replay::install(all);
    let result = f();
    let inputs = replay::finish();
    for (i, input) in inputs.iter().enumerate() {
        let expected = &calls[i % calls.len()]["input"];
        assert_eq!(input, expected, "ELK input graph {i} differs from Swift's");
    }
    result
}
