//! Port of elk-swift's `Tests/ElkSwiftTests/SelfLoopTests.swift`:
//! `SelfLoopHolder.needsSelfLoopProcessing`, `SelfLoopHolder.install`, and
//! `PolylineSelfLoopRouter.cutCorners`.

use upleft_elk::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};
use upleft_elk::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::loops::routing::polyline_self_loop_router::cut_corners;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::loops::self_loop_holder::SelfLoopHolder;
use upleft_elk::org::eclipse::elk::core::math::k_vector::KVector;
use upleft_elk::org::eclipse::elk::core::math::k_vector_chain::KVectorChain;

/// A graph with one node of `node_type`, two ports on it, and an edge
/// between them.
fn node_with_self_loop(node_type: NodeType) -> (LGraphArena, LNodeId) {
    let mut lg = LGraphArena::new();
    let graph = lg.new_graph();
    let node = lg.new_node(Some(graph));
    lg[node].node_type = node_type;
    let port1 = lg.new_port();
    lg.port_set_node(port1, Some(node));
    let port2 = lg.new_port();
    lg.port_set_node(port2, Some(node));
    let edge = lg.new_edge();
    lg.edge_set_source(edge, Some(port1));
    lg.edge_set_target(edge, Some(port2));
    (lg, node)
}

#[test]
fn needs_self_loop_processing_normal_node_with_self_loop() {
    let (lg, node) = node_with_self_loop(NodeType::NORMAL);
    assert!(SelfLoopHolder::needs_self_loop_processing(&lg, node), "Normal node with a self-loop edge should need self-loop processing");
}

#[test]
fn needs_self_loop_processing_normal_node_without_self_loop() {
    let mut lg = LGraphArena::new();
    let graph = lg.new_graph();
    let node1 = lg.new_node(Some(graph));
    lg[node1].node_type = NodeType::NORMAL;
    let node2 = lg.new_node(Some(graph));
    lg[node2].node_type = NodeType::NORMAL;
    let port1 = lg.new_port();
    lg.port_set_node(port1, Some(node1));
    let port2 = lg.new_port();
    lg.port_set_node(port2, Some(node2));
    let edge = lg.new_edge();
    lg.edge_set_source(edge, Some(port1));
    lg.edge_set_target(edge, Some(port2));
    assert!(!SelfLoopHolder::needs_self_loop_processing(&lg, node1), "Normal node without self-loop edges should not need self-loop processing");
}

#[test]
fn needs_self_loop_processing_non_normal_node() {
    let (lg, node) = node_with_self_loop(NodeType::LONG_EDGE);
    assert!(
        !SelfLoopHolder::needs_self_loop_processing(&lg, node),
        "Non-NORMAL node should not need self-loop processing even with self-loop edge"
    );
}

#[test]
fn install_creates_holder_with_correct_node() {
    let (mut lg, node) = node_with_self_loop(NodeType::NORMAL);
    let holder = SelfLoopHolder::install(&mut lg, node);
    assert_eq!(holder.borrow().get_l_node(), node, "Installed holder should reference the original node");
}

#[test]
fn install_detects_self_loop_edges() {
    let (mut lg, node) = node_with_self_loop(NodeType::NORMAL);
    let holder = SelfLoopHolder::install(&mut lg, node);
    let holder = holder.borrow();
    assert!(!holder.get_sl_hyper_loops().is_empty(), "Holder should detect at least one self-loop hyper-loop");
    assert_eq!(holder.get_sl_port_values().count(), 2, "Holder should have two self-loop ports (source and target)");
}

fn chain(points: &[(f64, f64)]) -> KVectorChain {
    let mut chain = KVectorChain::new();
    for &(x, y) in points {
        chain.add(KVector::new(x, y));
    }
    chain
}

#[track_caller]
fn assert_point_approx(point: KVector, expected_x: f64, expected_y: f64, label: &str) {
    let accuracy = 1.0;
    assert!((point.x - expected_x).abs() <= accuracy, "{label}: x should be ~{expected_x} but was {}", point.x);
    assert!((point.y - expected_y).abs() <= accuracy, "{label}: y should be ~{expected_y} but was {}", point.y);
}

#[test]
fn polyline_self_loop_router_cut_corners_usual_case() {
    let input = chain(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (-100.0, 100.0), (-100.0, -100.0), (0.0, -100.0)]);
    let result = cut_corners(&input, 10.0);

    // 6 input points -> 4 inner corners -> 8 output points (2 per corner).
    assert_eq!(result.size(), 8, "cutCorners should produce 2 points per inner corner (4 corners = 8 points)");
    assert_point_approx(result.get(0), 90.0, 0.0, "corner1-pre");
    assert_point_approx(result.get(1), 100.0, 10.0, "corner1-post");
    assert_point_approx(result.get(2), 100.0, 90.0, "corner2-pre");
    assert_point_approx(result.get(3), 90.0, 100.0, "corner2-post");
    assert_point_approx(result.get(4), -90.0, 100.0, "corner3-pre");
    assert_point_approx(result.get(5), -100.0, 90.0, "corner3-post");
    assert_point_approx(result.get(6), -100.0, -90.0, "corner4-pre");
    assert_point_approx(result.get(7), -90.0, -100.0, "corner4-post");
}

#[test]
fn polyline_self_loop_router_cut_corners_small_segment() {
    // A segment shorter than 2 * distance halves the effective distance:
    // min(10, 8 / 2, 100 / 2) = 4.
    let input = chain(&[(0.0, 0.0), (8.0, 0.0), (8.0, 100.0)]);
    let result = cut_corners(&input, 10.0);

    assert_eq!(result.size(), 2, "cutCorners with 3 points should produce 2 output points");
    assert_point_approx(result.get(0), 4.0, 0.0, "small-pre");
    assert_point_approx(result.get(1), 8.0, 4.0, "small-post");
}
