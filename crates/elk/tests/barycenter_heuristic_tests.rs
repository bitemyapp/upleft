//! Port of `Tests/ElkSwiftTests/BarycenterHeuristicTests.swift`.
//!
//! The Swift `setUp` also calls `LayoutMetaDataService.initElkReflect()`,
//! which has no Rust counterpart (nothing here reads the registry).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::TestGraphCreator;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::abstract_barycenter_port_distributor::AbstractBarycenterPortDistributor;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::barycenter_heuristic::BarycenterHeuristic;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::forster_constraint_resolver::ForsterConstraintResolver;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::node_relative_port_distributor::NodeRelativePortDistributor;
use upleft_elk::prelude::*;

// MARK: - Helper

/// Manually replicate Java's `IInitializable.init(List<IInitializable>, LNode[][])`.
/// Traverses layers -> nodes -> ports -> edges, calling initAt* on each component.
fn initialize_all(
    lg: &mut LGraphArena,
    mut pd: Option<&mut AbstractBarycenterPortDistributor>,
    mut cr: Option<&mut ForsterConstraintResolver>,
    mut bh: Option<&mut BarycenterHeuristic>,
    node_order: &[Vec<LNodeId>],
) {
    for (layer_index, layer) in node_order.iter().enumerate() {
        if let Some(pd) = pd.as_deref_mut() {
            pd.init_at_layer_level(layer_index, node_order);
        }
        if let Some(cr) = cr.as_deref_mut() {
            cr.init_at_layer_level(layer_index, node_order);
        }
        if let Some(bh) = bh.as_deref_mut() {
            bh.init_at_layer_level(lg, layer_index, node_order);
        }
        for (node_index, &node) in layer.iter().enumerate() {
            if let Some(pd) = pd.as_deref_mut() {
                pd.init_at_node_level(lg, layer_index, node_index, node_order);
            }
            if let Some(cr) = cr.as_deref_mut() {
                cr.init_at_node_level(lg, layer_index, node_index, node_order);
            }
            let port_count = lg[node].ports.len();
            for port_index in 0..port_count {
                if let Some(pd) = pd.as_deref_mut() {
                    pd.init_at_port_level(lg, layer_index, node_index, port_index, node_order);
                }
            }
        }
    }
    if let Some(pd) = pd.as_deref_mut() {
        pd.init_after_traversal();
    }
    if let Some(bh) = bh.as_deref_mut() {
        bh.init_after_traversal();
    }
}

/// `NodeRelativePortDistributor(nodes.count)`, shared like the Swift object.
fn node_relative_port_distributor(nodes: &[Vec<LNodeId>]) -> Rc<RefCell<AbstractBarycenterPortDistributor>> {
    Rc::new(RefCell::new(NodeRelativePortDistributor::new(nodes.len() as i64)))
}

// MARK: - Tests

/// Simple cross:
/// ```text
/// *  *
///  \/
///  /\
/// *  *
/// ```
#[test]
fn test_minimize_crossings_removes_crossing_in_simple_cross() {
    let mut creator = TestGraphCreator::new();
    let layer = creator.make_layer();
    let left_nodes = creator.add_nodes_to_layer(2, layer);
    let layer = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(2, layer);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
    creator.east_west_edge_from_to(left_nodes[1], right_nodes[0]);
    creator.set_up_ids();
    let mut nodes = creator.lg.graph_to_node_array(creator.graph);

    let port_dist = node_relative_port_distributor(&nodes);
    let mut constraint_resolver = ForsterConstraintResolver::new(&creator.lg, &nodes);
    initialize_all(&mut creator.lg, Some(&mut *port_dist.borrow_mut()), Some(&mut constraint_resolver), None, &nodes);

    port_dist.borrow_mut().calculate_port_ranks(&creator.lg, &nodes[0], PortType::OUTPUT);
    let mut cross_min = BarycenterHeuristic::new(constraint_resolver, Some(creator.random_ref()), port_dist.clone(), &nodes);
    initialize_all(&mut creator.lg, None, None, Some(&mut cross_min), &nodes);

    let expected_order = TestGraphCreator::switch_order_in_array(0, 1, &nodes[1]);

    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[1], false, false, true);

    assert_eq!(nodes[1], expected_order, "Expected crossing to be removed by switching nodes in layer 1");
}

/// Mock random first layer:
/// ```text
/// *  *
///  \/
///  /\
/// *  *
/// ```
#[test]
fn test_mock_randomize_first_layer() {
    let mut creator = TestGraphCreator::new();
    let layer = creator.make_layer();
    let left_nodes = creator.add_nodes_to_layer(2, layer);
    let layer = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(2, layer);
    creator.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
    creator.east_west_edge_from_to(left_nodes[1], right_nodes[0]);
    creator.set_up_ids();

    let mut nodes = creator.lg.graph_to_node_array(creator.graph);
    let port_dist = node_relative_port_distributor(&nodes);
    let mut constraint_resolver = ForsterConstraintResolver::new(&creator.lg, &nodes);
    initialize_all(&mut creator.lg, Some(&mut *port_dist.borrow_mut()), Some(&mut constraint_resolver), None, &nodes);
    port_dist.borrow_mut().calculate_port_ranks(&creator.lg, &nodes[0], PortType::OUTPUT);
    let mut cross_min = BarycenterHeuristic::new(constraint_resolver, Some(creator.random_ref()), port_dist.clone(), &nodes);
    initialize_all(&mut creator.lg, None, None, Some(&mut cross_min), &nodes);

    let expected_order = nodes[0].clone();
    let expected_switched_order = TestGraphCreator::switch_order_in_array(0, 1, &nodes[0]);

    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[0], false, true, true);
    assert_eq!(nodes[0], expected_order);

    creator.random.borrow_mut().set_change_by(-0.01);
    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[0], false, true, true);
    assert_eq!(nodes[0], expected_switched_order);
}

/// Filling in unknown barycenters:
/// ```text
///   *  *
///    \/
///    /\
/// *-*  *
/// ```
#[test]
fn test_filling_in_unknown_barycenters() {
    let mut creator = TestGraphCreator::new();
    let layer = creator.make_layer();
    let left_node = creator.add_node_to_layer(layer);
    let layer = creator.make_layer();
    let middle_nodes = creator.add_nodes_to_layer(2, layer);
    let layer = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(2, layer);
    creator.east_west_edge_from_to(middle_nodes[0], right_nodes[1]);
    creator.east_west_edge_from_to(middle_nodes[1], right_nodes[0]);
    creator.east_west_edge_from_to(left_node, middle_nodes[1]);
    creator.set_up_ids();

    let mut nodes = creator.lg.graph_to_node_array(creator.graph);
    let expected_switched_order = TestGraphCreator::switch_order_in_array(0, 1, &nodes[2]);
    let expected_order_second_layer = nodes[1].clone();

    let port_dist = node_relative_port_distributor(&nodes);
    let mut constraint_resolver = ForsterConstraintResolver::new(&creator.lg, &nodes);
    initialize_all(&mut creator.lg, Some(&mut *port_dist.borrow_mut()), Some(&mut constraint_resolver), None, &nodes);

    let mut cross_min = BarycenterHeuristic::new(constraint_resolver, Some(creator.random_ref()), port_dist.clone(), &nodes);
    initialize_all(&mut creator.lg, None, None, Some(&mut cross_min), &nodes);
    port_dist.borrow_mut().calculate_port_ranks(&creator.lg, &nodes[0], PortType::OUTPUT);
    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[0], false, true, true);

    port_dist.borrow_mut().calculate_port_ranks(&creator.lg, &nodes[1], PortType::OUTPUT);
    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[1], false, false, true);
    assert_eq!(nodes[1], expected_order_second_layer);

    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[2], false, false, true);
    assert_eq!(nodes[2], expected_switched_order);
}

/// Fixed port order, simple cross:
/// ```text
/// ____  *
/// |  |\/
/// |__|/\
///       *
/// ```
#[test]
fn test_assuming_fixed_port_order_given_simple_port_order_cross_removes_crossing_independent_of_random() {
    let mut creator = TestGraphCreator::new();
    let g = creator.graph;
    let left_layer = creator.make_layer_in(g);
    let right_layer = creator.make_layer_in(g);

    let left_node = creator.add_node_to_layer(left_layer);
    let right_top_node = creator.add_node_to_layer(right_layer);
    let right_bottom_node = creator.add_node_to_layer(right_layer);

    creator.east_west_edge_from_to(left_node, right_bottom_node);
    creator.east_west_edge_from_to(left_node, right_top_node);
    creator.set_fixed_order_constraint(left_node);
    creator.set_up_ids();

    let mut nodes = creator.lg.graph_to_node_array(creator.graph);

    let port_dist = node_relative_port_distributor(&nodes);
    let mut constraint_resolver = ForsterConstraintResolver::new(&creator.lg, &nodes);
    initialize_all(&mut creator.lg, Some(&mut *port_dist.borrow_mut()), Some(&mut constraint_resolver), None, &nodes);

    port_dist.borrow_mut().calculate_port_ranks(&creator.lg, &nodes[0], PortType::OUTPUT);
    let mut cross_min = BarycenterHeuristic::new(constraint_resolver, Some(creator.random_ref()), port_dist.clone(), &nodes);
    initialize_all(&mut creator.lg, None, None, Some(&mut cross_min), &nodes);

    let expected_order = TestGraphCreator::switch_order_in_array(0, 1, &nodes[1]);

    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[1], false, false, true);
    assert_eq!(nodes[1], expected_order);

    creator.random.borrow_mut().set_change_by(-0.1);
    creator.random.borrow_mut().set_next_boolean(false);
    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[1], false, false, true);
    assert_eq!(nodes[1], expected_order);
}

/// Fixed port order cross backwards:
/// ```text
/// *  ___
///  \/| |
///  /\|_|
/// *
/// ```
#[test]
fn test_assuming_fixed_port_order_given_simple_port_order_cross_removes_crossing_backwards() {
    let mut creator = TestGraphCreator::new();
    let g = creator.graph;
    let layer = creator.make_layer_in(g);
    let left_nodes = creator.add_nodes_to_layer(2, layer);
    let layer = creator.make_layer_in(g);
    let right_node = creator.add_node_to_layer(layer);
    creator.east_west_edge_from_to(left_nodes[0], right_node);
    creator.east_west_edge_from_to(left_nodes[1], right_node);
    creator.set_fixed_order_constraint(right_node);
    creator.set_up_ids();

    let mut nodes = creator.lg.graph_to_node_array(creator.graph);

    let port_dist = node_relative_port_distributor(&nodes);
    let mut constraint_resolver = ForsterConstraintResolver::new(&creator.lg, &nodes);
    initialize_all(&mut creator.lg, Some(&mut *port_dist.borrow_mut()), Some(&mut constraint_resolver), None, &nodes);

    port_dist.borrow_mut().calculate_port_ranks(&creator.lg, &nodes[1], PortType::INPUT);
    let mut cross_min = BarycenterHeuristic::new(constraint_resolver, Some(creator.random_ref()), port_dist.clone(), &nodes);
    initialize_all(&mut creator.lg, None, None, Some(&mut cross_min), &nodes);

    let expected_order = TestGraphCreator::switch_order_in_array(0, 1, &nodes[0]);

    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[0], false, false, false);

    assert_eq!(nodes[0], expected_order);
}

/// In-layer edges with fixed port order:
/// ```text
///       ___
///    ---| |
///    |  | |
/// ---+--|_|
/// |  |
/// *--|--*
///    |
///    ---*
/// ```
#[test]
fn test_in_layer_edges() {
    let mut creator = TestGraphCreator::new();
    let layer = creator.make_layer();
    let left_node = creator.add_node_to_layer(layer);
    let layer = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(3, layer);
    creator.set_fixed_order_constraint(right_nodes[0]);
    creator.east_west_edge_from_to(left_node, right_nodes[0]);
    creator.add_in_layer_edge(right_nodes[0], right_nodes[2], PortSide::WEST);
    creator.east_west_edge_from_to(left_node, right_nodes[1]);
    creator.set_up_ids();
    let mut nodes = creator.lg.graph_to_node_array(creator.graph);

    let port_dist = node_relative_port_distributor(&nodes);
    let mut constraint_resolver = ForsterConstraintResolver::new(&creator.lg, &nodes);
    initialize_all(&mut creator.lg, Some(&mut *port_dist.borrow_mut()), Some(&mut constraint_resolver), None, &nodes);

    port_dist.borrow_mut().calculate_port_ranks(&creator.lg, &nodes[0], PortType::INPUT);
    let mut cross_min = BarycenterHeuristic::new(constraint_resolver, Some(creator.random_ref()), port_dist.clone(), &nodes);
    initialize_all(&mut creator.lg, None, None, Some(&mut cross_min), &nodes);

    let expected_order = TestGraphCreator::get_array_in_index_order(&nodes[1], &[2, 0, 1]);

    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[1], false, false, true);

    assert_eq!(nodes[1], expected_order);
}

/// North-south edges:
/// ```text
///   ----*
///   |---*
///   ||
/// *-++--*
///   ||
///  ----
///  |__|
/// ```
#[test]
fn test_north_south_edges() {
    let mut creator = TestGraphCreator::new();
    let layer = creator.make_layer();
    let left_nodes = creator.add_nodes_to_layer(1, layer);
    let layer = creator.make_layer();
    let middle_nodes = creator.add_nodes_to_layer(4, layer);
    let layer = creator.make_layer();
    let right_nodes = creator.add_nodes_to_layer(3, layer);
    creator.east_west_edge_from_to(left_nodes[0], middle_nodes[2]);
    creator.east_west_edge_from_to(middle_nodes[2], right_nodes[2]);
    creator.set_as_long_edge_dummy(middle_nodes[2]);
    creator.add_north_south_edge(PortSide::NORTH, middle_nodes[3], middle_nodes[0], right_nodes[0], false);
    creator.add_north_south_edge(PortSide::NORTH, middle_nodes[3], middle_nodes[1], right_nodes[1], false);
    creator.set_up_ids();

    let mut nodes = creator.lg.graph_to_node_array(creator.graph);

    let port_dist = node_relative_port_distributor(&nodes);
    let mut constraint_resolver = ForsterConstraintResolver::new(&creator.lg, &nodes);
    initialize_all(&mut creator.lg, Some(&mut *port_dist.borrow_mut()), Some(&mut constraint_resolver), None, &nodes);

    port_dist.borrow_mut().calculate_port_ranks(&creator.lg, &nodes[0], PortType::INPUT);
    let mut cross_min = BarycenterHeuristic::new(constraint_resolver, Some(creator.random_ref()), port_dist.clone(), &nodes);
    initialize_all(&mut creator.lg, None, None, Some(&mut cross_min), &nodes);

    let expected_order = TestGraphCreator::get_array_in_index_order(&nodes[1], &[1, 0, 3, 2]);

    creator.random.borrow_mut().set_change_by(-0.01);
    cross_min.minimize_crossings_layer(&creator.lg, &mut nodes[1], false, false, true);

    assert_eq!(nodes[1], expected_order);
}
