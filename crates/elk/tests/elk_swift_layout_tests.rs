//! Ports of elk-swift's JSON-level tests (`Tests/ElkSwiftTests/`
//! `OverallLayoutTests`, `GoldenOutputSnapshotTests`, `IssueRegressionTests`,
//! `LayeredSpacingTests`, `ElkSwiftTests`; `ConcurrentLayoutTests` is in
//! `concurrent_layout_tests.rs`). The
//! graphs are the literals from those files, extracted into
//! `corpus/elk/elk-swift-tests/` by `tools/extract_test_graphs.py`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde_json::{Map, Value};
use upleft_elk::bridge::elk::Elk;
use upleft_elk::org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena;
use upleft_elk::org::eclipse::elk::core::math::k_vector::KVector;
use upleft_elk::org::eclipse::elk::core::options::port_side::PortSide;

fn graph(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/elk/elk-swift-tests").join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

fn layout(name: &str) -> Value {
    Elk::new().layout(&graph(name)).expect("layout")
}

fn objects<'a>(v: &'a Value, key: &str) -> Vec<&'a Map<String, Value>> {
    v.get(key).and_then(Value::as_array).map_or_else(Vec::new, |a| a.iter().filter_map(Value::as_object).collect())
}

fn num(v: &Map<String, Value>, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("missing {key}"))
}

fn assert_positive_size(result: &Value) {
    assert!(result["width"].as_f64().unwrap() > 0.0, "Graph width should be > 0");
    assert!(result["height"].as_f64().unwrap() > 0.0, "Graph height should be > 0");
}

fn assert_all_nodes_positioned(nodes: &[&Map<String, Value>]) {
    for n in nodes {
        assert!(num(n, "x") >= 0.0 && num(n, "y") >= 0.0, "node {:?} not positioned", n.get("id"));
    }
}

fn assert_all_edges_routed(edges: &[&Map<String, Value>]) {
    for e in edges {
        let sections = e.get("sections").and_then(Value::as_array).cloned().unwrap_or_default();
        assert!(!sections.is_empty(), "Edge {:?} should have sections", e.get("id"));
        for s in &sections {
            assert!(s.get("startPoint").is_some() && s.get("endPoint").is_some());
        }
    }
}

fn id(v: &Map<String, Value>) -> &str {
    v.get("id").and_then(Value::as_str).unwrap_or("")
}

/// The number of bend points in each section.
fn bend_counts(e: &Map<String, Value>) -> Vec<usize> {
    e.get("sections")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |sections| sections.iter().map(|s| s.get("bendPoints").and_then(Value::as_array).map_or(0, Vec::len)).collect())
}

// MARK: OverallLayoutTests (multipleEdgesInBothDirectionsNSNode)

#[test]
fn overall_node_coordinates_edges_size_orthogonality() {
    let result = layout("OverallLayoutTests.test_multipleEdgesInBothDirectionsNSNode");
    let children = objects(&result, "children");
    for c in &children {
        assert!(num(c, "x") >= 0.0 && num(c, "y") >= 0.0);
    }
    assert!(children.iter().any(|c| num(c, "x") > 0.0 || num(c, "y") > 0.0));
    for e in objects(&result, "edges") {
        for s in objects(&Value::Object(e.clone()), "sections") {
            let start = s["startPoint"].as_object().unwrap();
            let end = s["endPoint"].as_object().unwrap();
            assert!(num(start, "x") > 0.0 && num(start, "y") > 0.0 && num(end, "x") > 0.0 && num(end, "y") > 0.0);
            let mut points = vec![(num(start, "x"), num(start, "y"))];
            for b in s.get("bendPoints").and_then(Value::as_array).cloned().unwrap_or_default() {
                let b = b.as_object().unwrap().clone();
                points.push((num(&b, "x"), num(&b, "y")));
            }
            points.push((num(end, "x"), num(end, "y")));
            for w in points.windows(2) {
                let dx = (w[0].0 - w[1].0).abs();
                let dy = (w[0].1 - w[1].1).abs();
                assert!(dx < 0.05 || dy < 0.05, "Edge segment not orthogonal: {w:?}");
            }
        }
    }
    assert_positive_size(&result);
}

// MARK: GoldenOutputSnapshotTests

#[test]
fn golden_edge_labels() {
    let result = layout("GoldenOutputSnapshotTests.testEdgeLabelsSnapshot");
    assert_positive_size(&result);
    let nodes = objects(&result, "children");
    assert_eq!(nodes.len(), 4);
    assert_all_nodes_positioned(&nodes);
    let edges = objects(&result, "edges");
    assert_eq!(edges.len(), 4);
    assert_all_edges_routed(&edges);
    for e in &edges {
        for l in objects(&Value::Object((*e).clone()), "labels") {
            assert!(l.get("x").and_then(Value::as_f64).is_some() && l.get("y").and_then(Value::as_f64).is_some());
        }
    }
}

#[test]
fn golden_self_loops() {
    let result = layout("GoldenOutputSnapshotTests.testSelfLoopsSnapshot");
    assert_positive_size(&result);
    let nodes = objects(&result, "children");
    assert_eq!(nodes.len(), 2);
    assert_all_nodes_positioned(&nodes);
    let edges = objects(&result, "edges");
    assert_eq!(edges.len(), 3);
    assert_all_edges_routed(&edges);
    for e in &edges {
        if id(e).starts_with("e_self") {
            for bends in bend_counts(e) {
                assert!(bends > 0, "Self-loop {} should have bend points", id(e));
            }
        }
    }
}

#[test]
fn golden_reversed_edges() {
    let result = layout("GoldenOutputSnapshotTests.testReversedEdgesSnapshot");
    assert_positive_size(&result);
    let nodes = objects(&result, "children");
    assert_eq!(nodes.len(), 4);
    assert_all_nodes_positioned(&nodes);
    let edges = objects(&result, "edges");
    assert_eq!(edges.len(), 5);
    assert_all_edges_routed(&edges);
    let unique: HashSet<i64> = nodes.iter().map(|n| num(n, "x") as i64).collect();
    assert!(unique.len() > 1, "Nodes should span multiple layers");
}

#[test]
fn golden_multi_layer_long_edges() {
    let result = layout("GoldenOutputSnapshotTests.testMultiLayerLongEdgesSnapshot");
    assert_positive_size(&result);
    let nodes = objects(&result, "children");
    assert_eq!(nodes.len(), 7);
    assert_all_nodes_positioned(&nodes);
    let edges = objects(&result, "edges");
    assert_eq!(edges.len(), 9);
    assert_all_edges_routed(&edges);
    for e in &edges {
        if id(e).starts_with("e_long") {
            for bends in bend_counts(e) {
                assert!(bends > 0, "Long edge {} should have bend points", id(e));
            }
        }
    }
    let x: HashMap<&str, f64> = nodes.iter().map(|n| (id(n), num(n, "x"))).collect();
    if let (Some(a), Some(b), Some(c), Some(d)) = (x.get("a"), x.get("b"), x.get("c"), x.get("d")) {
        assert!(a < b && b < c && c <= d);
    }
}

#[test]
fn golden_comment_nodes() {
    let result = layout("GoldenOutputSnapshotTests.testCommentNodesSnapshot");
    assert_positive_size(&result);
    let nodes = objects(&result, "children");
    assert_eq!(nodes.len(), 5);
    assert_all_nodes_positioned(&nodes);
    let edges = objects(&result, "edges");
    let normal: Vec<_> = edges.into_iter().filter(|e| !id(e).starts_with("e_c")).collect();
    assert_all_edges_routed(&normal);
}

#[test]
fn golden_complex_graph() {
    let result = layout("GoldenOutputSnapshotTests.testComplexGraphSnapshot");
    assert_positive_size(&result);
    let nodes = objects(&result, "children");
    assert_eq!(nodes.len(), 5);
    assert_all_nodes_positioned(&nodes);
    let edges = objects(&result, "edges");
    assert_eq!(edges.len(), 6);
    assert_all_edges_routed(&edges);
    for e in &edges {
        if ["e1", "e3", "e5", "e6_back"].contains(&id(e)) {
            for l in objects(&Value::Object((*e).clone()), "labels") {
                assert!(l.get("x").and_then(Value::as_f64).is_some() && l.get("y").and_then(Value::as_f64).is_some());
            }
        }
    }
    let y: HashMap<&str, f64> = nodes.iter().map(|n| (id(n), num(n, "y"))).collect();
    if let (Some(s), Some(e)) = (y.get("Start"), y.get("End")) {
        assert!(s < e, "Start should be above End in DOWN layout");
    }
}

// MARK: IssueRegressionTests

#[test]
fn issue_562_inside_self_loops_no_exception() {
    layout("IssueRegressionTests.testIssue562_insideSelfLoopsNoException");
}

#[test]
fn issue_682_node_label_padding_does_not_crash() {
    layout("IssueRegressionTests.testIssue682_nodeLabelPaddingDoesNotCrash");
}

#[test]
fn issue_871_feedback_edge_basic() {
    let result = layout("IssueRegressionTests.testIssue871_feedbackEdgeBasic");
    for c in objects(&result, "children") {
        assert!(!num(c, "x").is_nan() && !num(c, "y").is_nan());
    }
    assert_positive_size(&result);
}

#[test]
fn issue_871_no_feedback_edges_chain_ordering() {
    let result = layout("IssueRegressionTests.testIssue871_noFeedbackEdgesChainOrdering");
    let x: HashMap<&str, f64> = objects(&result, "children").iter().map(|n| (id(n), num(n, "x"))).collect();
    assert!(x["n1"] < x["n2"] && x["n2"] < x["n3"] && x["n3"] < x["n4"]);
}

#[test]
fn self_loop_does_not_crash() {
    layout("IssueRegressionTests.testSelfLoopDoesNotCrash");
}

#[test]
fn hierarchical_layout_does_not_crash() {
    layout("IssueRegressionTests.testHierarchicalLayoutDoesNotCrash");
}

#[test]
fn multiple_edges_between_same_nodes() {
    let result = layout("IssueRegressionTests.testMultipleEdgesBetweenSameNodes");
    let edges = objects(&result, "edges");
    assert_eq!(edges.len(), 2);
    for e in edges {
        assert!(!objects(&Value::Object(e.clone()), "sections").is_empty());
    }
}

#[test]
fn disconnected_components() {
    let result = layout("IssueRegressionTests.testDisconnectedComponents");
    assert_eq!(objects(&result, "children").len(), 4);
    assert_positive_size(&result);
}

// MARK: LayeredSpacingTests

#[test]
fn spacing_node_node_between_layers() {
    let result = layout("LayeredSpacingTests.testSpacingNodeNodeBetweenLayers");
    let children = objects(&result, "children");
    let n1 = children.iter().find(|c| id(c) == "n1").unwrap();
    let n2 = children.iter().find(|c| id(c) == "n2").unwrap();
    let gap = num(n2, "x") - (num(n1, "x") + num(n1, "width"));
    assert!((gap - 66.0).abs() <= 1.0, "gap {gap}");
}

#[test]
fn spacing_node_node_no_edges() {
    let result = layout("LayeredSpacingTests.testSpacingNodeNodeNoEdges");
    assert_positive_size(&result);
}

// MARK: ElkSwiftTests

#[test]
fn version_is_non_empty() {
    assert!(!upleft_elk::elk_swift::VERSION.is_empty());
}

#[test]
fn kvector_core_ops() {
    let mut v = KVector::new(3.0, 4.0);
    assert!((v.length() - 5.0).abs() < 1e-9);
    v.normalize();
    assert!((v.length() - 1.0).abs() < 1e-9);
    v.scale_to_length(10.0);
    assert!((v.length() - 10.0).abs() < 1e-9);
}

#[test]
fn port_set_node_maintains_owner_list() {
    let mut lg = LGraphArena::new();
    let g = lg.new_graph();
    let a = lg.new_node(Some(g));
    let b = lg.new_node(Some(g));
    let p = lg.new_port();
    lg.port_set_node(p, Some(a));
    assert_eq!(lg[a].ports, vec![p]);
    lg.port_set_node(p, Some(b));
    assert!(lg[a].ports.is_empty());
    assert_eq!(lg[b].ports, vec![p]);
}

#[test]
fn edge_source_target_linking() {
    let mut lg = LGraphArena::new();
    let g = lg.new_graph();
    let sn = lg.new_node(Some(g));
    let tn = lg.new_node(Some(g));
    let sp = lg.new_port();
    let tp = lg.new_port();
    lg.port_set_node(sp, Some(sn));
    lg.port_set_node(tp, Some(tn));
    let e = lg.new_edge();
    lg.edge_set_source(e, Some(sp));
    lg.edge_set_target(e, Some(tp));
    assert_eq!(lg[sp].outgoing_edges.len(), 1);
    assert_eq!(lg[tp].incoming_edges.len(), 1);
    assert_eq!(lg.edge_other_port(e, sp), tp);
    assert_eq!(lg.edge_other_node(e, tn), sn);
}

#[test]
fn port_side_sets_default_anchor() {
    let mut lg = LGraphArena::new();
    let p = lg.new_port();
    lg[p].size.set_xy(20.0, 10.0);
    lg.port_set_side(p, PortSide::EAST);
    assert!((lg[p].anchor.x - 20.0).abs() < 1e-9 && (lg[p].anchor.y - 5.0).abs() < 1e-9);
    lg[p].explicitly_supplied_port_anchor = true;
    lg[p].anchor.set_xy(7.0, 8.0);
    lg.port_set_side(p, PortSide::WEST);
    assert!((lg[p].anchor.x - 7.0).abs() < 1e-9 && (lg[p].anchor.y - 8.0).abs() < 1e-9);
}
