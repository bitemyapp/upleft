//! Port of `Tests/ElkSwiftTests/NorthSouthEdgeNeighbouringNodeCrossingsCounterTests.swift`.

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::greedyswitch::north_south_edge_neighbouring_node_crossings_counter::NorthSouthEdgeNeighbouringNodeCrossingsCounter;
use upleft_elk::org::eclipse::elk::core::options::edge_routing::EdgeRouting;
use upleft_elk::prelude::*;

/// The Swift test case's instance state (`creator`, `counter`, `layer`),
/// set up as `setUp()` does.
struct Fixture {
    creator: TestGraphCreator,
    counter: Option<NorthSouthEdgeNeighbouringNodeCrossingsCounter>,
    layer: Option<Vec<LNodeId>>,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture { creator: TestGraphCreator::new(), counter: None, layer: None }
    }

    /// `layer = nil; creator = NorthSouthEdgeTestGraphCreator()`.
    fn reset_creator(&mut self) {
        self.layer = None;
        self.creator = TestGraphCreator::new();
    }

    // MARK: - Helpers

    fn count_crossings_in_layer_between_nodes(&mut self, layer_index: usize, upper_node_index: usize, lower_node_index: usize) {
        if self.layer.is_none() {
            let g = self.creator.get_graph();
            self.layer = Some(self.creator.lg.graph_to_node_array(g)[layer_index].clone());
        }
        let layer = self.layer.as_ref().unwrap();
        let mut counter = NorthSouthEdgeNeighbouringNodeCrossingsCounter::new(&self.creator.lg, layer);
        counter.count_crossings(&self.creator.lg, layer[upper_node_index], layer[lower_node_index]);
        self.counter = Some(counter);
    }

    fn switch_nodes(&mut self, upper: usize, lower: usize) {
        self.layer.as_mut().unwrap().swap(upper, lower);
    }

    fn switch_and_recount(&mut self, upper_node_index: usize, lower_node_index: usize) {
        self.switch_nodes(upper_node_index, lower_node_index);
        let layer = self.layer.as_ref().unwrap();
        let (upper, lower) = (layer[upper_node_index], layer[lower_node_index]);
        self.counter.as_mut().unwrap().count_crossings(&self.creator.lg, upper, lower);
    }

    fn upper_lower_crossings(&self) -> i64 {
        self.counter.as_ref().unwrap().get_upper_lower_crossings()
    }

    fn lower_upper_crossings(&self) -> i64 {
        self.counter.as_ref().unwrap().get_lower_upper_crossings()
    }
}

// MARK: - Tests

#[test]
fn test_no_north_south_node() {
    let mut f = Fixture::new();
    f.creator.get_cross_formed_graph();
    f.count_crossings_in_layer_between_nodes(0, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_southern_north_south_node_crossing() {
    let mut f = Fixture::new();
    f.creator.get_north_south_downward_crossing_graph();
    f.count_crossings_in_layer_between_nodes(0, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_northern_north_south_node_crossings() {
    let mut f = Fixture::new();
    f.creator.get_north_south_upward_crossing_graph();
    f.count_crossings_in_layer_between_nodes(0, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_one_node_is_long_edge_dummy() {
    let mut f = Fixture::new();
    f.creator.get_southern_north_south_dummy_edge_crossing_graph();
    f.count_crossings_in_layer_between_nodes(1, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);

    f.switch_nodes(1, 2);
    f.count_crossings_in_layer_between_nodes(1, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 1);
}

#[test]
fn test_one_node_is_long_edge_dummy_northern() {
    let mut f = Fixture::new();
    f.creator.get_northern_north_south_dummy_edge_crossing_graph();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);

    f.switch_nodes(0, 1);
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 1);
}

#[test]
fn test_with_normal_node() {
    let mut f = Fixture::new();
    f.creator.get_north_south_downward_crossing_graph();
    f.count_crossings_in_layer_between_nodes(0, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_north_south_edges_come_from_both_sides_dont_cross() {
    let mut f = Fixture::new();
    f.creator.get_southern_north_south_graph_edges_from_east_and_west_no_crossings();
    f.count_crossings_in_layer_between_nodes(1, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 0);

    // Reset for the northern case
    f.reset_creator();
    f.creator.get_northern_north_south_graph_edges_from_east_and_west_no_crossings();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_southern_north_south_edges_both_to_east() {
    let mut f = Fixture::new();
    f.creator.get_southern_north_south_edges_both_to_east();
    f.count_crossings_in_layer_between_nodes(0, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 1);
}

#[test]
fn test_crossings_with_north_south_ports_belonging_to_different_nodes_should_not_be_counted() {
    let mut f = Fixture::new();
    f.creator.get_graph_where_layout_unit_prevents_switch();
    f.count_crossings_in_layer_between_nodes(0, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_north_south_edges_come_from_both_sides_do_cross() {
    let mut f = Fixture::new();
    f.creator.get_north_south_edges_from_east_and_west_and_cross();
    f.count_crossings_in_layer_between_nodes(1, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 1);
}

#[test]
fn test_switch_nodes_and_recount() {
    let mut f = Fixture::new();
    f.creator.get_north_south_upward_crossing_graph();
    f.count_crossings_in_layer_between_nodes(0, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);
    f.switch_and_recount(0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 1);
}

#[test]
fn test_south_port_on_normal_node_below_long_edge_dummy() {
    let mut f = Fixture::new();
    f.creator.get_south_port_on_normal_node_below_long_edge_dummy();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 1);
    f.switch_and_recount(0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_north_port_on_normal_node_above_long_edge_dummy() {
    let mut f = Fixture::new();
    f.creator.get_north_port_ond_normal_node_above_long_edge_dummy();
    f.count_crossings_in_layer_between_nodes(1, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 1);
    f.switch_and_recount(1, 2);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_southern_two_western_edges() {
    let mut f = Fixture::new();
    f.creator.get_north_south_southern_two_western_edges();
    f.count_crossings_in_layer_between_nodes(1, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);
    f.switch_and_recount(1, 2);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 1);
}

#[test]
fn test_southern_western_port_to_east_and_eastern_port_to_west() {
    let mut f = Fixture::new();
    f.creator.get_north_south_southern_western_port_to_east_and_eastern_port_to_west();
    f.count_crossings_in_layer_between_nodes(1, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 1);
    f.switch_and_recount(1, 2);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 1);
}

#[test]
fn test_northern_both_edges_western() {
    let mut f = Fixture::new();
    f.creator.get_north_south_northern_western_edges();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 1);
    f.switch_and_recount(0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_northern_eastern_port_to_west_western_port_to_east() {
    let mut f = Fixture::new();
    f.creator.get_north_south_northern_eastern_port_to_west_western_port_to_east();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 1);
    f.switch_and_recount(0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 1);
}

#[test]
fn test_normal_nodes_north_south_edges_have_crossings_to_long_edge_dummy() {
    let mut f = Fixture::new();
    f.creator.get_northern_north_south_dummy_edge_crossing_graph();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);

    // Reset layer to force re-read
    f.layer = None;
    f.count_crossings_in_layer_between_nodes(1, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);

    // Southern case
    f.reset_creator();
    f.creator.get_southern_north_south_dummy_edge_crossing_graph();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);

    f.layer = None;
    f.count_crossings_in_layer_between_nodes(1, 1, 2);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_normal_nodes_north_south_edges_have_crossings_to_long_edge_dummy_on_both_sides() {
    let mut f = Fixture::new();
    f.creator.get_multiple_north_south_and_long_edge_dummies_on_both_sides();
    f.count_crossings_in_layer_between_nodes(1, 2, 3);
    assert_eq!(f.upper_lower_crossings(), 2);
    assert_eq!(f.lower_upper_crossings(), 2);
}

#[test]
fn test_ignores_unconnected_ports_for_normal_node_and_long_edge_dummies() {
    let mut f = Fixture::new();
    f.creator.get_long_edge_dummy_and_normal_node_with_unused_ports_on_southern_side();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);

    f.reset_creator();
    f.creator.get_long_edge_dummy_and_normal_node_with_unused_ports_on_northern_side();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
}

#[test]
fn test_one_edge_west_one_edge_east_dont_cross() {
    let mut f = Fixture::new();
    f.creator.get_northern_north_south_graph_edges_from_east_and_west_no_crossings();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 0);
}

#[test]
fn test_one_edge_east_one_edge_west_dont_cross() {
    let mut f = Fixture::new();
    f.creator.get_northern_north_south_graph_edges_from_east_and_west_no_crossings_upper_edge_east();
    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 0);
    assert_eq!(f.lower_upper_crossings(), 0);
}

/// Polyline routing with more than one edge into NS node:
#[test]
fn test_given_polyline_routing_when_more_than_one_edge_into_ns_node_counts_these_too() {
    let mut f = Fixture::new();
    let creator = &mut f.creator;
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
    let g = creator.get_graph();
    creator.lg[g].props.set(&LayeredOptions::EDGE_ROUTING, EdgeRouting::POLYLINE);

    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 2);
    assert_eq!(f.lower_upper_crossings(), 1);
}

/// Multiple edges in one NS node:
#[test]
fn test_given_multiple_edges_in_one_ns_node_counts_crossings() {
    let mut f = Fixture::new();
    let creator = &mut f.creator;
    let l0 = creator.make_layer();
    let left_node = creator.add_node_to_layer(l0);
    let l1 = creator.make_layer();
    let middle_layer = creator.add_nodes_to_layer(3, l1);
    let l2 = creator.make_layer();
    let right_layer = creator.add_nodes_to_layer(2, l2);

    creator.set_fixed_order_constraint(middle_layer[2]);

    creator.add_north_south_edge(PortSide::NORTH, middle_layer[2], middle_layer[1], left_node, true);

    let normal_node_port = creator.add_port_on_side(right_layer[1], PortSide::WEST);
    let dummy_node_port = creator.add_port_on_side(middle_layer[1], PortSide::EAST);
    creator.add_edge_between_ports(dummy_node_port, normal_node_port);
    let origin_port = creator.lg[middle_layer[2]].ports[0];
    creator.lg[dummy_node_port].props.set(&InternalProperties::ORIGIN, origin_port);

    creator.add_north_south_edge(PortSide::NORTH, middle_layer[2], middle_layer[0], right_layer[0], false);

    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 0);
}

/// Edges in both directions:
#[test]
fn test_edges_in_both_directions() {
    let mut f = Fixture::new();
    let creator = &mut f.creator;
    let l0 = creator.make_layer();
    let left_layer = creator.add_nodes_to_layer(2, l0);
    let l1 = creator.make_layer();
    let middle_layer = creator.add_nodes_to_layer(3, l1);
    let l2 = creator.make_layer();
    let right_layer = creator.add_nodes_to_layer(2, l2);

    creator.set_fixed_order_constraint(middle_layer[2]);

    creator.add_north_south_edge(PortSide::NORTH, middle_layer[2], middle_layer[1], left_layer[1], true);

    let normal_node_port = creator.add_port_on_side(right_layer[1], PortSide::WEST);
    let dummy_node_port = creator.add_port_on_side(middle_layer[1], PortSide::EAST);
    creator.add_edge_between_ports(dummy_node_port, normal_node_port);
    let origin_port = creator.lg[middle_layer[2]].ports[0];
    creator.lg[dummy_node_port].props.set(&InternalProperties::ORIGIN, origin_port);

    creator.add_north_south_edge(PortSide::NORTH, middle_layer[2], middle_layer[0], left_layer[0], true);

    let normal_node_port2 = creator.add_port_on_side(right_layer[0], PortSide::WEST);
    let dummy_node_port2 = creator.add_port_on_side(middle_layer[0], PortSide::EAST);
    creator.add_edge_between_ports(dummy_node_port2, normal_node_port2);
    let origin_port2 = creator.lg[middle_layer[2]].ports[1];
    creator.lg[dummy_node_port2].props.set(&InternalProperties::ORIGIN, origin_port2);

    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 1);
    assert_eq!(f.lower_upper_crossings(), 1);
}

/// Multiple edges in both directions NS node:
#[test]
fn test_multiple_edges_in_both_directions_ns_node() {
    let mut f = Fixture::new();
    let creator = &mut f.creator;
    let l0 = creator.make_layer();
    let left_layer = creator.add_nodes_to_layer(2, l0);
    let l1 = creator.make_layer();
    let middle_layer = creator.add_nodes_to_layer(3, l1);
    let l2 = creator.make_layer();
    let right_layer = creator.add_nodes_to_layer(2, l2);

    creator.set_fixed_order_constraint(middle_layer[2]);

    creator.add_north_south_edge(PortSide::NORTH, middle_layer[2], middle_layer[1], left_layer[1], true);

    let normal_node_port = creator.add_port_on_side(right_layer[1], PortSide::WEST);
    let dummy_node_port = creator.add_port_on_side(middle_layer[1], PortSide::EAST);
    creator.add_edge_between_ports(dummy_node_port, normal_node_port);
    creator.add_edge_between_ports(dummy_node_port, normal_node_port);
    let origin_port = creator.lg[middle_layer[2]].ports[0];
    creator.lg[dummy_node_port].props.set(&InternalProperties::ORIGIN, origin_port);

    creator.add_north_south_edge(PortSide::NORTH, middle_layer[2], middle_layer[0], right_layer[0], false);

    let normal_node_port2 = creator.add_port_on_side(right_layer[0], PortSide::EAST);
    let dummy_node_port2 = creator.add_port_on_side(middle_layer[0], PortSide::WEST);
    creator.add_edge_between_ports(dummy_node_port2, normal_node_port2);
    creator.add_edge_between_ports(dummy_node_port2, normal_node_port2);
    let origin_port2 = creator.lg[middle_layer[2]].ports[1];
    creator.lg[dummy_node_port2].props.set(&InternalProperties::ORIGIN, origin_port2);

    f.count_crossings_in_layer_between_nodes(1, 0, 1);
    assert_eq!(f.upper_lower_crossings(), 2);
    assert_eq!(f.lower_upper_crossings(), 2);
}
