//! Differential tests for the group C processors (node placement, edge
//! routing and their intermediate processors): each case is a pair of
//! layered-graph snapshots the instrumented elk-swift lab wrote before and
//! after one run of a processor. The Rust processor runs on the loaded
//! "before" state and must reproduce the "after" state exactly (every
//! coordinate as the same `Double`, every list in the same order, every
//! property value of the same type).
//!
//! The committed cases live in `tests/data/group_c/`. To check a larger set,
//! point `UPLEFT_ELK_SNAPDIR` at a directory tree of lab snapshots.

mod lgraph_snapshot;

use std::path::{Path, PathBuf};

use upleft_elk::org::eclipse::elk::alg::layered::intermediate::{
    hierarchical_port_constraint_processor::HierarchicalPortConstraintProcessor,
    hierarchical_port_dummy_size_processor::HierarchicalPortDummySizeProcessor,
    hierarchical_port_orthogonal_edge_router::HierarchicalPortOrthogonalEdgeRouter,
    hierarchical_port_position_processor::HierarchicalPortPositionProcessor, inverted_port_processor::InvertedPortProcessor,
    layer_size_and_graph_height_calculator::LayerSizeAndGraphHeightCalculator, long_edge_joiner::LongEdgeJoiner,
    north_south_port_postprocessor::NorthSouthPortPostprocessor, north_south_port_preprocessor::NorthSouthPortPreprocessor,
    port_side_processor::PortSideProcessor,
};
use upleft_elk::org::eclipse::elk::alg::layered::p4nodes::bk::bk_node_placer::BKNodePlacer;
use upleft_elk::org::eclipse::elk::alg::layered::p5edges::orthogonal_edge_router::OrthogonalEdgeRouter;
use upleft_elk::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use upleft_elk::org::eclipse::elk::core::util::basic_progress_monitor::BasicProgressMonitor;

fn processor(name: &str) -> Option<Box<dyn ILayoutProcessor>> {
    Some(match name {
        "BKNodePlacer" => Box::new(BKNodePlacer::new()),
        "OrthogonalEdgeRouter" => Box::new(OrthogonalEdgeRouter::new()),
        "LongEdgeJoiner" => Box::new(LongEdgeJoiner::new()),
        "LayerSizeAndGraphHeightCalculator" => Box::new(LayerSizeAndGraphHeightCalculator::new()),
        "NorthSouthPortPreprocessor" => Box::new(NorthSouthPortPreprocessor::new()),
        "NorthSouthPortPostprocessor" => Box::new(NorthSouthPortPostprocessor::new()),
        "InvertedPortProcessor" => Box::new(InvertedPortProcessor::new()),
        "PortSideProcessor" => Box::new(PortSideProcessor::new()),
        "HierarchicalPortConstraintProcessor" => Box::new(HierarchicalPortConstraintProcessor::new()),
        "HierarchicalPortDummySizeProcessor" => Box::new(HierarchicalPortDummySizeProcessor::new()),
        "HierarchicalPortPositionProcessor" => Box::new(HierarchicalPortPositionProcessor::new()),
        "HierarchicalPortOrthogonalEdgeRouter" => Box::new(HierarchicalPortOrthogonalEdgeRouter::new()),
        _ => return None,
    })
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect(&p, out);
        } else if p.to_string_lossy().ends_with("-before.json") {
            out.push(p);
        }
    }
}

/// Runs every snapshot pair under `dir`; returns (passed, changed, failures),
/// `changed` counting the passed cases where the processor changed anything.
fn run_dir(dir: &Path, round_trip: bool) -> (usize, usize, Vec<String>) {
    let mut befores = Vec::new();
    collect(dir, &mut befores);
    let mut passed = 0;
    let mut changed = 0;
    let mut failures = Vec::new();
    for before in befores {
        let fname = before.file_name().unwrap().to_string_lossy().to_string();
        let proc_name = fname.trim_end_matches("-before.json").splitn(2, '-').nth(1).unwrap().to_string();
        let Some(mut p) = processor(&proc_name) else { continue };
        let after = PathBuf::from(before.to_string_lossy().replace("-before.json", "-after.json"));
        let before_text = std::fs::read_to_string(&before).unwrap();
        let Ok(after_text) = std::fs::read_to_string(&after) else { continue };

        let mut loaded = lgraph_snapshot::load(&before_text);
        if round_trip {
            let again = lgraph_snapshot::dump(&loaded.lg, loaded.graph, &loaded.elk_types);
            let diffs = lgraph_snapshot::compare(&before_text, &again);
            if !diffs.is_empty() {
                failures.push(format!("{} (round trip):\n  {}", before.display(), diffs.join("\n  ")));
                continue;
            }
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut monitor = BasicProgressMonitor::new();
            p.process(&mut loaded.lg, loaded.graph, &mut monitor);
            lgraph_snapshot::dump(&loaded.lg, loaded.graph, &loaded.elk_types)
        }));
        match result {
            Ok(rust_after) => {
                let diffs = lgraph_snapshot::compare(&after_text, &rust_after);
                if diffs.is_empty() {
                    passed += 1;
                    if !lgraph_snapshot::compare(&before_text, &after_text).is_empty() {
                        changed += 1;
                    }
                } else {
                    failures.push(format!("{}:\n  {}", before.display(), diffs.join("\n  ")));
                }
            }
            Err(e) => {
                let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                failures.push(format!("{}: panicked: {msg}", before.display()));
            }
        }
    }
    (passed, changed, failures)
}

#[test]
fn group_c_processors_match_swift_snapshots() {
    let dir = std::env::var_os("UPLEFT_ELK_SNAPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/group_c"));
    let (passed, changed, failures) = run_dir(&dir, true);
    eprintln!("group C snapshots: {passed} passed ({changed} changed the graph), {} failed", failures.len());
    if !failures.is_empty() {
        let shown: Vec<&String> = failures.iter().take(10).collect();
        panic!("{} snapshot case(s) differ:\n{}", failures.len(), shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
    }
}
