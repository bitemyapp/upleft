//! Port of `Tests/ElkSwiftTests/GreedyPortDistributorTests.swift`.

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::greedy_port_distributor::GreedyPortDistributor;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::i_sweep_port_distributor::ISweepPortDistributor;
use upleft_elk::prelude::*;

// MARK: - Helper

fn set_up_distributor(creator: &mut TestGraphCreator) -> GreedyPortDistributor {
    let mut port_dist = GreedyPortDistributor::new();
    let node_array = creator.lg.graph_to_node_array(creator.graph);
    initialize_all(&creator.lg, &mut port_dist, &node_array);
    port_dist
}

fn ports_ordered_as(creator: &TestGraphCreator, node: LNodeId, indices: &[usize]) -> Vec<LPortId> {
    let mut ordered = Vec::new();
    for &i in indices {
        ordered.push(creator.lg[node].ports[i]);
    }
    ordered
}

fn initialize_all(lg: &LGraphArena, pd: &mut GreedyPortDistributor, node_order: &[Vec<LNodeId>]) {
    for (layer_index, layer) in node_order.iter().enumerate() {
        pd.init_at_node_level(lg, layer_index, 0, node_order);
        for node_index in 0..layer.len() {
            pd.init_at_node_level(lg, layer_index, node_index, node_order);
            // GreedyPortDistributor has no initAtPortLevel — port counting
            // is handled via initAtNodeLevel which counts all ports on each node.
        }
    }
    pd.init_after_traversal();
}

fn ports_of(creator: &TestGraphCreator, node: LNodeId) -> Vec<LPortId> {
    creator.lg[node].ports.clone()
}

fn node_order(creator: &TestGraphCreator) -> Vec<Vec<LNodeId>> {
    creator.lg.graph_to_node_array(creator.graph)
}

// MARK: - Tests

/// Cross on western side:
/// ```text
/// *  ___
///  \/| |
///  /\| |
/// *  |_|
/// ```
#[test]
fn test_distribute_ports_given_cross_on_western_side_remove_crossing() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(2, layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_node = creator.add_node_to_layer(layer);
    creator.east_west_edge_from_to(left_nodes[0], right_node);
    creator.east_west_edge_from_to(left_nodes[1], right_node);

    let expected_port_order_right_node = ports_ordered_as(&creator, right_node, &[1, 0]);

    let mut port_dist = set_up_distributor(&mut creator);
    let order = node_order(&creator);
    let improved = port_dist.distribute_ports_while_sweeping(&mut creator.lg, &order, 1, true);

    assert!(improved);
    assert_eq!(ports_of(&creator, right_node), expected_port_order_right_node);
}

/// No ports on right side:
/// ```text
/// *  ___
///  \/| | *
///  /\| | *
/// *  |_|
/// ```
#[test]
fn test_distribute_ports_given_no_ports_on_right_side_nothing_happens() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(2, layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let middle_node = creator.add_node_to_layer(layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    creator.add_nodes_to_layer(2, layer);
    creator.east_west_edge_from_to(left_nodes[0], middle_node);
    creator.east_west_edge_from_to(left_nodes[1], middle_node);

    let expected_port_order_right_node = ports_ordered_as(&creator, middle_node, &[0, 1]);

    let mut port_dist = set_up_distributor(&mut creator);
    let order = node_order(&creator);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &order, 1, false);

    assert_eq!(ports_of(&creator, middle_node), expected_port_order_right_node);
}

/// Multiple crossings on western side:
/// ```text
/// *    ___
///  \/--| |
///  /\ /| |
/// *  x | |
/// *-/ \|_|
/// ```
#[test]
fn test_distribute_ports_given_multiple_crossings_on_western_side_remove_crossing() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(3, layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_node = creator.add_node_to_layer(layer);
    creator.east_west_edge_from_to(left_nodes[0], right_node);
    creator.east_west_edge_from_to(left_nodes[2], right_node);
    creator.east_west_edge_from_to(left_nodes[1], right_node);

    let expected_port_order_right_node = ports_ordered_as(&creator, right_node, &[1, 2, 0]);

    let mut port_dist = set_up_distributor(&mut creator);
    let order = node_order(&creator);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &order, 1, true);

    assert_eq!(ports_of(&creator, right_node), expected_port_order_right_node);
}

/// Cross on eastern side:
/// ```text
/// ___
/// | |\/*
/// |_|/\*
/// ```
#[test]
fn test_distribute_ports_given_crossings_on_eastern_side_remove_them() {
    let mut creator = TestGraphCreator::new();
    let layer = creator.make_layer();
    let left_nodes = creator.add_nodes_to_layer(1, layer);
    let layer = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(2, layer);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[0]);

    let expected_port_order_left_node = ports_ordered_as(&creator, left_nodes[0], &[1, 0]);

    let mut port_dist = set_up_distributor(&mut creator);
    let order = node_order(&creator);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &order, 0, false);

    assert_eq!(ports_of(&creator, left_nodes[0]), expected_port_order_left_node);
}

/// Fixed port order, no change:
/// ```text
/// ___
/// | |\/*
/// |_|/\*
/// ```
#[test]
fn test_distribute_ports_fixed_port_order_no_change() {
    let mut creator = TestGraphCreator::new();
    let layer = creator.make_layer();
    let left_nodes = creator.add_nodes_to_layer(1, layer);
    let layer = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(2, layer);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[0]);
    creator.set_fixed_order_constraint(left_nodes[0]);

    let expected_port_order_left_node = ports_ordered_as(&creator, left_nodes[0], &[0, 1]);

    let mut port_dist = set_up_distributor(&mut creator);
    let order = node_order(&creator);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &order, 0, false);

    assert_eq!(ports_of(&creator, left_nodes[0]), expected_port_order_left_node);
}

/// Double cross between compound and non-compound nodes switches ports:
/// ```text
/// ____
/// |*-+   *
/// |  |\\/
/// |*-+/\\
/// |--|   *
/// ```
#[test]
fn test_given_double_cross_between_compound_and_non_compound_nodes_switches_ports() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_outer_node = creator.add_node_to_layer(layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_nodes = creator.add_nodes_to_layer(2, layer);
    let left_outer_ports = creator.add_ports_on_side(2, left_outer_node, PortSide::EAST);
    creator.east_west_edge_from_port_to(left_outer_ports[0], right_nodes[1]);
    creator.east_west_edge_from_port_to(left_outer_ports[0], right_nodes[1]);
    creator.east_west_edge_from_port_to(left_outer_ports[1], right_nodes[0]);
    let left_inner_graph = creator.nested_graph(left_outer_node);
    let layer = creator.make_layer_in(left_inner_graph);
    let left_inner_nodes = creator.add_nodes_to_layer(2, layer);
    let layer = creator.make_layer_in(left_inner_graph);
    let left_inner_dummy_nodes = creator.add_external_port_dummies_to_layer(layer, &left_outer_ports);
    creator.east_west_edge_from_to(left_inner_nodes[0], left_inner_dummy_nodes[0]);
    creator.east_west_edge_from_to(left_inner_nodes[1], left_inner_dummy_nodes[1]);
    creator.set_up_ids();

    let expected_port_order_left_node = ports_ordered_as(&creator, left_outer_node, &[1, 0]);

    let mut port_dist = set_up_distributor(&mut creator);
    let order = node_order(&creator);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &order, 0, false);

    assert_eq!(ports_of(&creator, left_outer_node), expected_port_order_left_node);
}

/// Single cross between compound and non-compound nodes does not switch:
/// ```text
/// ____
/// |*-+  *
/// |  |\/
/// |*-+/\
/// |--|  *
/// ```
#[test]
fn test_given_single_cross_between_compound_and_non_compound_nodes_does_not_switch_ports() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_outer_node = creator.add_node_to_layer(layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_nodes = creator.add_nodes_to_layer(2, layer);
    let left_outer_ports = creator.add_ports_on_side(2, left_outer_node, PortSide::EAST);
    creator.east_west_edge_from_port_to(left_outer_ports[0], right_nodes[1]);
    creator.east_west_edge_from_port_to(left_outer_ports[1], right_nodes[0]);
    let left_inner_graph = creator.nested_graph(left_outer_node);
    let layer = creator.make_layer_in(left_inner_graph);
    let left_inner_nodes = creator.add_nodes_to_layer(2, layer);
    let layer = creator.make_layer_in(left_inner_graph);
    let left_inner_dummy_nodes = creator.add_external_port_dummies_to_layer(layer, &left_outer_ports);
    creator.east_west_edge_from_to(left_inner_nodes[0], left_inner_dummy_nodes[0]);
    creator.east_west_edge_from_to(left_inner_nodes[1], left_inner_dummy_nodes[1]);

    let expected_port_order_left_node = ports_ordered_as(&creator, left_outer_node, &[0, 1]);

    let mut port_dist = set_up_distributor(&mut creator);
    let order = node_order(&creator);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &order, 0, false);

    assert_eq!(ports_of(&creator, left_outer_node), expected_port_order_left_node);
}

/// More hierarchical nodes, does not switch:
#[test]
fn test_given_more_hierarchical_nodes_does_not_switch_ports() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_outer_node = creator.add_node_to_layer(layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_nodes = creator.add_nodes_to_layer(3, layer);
    let left_outer_ports = creator.add_ports_on_side(3, left_outer_node, PortSide::EAST);
    creator.east_west_edge_from_port_to(left_outer_ports[0], right_nodes[1]);
    creator.east_west_edge_from_port_to(left_outer_ports[1], right_nodes[0]);
    creator.east_west_edge_from_port_to(left_outer_ports[2], right_nodes[2]);
    let left_inner_graph = creator.nested_graph(left_outer_node);
    let layer = creator.make_layer_in(left_inner_graph);
    let left_inner_nodes = creator.add_nodes_to_layer(3, layer);
    let layer = creator.make_layer_in(left_inner_graph);
    let left_inner_dummy_nodes = creator.add_external_port_dummies_to_layer(layer, &left_outer_ports);
    creator.east_west_edge_from_to(left_inner_nodes[0], left_inner_dummy_nodes[0]);
    creator.east_west_edge_from_to(left_inner_nodes[1], left_inner_dummy_nodes[1]);
    creator.east_west_edge_from_to(left_inner_nodes[2], left_inner_dummy_nodes[2]);

    let expected_port_order_left_node = ports_ordered_as(&creator, left_outer_node, &[0, 1, 2]);

    let mut port_dist = set_up_distributor(&mut creator);
    let order = node_order(&creator);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &order, 0, false);

    assert_eq!(ports_of(&creator, left_outer_node), expected_port_order_left_node);
}

/// More hierarchical nodes variant 2:
#[test]
fn test_given_more_hierarchical_nodes2_does_not_switch_ports() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_outer_node = creator.add_node_to_layer(layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_nodes = creator.add_nodes_to_layer(3, layer);
    let left_outer_ports = creator.add_ports_on_side(3, left_outer_node, PortSide::EAST);
    creator.east_west_edge_from_port_to(left_outer_ports[0], right_nodes[1]);
    creator.east_west_edge_from_port_to(left_outer_ports[1], right_nodes[0]);
    creator.east_west_edge_from_port_to(left_outer_ports[2], right_nodes[2]);
    let left_inner_graph = creator.nested_graph(left_outer_node);
    let layer = creator.make_layer_in(left_inner_graph);
    let left_inner_node = creator.add_node_to_layer(layer);
    let layer = creator.make_layer_in(left_inner_graph);
    let right_inner_nodes = creator.add_nodes_to_layer(3, layer);
    let layer = creator.make_layer_in(left_inner_graph);
    let dummy_nodes = creator.add_external_port_dummies_to_layer(layer, &left_outer_ports);
    creator.east_west_edge_from_to(left_inner_node, right_inner_nodes[2]);
    creator.east_west_edge_from_to(right_inner_nodes[0], dummy_nodes[0]);
    creator.east_west_edge_from_to(right_inner_nodes[1], dummy_nodes[1]);
    creator.east_west_edge_from_to(right_inner_nodes[2], dummy_nodes[2]);

    let expected_port_order_left_node = ports_ordered_as(&creator, left_outer_node, &[0, 1, 2]);

    let mut port_dist = set_up_distributor(&mut creator);
    let order = node_order(&creator);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &order, 0, false);

    assert_eq!(ports_of(&creator, left_outer_node), expected_port_order_left_node);
}

/// Two hierarchical nodes in one layer:
#[test]
fn test_distribute_ports_while_sweeping_given_two_hierarchical_nodes_in_one_layer() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let left_layer = creator.make_layer_in(g);
    let g = creator.get_graph();
    let right_layer = creator.make_layer_in(g);
    for _ in 0..2 {
        let left_nodes = creator.add_nodes_to_layer(1, left_layer);
        let right_nodes = creator.add_nodes_to_layer(2, right_layer);
        creator.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
        creator.east_west_edge_from_to(left_nodes[0], right_nodes[0]);
    }
    let left_outer_node = creator.lg[left_layer].nodes[0];
    let expected_port_order_left_node = ports_ordered_as(&creator, left_outer_node, &[1, 0]);

    let g = creator.get_graph();
    let node_order = creator.lg.graph_to_node_array(g);
    let mut port_dist = set_up_distributor(&mut creator);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &node_order, 0, false);

    assert_eq!(ports_of(&creator, left_outer_node), expected_port_order_left_node);
}

/// No change needed:
/// ```text
/// ___
/// | |--*
/// |_|--*
/// ```
#[test]
fn test_no_change() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let left_layer = creator.make_layer_in(g);
    let g = creator.get_graph();
    let right_layer = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(1, left_layer);
    let right_nodes = creator.add_nodes_to_layer(2, right_layer);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[0]);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
    let g = creator.get_graph();
    let node_order = creator.lg.graph_to_node_array(g);
    let mut port_dist = set_up_distributor(&mut creator);
    let improved = port_dist.distribute_ports_while_sweeping(&mut creator.lg, &node_order, 0, false);

    assert!(!improved);
}
