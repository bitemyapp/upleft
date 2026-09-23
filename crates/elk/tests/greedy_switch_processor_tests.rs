//! Port of `Tests/ElkSwiftTests/GreedySwitchProcessorTests.swift`.

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::{CrossMinType, LayerSweepCrossingMinimizer};
use upleft_elk::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use upleft_elk::org::eclipse::elk::core::util::basic_progress_monitor::BasicProgressMonitor;
use upleft_elk::prelude::*;

const GREEDY_TYPES: [CrossMinType; 2] = [CrossMinType::ONE_SIDED_GREEDY_SWITCH, CrossMinType::TWO_SIDED_GREEDY_SWITCH];

// MARK: - Helpers

/// `Array(creator.graph.getLayers()[layerIndex].getNodes())`.
fn copy_of_nodes_in_layer(creator: &TestGraphCreator, layer_index: usize) -> Vec<LNodeId> {
    let layer = creator.lg[creator.graph].layers[layer_index];
    creator.lg[layer].nodes.clone()
}

fn copy_of_switch_order_of_nodes_in_layer(creator: &TestGraphCreator, node_one: usize, node_two: usize, layer_index: usize) -> Vec<LNodeId> {
    let mut layer = copy_of_nodes_in_layer(creator, layer_index);
    let first = layer[node_one];
    layer[node_one] = layer[node_two];
    layer[node_two] = first;
    layer
}

fn get_copy_with_switched_order(node_one: usize, node_two: usize, layer: &[LNodeId]) -> Vec<LNodeId> {
    let mut switched = layer.to_vec();
    let first = switched[node_one];
    switched[node_one] = switched[node_two];
    switched[node_two] = first;
    switched
}

fn start_greedy_switcher(creator: &mut TestGraphCreator, greedy_type: CrossMinType) {
    let mut minimizer = LayerSweepCrossingMinimizer::new(greedy_type);
    let mut monitor = BasicProgressMonitor::new();
    let g = creator.get_graph();
    minimizer.process(&mut creator.lg, g, &mut monitor);
}

fn nodes_are_equal(a: &[LNodeId], b: &[LNodeId]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).all(|(x, y)| x == y)
}

// MARK: - Tests

#[test]
fn test_should_switch_cross() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_cross_formed_graph();

        let expected_order_layer_one;
        let expected_order_layer_two;
        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            expected_order_layer_one = copy_of_nodes_in_layer(&creator, 0);
            expected_order_layer_two = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 1);
        } else {
            expected_order_layer_one = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 0);
            expected_order_layer_two = copy_of_nodes_in_layer(&creator, 1);
        }

        start_greedy_switcher(&mut creator, greedy_type);

        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 0), &expected_order_layer_one), "Layer one {greedy_type:?}");
        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &expected_order_layer_two), "Layer two {greedy_type:?}");
    }
}

#[test]
fn test_constraints_prevent_switch_in_second_layer() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_cross_formed_graph_with_constraints_in_second_layer();

        let expected_order_layer_one = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 0);
        let expected_order_layer_two = copy_of_nodes_in_layer(&creator, 1);

        start_greedy_switcher(&mut creator, greedy_type);

        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 0), &expected_order_layer_one), "Layer one {greedy_type:?}");
        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &expected_order_layer_two), "Layer two {greedy_type:?}");
    }
}

#[test]
fn test_constraints_prevent_any_switch() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_cross_formed_graph_constraints_prevent_any_switch();

        let expected_order_layer_one = copy_of_nodes_in_layer(&creator, 0);
        let expected_order_layer_two = copy_of_nodes_in_layer(&creator, 1);

        start_greedy_switcher(&mut creator, greedy_type);

        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 0), &expected_order_layer_one), "Layer one {greedy_type:?}");
        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &expected_order_layer_two), "Layer two {greedy_type:?}");
    }
}

#[test]
fn test_layout_unit_constraint_prevents_switch() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_nodes_in_different_layout_units_prevent_switch();

        let expected_order_layer_two = copy_of_nodes_in_layer(&creator, 1);

        start_greedy_switcher(&mut creator, greedy_type);

        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &expected_order_layer_two), "Layer one {greedy_type:?}");
    }
}

#[test]
fn test_one_node() {
    for _greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_one_node_graph();
        // Should cause no errors
        let _ = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 0, 0);
    }
}

#[test]
fn test_in_layer_switchable() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_in_layer_edges_graph();

        let expected_order = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 1);

        start_greedy_switcher(&mut creator, greedy_type);

        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &expected_order), "inLayerSwitchable {greedy_type:?}");
    }
}

#[test]
fn test_multiple_edges_between_same_nodes() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_multiple_edges_between_same_nodes_graph();

        let expected_order_layer_one;
        let expected_order_layer_two;
        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            expected_order_layer_one = copy_of_nodes_in_layer(&creator, 0);
            expected_order_layer_two = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 1);
        } else {
            expected_order_layer_one = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 0);
            expected_order_layer_two = copy_of_nodes_in_layer(&creator, 1);
        }

        start_greedy_switcher(&mut creator, greedy_type);

        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 0), &expected_order_layer_one), "Layer one {greedy_type:?}");
        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &expected_order_layer_two), "Layer two {greedy_type:?}");
    }
}

#[test]
fn test_self_loops() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let g = creator.get_graph();
        let left_layer = creator.make_layer_in(g);
        let g = creator.get_graph();
        let right_layer = creator.make_layer_in(g);

        let top_left = creator.add_node_to_layer(left_layer);
        let bottom_left = creator.add_node_to_layer(left_layer);
        let top_right = creator.add_node_to_layer(right_layer);
        let bottom_right = creator.add_node_to_layer(right_layer);

        let top_left_port = creator.add_port_on_side(top_left, PortSide::EAST);
        let bottom_left_port = creator.add_port_on_side(bottom_left, PortSide::EAST);
        creator.set_up_ids();
        let self_loop_cross_graph = creator.get_graph();
        for layer in creator.lg[self_loop_cross_graph].layers.clone() {
            for node in creator.lg[layer].nodes.clone() {
                creator.self_loop_on(node, PortSide::EAST);
                creator.self_loop_on(node, PortSide::EAST);
                creator.self_loop_on(node, PortSide::EAST);
                creator.self_loop_on(node, PortSide::WEST);
                creator.self_loop_on(node, PortSide::WEST);
                creator.self_loop_on(node, PortSide::WEST);
            }
        }
        let top_right_port = creator.add_port_on_side(top_right, PortSide::WEST);
        let bottom_right_port = creator.add_port_on_side(bottom_right, PortSide::WEST);

        creator.add_edge_between_ports(top_left_port, bottom_right_port);
        creator.add_edge_between_ports(bottom_left_port, top_right_port);

        let expected_order_layer_one;
        let expected_order_layer_two;
        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            expected_order_layer_one = copy_of_nodes_in_layer(&creator, 0);
            expected_order_layer_two = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 1);
        } else {
            expected_order_layer_one = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 0);
            expected_order_layer_two = copy_of_nodes_in_layer(&creator, 1);
        }

        start_greedy_switcher(&mut creator, greedy_type);

        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 0), &expected_order_layer_one), "Layer one {greedy_type:?}");
        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &expected_order_layer_two), "Layer two {greedy_type:?}");
    }
}

#[test]
fn test_north_south_port_crossing() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        creator.graph = creator.get_north_south_downward_crossing_graph();

        let layer_index = 0;
        let expected_order_two_sided = copy_of_nodes_in_layer(&creator, layer_index);
        let expected_order_one_sided = copy_of_switch_order_of_nodes_in_layer(&creator, 1, 2, layer_index);

        start_greedy_switcher(&mut creator, greedy_type);

        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, layer_index), &expected_order_one_sided), "northSouth ONE_SIDED");
        } else {
            assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, layer_index), &expected_order_two_sided), "northSouth TWO_SIDED");
        }
    }
}

#[test]
fn test_more_complex() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_more_complex_three_layer_graph();

        let expected_order_layer_two = copy_of_nodes_in_layer(&creator, 1);
        let expected_order_layer_three = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 2);

        start_greedy_switcher(&mut creator, greedy_type);

        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &expected_order_layer_two), "Layer two {greedy_type:?}");
        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 2), &expected_order_layer_three), "Layer three {greedy_type:?}");
    }
}

#[test]
fn test_switch_only_for_one_sided() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_switch_only_one_sided();

        let layer_index = 1;
        let expected_order_one_sided = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, layer_index);
        let expected_order_two_sided = copy_of_nodes_in_layer(&creator, layer_index);

        start_greedy_switcher(&mut creator, greedy_type);

        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            assert!(
                nodes_are_equal(&copy_of_nodes_in_layer(&creator, layer_index), &expected_order_one_sided),
                "switchOnlyForOneSided ONE_SIDED"
            );
        } else {
            assert!(
                nodes_are_equal(&copy_of_nodes_in_layer(&creator, layer_index), &expected_order_two_sided),
                "switchOnlyForOneSided TWO_SIDED"
            );
        }
    }
}

#[test]
fn test_does_not_worsen_cross_amount() {
    for greedy_type in GREEDY_TYPES {
        let mut creator = TestGraphCreator::new();
        let _ = creator.get_graph_which_could_be_worsened_by_switch();

        let expected_order_first_layer = copy_of_nodes_in_layer(&creator, 0);
        let expected_order_second_layer = copy_of_nodes_in_layer(&creator, 1);

        start_greedy_switcher(&mut creator, greedy_type);

        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 0), &expected_order_first_layer), "Layer one {greedy_type:?}");
        assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &expected_order_second_layer), "Layer two {greedy_type:?}");
    }
}

#[test]
fn test_switch_more_than_once() {
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

        let one_sided_first_layer = copy_of_nodes_in_layer(&creator, 0);
        let one_sided_first_switch = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 1);
        let one_sided_second_switch = get_copy_with_switched_order(2, 3, &one_sided_first_switch);
        let one_sided_third_switch = get_copy_with_switched_order(1, 2, &one_sided_second_switch);

        let two_sided_first_layer = copy_of_switch_order_of_nodes_in_layer(&creator, 0, 1, 0);
        let two_sided_first_switch = copy_of_switch_order_of_nodes_in_layer(&creator, 1, 2, 1);
        let two_sided_second_switch = get_copy_with_switched_order(0, 1, &two_sided_first_switch);

        start_greedy_switcher(&mut creator, greedy_type);

        if greedy_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH {
            assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 0), &one_sided_first_layer), "Layer one ONE_SIDED");
            assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &one_sided_third_switch), "Layer two ONE_SIDED");
        } else {
            assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 0), &two_sided_first_layer), "Layer one TWO_SIDED");
            assert!(nodes_are_equal(&copy_of_nodes_in_layer(&creator, 1), &two_sided_second_switch), "Layer two TWO_SIDED");
        }
    }
}
