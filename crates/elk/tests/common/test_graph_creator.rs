//! Port of `Tests/ElkSwiftTests/TestHelpers/TestGraphCreator.swift`
//! (`MockRandom` and `TestGraphCreator`).
//!
//! The Swift creator owns an `LGraph`; here it owns the whole arena
//! (`lg`) and the id of its main graph (`graph`). Methods keep the Swift
//! names in snake_case; Swift overloads get distinct names.

use std::cell::RefCell;
use std::rc::Rc;

use upleft_elk::org::eclipse::elk::alg::layered::graph_configurator::Random;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::{RandomDraws, RandomOverride, RandomRef};
use upleft_elk::org::eclipse::elk::core::options::edge_routing::EdgeRouting;
use upleft_elk::org::eclipse::elk::core::options::hierarchy_handling::HierarchyHandling;
use upleft_elk::prelude::*;

/// `MockRandom`: a `Random` subclass with predictable draws.
pub struct MockRandom {
    base: Random,
    current: f32,
    change_by_value: f64,
    next_boolean_value: bool,
}

impl MockRandom {
    pub fn new() -> MockRandom {
        // `super.init()` seeds from the clock; the overridden draws never
        // use the seed.
        MockRandom { base: Random::with_seed(0), current: 0.0, change_by_value: 0.0001, next_boolean_value: true }
    }

    pub fn set_next_boolean(&mut self, nb: bool) {
        self.next_boolean_value = nb;
    }

    pub fn set_change_by(&mut self, cb: f64) {
        self.change_by_value = cb;
    }

    pub fn set_current(&mut self, c: f32) {
        self.current = c;
    }
}

impl RandomDraws for MockRandom {
    fn next_boolean(&mut self) -> bool {
        self.next_boolean_value
    }

    fn next_float(&mut self) -> f32 {
        self.current += self.change_by_value as f32;
        self.current
    }

    fn next_double(&mut self) -> f64 {
        self.next_float() as f64
    }

    fn next_long(&mut self) -> i64 {
        self.base.next_long()
    }

    fn set_seed(&mut self, seed: i64) {
        self.base.set_seed(seed);
    }
}

pub struct TestGraphCreator {
    port_id: i32,
    node_id: i32,
    edge_id: i32,
    pub lg: LGraphArena,
    pub graph: LGraphId,
    pub random: Rc<RefCell<MockRandom>>,
}

impl Default for TestGraphCreator {
    fn default() -> Self {
        Self::new()
    }
}

impl TestGraphCreator {
    pub fn new() -> TestGraphCreator {
        let mut lg = LGraphArena::new();
        let graph = lg.new_graph();
        let mut creator = TestGraphCreator { port_id: 0, node_id: 0, edge_id: 0, lg, graph, random: Rc::new(RefCell::new(MockRandom::new())) };
        creator.set_up_graph(graph);
        creator
    }

    /// The mock as the minimizer sees it.
    pub fn random_ref(&self) -> RandomRef {
        self.random.clone()
    }

    // MARK: - Setup

    pub fn set_up_graph(&mut self, g: LGraphId) -> LGraphId {
        self.set_up_ids();
        self.lg[g].props.set(&LayeredOptions::EDGE_ROUTING, EdgeRouting::ORTHOGONAL);
        let random: RandomRef = self.random.clone();
        self.lg[g].props.set(&InternalProperties::RANDOM, PropValue::object(Rc::new(RandomOverride(random))));
        self.lg[g].props.set(&LayeredOptions::HIERARCHY_HANDLING, HierarchyHandling::INCLUDE_CHILDREN);
        g
    }

    /// `getGraph()`: sets the graph up again and returns it.
    pub fn get_graph(&mut self) -> LGraphId {
        let g = self.graph;
        self.set_up_graph(g);
        g
    }

    // MARK: - setUpIds

    pub fn set_up_ids(&mut self) {
        let mut graphs: std::collections::VecDeque<LGraphId> = std::collections::VecDeque::from([self.graph]);
        while let Some(g) = graphs.pop_front() {
            let mut l_id = 0;
            let mut p_id = 0;
            for l in self.lg[g].layers.clone() {
                self.lg[l].id = l_id;
                l_id += 1;
                let mut i = 0;
                for n in self.lg[l].nodes.clone() {
                    if let Some(nested) = self.lg[n].nested_graph {
                        graphs.push_back(nested);
                    }
                    self.lg[n].id = i;
                    i += 1;
                    for p in self.lg[n].ports.clone() {
                        self.lg[p].id = p_id;
                        p_id += 1;
                    }
                }
            }
        }
    }

    // MARK: - Layer Creation

    /// `makeLayer(_ g:)`.
    pub fn make_layer_in(&mut self, g: LGraphId) -> LayerId {
        let layer = self.lg.new_layer(g);
        self.lg[g].layers.push(layer);
        layer
    }

    /// `makeLayer()`.
    pub fn make_layer(&mut self) -> LayerId {
        let g = self.graph;
        self.make_layer_in(g)
    }

    /// `makeLayers(_:)`.
    pub fn make_layers(&mut self, amount: usize) -> Vec<LayerId> {
        let g = self.graph;
        self.make_layers_in(amount, g)
    }

    /// `makeLayers(_:_:)`.
    pub fn make_layers_in(&mut self, amount: usize, g: LGraphId) -> Vec<LayerId> {
        (0..amount).map(|_| self.make_layer_in(g)).collect()
    }

    // MARK: - Node Creation

    pub fn add_node_to_layer(&mut self, layer: LayerId) -> LNodeId {
        let owner = self.lg[layer].owner;
        let node = self.lg.new_node(Some(owner));
        self.lg.node_set_layer(node, Some(layer));
        self.lg[node].id = self.node_id;
        self.node_id += 1;
        self.lg[node].node_type = NodeType::NORMAL;
        node
    }

    pub fn add_nodes_to_layer(&mut self, amount_of_nodes: usize, layer: LayerId) -> Vec<LNodeId> {
        (0..amount_of_nodes).map(|_| self.add_node_to_layer(layer)).collect()
    }

    // MARK: - Edge Creation

    /// `eastWestEdgeFromTo(_ left: LNode, _ right: LNode)`.
    pub fn east_west_edge_from_to(&mut self, left: LNodeId, right: LNodeId) {
        let left_port = self.add_port_on_side(left, PortSide::EAST);
        let right_port = self.add_port_on_side(right, PortSide::WEST);
        self.add_edge_between_ports(left_port, right_port);
    }

    /// `eastWestEdgeFromTo(_ leftPort: LPort, _ rightNode: LNode)`.
    pub fn east_west_edge_from_port_to(&mut self, left_port: LPortId, right_node: LNodeId) {
        let right_port = self.add_port_on_side(right_node, PortSide::WEST);
        self.add_edge_between_ports(left_port, right_port);
    }

    /// `eastWestEdgeFromTo(_ left: LNode, _ right: LPort)`.
    pub fn east_west_edge_from_to_port(&mut self, left: LNodeId, right: LPortId) {
        let left_port = self.add_port_on_side(left, PortSide::EAST);
        self.add_edge_between_ports(left_port, right);
    }

    pub fn east_west_edges_from_to(&mut self, number_of_edges: usize, left: LNodeId, right: LNodeId) {
        for _ in 0..number_of_edges {
            self.east_west_edge_from_to(left, right);
        }
    }

    pub fn add_edge_between_ports(&mut self, from: LPortId, to: LPortId) -> LEdgeId {
        let edge = self.lg.new_edge();
        self.lg.edge_set_source(edge, Some(from));
        self.lg.edge_set_target(edge, Some(to));
        self.lg[edge].id = self.edge_id;
        self.edge_id += 1;
        edge
    }

    /// `addInLayerEdge(_ nodeOne: LNode, _ nodeTwo: LNode, _ portSide:)`.
    pub fn add_in_layer_edge(&mut self, node_one: LNodeId, node_two: LNodeId, port_side: PortSide) {
        let port_one = self.add_port_on_side(node_one, port_side);
        let port_two = self.add_port_on_side(node_two, port_side);
        self.add_edge_between_ports(port_one, port_two);
    }

    /// `addInLayerEdge(_ nodeOne: LNode, _ portTwo: LPort, _ portSide:)`.
    pub fn add_in_layer_edge_to_port(&mut self, node_one: LNodeId, port_two: LPortId, port_side: PortSide) {
        let port_one = self.add_port_on_side(node_one, port_side);
        self.add_edge_between_ports(port_one, port_two);
    }

    /// `addInLayerEdge(_ portOne: LPort, _ nodeTwo: LNode)`.
    pub fn add_in_layer_edge_from_port(&mut self, port_one: LPortId, node_two: LNodeId) {
        let side = self.lg[port_one].side;
        let port_two = self.add_port_on_side(node_two, side);
        self.add_edge_between_ports(port_one, port_two);
    }

    // MARK: - Port Creation

    pub fn add_port_on_side(&mut self, node: LNodeId, port_side: PortSide) -> LPortId {
        let port = self.add_port_to(node);
        self.lg.port_set_side(port, port_side);
        let constraints: PortConstraints = self.lg[node].props.get_typed(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
        if !constraints.is_side_fixed() {
            self.lg[node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_SIDE);
        }
        port
    }

    pub fn add_ports_on_side(&mut self, n: usize, node: LNodeId, port_side: PortSide) -> Vec<LPortId> {
        (0..n).map(|_| self.add_port_on_side(node, port_side)).collect()
    }

    fn add_port_to(&mut self, node: LNodeId) -> LPortId {
        let port = self.lg.new_port();
        self.lg.port_set_node(port, Some(node));
        self.lg[port].id = self.port_id;
        self.port_id += 1;
        port
    }

    // MARK: - Self-Loops

    pub fn self_loop_on(&mut self, node: LNodeId, side: PortSide) {
        let p1 = self.add_port_on_side(node, side);
        let p2 = self.add_port_on_side(node, side);
        self.add_edge_between_ports(p1, p2);
    }

    // MARK: - Constraints

    pub fn set_in_layer_order_constraint(&mut self, this_node: LNodeId, before_this_node: LNodeId) {
        let mut list: Vec<LNodeId> = self.lg[this_node].props.get_typed(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS).unwrap_or_default();
        list.push(before_this_node);
        self.lg[this_node].props.set(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS, list);
    }

    pub fn set_fixed_order_constraint(&mut self, node: LNodeId) -> LNodeId {
        self.lg[node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_ORDER);
        node
    }

    pub fn set_fixed_order_constraints(&mut self, nodes: &[LNodeId]) {
        for &n in nodes {
            self.set_fixed_order_constraint(n);
        }
    }

    pub fn set_port_order_fixed(&mut self, node: LNodeId) {
        self.set_fixed_order_constraint(node);
        let g = self.graph;
        let mut gps: EnumSet<GraphProperties> = self.lg[g].props.get_typed(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
        gps.insert(GraphProperties::NON_FREE_PORTS);
        self.lg[g].props.set(&InternalProperties::GRAPH_PROPERTIES, gps);
    }

    // MARK: - Node Type Setters

    pub fn set_as_north_south_node(&mut self, node: LNodeId) {
        self.lg[node].node_type = NodeType::NORTH_SOUTH_PORT;
    }

    pub fn set_as_long_edge_dummy(&mut self, node: LNodeId) {
        self.lg[node].node_type = NodeType::LONG_EDGE;
        self.lg[node].props.set_opt(&InternalProperties::IN_LAYER_LAYOUT_UNIT, None);
    }

    // MARK: - North-South Dummy Edge Builder

    pub fn add_north_south_edge(
        &mut self,
        side: PortSide,
        node_with_ns_ports: LNodeId,
        north_south_dummy: LNodeId,
        node_with_east_west_ports: LNodeId,
        node_with_east_west_ports_is_origin: bool,
    ) {
        self.lg[north_south_dummy].node_type = NodeType::NORTH_SOUTH_PORT;
        self.lg[north_south_dummy].props.set(&InternalProperties::IN_LAYER_LAYOUT_UNIT, node_with_ns_ports);

        let ew_layer = self.lg[node_with_east_west_ports].layer.unwrap();
        let ns_layer = self.lg[node_with_ns_ports].layer.unwrap();
        let normal_node_east_of_ns_port_node = self.lg.layer_index(ew_layer) < self.lg.layer_index(ns_layer);
        let direction = if normal_node_east_of_ns_port_node { PortSide::WEST } else { PortSide::EAST };

        let dummy_port = self.add_port_on_side(north_south_dummy, direction);
        let normal_port = self.add_port_on_side(node_with_east_west_ports, if direction == PortSide::WEST { PortSide::EAST } else { PortSide::WEST });

        let origin_port = self.add_port_on_side(node_with_ns_ports, side);
        self.lg[north_south_dummy].props.set(&InternalProperties::ORIGIN, node_with_ns_ports);
        self.lg[dummy_port].props.set(&InternalProperties::ORIGIN, origin_port);
        self.lg[origin_port].props.set(&InternalProperties::PORT_DUMMY, north_south_dummy);

        if node_with_east_west_ports_is_origin {
            self.add_edge_between_ports(normal_port, dummy_port);
        } else {
            self.add_edge_between_ports(dummy_port, normal_port);
        }

        let mut barycenter_associates: Vec<LNodeId> = self.lg[node_with_ns_ports].props.get_typed(&InternalProperties::BARYCENTER_ASSOCIATES).unwrap_or_default();
        barycenter_associates.push(north_south_dummy);
        self.lg[node_with_ns_ports].props.set(&InternalProperties::BARYCENTER_ASSOCIATES, barycenter_associates);

        if side == PortSide::NORTH {
            self.set_in_layer_order_constraint(north_south_dummy, node_with_ns_ports);
        } else {
            self.set_in_layer_order_constraint(node_with_ns_ports, north_south_dummy);
        }
    }

    // MARK: - External Port Dummies

    pub fn add_external_port_dummy_node_to_layer(&mut self, layer: LayerId, port: LPortId) -> LNodeId {
        let dummy = self.add_node_to_layer(layer);
        self.lg[dummy].props.set(&InternalProperties::ORIGIN, port);
        self.lg[dummy].node_type = NodeType::EXTERNAL_PORT;
        let side = self.lg[port].side;
        self.lg[dummy].props.set(&InternalProperties::EXT_PORT_SIDE, side);
        self.lg[port].props.set(&InternalProperties::PORT_DUMMY, dummy);
        self.lg[port].props.set(&InternalProperties::INSIDE_CONNECTIONS, true);
        let g = self.graph;
        let mut gps: EnumSet<GraphProperties> = self.lg[g].props.get_typed(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
        gps.insert(GraphProperties::EXTERNAL_PORTS);
        self.lg[g].props.set(&InternalProperties::GRAPH_PROPERTIES, gps);
        dummy
    }

    pub fn add_external_port_dummies_to_layer(&mut self, layer: LayerId, ports: &[LPortId]) -> Vec<LNodeId> {
        let mut dummies: Vec<Option<LNodeId>> = vec![None; ports.len()];
        for i in 0..ports.len() {
            let index = if self.lg[ports[i]].side == PortSide::EAST { i } else { ports.len() - 1 - i };
            dummies[index] = Some(self.add_external_port_dummy_node_to_layer(layer, ports[i]));
        }
        dummies.into_iter().map(|d| d.unwrap()).collect()
    }

    // MARK: - Nested Graph

    pub fn nested_graph(&mut self, node: LNodeId) -> LGraphId {
        if let Some(existing) = self.lg[node].nested_graph {
            return existing;
        }
        self.lg[node].props.set(&InternalProperties::COMPOUND_NODE, true);
        let nested = self.lg.new_graph();
        self.set_up_graph(nested);
        self.lg[node].nested_graph = Some(nested);
        self.lg[nested].parent_node = Some(node);
        nested
    }

    // MARK: - getCurrentOrder

    pub fn get_current_order(&self, g: LGraphId) -> Vec<Vec<LNodeId>> {
        self.lg.graph_to_node_array(g)
    }

    // MARK: - List Manipulation Helpers

    pub fn get_list_copy_in_index_order<T: Clone>(li: &[T], indices: &[usize]) -> Vec<T> {
        indices.iter().map(|&i| li[i].clone()).collect()
    }

    pub fn get_array_in_index_order<T: Clone>(arr: &[T], indices: &[usize]) -> Vec<T> {
        let mut r = arr.to_vec();
        for i in 0..indices.len() {
            r[i] = arr[indices[i]].clone();
        }
        r
    }

    pub fn copy_of_list_switching_order<T: Clone>(i: usize, j: usize, list: &[T]) -> Vec<T> {
        let mut copy = list.to_vec();
        copy[i] = list[j].clone();
        copy[j] = list[i].clone();
        copy
    }

    pub fn switch_order_in_array<T: Clone>(i: usize, j: usize, arr: &[T]) -> Vec<T> {
        Self::copy_of_list_switching_order(i, j, arr)
    }

    pub fn switch_order_of_nodes_in_layer(&mut self, node_one: usize, node_two: usize, layer: LayerId) -> Vec<LNodeId> {
        self.lg[layer].nodes.swap(node_one, node_two);
        self.lg[layer].nodes.clone()
    }

    pub fn copy_of_nodes_in_layer(&self, layer_index: usize) -> Vec<LNodeId> {
        let layer = self.lg[self.graph].layers[layer_index];
        self.lg[layer].nodes.clone()
    }

    pub fn copy_of_switch_order_of_nodes_in_layer(&self, node_one: usize, node_two: usize, layer_index: usize) -> Vec<LNodeId> {
        Self::copy_of_list_switching_order(node_one, node_two, &self.copy_of_nodes_in_layer(layer_index))
    }

    pub fn get_copy_with_switched_order(node_one: usize, node_two: usize, layer: &[LNodeId]) -> Vec<LNodeId> {
        Self::copy_of_list_switching_order(node_one, node_two, layer)
    }

    pub fn copy_ports_in_index_order(&self, node: LNodeId, indices: &[usize]) -> Vec<LPortId> {
        indices.iter().map(|&i| self.lg[node].ports[i]).collect()
    }

    // MARK: - setOnAllGraphs

    pub fn set_on_all_graphs(&mut self, prop: &Property, val: Option<PropValue>, g: LGraphId) {
        self.lg[g].props.set_opt(prop, val.clone());
        for l in self.lg[g].layers.clone() {
            for n in self.lg[l].nodes.clone() {
                if let Some(nested) = self.lg[n].nested_graph {
                    self.set_on_all_graphs(prop, val.clone(), nested);
                }
            }
        }
    }

    // MARK: - Random Getter/Setter

    pub fn get_random(&self) -> Rc<RefCell<MockRandom>> {
        self.random.clone()
    }

    pub fn set_random(&mut self, r: Rc<RefCell<MockRandom>>) {
        self.random = r;
    }

    // MARK: - Graph Factory Methods

    pub fn get_empty_graph(&mut self) -> LGraphId {
        self.get_graph()
    }

    pub fn get_two_nodes_no_connection_graph(&mut self) -> LGraphId {
        let layer = self.make_layer();
        self.add_nodes_to_layer(2, layer);
        self.get_graph()
    }

    pub fn get_cross_formed_graph(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, left_layer);
        let right_nodes = self.add_nodes_to_layer(2, right_layer);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[0]);
        self.get_graph()
    }

    pub fn multiple_edges_and_single_edge(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, left_layer);
        let right_nodes = self.add_nodes_to_layer(2, right_layer);
        self.east_west_edges_from_to(2, left_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[1]);
        self.get_graph()
    }

    pub fn get_cross_formed_graph_with_constraints_in_second_layer(&mut self) -> LGraphId {
        self.get_cross_formed_graph();
        let right_layer = self.lg[self.graph].layers[1];
        let top = self.lg[right_layer].nodes[0];
        let bottom = self.lg[right_layer].nodes[1];
        self.set_in_layer_order_constraint(top, bottom);
        self.get_graph()
    }

    pub fn get_cross_formed_graph_constraints_prevent_any_switch(&mut self) -> LGraphId {
        self.get_cross_formed_graph_with_constraints_in_second_layer();
        let left_layer = self.lg[self.graph].layers[0];
        let top = self.lg[left_layer].nodes[0];
        let bottom = self.lg[left_layer].nodes[1];
        self.set_in_layer_order_constraint(top, bottom);
        self.get_graph()
    }

    pub fn get_one_node_graph(&mut self) -> LGraphId {
        let layer = self.make_layer();
        self.add_node_to_layer(layer);
        self.get_graph()
    }

    pub fn get_in_layer_edges_graph(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let middle_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_node = self.add_node_to_layer(left_layer);
        let middle_nodes = self.add_nodes_to_layer(3, middle_layer);
        let right_node = self.add_node_to_layer(right_layer);
        // add east side ports first to get expected port ordering
        self.east_west_edge_from_to(middle_nodes[1], right_node);
        self.east_west_edge_from_to(left_node, middle_nodes[1]);
        self.add_in_layer_edge(middle_nodes[0], middle_nodes[2], PortSide::WEST);
        self.set_up_ids();
        self.graph
    }

    pub fn get_in_layer_edges_graph_which_results_in_crossings_when_switched(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_node = self.add_node_to_layer(left_layer);
        let right_nodes = self.add_nodes_to_layer(3, right_layer);
        self.add_in_layer_edge(right_nodes[0], right_nodes[1], PortSide::WEST);
        self.east_west_edge_from_to(left_node, right_nodes[2]);
        self.get_graph()
    }

    pub fn get_multiple_edges_between_same_nodes_graph(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, left_layer);
        let right_nodes = self.add_nodes_to_layer(2, right_layer);
        self.east_west_edges_from_to(2, left_nodes[0], right_nodes[1]);
        self.east_west_edges_from_to(2, left_nodes[1], right_nodes[0]);
        self.get_graph()
    }

    pub fn get_cross_with_extra_edge_in_between_graph(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, left_layer);
        let right_nodes = self.add_nodes_to_layer(3, right_layer);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[2]);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[0]);
        self.get_graph()
    }

    pub fn get_cross_with_many_self_loops_graph(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let top_left = self.add_node_to_layer(left_layer);
        let bottom_left = self.add_node_to_layer(left_layer);
        let top_right = self.add_node_to_layer(right_layer);
        let bottom_right = self.add_node_to_layer(right_layer);
        let top_left_port = self.add_port_on_side(top_left, PortSide::EAST);
        let bottom_left_port = self.add_port_on_side(bottom_left, PortSide::EAST);
        self.set_up_ids();
        for l in self.lg[self.graph].layers.clone() {
            for n in self.lg[l].nodes.clone() {
                self.self_loop_on(n, PortSide::EAST);
                self.self_loop_on(n, PortSide::EAST);
                self.self_loop_on(n, PortSide::EAST);
                self.self_loop_on(n, PortSide::WEST);
                self.self_loop_on(n, PortSide::WEST);
                self.self_loop_on(n, PortSide::WEST);
            }
        }
        let top_right_port = self.add_port_on_side(top_right, PortSide::WEST);
        let bottom_right_port = self.add_port_on_side(bottom_right, PortSide::WEST);
        self.add_edge_between_ports(top_left_port, bottom_right_port);
        self.add_edge_between_ports(bottom_left_port, top_right_port);
        self.graph
    }

    pub fn get_more_complex_three_layer_graph(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let middle_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(3, left_layer);
        let middle_nodes = self.add_nodes_to_layer(2, middle_layer);
        let right_nodes = self.add_nodes_to_layer(3, right_layer);
        let left_middle_node_port = self.add_port_on_side(left_nodes[1], PortSide::EAST);
        let middle_lower_node_port_east = self.add_port_on_side(middle_nodes[1], PortSide::EAST);
        let middle_upper_node_port_east = self.add_port_on_side(middle_nodes[0], PortSide::EAST);
        let right_upper_node_port = self.add_port_on_side(right_nodes[0], PortSide::WEST);
        let right_middle_node_port = self.add_port_on_side(right_nodes[1], PortSide::WEST);
        self.set_up_ids();
        self.add_edge_between_ports(middle_upper_node_port_east, right_upper_node_port);
        self.add_edge_between_ports(middle_upper_node_port_east, right_middle_node_port);
        self.add_edge_between_ports(middle_upper_node_port_east, right_middle_node_port);
        self.east_west_edge_from_port_to(middle_lower_node_port_east, right_nodes[2]);
        self.east_west_edge_from_port_to(left_middle_node_port, middle_nodes[0]);
        self.east_west_edge_from_to_port(middle_nodes[1], right_upper_node_port);
        self.east_west_edge_from_port_to(left_middle_node_port, middle_nodes[1]);
        self.east_west_edge_from_to(left_nodes[2], middle_nodes[0]);
        self.east_west_edge_from_to(left_nodes[0], middle_nodes[0]);
        self.set_up_ids();
        self.graph
    }

    pub fn get_fixed_port_order_graph(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_node = self.add_node_to_layer(left_layer);
        let right_nodes = self.add_nodes_to_layer(2, right_layer);
        self.set_fixed_order_constraint(left_node);
        self.east_west_edge_from_to(left_node, right_nodes[1]);
        self.east_west_edge_from_to(left_node, right_nodes[0]);
        self.get_graph()
    }

    pub fn get_graph_no_crossings_due_to_port_order_not_fixed(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_node = self.add_node_to_layer(left_layer);
        let right_nodes = self.add_nodes_to_layer(2, right_layer);
        self.east_west_edge_from_to(left_node, right_nodes[1]);
        self.east_west_edge_from_to(left_node, right_nodes[0]);
        self.get_graph()
    }

    pub fn get_switch_only_one_sided(&mut self) -> LGraphId {
        let layers = self.make_layers(3);
        let left_nodes = self.add_nodes_to_layer(2, layers[0]);
        let middle_nodes = self.add_nodes_to_layer(2, layers[1]);
        let right_nodes = self.add_nodes_to_layer(2, layers[2]);
        self.east_west_edge_from_to(middle_nodes[0], right_nodes[0]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[1]);
        self.east_west_edge_from_to(left_nodes[0], middle_nodes[1]);
        self.east_west_edge_from_to(left_nodes[1], middle_nodes[0]);
        self.set_up_ids();
        self.graph
    }

    pub fn get_switch_only_east_one_sided(&mut self) -> LGraphId {
        let layers = self.make_layers(3);
        let left_nodes = self.add_nodes_to_layer(2, layers[0]);
        let middle_nodes = self.add_nodes_to_layer(2, layers[1]);
        let right_nodes = self.add_nodes_to_layer(2, layers[2]);
        self.east_west_edge_from_to(left_nodes[0], middle_nodes[0]);
        self.east_west_edge_from_to(left_nodes[1], middle_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[0]);
        self.set_up_ids();
        self.graph
    }

    pub fn get_fixed_port_order_in_layer_edges_dont_cross_each_other(&mut self) -> LGraphId {
        let layer = self.make_layer();
        let nodes = self.add_nodes_to_layer(2, layer);
        self.set_fixed_order_constraint(nodes[0]);
        self.set_fixed_order_constraint(nodes[1]);
        let top0_port = self.add_port_on_side(nodes[0], PortSide::EAST);
        let bottom0_port = self.add_port_on_side(nodes[0], PortSide::EAST);
        let top1_port = self.add_port_on_side(nodes[1], PortSide::EAST);
        let bottom1_port = self.add_port_on_side(nodes[1], PortSide::EAST);
        self.add_edge_between_ports(bottom0_port, top1_port);
        self.add_edge_between_ports(top0_port, bottom1_port);
        self.get_graph()
    }

    pub fn get_fixed_port_order_in_layer_edges_with_crossings(&mut self) -> LGraphId {
        let layer = self.make_layer();
        let nodes = self.add_nodes_to_layer(2, layer);
        self.set_fixed_order_constraint(nodes[0]);
        self.set_fixed_order_constraint(nodes[1]);
        self.add_in_layer_edge(nodes[0], nodes[1], PortSide::EAST);
        self.add_in_layer_edge(nodes[0], nodes[1], PortSide::EAST);
        self.set_up_ids();
        self.graph
    }

    pub fn get_more_complex_in_layer_graph(&mut self) -> LGraphId {
        let layers = self.make_layers(3);
        let left_nodes = self.add_nodes_to_layer(4, layers[0]);
        let middle_nodes = self.add_nodes_to_layer(3, layers[1]);
        let right_node = self.add_node_to_layer(layers[2]);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.set_fixed_order_constraint(middle_nodes[1]);
        self.east_west_edge_from_to(left_nodes[1], middle_nodes[0]);
        self.east_west_edge_from_to(left_nodes[3], middle_nodes[1]);
        self.east_west_edge_from_to(left_nodes[2], middle_nodes[1]);
        self.add_in_layer_edge(middle_nodes[0], middle_nodes[1], PortSide::WEST);
        self.east_west_edge_from_to(left_nodes[0], middle_nodes[0]);
        self.add_in_layer_edge(middle_nodes[0], middle_nodes[2], PortSide::WEST);
        self.add_in_layer_edge(middle_nodes[0], middle_nodes[1], PortSide::EAST);
        self.east_west_edge_from_to(middle_nodes[0], right_node);
        self.set_up_ids();
        self.graph
    }

    pub fn get_graph_which_could_be_worsened_by_switch(&mut self) -> LGraphId {
        let layers = self.make_layers(3);
        let left_nodes = self.add_nodes_to_layer(2, layers[0]);
        let middle_nodes = self.add_nodes_to_layer(2, layers[1]);
        let right_nodes = self.add_nodes_to_layer(2, layers[2]);
        self.set_in_layer_order_constraint(left_nodes[0], left_nodes[1]);
        self.set_in_layer_order_constraint(right_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(left_nodes[0], middle_nodes[0]);
        self.east_west_edge_from_to(left_nodes[0], middle_nodes[1]);
        self.east_west_edge_from_to(left_nodes[1], middle_nodes[0]);
        self.east_west_edge_from_to(left_nodes[1], middle_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[0], right_nodes[0]);
        self.east_west_edge_from_to(middle_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[0]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[1]);
        self.get_graph()
    }

    pub fn get_nodes_in_different_layout_units_prevent_switch(&mut self) -> LGraphId {
        let layers = self.make_layers(2);
        let left_node = self.add_node_to_layer(layers[0]);
        let right_nodes = self.add_nodes_to_layer(3, layers[1]);
        self.set_as_north_south_node(right_nodes[1]);
        self.lg[right_nodes[1]].props.set(&InternalProperties::IN_LAYER_LAYOUT_UNIT, right_nodes[0]);
        self.east_west_edge_from_to(left_node, right_nodes[2]);
        self.get_graph()
    }

    pub fn multiple_in_between_layer_edges_into_node_with_no_fixed_port_order(&mut self) -> LGraphId {
        let layers = self.make_layers(2);
        let left_nodes = self.add_nodes_to_layer(2, layers[0]);
        let right_nodes = self.add_nodes_to_layer(2, layers[1]);
        self.add_in_layer_edge(right_nodes[0], right_nodes[1], PortSide::WEST);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[1]);
        self.get_graph()
    }

    pub fn multiple_in_between_layer_edges_into_node_with_no_fixed_port_order_cause_crossings(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, left_layer);
        let right_layer = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(3, right_layer);
        self.add_in_layer_edge(right_nodes[0], right_nodes[2], PortSide::WEST);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
        self.set_up_ids();
        self.graph
    }

    pub fn get_switched_problem_graph(&mut self) -> LGraphId {
        let l0 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, l0);
        let l1 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(4, l1);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[2]);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[3]);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[0]);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[2]);
        self.set_up_ids();
        self.graph
    }

    pub fn two_edges_into_same_port(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let top_left = self.add_node_to_layer(left_layer);
        let bottom_left = self.add_node_to_layer(left_layer);
        let top_right = self.add_node_to_layer(right_layer);
        let bottom_right = self.add_node_to_layer(right_layer);
        self.east_west_edge_from_to(top_left, bottom_right);
        let bottom_left_first_port = self.add_port_on_side(bottom_left, PortSide::EAST);
        let bottom_left_second_port = self.add_port_on_side(bottom_left, PortSide::EAST);
        let top_right_first_port = self.add_port_on_side(top_right, PortSide::WEST);
        let top_right_second_port = self.add_port_on_side(top_right, PortSide::WEST);
        self.add_edge_between_ports(bottom_left_first_port, top_right_first_port);
        self.add_edge_between_ports(bottom_left_second_port, top_right_second_port);
        self.set_up_ids();
        self.graph
    }

    pub fn two_edges_into_same_port_crosses_when_switched(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let top_left = self.add_node_to_layer(left_layer);
        let bottom_left = self.add_node_to_layer(left_layer);
        let top_right = self.add_node_to_layer(right_layer);
        let bottom_right = self.add_node_to_layer(right_layer);
        let top_right_port = self.add_port_on_side(top_right, PortSide::WEST);
        let bottom_left_port = self.add_port_on_side(bottom_left, PortSide::EAST);
        self.add_edge_between_ports(bottom_left_port, top_right_port);
        let top_left_port = self.add_port_on_side(top_left, PortSide::EAST);
        self.add_edge_between_ports(top_left_port, top_right_port);
        self.east_west_edge_from_to(bottom_left, bottom_right);
        self.set_up_ids();
        self.graph
    }

    pub fn two_edges_into_same_port_resolves_crossing_when_switched(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let top_left = self.add_node_to_layer(left_layer);
        let bottom_left = self.add_node_to_layer(left_layer);
        let top_right = self.add_node_to_layer(right_layer);
        let bottom_right = self.add_node_to_layer(right_layer);
        let top_left_port = self.add_port_on_side(top_left, PortSide::EAST);
        let bottom_left_port = self.add_port_on_side(bottom_left, PortSide::EAST);
        let bottom_right_port = self.add_port_on_side(bottom_right, PortSide::WEST);
        self.add_edge_between_ports(top_left_port, bottom_right_port);
        self.add_edge_between_ports(bottom_left_port, bottom_right_port);
        self.east_west_edge_from_to(bottom_left, top_right);
        self.set_up_ids();
        self.graph
    }

    pub fn two_edges_into_same_port_from_east_with_fixed_port_order(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let top_left = self.add_node_to_layer(left_layer);
        let bottom_left = self.add_node_to_layer(left_layer);
        let top_right = self.add_node_to_layer(right_layer);
        let bottom_right = self.add_node_to_layer(right_layer);
        self.set_fixed_order_constraint(bottom_left);
        self.set_fixed_order_constraint(top_right);
        let top_left_port = self.add_port_on_side(top_left, PortSide::EAST);
        let bottom_left_port = self.add_port_on_side(bottom_left, PortSide::EAST);
        let top_right_port = self.add_port_on_side(top_right, PortSide::WEST);
        let bottom_right_port = self.add_port_on_side(bottom_right, PortSide::WEST);
        self.add_edge_between_ports(bottom_left_port, bottom_right_port);
        self.add_edge_between_ports(bottom_left_port, top_right_port);
        self.add_edge_between_ports(top_left_port, top_right_port);
        self.set_up_ids();
        self.graph
    }

    pub fn multiple_edges_into_one_port_and_free_port_order(&mut self) -> LGraphId {
        let layer = self.make_layer();
        let nodes = self.add_nodes_to_layer(3, layer);
        let port_one = self.add_port_on_side(nodes[0], PortSide::WEST);
        let port_two = self.add_port_on_side(nodes[1], PortSide::WEST);
        let port_three = self.add_port_on_side(nodes[2], PortSide::WEST);
        self.add_edge_between_ports(port_one, port_three);
        self.add_edge_between_ports(port_two, port_three);
        self.get_graph()
    }

    pub fn get_only_correctly_improved_by_best_of_forward_and_backward_sweeps_in_single_layer(&mut self) -> LGraphId {
        let left_layer = self.make_layer();
        let right_layer = self.make_layer();
        let left_node = self.add_node_to_layer(left_layer);
        let right_nodes = self.add_nodes_to_layer(3, right_layer);
        self.set_fixed_order_constraint(left_node);
        self.add_in_layer_edge(right_nodes[0], right_nodes[1], PortSide::WEST);
        self.east_west_edge_from_to(left_node, right_nodes[2]);
        self.east_west_edge_from_to(left_node, right_nodes[2]);
        self.get_graph()
    }
}
