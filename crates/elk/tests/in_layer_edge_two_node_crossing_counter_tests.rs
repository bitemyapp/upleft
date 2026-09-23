//! Port of `Tests/ElkSwiftTests/InLayerEdgeTwoNodeCrossingCounterTests.swift`.

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::counting::crossings_counter::CrossingsCounter;
use upleft_elk::prelude::*;

// MARK: - Fixture

/// The XCTestCase's instance state; `new()` is `setUp()`.
struct Fixture {
    creator: TestGraphCreator,
    left_counter: Option<CrossingsCounter>,
    right_counter: Option<CrossingsCounter>,
    node_order: Vec<LNodeId>,
    upper_lower_crossings: i64,
    lower_upper_crossings: i64,
}

// MARK: - Helpers

fn get_n_ports(creator: &TestGraphCreator, current_order: &[Vec<LNodeId>]) -> usize {
    let mut n_ports = 0;
    for layer in current_order {
        for &node in layer {
            n_ports += creator.lg[node].ports.len();
        }
    }
    n_ports
}

fn number_ids_ascendingly(creator: &mut TestGraphCreator, nodes: &[LNodeId]) {
    for (i, &node) in nodes.iter().enumerate() {
        creator.lg[node].id = i as i32;
    }
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            creator: TestGraphCreator::new(),
            left_counter: None,
            right_counter: None,
            node_order: Vec::new(),
            upper_lower_crossings: 0,
            lower_upper_crossings: 0,
        }
    }

    fn init_crossing_counter_for_layer_index(&mut self, layer_index: usize) {
        let g = self.creator.get_graph();
        let current_order = self.creator.lg.graph_to_node_array(g);
        self.node_order = current_order[layer_index].clone();
        number_ids_ascendingly(&mut self.creator, &self.node_order);
        let num_ports = get_n_ports(&self.creator, &current_order);
        let mut left_counter = CrossingsCounter::from_values(vec![0; num_ports]);
        let mut right_counter = CrossingsCounter::from_values(vec![0; num_ports]);
        left_counter.init_port_positions_for_in_layer_crossings(&self.creator.lg, &self.node_order, PortSide::WEST);
        right_counter.init_port_positions_for_in_layer_crossings(&self.creator.lg, &self.node_order, PortSide::EAST);
        self.left_counter = Some(left_counter);
        self.right_counter = Some(right_counter);
    }

    fn count_crossings_in_layer_for_upper_node_lower_node(&mut self, layer_index: usize, upper: usize, lower: usize) {
        self.init_crossing_counter_for_layer_index(layer_index);
        self.count_crossings(upper, lower);
    }

    fn count_crossings(&mut self, upper_index: usize, lower_index: usize) {
        let upper = self.node_order[upper_index];
        let lower = self.node_order[lower_index];
        let left_result = self.left_counter.as_mut().unwrap().count_in_layer_crossings_between_nodes_in_both_orders(
            &self.creator.lg,
            upper,
            lower,
            PortSide::WEST,
        );
        let right_result = self.right_counter.as_mut().unwrap().count_in_layer_crossings_between_nodes_in_both_orders(
            &self.creator.lg,
            upper,
            lower,
            PortSide::EAST,
        );
        self.upper_lower_crossings = left_result.0 + right_result.0;
        self.lower_upper_crossings = left_result.1 + right_result.1;
    }

    fn switch_order_and_notify_counter(&mut self, index_one: usize, index_two: usize) {
        let first = self.node_order[index_one];
        let second = self.node_order[index_two];
        self.left_counter.as_mut().unwrap().switch_nodes(&self.creator.lg, first, second, PortSide::WEST);
        self.right_counter.as_mut().unwrap().switch_nodes(&self.creator.lg, first, second, PortSide::EAST);
        let one = self.node_order[index_one];
        self.node_order[index_one] = self.node_order[index_two];
        self.node_order[index_two] = one;
    }
}

// MARK: - Tests

#[test]
fn test_ignores_in_between_layer_edges() {
    let mut f = Fixture::new();
    f.creator.get_cross_formed_graph();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_count_in_layer_edge_with_normal_edge_crossing() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_graph();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_crossings_when_switched() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_graph_which_results_in_crossings_when_switched();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 1, 2);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 1, "lowerUpperCrossings");
}

#[test]
fn test_in_layer_edge_on_lower_node() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_graph();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_switch_node_order() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_graph();
    f.init_crossing_counter_for_layer_index(1);
    f.switch_order_and_notify_counter(1, 2);
    f.count_crossings(0, 1);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_fixed_port_order_crossing_to_in_between_layer_edge() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_graph_with_crossings_to_between_layer_edge_with_fixed_port_order();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 2, "lowerUpperCrossings");

    f.switch_order_and_notify_counter(0, 1);
    f.count_crossings(0, 1);
    assert_eq!(f.upper_lower_crossings, 2, "upperLowerCrossings after switch");
    assert_eq!(f.lower_upper_crossings, 1, "lowerUpperCrossings after switch");
}

#[test]
fn test_fixed_port_order_crossings_and_normal_edge_crossings() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_with_fixed_port_order_and_normal_edge_crossings();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 2, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 1, "lowerUpperCrossings");

    f.switch_order_and_notify_counter(0, 1);
    f.count_crossings(0, 1);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings after switch");
    assert_eq!(f.lower_upper_crossings, 2, "lowerUpperCrossings after switch");
}

#[test]
fn test_ignores_self_loops() {
    let mut f = Fixture::new();
    f.creator.get_cross_with_many_self_loops_graph();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_crossings_on_both_sides() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_crossings_on_both_sides();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 2, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_fixed_port_order_in_layer_no_crossings() {
    let mut f = Fixture::new();
    f.creator.get_fixed_port_order_in_layer_edges_dont_cross_each_other();
    f.count_crossings_in_layer_for_upper_node_lower_node(0, 0, 1);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_fixed_port_order_in_layer_with_always_remaining_crossings_are_not_counted() {
    let mut f = Fixture::new();
    f.creator.get_fixed_port_order_in_layer_edges_with_crossings();
    f.count_crossings_in_layer_for_upper_node_lower_node(0, 0, 1);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 1, "lowerUpperCrossings");
}

#[test]
fn test_one_node() {
    let mut f = Fixture::new();
    f.creator.get_one_node_graph();
    f.count_crossings_in_layer_for_upper_node_lower_node(0, 0, 0);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_more_complex() {
    let mut f = Fixture::new();
    f.creator.get_more_complex_in_layer_graph();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 6, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 6, "lowerUpperCrossings");
}

#[test]
fn test_downward_in_layer_edges_on_lower_node() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_fixed_port_order_in_layer_and_in_between_layer_crossing();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 2, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 2, "lowerUpperCrossings");
}

#[test]
fn test_one_layer_in_layer_crossing_should_disappear_after_any_switch() {
    let mut f = Fixture::new();
    f.creator.get_one_layer_with_in_layer_crossings();

    f.count_crossings_in_layer_for_upper_node_lower_node(0, 0, 1);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings (0,1)");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings (0,1)");

    f.count_crossings_in_layer_for_upper_node_lower_node(0, 1, 2);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings (1,2)");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings (1,2)");

    f.count_crossings_in_layer_for_upper_node_lower_node(0, 2, 3);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings (2,3)");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings (2,3)");

    f.switch_order_and_notify_counter(0, 1);
    f.count_crossings(0, 1);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings after switch (0,1)");
    assert_eq!(f.lower_upper_crossings, 1, "lowerUpperCrossings after switch (0,1)");

    f.switch_order_and_notify_counter(0, 1);
    f.switch_order_and_notify_counter(1, 2);
    f.count_crossings(1, 2);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings after switch (1,2)");
    assert_eq!(f.lower_upper_crossings, 1, "lowerUpperCrossings after switch (1,2)");

    f.switch_order_and_notify_counter(1, 2);
    f.switch_order_and_notify_counter(2, 3);
    f.count_crossings(2, 3);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings after switch (2,3)");
    assert_eq!(f.lower_upper_crossings, 1, "lowerUpperCrossings after switch (2,3)");
}

#[test]
fn test_more_than_one_edge_into_a_port() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_multiple_edges_into_single_port();
    f.count_crossings_in_layer_for_upper_node_lower_node(1, 1, 2);
    assert_eq!(f.upper_lower_crossings, 2, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_in_between_layer_edges_into_node_with_no_fixed_port_order_cause_crossings() {
    let mut f = Fixture::new();
    f.creator.multiple_in_between_layer_edges_into_node_with_no_fixed_port_order_cause_crossings();

    f.count_crossings_in_layer_for_upper_node_lower_node(1, 0, 1);
    assert_eq!(f.upper_lower_crossings, 2, "upperLowerCrossings (0,1)");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings (0,1)");

    f.count_crossings_in_layer_for_upper_node_lower_node(1, 1, 2);
    assert_eq!(f.upper_lower_crossings, 2, "upperLowerCrossings (1,2)");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings (1,2)");
}

#[test]
fn test_in_layer_edges_pass_each_other() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_one_layer_no_crossings();

    f.count_crossings_in_layer_for_upper_node_lower_node(0, 0, 1);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings (0,1)");
    assert_eq!(f.lower_upper_crossings, 1, "lowerUpperCrossings (0,1)");

    f.count_crossings_in_layer_for_upper_node_lower_node(0, 1, 2);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings (1,2)");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings (1,2)");

    f.count_crossings_in_layer_for_upper_node_lower_node(0, 2, 3);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings (2,3)");
    assert_eq!(f.lower_upper_crossings, 1, "lowerUpperCrossings (2,3)");
}

#[test]
fn test_fixed_port_order_crossing_to_in_layer_edge() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_fixed_port_order_in_layer_crossing();
    f.count_crossings_in_layer_for_upper_node_lower_node(0, 1, 2);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_fixed_port_order_two_in_layer_edges_cross_each_other() {
    let mut f = Fixture::new();
    f.creator.get_fixed_port_order_two_in_layer_edges_cross_each_other();
    f.count_crossings_in_layer_for_upper_node_lower_node(0, 0, 1);
    assert_eq!(f.upper_lower_crossings, 1, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}

#[test]
fn test_multiple_edges_into_one_port_should_not_cause_crossing() {
    let mut f = Fixture::new();
    let g = f.creator.get_graph();
    let layer = f.creator.make_layer_in(g);
    let nodes = f.creator.add_nodes_to_layer(3, layer);
    let port_side = PortSide::EAST;
    let port_one = f.creator.add_port_on_side(nodes[0], port_side);
    let port_two = f.creator.add_port_on_side(nodes[1], port_side);
    let port_three = f.creator.add_port_on_side(nodes[2], port_side);
    f.creator.add_edge_between_ports(port_one, port_three);
    f.creator.add_edge_between_ports(port_two, port_three);

    f.count_crossings_in_layer_for_upper_node_lower_node(0, 0, 1);
    assert_eq!(f.upper_lower_crossings, 0, "upperLowerCrossings");
    assert_eq!(f.lower_upper_crossings, 0, "lowerUpperCrossings");
}
