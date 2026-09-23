//! Port of elk-swift's `Tests/ElkSwiftTests/ConcurrentLayoutTests.swift`.
//!
//! elk-swift shares process-wide state between layouts (the layout metadata
//! service, the provider pool); the port keeps that state per thread, so
//! these tests check that layouts on many threads at once neither fail nor
//! diverge from a serial baseline. `DispatchQueue.concurrentPerform` and the
//! concurrent `DispatchQueue` become a pool of one scoped thread per core
//! sharing the iterations, as GCD runs them.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Value};
use upleft_elk::bridge::elk::{Elk, ElkError};

fn graph(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/elk/elk-swift-tests").join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

fn flow18() -> Value {
    graph("ConcurrentLayoutTests.flow18")
}

/// `JSONSerialization.data(withJSONObject:options: .sortedKeys)`.
fn sorted_data(value: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), sort(&map[k]));
                }
                Value::Object(out)
            }
            Value::Array(items) => Value::Array(items.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(value)).unwrap()
}

/// `max(8, activeProcessorCount * 2)`.
fn workers() -> usize {
    std::thread::available_parallelism().map_or(8, |n| (n.get() * 2).max(8))
}

/// `concurrentPerform(iterations:)`: runs `body(index)` for every index,
/// spread over one thread per core.
fn concurrent_perform(iterations: usize, body: impl Fn(usize) + Sync) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(iterations).max(1);
    std::thread::scope(|scope| {
        for thread in 0..threads {
            let body = &body;
            scope.spawn(move || {
                for index in (thread..iterations).step_by(threads) {
                    body(index);
                }
            });
        }
    });
}

/// `makeGraphCorpus()`: one diamond per direction and edge routing.
fn make_graph_corpus() -> Vec<Value> {
    let mut corpus = Vec::new();
    for (i, direction) in ["RIGHT", "DOWN", "LEFT", "UP"].iter().enumerate() {
        for routing in ["ORTHOGONAL", "POLYLINE"] {
            corpus.push(json!({
                "id": format!("root_{i}_{routing}"),
                "layoutOptions": {
                    "elk.algorithm": "layered",
                    "elk.direction": direction,
                    "elk.edgeRouting": routing,
                    "elk.spacing.nodeNode": "20"
                },
                "children": [
                    {"id": "A", "width": 50, "height": 30},
                    {"id": "B", "width": 50, "height": 30},
                    {"id": "C", "width": 50, "height": 30},
                    {"id": "D", "width": 50, "height": 30}
                ],
                "edges": [
                    {"id": "e1", "sources": ["A"], "targets": ["B"]},
                    {"id": "e2", "sources": ["A"], "targets": ["C"]},
                    {"id": "e3", "sources": ["B"], "targets": ["D"]},
                    {"id": "e4", "sources": ["C"], "targets": ["D"]}
                ]
            }));
        }
    }
    corpus
}

#[test]
fn concurrent_layouts_produce_same_results() {
    let graph = flow18();

    // Ten serial runs give the baseline.
    let baseline_results: Vec<String> = (0..10).map(|_| sorted_data(&Elk::new().layout(&graph).unwrap())).collect();
    let baseline_data = &baseline_results[0];
    for (i, data) in baseline_results.iter().enumerate() {
        assert_eq!(data, baseline_data, "Serial run {i} differs from baseline");
    }

    // Ten concurrent runs.
    let results = Mutex::new(Vec::new());
    let errors = Mutex::new(Vec::new());
    concurrent_perform(10, |_| match Elk::new().layout(&graph) {
        Ok(result) => results.lock().unwrap().push(sorted_data(&result)),
        Err(error) => errors.lock().unwrap().push(error),
    });
    let errors = errors.into_inner().unwrap();
    assert!(errors.is_empty(), "Concurrent errors: {errors:?}");
    let results = results.into_inner().unwrap();
    assert_eq!(results.len(), 10);
    for (i, data) in results.iter().enumerate() {
        assert_eq!(data, baseline_data, "Concurrent run {i} differs from serial baseline");
    }
}

#[test]
fn concurrent_layouts_no_crash() {
    let graphs = [graph("ConcurrentLayoutTests.simpleChainGraph"), graph("ConcurrentLayoutTests.diamondGraph"), flow18()];
    let errors = Mutex::new(Vec::new());
    concurrent_perform(20, |i| match Elk::new().layout(&graphs[i % graphs.len()]) {
        Ok(result) => {
            for child in result["children"].as_array().into_iter().flatten() {
                if let (Some(x), Some(y)) = (child["x"].as_f64(), child["y"].as_f64()) {
                    assert!(x.is_finite(), "Node x is not finite");
                    assert!(y.is_finite(), "Node y is not finite");
                }
            }
        }
        Err(error) => errors.lock().unwrap().push(error),
    });
    let errors = errors.into_inner().unwrap();
    assert!(errors.is_empty(), "Concurrent errors: {errors:?}");
}

#[test]
fn concurrent_init_no_crash() {
    let instances = Mutex::new(Vec::new());
    concurrent_perform(10, |_| {
        let elk = Elk::new();
        instances.lock().unwrap().push(elk);
    });
    assert_eq!(instances.into_inner().unwrap().len(), 10);
}

/// Many threads at once over a varied corpus, in 40 batches.
#[test]
fn high_concurrency_stress() {
    let corpus = make_graph_corpus();
    let workers = workers();
    let errors = Mutex::new(Vec::new());
    for batch in 0..40usize {
        concurrent_perform(workers, |idx| {
            if let Err(error) = Elk::new().layout(&corpus[batch.wrapping_add(idx) % corpus.len()]) {
                errors.lock().unwrap().push(error);
            }
        });
    }
    let errors = errors.into_inner().unwrap();
    assert!(errors.is_empty(), "Stress run produced {} errors: {:?}", errors.len(), &errors[..errors.len().min(3)]);
}

/// A single burst of 500 concurrent layouts.
#[test]
fn warm_cache_burst() {
    let corpus = make_graph_corpus();
    let errors = Mutex::new(Vec::new());
    concurrent_perform(500, |idx| {
        if let Err(error) = Elk::new().layout(&corpus[idx % corpus.len()]) {
            errors.lock().unwrap().push(error);
        }
    });
    let errors = errors.into_inner().unwrap();
    assert!(errors.is_empty(), "Warm-cache burst produced {} errors: {:?}", errors.len(), &errors[..errors.len().min(3)]);
}

/// Concurrent renders of one graph must each equal the serial baseline.
#[test]
fn concurrent_determinism() {
    let graph = flow18();
    let baseline = sorted_data(&Elk::new().layout(&graph).unwrap());
    let workers = workers();
    let results = Mutex::new(Vec::new());
    let errors = Mutex::new(Vec::new());
    for _ in 0..10 {
        concurrent_perform(workers, |_| match Elk::new().layout(&graph) {
            Ok(out) => results.lock().unwrap().push(sorted_data(&out)),
            Err(error) => errors.lock().unwrap().push(error),
        });
    }
    let errors = errors.into_inner().unwrap();
    assert!(errors.is_empty(), "Errors during determinism stress: {:?}", &errors[..errors.len().min(3)]);
    for (i, data) in results.into_inner().unwrap().iter().enumerate() {
        assert_eq!(data, &baseline, "Concurrent run {i} diverged from serial baseline");
    }
}

#[test]
fn layout_timeout() {
    // timeout 0 means deadline == now, so isCanceled() is true on the first check.
    match Elk::new().layout_with(&flow18(), None, 0.0) {
        Err(ElkError::TimedOut(_)) => {}
        other => panic!("Expected .timedOut, got {other:?}"),
    }
}
