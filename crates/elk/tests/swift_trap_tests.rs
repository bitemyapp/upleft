//! Graphs on which elk-swift 1.0.2 traps (SIGTRAP, an out-of-range index in
//! `CrossingsCounter.countCrossingsOnPorts` on compound graphs): release-build
//! Swift traps become panics in the port, at the same place.

use std::path::PathBuf;

use upleft_elk::bridge::elk::Elk;

fn traps(name: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/swift-traps").join(name);
    let graph: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let panic = std::panic::catch_unwind(|| Elk::new().layout(&graph)).expect_err("elk-swift traps on this graph");
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(message.contains("index out of bounds"), "unexpected panic: {message}");
}

#[test]
fn compound_crossings_counter_oob_1() {
    traps("compound-crossings-counter-oob-1.json");
}

#[test]
fn compound_crossings_counter_oob_2() {
    traps("compound-crossings-counter-oob-2.json");
}
