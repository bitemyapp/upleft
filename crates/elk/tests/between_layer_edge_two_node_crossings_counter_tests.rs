//! Port of `Tests/ElkSwiftTests/BetweenLayerEdgeTwoNodeCrossingsCounterTests.swift`.

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::greedyswitch::between_layer_edge_two_node_crossings_counter::BetweenLayerEdgeTwoNodeCrossingsCounter;
use upleft_elk::prelude::*;

// MARK: - Fixture

/// The XCTestCase's instance state; `new()` is `setUp()`.
struct Fixture {
    creator: TestGraphCreator,
    crossing_counter: Option<BetweenLayerEdgeTwoNodeCrossingsCounter>,
    upper_node: Option<LNodeId>,
    lower_node: Option<LNodeId>,
    layer_to_count_in: Option<LayerId>,
    node_order: Vec<Vec<LNodeId>>,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture {
            creator: TestGraphCreator::new(),
            crossing_counter: None,
            upper_node: None,
            lower_node: None,
            layer_to_count_in: None,
            node_order: Vec::new(),
        }
    }

    // MARK: - Helpers

    /// `creator.getGraph().toNodeArray()`.
    fn graph_node_array(&mut self) -> Vec<Vec<LNodeId>> {
        let g = self.creator.get_graph();
        self.creator.lg.graph_to_node_array(g)
    }

    /// `creator.getGraph().getLayers()[index]`.
    fn graph_layer(&mut self, index: usize) -> LayerId {
        let g = self.creator.get_graph();
        self.creator.lg[g].layers[index]
    }

    fn new_crossing_counter(&mut self, free_layer_index: i64) {
        self.crossing_counter = Some(BetweenLayerEdgeTwoNodeCrossingsCounter::new(&self.creator.lg, &self.node_order, free_layer_index));
    }

    fn set_upper_node(&mut self, node_index: usize) {
        self.upper_node = Some(self.creator.lg[self.layer_to_count_in.unwrap()].nodes[node_index]);
    }

    fn set_lower_node(&mut self, node_index: usize) {
        self.lower_node = Some(self.creator.lg[self.layer_to_count_in.unwrap()].nodes[node_index]);
    }

    #[track_caller]
    fn assert_eastern_side_upper_lower_crossings_is(&mut self, expected: i64) {
        let counter = self.crossing_counter.as_mut().unwrap();
        counter.count_eastern_edge_crossings(&self.creator.lg, self.upper_node.unwrap(), self.lower_node.unwrap());
        assert_eq!(counter.get_upper_lower_crossings(), expected, "east, upper lower");
    }

    #[track_caller]
    fn assert_eastern_side_lower_upper_crossings_is(&mut self, expected: i64) {
        let counter = self.crossing_counter.as_mut().unwrap();
        counter.count_eastern_edge_crossings(&self.creator.lg, self.upper_node.unwrap(), self.lower_node.unwrap());
        assert_eq!(counter.get_lower_upper_crossings(), expected, "east, lower upper");
    }

    #[track_caller]
    fn assert_western_side_upper_lower_crossings_is(&mut self, expected: i64) {
        let counter = self.crossing_counter.as_mut().unwrap();
        counter.count_western_edge_crossings(&self.creator.lg, self.upper_node.unwrap(), self.lower_node.unwrap());
        assert_eq!(counter.get_upper_lower_crossings(), expected, "west, upper lower");
    }

    #[track_caller]
    fn assert_western_side_lower_upper_crossings_is(&mut self, expected: i64) {
        let counter = self.crossing_counter.as_mut().unwrap();
        counter.count_western_edge_crossings(&self.creator.lg, self.upper_node.unwrap(), self.lower_node.unwrap());
        assert_eq!(counter.get_lower_upper_crossings(), expected, "west, lower upper");
    }

    #[track_caller]
    fn assert_both_side_upper_lower_crossings_is(&mut self, expected: i64) {
        let counter = self.crossing_counter.as_mut().unwrap();
        counter.count_both_side_crossings(&self.creator.lg, self.upper_node.unwrap(), self.lower_node.unwrap());
        assert_eq!(counter.get_upper_lower_crossings(), expected, "both, upper lower");
    }

    #[track_caller]
    fn assert_both_side_lower_upper_crossings_is(&mut self, expected: i64) {
        let counter = self.crossing_counter.as_mut().unwrap();
        counter.count_both_side_crossings(&self.creator.lg, self.upper_node.unwrap(), self.lower_node.unwrap());
        assert_eq!(counter.get_lower_upper_crossings(), expected, "both, lower upper");
    }
}

// MARK: - Tests

#[test]
fn test_two_node_no_edges() {
    let mut f = Fixture::new();
    f.creator.get_two_nodes_no_connection_graph();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(0));
    f.new_crossing_counter(0);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_both_side_upper_lower_crossings_is(0);
    f.assert_both_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(0);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(0);
}

#[test]
fn test_cross_formed() {
    let mut f = Fixture::new();
    f.creator.get_cross_formed_graph();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(1));
    f.new_crossing_counter(1);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_both_side_upper_lower_crossings_is(1);
    f.assert_both_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(1);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(0);
}

#[test]
fn test_one_node() {
    let mut f = Fixture::new();
    f.creator.get_one_node_graph();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(0));
    f.new_crossing_counter(0);
    f.set_upper_node(0);
    f.set_lower_node(0);

    f.assert_both_side_upper_lower_crossings_is(0);
    f.assert_both_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(0);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(0);
}

#[test]
fn test_cross_formed_multiple_edges_between_same_nodes() {
    let mut f = Fixture::new();
    f.creator.get_multiple_edges_between_same_nodes_graph();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(1));
    f.new_crossing_counter(1);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_both_side_upper_lower_crossings_is(4);
    f.assert_both_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(4);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(0);
}

#[test]
fn test_cross_with_extra_edge_in_between() {
    let mut f = Fixture::new();
    f.creator.get_cross_with_extra_edge_in_between_graph();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(1));
    f.new_crossing_counter(1);
    f.set_upper_node(0);
    f.set_lower_node(2);

    f.assert_both_side_upper_lower_crossings_is(1);
    f.assert_both_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(1);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(0);
}

#[test]
fn test_ignore_in_layer_edges() {
    let mut f = Fixture::new();
    f.creator.get_in_layer_edges_graph();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(1));
    f.new_crossing_counter(1);
    f.set_upper_node(0);
    f.set_lower_node(2);

    f.assert_both_side_upper_lower_crossings_is(0);
    f.assert_both_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(0);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(0);
}

#[test]
fn test_ignore_self_loops() {
    let mut f = Fixture::new();
    f.creator.get_cross_with_many_self_loops_graph();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(1));
    f.new_crossing_counter(1);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_both_side_upper_lower_crossings_is(1);
    f.assert_both_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(1);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(0);
}

#[test]
fn test_more_complex_three_layer_graph() {
    let mut f = Fixture::new();
    f.creator.get_more_complex_three_layer_graph();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(1));
    f.new_crossing_counter(1);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_western_side_upper_lower_crossings_is(1);
    f.assert_western_side_lower_upper_crossings_is(1);
    f.assert_eastern_side_upper_lower_crossings_is(2);
    f.assert_eastern_side_lower_upper_crossings_is(3);
    f.assert_both_side_upper_lower_crossings_is(3);
    f.assert_both_side_lower_upper_crossings_is(4);
}

#[test]
fn test_fixed_port_order() {
    let mut f = Fixture::new();
    f.creator.get_fixed_port_order_graph();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(1));
    f.new_crossing_counter(1);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(1);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_both_side_upper_lower_crossings_is(1);
    f.assert_both_side_lower_upper_crossings_is(0);
}

#[test]
fn test_switch_three_times() {
    let mut f = Fixture::new();
    let g = f.creator.get_graph();
    let l0 = f.creator.make_layer_in(g);
    let left_nodes = f.creator.add_nodes_to_layer(2, l0);
    let g = f.creator.get_graph();
    let l1 = f.creator.make_layer_in(g);
    let right_nodes = f.creator.add_nodes_to_layer(4, l1);
    let left_top_port = f.creator.add_port_on_side(left_nodes[0], PortSide::EAST);
    let left_lower_port = f.creator.add_port_on_side(left_nodes[1], PortSide::EAST);
    let right_top_port = f.creator.add_port_on_side(right_nodes[0], PortSide::WEST);

    f.creator.add_edge_between_ports(left_lower_port, right_top_port);
    f.creator.east_west_edge_from_port_to(left_lower_port, right_nodes[2]);
    f.creator.add_edge_between_ports(left_top_port, right_top_port);
    f.creator.east_west_edge_from_port_to(left_top_port, right_nodes[1]);
    f.creator.east_west_edge_from_port_to(left_top_port, right_nodes[3]);

    f.creator.set_up_ids();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(0));
    f.new_crossing_counter(0);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_eastern_side_upper_lower_crossings_is(3);
    f.assert_eastern_side_lower_upper_crossings_is(2);
    f.assert_western_side_upper_lower_crossings_is(0);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_both_side_upper_lower_crossings_is(3);
    f.assert_both_side_lower_upper_crossings_is(2);
}

#[test]
fn test_into_same_port() {
    let mut f = Fixture::new();
    f.creator.two_edges_into_same_port();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(1));
    f.new_crossing_counter(1);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(2);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_both_side_upper_lower_crossings_is(2);
    f.assert_both_side_lower_upper_crossings_is(0);
}

#[test]
fn test_into_same_port_causes_crossings_on_switch() {
    let mut f = Fixture::new();
    f.creator.two_edges_into_same_port_crosses_when_switched();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(0));
    f.new_crossing_counter(0);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(1);
    f.assert_western_side_upper_lower_crossings_is(0);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_both_side_upper_lower_crossings_is(0);
    f.assert_both_side_lower_upper_crossings_is(1);
}

#[test]
fn test_into_same_port_reduces_crossings_on_switch() {
    let mut f = Fixture::new();
    f.creator.two_edges_into_same_port_resolves_crossing_when_switched();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(0));
    f.new_crossing_counter(0);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_eastern_side_upper_lower_crossings_is(1);
    f.assert_eastern_side_lower_upper_crossings_is(0);
    f.assert_western_side_upper_lower_crossings_is(0);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_both_side_upper_lower_crossings_is(1);
    f.assert_both_side_lower_upper_crossings_is(0);
}

#[test]
fn test_into_same_port_from_east_switch_with_fixed_port_order() {
    let mut f = Fixture::new();
    f.creator.two_edges_into_same_port_from_east_with_fixed_port_order();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(0));
    f.new_crossing_counter(0);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_lower_upper_crossings_is(1);
    f.assert_western_side_upper_lower_crossings_is(0);
    f.assert_western_side_lower_upper_crossings_is(0);
    f.assert_both_side_upper_lower_crossings_is(0);
    f.assert_both_side_lower_upper_crossings_is(1);
}

#[test]
fn test_multiple_edges_into_same_port_causes_no_crossings() {
    let mut f = Fixture::new();
    let g = f.creator.graph;
    let left_layer = f.creator.make_layer_in(g);
    let right_layer = f.creator.make_layer_in(g);

    let top_left = f.creator.add_node_to_layer(left_layer);
    let bottom_left = f.creator.add_node_to_layer(left_layer);
    let bottom_right = f.creator.add_node_to_layer(right_layer);

    let bottom_right_port = f.creator.add_port_on_side(bottom_right, PortSide::WEST);

    f.creator.east_west_edge_from_to_port(top_left, bottom_right_port);
    f.creator.east_west_edge_from_to_port(top_left, bottom_right_port);
    f.creator.east_west_edge_from_to_port(bottom_left, bottom_right_port);
    f.creator.set_up_ids();
    f.node_order = f.graph_node_array();
    f.layer_to_count_in = Some(f.graph_layer(0));
    f.new_crossing_counter(0);
    f.set_upper_node(0);
    f.set_lower_node(1);

    f.assert_eastern_side_upper_lower_crossings_is(0);
    f.assert_eastern_side_upper_lower_crossings_is(0);
}
