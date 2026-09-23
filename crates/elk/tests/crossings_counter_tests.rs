//! Port of `Tests/ElkSwiftTests/CrossingsCounterTests.swift`.

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::counting::crossings_counter::CrossingsCounter;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::graph_info_holder::{GraphInfoHolder, LayerSweepCrossingMinimizerCrossMinType};
use upleft_elk::org::eclipse::elk::alg::layered::p3order::i_sweep_port_distributor::ISweepPortDistributor;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::CrossMinType;
use upleft_elk::prelude::*;

// MARK: - Helpers

fn order(creator: &mut TestGraphCreator) -> Vec<Vec<LNodeId>> {
    let g = creator.get_graph();
    creator.lg.graph_to_node_array(g)
}

fn get_num_ports(creator: &TestGraphCreator, current_order: &[Vec<LNodeId>]) -> usize {
    current_order.iter().flatten().map(|&n| creator.lg[n].ports.len()).sum()
}

fn counter_for(creator: &TestGraphCreator, node_order: &[Vec<LNodeId>]) -> CrossingsCounter {
    CrossingsCounter::from_values(vec![0; get_num_ports(creator, node_order)])
}

// MARK: - Between-Layer Crossing Tests

/// Two single-node layers, two edges forming a cross => 1 crossing
#[test]
fn test_count_crossings_between_layers_fixed_port_order_crossing_on_two_nodes() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let l0 = creator.make_layer_in(g);
    let left = creator.add_node_to_layer(l0);
    let g = creator.get_graph();
    let l1 = creator.make_layer_in(g);
    let right = creator.add_node_to_layer(l1);
    creator.east_west_edge_from_to(left, right);
    creator.east_west_edge_from_to(left, right);

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);

    assert_eq!(counter.count_crossings_between_layers(&creator.lg, &node_order[0], &node_order[1]), 1);
}

/// 5 nodes in 1 layer, 3 in-layer edges => 1 in-layer crossing on EAST side
#[test]
fn test_long_in_layer_crossings() {
    let mut creator = TestGraphCreator::new();
    let l = creator.make_layer();
    let nodes = creator.add_nodes_to_layer(5, l);
    creator.add_in_layer_edge(nodes[0], nodes[1], PortSide::EAST);
    creator.add_in_layer_edge(nodes[1], nodes[3], PortSide::EAST);
    creator.add_in_layer_edge(nodes[2], nodes[4], PortSide::EAST);

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);

    assert_eq!(counter.count_in_layer_crossings_on_side(&creator.lg, &node_order[0], PortSide::EAST), 1);
}

/// Cross-formed graph => 1 crossing between layers
#[test]
fn test_count_crossings_between_layers_cross_formed() {
    let mut creator = TestGraphCreator::new();
    creator.get_cross_formed_graph();

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);

    assert_eq!(counter.count_crossings_between_layers(&creator.lg, &node_order[0], &node_order[1]), 1);
}

/// Cross formed with multiple edges between same nodes => 4 crossings
#[test]
fn test_count_crossings_between_layers_cross_formed_multiple_edges_between_same_nodes() {
    let mut creator = TestGraphCreator::new();
    let g = creator.graph;
    let left_layer = creator.make_layer_in(g);
    let right_layer = creator.make_layer_in(g);

    let top_left = creator.add_node_to_layer(left_layer);
    let bottom_left = creator.add_node_to_layer(left_layer);
    let top_right = creator.add_node_to_layer(right_layer);
    let bottom_right = creator.add_node_to_layer(right_layer);

    let top_left_top_port = creator.add_port_on_side(top_left, PortSide::EAST);
    let top_left_bottom_port = creator.add_port_on_side(top_left, PortSide::EAST);
    let bottom_right_bottom_port = creator.add_port_on_side(bottom_right, PortSide::WEST);
    let bottom_right_top_port = creator.add_port_on_side(bottom_right, PortSide::WEST);
    creator.add_edge_between_ports(top_left_top_port, bottom_right_top_port);
    creator.add_edge_between_ports(top_left_bottom_port, bottom_right_bottom_port);

    let bottom_left_top_port = creator.add_port_on_side(bottom_left, PortSide::EAST);
    let bottom_left_bottom_port = creator.add_port_on_side(bottom_left, PortSide::EAST);
    let top_right_bottom_port = creator.add_port_on_side(top_right, PortSide::WEST);
    let top_right_top_port = creator.add_port_on_side(top_right, PortSide::WEST);
    creator.add_edge_between_ports(bottom_left_top_port, top_right_top_port);
    creator.add_edge_between_ports(bottom_left_bottom_port, top_right_bottom_port);

    let node_order = order(&mut creator);
    let mut gd = GraphInfoHolder::new(&mut creator.lg, g, LayerSweepCrossingMinimizerCrossMinType::BARYCENTER, &[], CrossMinType::BARYCENTER);
    gd.port_distributor.distribute_ports_while_sweeping(&mut creator.lg, &node_order, 1, true);

    let mut counter = counter_for(&creator, &node_order);

    assert_eq!(counter.count_crossings_between_layers(&creator.lg, &node_order[0], &node_order[1]), 4);
}

/// Cross with extra edge in between => 3 crossings
#[test]
fn test_count_crossings_between_layers_cross_with_extra_edge_in_between() {
    let mut creator = TestGraphCreator::new();
    creator.get_cross_with_extra_edge_in_between_graph();

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);

    assert_eq!(counter.count_crossings_between_layers(&creator.lg, &node_order[0], &node_order[1]), 3);
}

/// Self loops should be ignored => 1 crossing
#[test]
fn test_count_crossings_between_layers_ignore_self_loops() {
    let mut creator = TestGraphCreator::new();
    creator.get_cross_with_many_self_loops_graph();

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);

    assert_eq!(counter.count_crossings_between_layers(&creator.lg, &node_order[0], &node_order[1]), 1);
}

/// More complex three-layer graph with port distribution => 1 crossing
#[test]
fn test_count_crossings_between_layers_more_complex_three_layer_graph() {
    let mut creator = TestGraphCreator::new();
    creator.get_more_complex_three_layer_graph();
    let node_order = order(&mut creator);
    let g = creator.graph;
    let mut gd = GraphInfoHolder::new(&mut creator.lg, g, LayerSweepCrossingMinimizerCrossMinType::BARYCENTER, &[], CrossMinType::BARYCENTER);
    gd.port_distributor.distribute_ports_while_sweeping(&mut creator.lg, &node_order, 1, true);

    let mut counter = counter_for(&creator, &node_order);

    assert_eq!(counter.count_crossings_between_layers(&creator.lg, &node_order[0], &node_order[1]), 1);
}

/// Fixed port order graph => 1 crossing
#[test]
fn test_count_crossings_between_layers_fixed_port_order() {
    let mut creator = TestGraphCreator::new();
    creator.get_fixed_port_order_graph();

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);

    assert_eq!(counter.count_crossings_between_layers(&creator.lg, &node_order[0], &node_order[1]), 1);
}

/// Two edges into the same port => 2 crossings
#[test]
fn test_count_crossings_between_layers_into_same_port() {
    let mut creator = TestGraphCreator::new();
    let g = creator.graph;
    let left_layer = creator.make_layer_in(g);
    let right_layer = creator.make_layer_in(g);

    let top_left = creator.add_node_to_layer(left_layer);
    let bottom_left = creator.add_node_to_layer(left_layer);
    let top_right = creator.add_node_to_layer(right_layer);
    let bottom_right = creator.add_node_to_layer(right_layer);

    creator.east_west_edge_from_to(top_left, bottom_right);
    let bottom_left_first_port = creator.add_port_on_side(bottom_left, PortSide::EAST);
    let bottom_left_second_port = creator.add_port_on_side(bottom_left, PortSide::EAST);
    let top_right_first_port = creator.add_port_on_side(top_right, PortSide::WEST);

    creator.add_edge_between_ports(bottom_left_first_port, top_right_first_port);
    creator.add_edge_between_ports(bottom_left_second_port, top_right_first_port);
    creator.set_up_ids();

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);

    assert_eq!(counter.count_crossings_between_layers(&creator.lg, &node_order[0], &node_order[1]), 2);
}

// MARK: - Between-Port Crossing Tests

/// Western crossings on given ports => 1 crossing
#[test]
fn test_count_crossings_between_ports_given_western_crossings() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let l0 = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(2, l0);
    let g = creator.get_graph();
    let l1 = creator.make_layer_in(g);
    let right_nodes = creator.add_nodes_to_layer(2, l1);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
    creator.east_west_edge_from_to(left_nodes[1], right_nodes[1]);
    creator.east_west_edge_from_to(left_nodes[1], right_nodes[0]);

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);
    counter.init_for_counting_between(&creator.lg, &left_nodes, &right_nodes);

    let result = counter.count_crossings_between_ports_in_both_orders(&creator.lg, creator.lg[right_nodes[1]].ports[1], creator.lg[right_nodes[1]].ports[0]);
    assert_eq!(result.0, 1);
}

/// Eastern-side crossings => 1 crossing
#[test]
fn test_count_crossings_between_ports_given_crossings_on_eastern_side() {
    let mut creator = TestGraphCreator::new();
    let l0 = creator.make_layer();
    let left_nodes = creator.add_nodes_to_layer(1, l0);
    let l1 = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(2, l1);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[0]);

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);
    counter.init_for_counting_between(&creator.lg, &left_nodes, &right_nodes);

    let result = counter.count_crossings_between_ports_in_both_orders(&creator.lg, creator.lg[left_nodes[0]].ports[0], creator.lg[left_nodes[0]].ports[1]);
    assert_eq!(result.0, 1);
}

/// Counting two different graphs does not interfere: 1 crossing, then 0 after port switch
#[test]
fn test_counting_two_different_graphs_does_not_interfere() {
    let mut creator = TestGraphCreator::new();
    let l0 = creator.make_layer();
    let left_nodes = creator.add_nodes_to_layer(3, l0);
    let l1 = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(3, l1);
    let left_node = left_nodes[1];
    let left_ports = creator.add_ports_on_side(2, left_node, PortSide::EAST);
    creator.east_west_edge_from_to(left_nodes[2], right_nodes[1]);
    creator.east_west_edge_from_port_to(left_ports[0], right_nodes[1]);
    creator.east_west_edge_from_port_to(left_ports[1], right_nodes[0]);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[0]);

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);
    counter.init_for_counting_between(&creator.lg, &left_nodes, &right_nodes);

    let result1 = counter.count_crossings_between_ports_in_both_orders(&creator.lg, creator.lg[left_node].ports[0], creator.lg[left_node].ports[1]);
    assert_eq!(result1.0, 1);

    counter.switch_ports(&creator.lg, left_ports[0], left_ports[1]);
    // Swap ports in the node's port list to match Java: set(0, leftPorts[1]), set(1, leftPorts[0])
    creator.lg[left_node].ports[0] = left_ports[1];
    creator.lg[left_node].ports[1] = left_ports[0];

    let result2 = counter.count_crossings_between_ports_in_both_orders(&creator.lg, creator.lg[left_node].ports[0], creator.lg[left_node].ports[1]);
    assert_eq!(result2.0, 0);
}

/// Two edges into same port, counting between specific ports => 2 crossings
#[test]
fn test_count_crossings_between_ports_two_edges_into_same_port() {
    let mut creator = TestGraphCreator::new();
    let left_layer = creator.make_layer();
    let right_layer = creator.make_layer();

    let top_left = creator.add_node_to_layer(left_layer);
    let bottom_left = creator.add_node_to_layer(left_layer);
    let top_right = creator.add_node_to_layer(right_layer);
    let bottom_right = creator.add_node_to_layer(right_layer);

    creator.east_west_edge_from_to(top_left, bottom_right);
    let bottom_left_port = creator.add_port_on_side(bottom_left, PortSide::EAST);
    let top_right_port = creator.add_port_on_side(top_right, PortSide::WEST);

    creator.add_edge_between_ports(bottom_left_port, top_right_port);
    creator.add_edge_between_ports(bottom_left_port, top_right_port);
    creator.set_up_ids();

    let node_order = order(&mut creator);
    let mut counter = counter_for(&creator, &node_order);
    counter.init_for_counting_between(&creator.lg, &node_order[0], &node_order[1]);

    let result = counter.count_crossings_between_ports_in_both_orders(&creator.lg, bottom_left_port, creator.lg[top_left].ports[0]);
    assert_eq!(result.0, 2);
}
