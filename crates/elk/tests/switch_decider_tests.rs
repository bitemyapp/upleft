//! Port of `Tests/ElkSwiftTests/SwitchDeciderTests.swift`.

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::greedyswitch::crossing_matrix_filler::CrossingMatrixFiller;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::greedyswitch::switch_decider::{CrossingCountSide, SwitchDecider};
use upleft_elk::org::eclipse::elk::alg::layered::p3order::counting::shared_int_array::SharedIntArray;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::graph_info_holder::{GraphInfoHolder, LayerSweepCrossingMinimizerCrossMinType};
use upleft_elk::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::CrossMinType;
use upleft_elk::prelude::*;

const GREEDY_TYPES: [CrossMinType; 2] = [CrossMinType::ONE_SIDED_GREEDY_SWITCH, CrossMinType::TWO_SIDED_GREEDY_SWITCH];

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

fn given_decider(
    creator: &mut TestGraphCreator,
    free_layer_index: i64,
    direction: CrossingCountSide,
    greedy_type: CrossMinType,
) -> (SwitchDecider, Vec<Vec<LNodeId>>) {
    let graph = creator.get_graph();
    let current_node_order = creator.lg.graph_to_node_array(graph);
    let crossing_matrix_filler = CrossingMatrixFiller::new(&creator.lg, greedy_type, &current_node_order, free_layer_index, direction);
    let graph_data = GraphInfoHolder::new(&mut creator.lg, graph, LayerSweepCrossingMinimizerCrossMinType::GREEDY_SWITCH, &[], CrossMinType::BARYCENTER);
    let decider = SwitchDecider::new(
        &creator.lg,
        free_layer_index,
        &current_node_order,
        crossing_matrix_filler,
        SharedIntArray::repeating(0, get_n_ports(creator, &current_node_order)),
        &graph_data.view(None),
        greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH,
    );
    (decider, current_node_order)
}

/// `Array(creator.getGraph().getLayers()[layerIndex].getNodes())`.
fn copy_of_nodes_in_layer(creator: &mut TestGraphCreator, layer_index: usize) -> Vec<LNodeId> {
    let g = creator.get_graph();
    let layer = creator.lg[g].layers[layer_index];
    creator.lg[layer].nodes.clone()
}

fn switch_nodes(current_node_order: &mut [Vec<LNodeId>], free_layer_index: usize, upper: usize, lower: usize) {
    let upper_node = current_node_order[free_layer_index][upper];
    current_node_order[free_layer_index][upper] = current_node_order[free_layer_index][lower];
    current_node_order[free_layer_index][lower] = upper_node;
}

// MARK: - Tests

#[test]
fn test_cross_formed() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_cross_formed_graph();
        creator.set_up_ids();

        let (mut decider1, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        assert!(decider1.does_switch_reduce_crossings(&creator.lg, 0, 1), "crossFormed WEST {greedy_type:?}");

        let (mut decider2, _) = given_decider(&mut creator, 0, CrossingCountSide::EAST, greedy_type);
        assert!(decider2.does_switch_reduce_crossings(&creator.lg, 0, 1), "crossFormed EAST {greedy_type:?}");
    }
}

#[test]
fn test_one_node() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_one_node_graph();

        let (mut decider1, _) = given_decider(&mut creator, 0, CrossingCountSide::WEST, greedy_type);
        assert!(!decider1.does_switch_reduce_crossings(&creator.lg, 0, 0), "oneNode WEST {greedy_type:?}");

        let (mut decider2, _) = given_decider(&mut creator, 0, CrossingCountSide::EAST, greedy_type);
        assert!(!decider2.does_switch_reduce_crossings(&creator.lg, 0, 0), "oneNode EAST {greedy_type:?}");
    }
}

#[test]
fn test_in_layer_switchable() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_in_layer_edges_graph();

        let (mut decider1, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        assert!(decider1.does_switch_reduce_crossings(&creator.lg, 0, 1), "inLayerSwitchable WEST {greedy_type:?}");

        let (mut decider2, _) = given_decider(&mut creator, 1, CrossingCountSide::EAST, greedy_type);
        assert!(decider2.does_switch_reduce_crossings(&creator.lg, 0, 1), "inLayerSwitchable EAST {greedy_type:?}");
    }
}

#[test]
fn test_multiple_edges_between_same_nodes() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_multiple_edges_between_same_nodes_graph();

        let (mut decider1, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        assert!(decider1.does_switch_reduce_crossings(&creator.lg, 0, 1), "multipleEdges WEST {greedy_type:?}");

        let (mut decider2, _) = given_decider(&mut creator, 0, CrossingCountSide::EAST, greedy_type);
        assert!(decider2.does_switch_reduce_crossings(&creator.lg, 0, 1), "multipleEdges EAST {greedy_type:?}");
    }
}

#[test]
fn test_self_loops() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_cross_with_many_self_loops_graph();

        let (mut decider1, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        assert!(decider1.does_switch_reduce_crossings(&creator.lg, 0, 1), "selfLoops WEST {greedy_type:?}");

        let (mut decider2, _) = given_decider(&mut creator, 0, CrossingCountSide::EAST, greedy_type);
        assert!(decider2.does_switch_reduce_crossings(&creator.lg, 0, 1), "selfLoops EAST {greedy_type:?}");
    }
}

#[test]
fn test_north_south_port_crossing() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_three_layer_north_south_crossing_graph();

        let (mut decider, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            assert!(decider.does_switch_reduce_crossings(&creator.lg, 1, 2), "northSouth ONE_SIDED");
        } else {
            assert!(!decider.does_switch_reduce_crossings(&creator.lg, 1, 2), "northSouth TWO_SIDED");
        }
    }
}

#[test]
fn test_more_complex() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_more_complex_three_layer_graph();

        let (mut decider1, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        assert!(!decider1.does_switch_reduce_crossings(&creator.lg, 0, 1), "moreComplex layer1 WEST (0,1) {greedy_type:?}");

        let (mut decider2, _) = given_decider(&mut creator, 2, CrossingCountSide::WEST, greedy_type);
        assert!(decider2.does_switch_reduce_crossings(&creator.lg, 0, 1), "moreComplex layer2 WEST (0,1) {greedy_type:?}");
        assert!(!decider2.does_switch_reduce_crossings(&creator.lg, 1, 2), "moreComplex layer2 WEST (1,2) {greedy_type:?}");

        let (mut decider3, _) = given_decider(&mut creator, 1, CrossingCountSide::EAST, greedy_type);
        assert!(!decider3.does_switch_reduce_crossings(&creator.lg, 0, 1), "moreComplex layer1 EAST (0,1) {greedy_type:?}");

        let (mut decider4, _) = given_decider(&mut creator, 0, CrossingCountSide::EAST, greedy_type);
        assert!(!decider4.does_switch_reduce_crossings(&creator.lg, 0, 1), "moreComplex layer0 EAST (0,1) {greedy_type:?}");
        assert!(decider4.does_switch_reduce_crossings(&creator.lg, 1, 2), "moreComplex layer0 EAST (1,2) {greedy_type:?}");
    }
}

#[test]
fn test_switch_only_true_for_one_sided() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_switch_only_one_sided();

        let (mut decider, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            assert!(decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "switchOnlyOneSided ONE_SIDED");
        } else {
            assert!(!decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "switchOnlyOneSided TWO_SIDED");
        }
    }
}

#[test]
fn test_switch_only_true_for_one_sided_eastern_side() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_switch_only_east_one_sided();

        let (mut decider, _) = given_decider(&mut creator, 1, CrossingCountSide::EAST, greedy_type);
        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            assert!(decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "switchOnlyEast ONE_SIDED");
        } else {
            assert!(!decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "switchOnlyEast TWO_SIDED");
        }
    }
}

#[test]
fn test_constraints_prevent_switch() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_cross_formed_graph_with_constraints_in_second_layer();

        let (mut decider, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "constraintsPrevent {greedy_type:?}");
    }
}

#[test]
fn test_in_layer_unit_constraints_prevent_switch() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_graph_where_layout_unit_prevents_switch();

        let (mut decider, _) = given_decider(&mut creator, 0, CrossingCountSide::WEST, greedy_type);
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 1, 2), "inLayerUnitConstraints {greedy_type:?}");
    }
}

#[test]
fn test_switch_and_recount() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_cross_formed_graph();

        let (mut decider1, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        assert!(decider1.does_switch_reduce_crossings(&creator.lg, 0, 1), "switchAndRecount WEST {greedy_type:?}");

        let (mut decider2, mut node_order) = given_decider(&mut creator, 0, CrossingCountSide::EAST, greedy_type);
        assert!(decider2.does_switch_reduce_crossings(&creator.lg, 0, 1), "switchAndRecount EAST {greedy_type:?}");

        switch_nodes(&mut node_order, 0, 0, 1);
        let layer0_nodes = copy_of_nodes_in_layer(&mut creator, 0);
        decider2.notify_of_switch(&creator.lg, layer0_nodes[0], layer0_nodes[1]);
        assert!(!decider2.does_switch_reduce_crossings(&creator.lg, 0, 1), "switchAndRecount after switch {greedy_type:?}");
    }
}

#[test]
fn test_switch_and_recount_counter_bug() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let g = creator.get_graph();
        let l0 = creator.make_layer_in(g);
        let left_nodes = creator.add_nodes_to_layer(2, l0);
        let g = creator.get_graph();
        let l1 = creator.make_layer_in(g);
        let right_nodes = creator.add_nodes_to_layer(4, l1);
        let left_top_port = creator.add_port_on_side(left_nodes[0], PortSide::EAST);
        let left_lower_port = creator.add_port_on_side(left_nodes[1], PortSide::EAST);
        let right_top_port = creator.add_port_on_side(right_nodes[0], PortSide::WEST);

        creator.add_edge_between_ports(left_lower_port, right_top_port);
        creator.east_west_edge_from_port_to(left_lower_port, right_nodes[2]);
        creator.add_edge_between_ports(left_top_port, right_top_port);
        creator.east_west_edge_from_port_to(left_top_port, right_nodes[1]);
        creator.east_west_edge_from_port_to(left_top_port, right_nodes[3]);

        creator.set_up_ids();
        creator.graph = creator.get_graph();

        let free_layer_index = 1;
        let (mut decider, mut node_order) = given_decider(&mut creator, free_layer_index as i64, CrossingCountSide::WEST, greedy_type);
        assert!(decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "counterBug (0,1) {greedy_type:?}");
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 1, 2), "counterBug (1,2) {greedy_type:?}");
        assert!(decider.does_switch_reduce_crossings(&creator.lg, 2, 3), "counterBug (2,3) {greedy_type:?}");

        decider.notify_of_switch(&creator.lg, node_order[free_layer_index][0], node_order[free_layer_index][1]);
        switch_nodes(&mut node_order, free_layer_index, 0, 1);
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "counterBug after switch1 (0,1) {greedy_type:?}");
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 1, 2), "counterBug after switch1 (1,2) {greedy_type:?}");
        assert!(decider.does_switch_reduce_crossings(&creator.lg, 2, 3), "counterBug after switch1 (2,3) {greedy_type:?}");

        decider.notify_of_switch(&creator.lg, node_order[free_layer_index][2], node_order[free_layer_index][3]);
        switch_nodes(&mut node_order, free_layer_index, 2, 3);
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "counterBug after switch2 (0,1) {greedy_type:?}");
        assert!(decider.does_switch_reduce_crossings(&creator.lg, 1, 2), "counterBug after switch2 (1,2) {greedy_type:?}");
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 2, 3), "counterBug after switch2 (2,3) {greedy_type:?}");

        decider.notify_of_switch(&creator.lg, node_order[free_layer_index][1], node_order[free_layer_index][2]);
        switch_nodes(&mut node_order, free_layer_index, 1, 2);
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "counterBug after switch3 (0,1) {greedy_type:?}");
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 1, 2), "counterBug after switch3 (1,2) {greedy_type:?}");
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 2, 3), "counterBug after switch3 (2,3) {greedy_type:?}");
    }
}

#[test]
fn test_switch_and_recount_reduced_counter_bug() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_switched_problem_graph();

        let (mut decider, _) = given_decider(&mut creator, 1, CrossingCountSide::WEST, greedy_type);
        let nodes_in_layer = copy_of_nodes_in_layer(&mut creator, 1);
        for i in 0..(nodes_in_layer.len() - 1) {
            assert!(!decider.does_switch_reduce_crossings(&creator.lg, i, i + 1), "reducedCounterBug switch {i} with {} {greedy_type:?}", i + 1);
        }
    }
}

#[test]
fn test_should_switch_with_long_edge_dummies() {
    for greedy_type in GREEDY_TYPES {
        let mut creator1 = TestGraphCreator::new();
        creator1.graph = creator1.get_northern_north_south_dummy_edge_crossing_graph();
        let (mut decider1, _) = given_decider(&mut creator1, 1, CrossingCountSide::WEST, greedy_type);
        assert!(decider1.does_switch_reduce_crossings(&creator1.lg, 1, 2), "longEdgeDummies northern (1,2) {greedy_type:?}");
        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            assert!(decider1.does_switch_reduce_crossings(&creator1.lg, 0, 1), "longEdgeDummies northern (0,1) ONE_SIDED");
        }

        let mut creator2 = TestGraphCreator::new();
        creator2.graph = creator2.get_southern_north_south_dummy_edge_crossing_graph();
        let (mut decider2, _) = given_decider(&mut creator2, 1, CrossingCountSide::WEST, greedy_type);
        assert!(decider2.does_switch_reduce_crossings(&creator2.lg, 0, 1), "longEdgeDummies southern (0,1) {greedy_type:?}");
        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            assert!(decider2.does_switch_reduce_crossings(&creator2.lg, 1, 2), "longEdgeDummies southern (1,2) ONE_SIDED");
        }
    }
}

#[test]
fn test_layout_unit_constraint_prevents_switch_with_node_with_northern_ports() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_graph_layout_unit_prevents_switch_with_node_with_node_with_northern_edges();

        let (mut decider, _) = given_decider(&mut creator, 0, CrossingCountSide::EAST, greedy_type);
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 1, 2), "layoutUnitNorthern {greedy_type:?}");
    }
}

#[test]
fn test_layout_unit_constraint_prevents_switch_with_node_with_southern_ports() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_graph_layout_unit_prevents_switch_with_node_with_node_with_southern_edges();

        let (mut decider, _) = given_decider(&mut creator, 0, CrossingCountSide::EAST, greedy_type);
        assert!(!decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "layoutUnitSouthern {greedy_type:?}");
    }
}

#[test]
fn test_layout_unit_constraint_does_not_prevent_switch_with_when_other_node_is_long_edge_dummy() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_graph_layout_unit_does_not_prevent_switch_with_long_edge_dummy();

        let (mut decider, _) = given_decider(&mut creator, 1, CrossingCountSide::EAST, greedy_type);
        assert!(decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "layoutUnitLongEdgeDummy {greedy_type:?}");
    }
}

#[test]
fn test_switching_dummy_nodes_notifies_port_switch() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let l0 = creator.make_layer();
        let left_node = creator.add_node_to_layer(l0);
        let l1 = creator.make_layer();
        let right_nodes = creator.add_nodes_to_layer(2, l1);
        let left_ports = creator.add_ports_on_side(2, left_node, PortSide::EAST);
        let nested_graph = creator.nested_graph(left_node);
        let nested_layer = creator.make_layer_in(nested_graph);
        let dummies = creator.add_external_port_dummies_to_layer(nested_layer, &left_ports);
        creator.east_west_edge_from_port_to(left_ports[0], right_nodes[1]);
        creator.east_west_edge_from_port_to(left_ports[1], right_nodes[0]);

        let graph = creator.get_graph();
        let nested_node_order = creator.lg.graph_to_node_array(nested_graph);
        let crossing_matrix_filler = CrossingMatrixFiller::new(&creator.lg, greedy_type, &nested_node_order, 0, CrossingCountSide::EAST);
        let parent_graph_data = GraphInfoHolder::new(&mut creator.lg, graph, LayerSweepCrossingMinimizerCrossMinType::GREEDY_SWITCH, &[], CrossMinType::BARYCENTER);
        let graphs = [parent_graph_data];
        let graph_data = GraphInfoHolder::new(&mut creator.lg, nested_graph, LayerSweepCrossingMinimizerCrossMinType::GREEDY_SWITCH, &graphs, CrossMinType::BARYCENTER);
        let mut switch_decider = SwitchDecider::new(
            &creator.lg,
            0,
            &nested_node_order,
            crossing_matrix_filler,
            SharedIntArray::repeating(0, get_n_ports(&creator, &nested_node_order)),
            &graph_data.view(graph_data.parent_graph_data.and_then(|p| graphs.get(p))),
            false,
        );

        if greedy_type == CrossMinType::TWO_SIDED_GREEDY_SWITCH {
            assert!(switch_decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "dummyPortSwitch before {greedy_type:?}");
            switch_decider.notify_of_switch(&creator.lg, dummies[0], dummies[1]);
            assert!(!switch_decider.does_switch_reduce_crossings(&creator.lg, 0, 1), "dummyPortSwitch after {greedy_type:?}");
        }
    }
}
