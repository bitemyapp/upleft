//! Port of `Tests/ElkSwiftTests/AllCrossingsCounterTests.swift`.

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::graph_info_holder::{GraphInfoHolder, LayerSweepCrossingMinimizerCrossMinType};
use upleft_elk::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::CrossMinType;
use upleft_elk::prelude::*;

// MARK: - Helpers

fn all_crossings(creator: &mut TestGraphCreator) -> i64 {
    let graph = creator.graph;
    let node_array = creator.lg.graph_to_node_array(graph);
    let mut port_id = 0;
    for l_nodes in &node_array {
        for &l_node in l_nodes {
            for port in creator.lg[l_node].ports.clone() {
                creator.lg[port].id = port_id;
                port_id += 1;
            }
        }
    }
    let mut gd = GraphInfoHolder::new(&mut creator.lg, graph, LayerSweepCrossingMinimizerCrossMinType::BARYCENTER, &[], CrossMinType::BARYCENTER);
    gd.crossings_counter.count_all_crossings(&creator.lg, &node_array)
}

fn switch_nodes_in_layer(creator: &mut TestGraphCreator, upper_node_index: usize, lower_node_index: usize, layer_index: usize, graph: LGraphId) {
    let layer = creator.lg[graph].layers[layer_index];
    let mut nodes = creator.lg[layer].nodes.clone();
    let upper_node = nodes[upper_node_index];
    nodes[upper_node_index] = nodes[lower_node_index];
    nodes[lower_node_index] = upper_node;
    creator.lg[layer].nodes = nodes;
}

// MARK: - Tests

#[test]
fn test_count_one_crossing() {
    let mut creator = TestGraphCreator::new();
    creator.get_cross_formed_graph();
    assert_eq!(all_crossings(&mut creator), 1);
}

#[test]
fn test_count_in_layer_crossing() {
    let mut creator = TestGraphCreator::new();
    creator.get_in_layer_edges_graph();
    assert_eq!(all_crossings(&mut creator), 1);
}

#[test]
fn test_count_in_layer_crossing_and_switch() {
    let mut creator = TestGraphCreator::new();
    creator.get_in_layer_edges_graph();
    assert_eq!(all_crossings(&mut creator), 1);
}

// Swift builds the graph with a separate `NorthSouthEdgeTestGraphCreator`
// (or `InLayerEdgeTestGraphCreator`) and assigns it to `creator.graph`; here
// one creator carries every factory method, so the graph is built on the
// fresh creator directly.
#[test]
fn test_count_north_south_crossing() {
    let mut creator = TestGraphCreator::new();
    creator.graph = creator.get_north_south_downward_crossing_graph();
    assert_eq!(all_crossings(&mut creator), 1);
}

#[test]
fn test_count_northern_north_south_crossing() {
    let mut creator = TestGraphCreator::new();
    creator.graph = creator.get_north_south_upward_crossing_graph();
    assert_eq!(all_crossings(&mut creator), 1);
}

#[test]
fn test_north_south_dummy_edge_crossing() {
    let mut creator = TestGraphCreator::new();
    creator.graph = creator.get_southern_north_south_dummy_edge_crossing_graph();
    assert_eq!(all_crossings(&mut creator), 1);
}

#[test]
fn test_switch_and_count_twice() {
    let mut creator = TestGraphCreator::new();
    creator.get_cross_formed_graph();
    assert_eq!(all_crossings(&mut creator), 1);
    let g = creator.graph;
    switch_nodes_in_layer(&mut creator, 0, 1, 1, g);
    assert_eq!(all_crossings(&mut creator), 0);
}

#[test]
fn test_too_many_in_layer_crossings_with_the_old_method() {
    let mut creator = TestGraphCreator::new();
    creator.graph = creator.get_in_layer_one_layer_no_crossings();
    assert_eq!(all_crossings(&mut creator), 0);
}

#[test]
fn test_count_crossings_with_multiple_edges_between_same_nodes() {
    let mut creator = TestGraphCreator::new();
    let l0 = creator.make_layer();
    let left = creator.add_nodes_to_layer(2, l0);
    let l1 = creator.make_layer();
    let right = creator.add_nodes_to_layer(2, l1);

    let right_lower_ports = creator.add_ports_on_side(2, right[1], PortSide::WEST);
    creator.east_west_edge_from_to_port(left[0], right_lower_ports[1]);
    creator.east_west_edge_from_to_port(left[0], right_lower_ports[0]);
    let right_upper_ports = creator.add_ports_on_side(2, right[0], PortSide::WEST);
    creator.east_west_edge_from_to_port(left[1], right_upper_ports[1]);
    creator.east_west_edge_from_to_port(left[1], right_upper_ports[0]);

    assert_eq!(all_crossings(&mut creator), 4);
}

#[test]
fn test_count_crossings_in_empty_graph() {
    let mut creator = TestGraphCreator::new();
    creator.get_empty_graph();
    assert_eq!(all_crossings(&mut creator), 0);
}

#[test]
fn test_one_node_is_long_edge_dummy() {
    let mut creator = TestGraphCreator::new();
    creator.graph = creator.get_southern_north_south_dummy_edge_crossing_graph();
    assert_eq!(all_crossings(&mut creator), 1);
}

#[test]
fn test_one_node_is_long_edge_dummy_northern() {
    let mut creator = TestGraphCreator::new();
    creator.graph = creator.get_northern_north_south_dummy_edge_crossing_graph();
    assert_eq!(all_crossings(&mut creator), 1);
}

#[test]
fn test_multiple_north_south_and_long_edge_dummies_on_both_sides() {
    let mut creator = TestGraphCreator::new();
    let l0 = creator.make_layer();
    let left_nodes = creator.add_nodes_to_layer(2, l0);
    let l1 = creator.make_layer();
    let middle_nodes = creator.add_nodes_to_layer(7, l1);
    let l2 = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(6, l2);

    creator.east_west_edge_from_to(left_nodes[0], middle_nodes[2]);
    creator.east_west_edge_from_to(middle_nodes[2], right_nodes[2]);
    creator.east_west_edge_from_to(left_nodes[1], middle_nodes[4]);
    creator.east_west_edge_from_to(middle_nodes[4], right_nodes[3]);

    creator.set_as_long_edge_dummy(middle_nodes[2]);
    creator.set_as_long_edge_dummy(middle_nodes[4]);

    creator.add_north_south_edge(PortSide::NORTH, middle_nodes[3], middle_nodes[0], right_nodes[0], false);
    creator.add_north_south_edge(PortSide::NORTH, middle_nodes[3], middle_nodes[1], right_nodes[1], false);
    creator.add_north_south_edge(PortSide::SOUTH, middle_nodes[3], middle_nodes[5], right_nodes[4], false);
    creator.add_north_south_edge(PortSide::SOUTH, middle_nodes[3], middle_nodes[6], right_nodes[5], false);

    assert_eq!(all_crossings(&mut creator), 4);
}

#[test]
fn test_in_layer_crossings_on_far_left() {
    let mut creator = TestGraphCreator::new();
    let g = creator.graph;
    let l = creator.make_layer_in(g);
    let nodes = creator.add_nodes_to_layer(3, l);

    creator.set_fixed_order_constraint(nodes[1]);

    creator.add_in_layer_edge(nodes[0], nodes[1], PortSide::WEST);
    creator.add_in_layer_edge(nodes[1], nodes[2], PortSide::WEST);

    assert_eq!(all_crossings(&mut creator), 1);
}
