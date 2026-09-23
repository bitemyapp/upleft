//! Port of `Tests/ElkSwiftTests/NorthSouthEdgeAllCrossingsCounterTests.swift`.

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::counting::crossings_counter::CrossingsCounter;
use upleft_elk::prelude::*;

// MARK: - Helpers

fn count_ns_crossings_in_layer(creator: &mut TestGraphCreator, layer_index: usize) -> i64 {
    creator.set_up_ids();
    let graph = creator.get_graph();
    let mut num_ports = 0;
    for &layer in &creator.lg[graph].layers {
        for &node in &creator.lg[layer].nodes {
            num_ports += creator.lg[node].ports.len();
        }
    }
    let mut counter = CrossingsCounter::from_values(vec![0; num_ports]);
    counter.count_north_south_port_crossings_in_layer(&creator.lg, &creator.lg.graph_to_node_array(graph)[layer_index])
}

// MARK: - Tests

#[test]
fn test_northern_north_south_node_single_crossing() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_upward_crossing_graph();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 0), 1);
}

#[test]
fn test_northern_north_south_node_multiple_crossings() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_upward_multiple_crossing_graph();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 0), 3);
}

#[test]
fn test_southern_two_edge_east_crossing() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_downward_crossing_graph();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 0), 1);
}

#[test]
fn test_southern_north_south_multiple_node_crossing() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_downward_multiple_crossing_graph();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 0), 3);
}

#[test]
fn test_southern_two_western_edges() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_southern_two_western_edges();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 1);
}

#[test]
fn test_southern_three_western_edges() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_southern_three_western_edges();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 3);
}

#[test]
fn test_north_south_edges_come_from_both_sides_dont_cross() {
    let mut creator = TestGraphCreator::new();
    creator.get_southern_north_south_graph_edges_from_east_and_west_no_crossings();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 0);
}

#[test]
fn test_southern_north_south_edges_both_to_east_dont_cross() {
    let mut creator = TestGraphCreator::new();
    creator.get_southern_north_south_edges_both_to_east();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 0), 0);
}

#[test]
fn test_north_south_edges_come_from_both_sides_do_cross() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_edges_from_east_and_west_and_cross();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 1);
}

#[test]
fn test_northern_both_edges_western() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_northern_western_edges();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 0), 0);
}

#[test]
fn test_northern_eastern_port_to_west_western_port_to_east() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_northern_eastern_port_to_west_western_port_to_east();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 1);
}

#[test]
fn test_all_sides_multiple_crossings() {
    let mut creator = TestGraphCreator::new();
    creator.get_north_south_all_sides_multiple_crossings();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 4);
}

#[test]
fn test_one_edge_dummy_is_crossed_by_one_southern_north_south_port_edge() {
    let mut creator = TestGraphCreator::new();
    creator.get_southern_north_south_dummy_edge_crossing_graph();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 1);
}

#[test]
fn test_one_edge_dummy_is_crossed_by_two_southern_north_south_port_edges() {
    let mut creator = TestGraphCreator::new();
    creator.get_southern_north_south_dummy_edge_two_crossing_graph();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 2);
}

#[test]
fn test_southern_two_dummy_edge_and_two_north_south_should_cross_four_times() {
    let mut creator = TestGraphCreator::new();
    creator.get_southern_two_dummy_edge_and_north_south_crossing_graph();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 4);
}

#[test]
fn test_normal_nodes_north_south_edges_have_crossings_to_long_edge_dummy_on_both_sides() {
    let mut creator = TestGraphCreator::new();
    creator.get_multiple_north_south_and_long_edge_dummies_on_both_sides();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 4);
}

#[test]
fn test_ignores_unconnected_ports_for_normal_node_and_long_edge_dummies() {
    let mut creator = TestGraphCreator::new();
    creator.get_long_edge_dummy_and_normal_node_with_unused_ports_on_southern_side();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 0);
}

#[test]
fn test_no_north_south_node() {
    let mut creator = TestGraphCreator::new();
    creator.get_cross_formed_graph();
    assert_eq!(count_ns_crossings_in_layer(&mut creator, 0), 0);
}

#[test]
fn test_more_than_one_edge_into_ns_node_counts_these_too() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let l0 = creator.make_layer_in(g);
    let left_node = creator.add_node_to_layer(l0);
    let g = creator.get_graph();
    let l1 = creator.make_layer_in(g);
    let middle_nodes = creator.add_nodes_to_layer(3, l1);
    let g = creator.get_graph();
    let l2 = creator.make_layer_in(g);
    let right_nodes = creator.add_nodes_to_layer(2, l2);

    creator.set_fixed_order_constraint(middle_nodes[2]);

    // ports are added in clockwise fashion
    creator.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[1], right_nodes[0], false);
    creator.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[0], left_node, true);
    // second edge on middle node
    let middle_node_port = creator.lg[middle_nodes[1]].ports[0];
    creator.east_west_edge_from_port_to(middle_node_port, right_nodes[1]);

    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 2);
}

#[test]
fn test_the_one_that_failed_with_the_old_counting() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let l0 = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(4, l0);
    let g = creator.get_graph();
    let l1 = creator.make_layer_in(g);
    let middle_nodes = creator.add_nodes_to_layer(5, l1);

    creator.set_fixed_order_constraint(middle_nodes[4]);

    for i in (0..=3).rev() {
        creator.add_north_south_edge(PortSide::NORTH, middle_nodes[4], middle_nodes[i], left_nodes[i], false);
    }

    assert_eq!(count_ns_crossings_in_layer(&mut creator, 1), 0);
}
