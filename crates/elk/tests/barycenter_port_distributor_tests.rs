//! Port of `Tests/ElkSwiftTests/BarycenterPortDistributorTests.swift`.
//!
//! The Swift `setUp` also calls `LayoutMetaDataService.getInstance()`,
//! which has no Rust counterpart (nothing here reads the registry).

mod common;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::abstract_barycenter_port_distributor::AbstractBarycenterPortDistributor;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::graph_info_holder::{GraphInfoHolder, LayerSweepCrossingMinimizerCrossMinType};
use upleft_elk::org::eclipse::elk::alg::layered::p3order::i_sweep_port_distributor::ISweepPortDistributor;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::CrossMinType;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::layer_total_port_distributor::LayerTotalPortDistributor;
use upleft_elk::prelude::*;

// MARK: - Helper

fn distribute_ports_in_complete_graph(creator: &mut TestGraphCreator, _number_of_ports: usize) {
    let g = creator.graph;
    let mut gd = GraphInfoHolder::new(&mut creator.lg, g, LayerSweepCrossingMinimizerCrossMinType::BARYCENTER, &[], CrossMinType::BARYCENTER);
    let nodes = creator.lg.graph_to_node_array(g);
    for i in 0..nodes.len() {
        gd.port_distributor.distribute_ports_while_sweeping(&mut creator.lg, &nodes, i, true);
    }
    for i in (0..nodes.len()).rev() {
        gd.port_distributor.distribute_ports_while_sweeping(&mut creator.lg, &nodes, i, false);
    }
}

fn ports_of(creator: &TestGraphCreator, node: LNodeId) -> Vec<LPortId> {
    creator.lg[node].ports.clone()
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
fn test_distribute_ports_on_side_given_cross_on_western_side_should_remove_crossing() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(2, layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_node = creator.add_node_to_layer(layer);
    creator.east_west_edge_from_to(left_nodes[0], right_node);
    creator.east_west_edge_from_to(left_nodes[1], right_node);

    let expected_port_order_right_node = vec![creator.lg[right_node].ports[1], creator.lg[right_node].ports[0]];

    distribute_ports_in_complete_graph(&mut creator, 4);

    assert_eq!(ports_of(&creator, right_node), expected_port_order_right_node);
}

/// Cross on both sides:
/// ```text
/// *  ___  *
///  \/| |\/
///  /\| |/\
/// *  |_|  *
/// ```
#[test]
fn test_distribute_ports_of_graph_given_cross_on_both_sides_should_remove_crossing() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(2, layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let middle_node = creator.add_node_to_layer(layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_nodes = creator.add_nodes_to_layer(2, layer);
    creator.east_west_edge_from_to(middle_node, right_nodes[1]);
    creator.east_west_edge_from_to(middle_node, right_nodes[0]);
    creator.east_west_edge_from_to(left_nodes[0], middle_node);
    creator.east_west_edge_from_to(left_nodes[1], middle_node);
    creator.set_up_ids();
    let expected_port_order_middle_node = creator.copy_ports_in_index_order(middle_node, &[1, 0, 3, 2]);

    distribute_ports_in_complete_graph(&mut creator, 8);

    assert_eq!(ports_of(&creator, middle_node), expected_port_order_middle_node);
}

/// Cross on eastern side:
/// ```text
/// ___
/// | |\ /-*
/// | | x
/// |_|/ \-*
/// ```
#[test]
fn test_distribute_ports_of_graph_given_cross_on_eastern_side_should_remove_crossing() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_node = creator.add_node_to_layer(layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_nodes = creator.add_nodes_to_layer(2, layer);
    creator.east_west_edge_from_to(left_node, right_nodes[1]);
    creator.east_west_edge_from_to(left_node, right_nodes[0]);

    let expected_port_order_left_node = creator.copy_ports_in_index_order(left_node, &[1, 0]);

    distribute_ports_in_complete_graph(&mut creator, 4);

    assert_eq!(ports_of(&creator, left_node), expected_port_order_left_node);
}

/// In-layer edge port order crossing:
/// ```text
///     *-----
///     *-\  |
///   ____ | |
/// * |  |-+--
///   |__|-|
/// ```
#[test]
fn test_distribute_ports_of_graph_given_in_layer_edge_port_order_crossing_should_remove_it() {
    let mut creator = TestGraphCreator::new();
    let layer = creator.make_layer();
    creator.add_node_to_layer(layer);
    let layer = creator.make_layer();
    let nodes = creator.add_nodes_to_layer(3, layer);
    creator.add_in_layer_edge(nodes[0], nodes[2], PortSide::EAST);
    creator.add_in_layer_edge(nodes[1], nodes[2], PortSide::EAST);

    let expected_port_order_lower_node = creator.copy_ports_in_index_order(nodes[2], &[1, 0]);

    distribute_ports_in_complete_graph(&mut creator, 4);

    assert_eq!(ports_of(&creator, nodes[2]), expected_port_order_lower_node);
}

/// North-south port order crossing:
/// ```text
///     *-->*
///     |
///   *-+-->*
///   | |
///  _|_|_
///  |   |
///  |___|
/// ```
#[test]
fn test_distribute_ports_of_graph_given_north_south_port_order_crossing_should_switch_port_order() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(3, layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_nodes = creator.add_nodes_to_layer(2, layer);

    creator.add_north_south_edge(PortSide::NORTH, left_nodes[2], left_nodes[1], right_nodes[1], false);
    creator.add_north_south_edge(PortSide::NORTH, left_nodes[2], left_nodes[0], right_nodes[0], false);

    let expected_port_order_lower_node = vec![creator.lg[left_nodes[2]].ports[1], creator.lg[left_nodes[2]].ports[0]];

    distribute_ports_in_complete_graph(&mut creator, 6);

    assert_eq!(ports_of(&creator, left_nodes[2]), expected_port_order_lower_node);
}

/// Simple cross with distributePortsWhileSweeping:
/// ```text
/// ___  ____
/// | |\/|  |
/// |_|/\|  |
///      |--|
/// ```
#[test]
fn test_distribute_ports_while_sweeping_given_simple_cross_should_remove_crossing() {
    let mut creator = TestGraphCreator::new();
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let left_node = creator.add_node_to_layer(layer);
    let g = creator.get_graph();
    let layer = creator.make_layer_in(g);
    let right_node = creator.add_node_to_layer(layer);
    creator.east_west_edge_from_to(left_node, right_node);
    creator.east_west_edge_from_to(left_node, right_node);
    let expected_port_right_node = creator.copy_ports_in_index_order(right_node, &[1, 0]);
    creator.set_up_ids();
    let node_array = creator.lg.graph_to_node_array(creator.graph);
    let mut port_dist = LayerTotalPortDistributor::new(node_array.len() as i64);
    initialize_all(&mut creator.lg, &mut port_dist, &node_array);
    port_dist.distribute_ports_while_sweeping(&mut creator.lg, &node_array, 1, true);

    assert_eq!(ports_of(&creator, right_node), expected_port_right_node);
}

// MARK: - Manual IInitializable traversal

fn initialize_all(lg: &mut LGraphArena, pd: &mut AbstractBarycenterPortDistributor, node_order: &[Vec<LNodeId>]) {
    for (layer_index, layer) in node_order.iter().enumerate() {
        pd.init_at_layer_level(layer_index, node_order);
        for (node_index, &node) in layer.iter().enumerate() {
            pd.init_at_node_level(lg, layer_index, node_index, node_order);
            let port_count = lg[node].ports.len();
            for port_index in 0..port_count {
                pd.init_at_port_level(lg, layer_index, node_index, port_index, node_order);
            }
        }
    }
    pd.init_after_traversal();
}
