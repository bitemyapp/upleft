//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/counting/org_eclipse_elk_alg_layered_p3order_counting_CrossingsCounter.swift`.
//!
//! Counts crossings with a binary indexed tree over port positions. The
//! position array is a [`SharedIntArray`]: several counters may share it (as
//! Java's `int[]` is shared), so each public method borrows it once and hands
//! the slice to the private helpers.

use super::binary_indexed_tree::BinaryIndexedTree;
use super::cross_min_util::{port_side_view, CrossMinUtil};
use super::shared_int_array::SharedIntArray;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

#[derive(Clone, Debug, Default)]
pub struct CrossingsCounter {
    /// Shared with every counter built from the same `SharedIntArray`.
    pub port_positions: SharedIntArray,
    index_tree: Option<BinaryIndexedTree>,
    ends: Vec<i64>,
    node_cardinalities: Vec<i64>,
}

const INDEXING_SIDE: PortSide = PortSide::WEST;
const STACK_SIDE: PortSide = PortSide::EAST;

/// `positionOf(_:)`: the port's position, `0` for an id outside the array.
#[inline]
fn position_of(lg: &LGraphArena, pp: &[i64], port: LPortId) -> i64 {
    let id = lg[port].id;
    if id < 0 || id as usize >= pp.len() {
        return 0;
    }
    pp[id as usize]
}

/// `portPositions[port.id] = value` behind the id range check the Swift
/// makes before writing.
#[inline]
fn set_position_checked(lg: &LGraphArena, pp: &mut [i64], port: LPortId, value: i64) {
    let id = lg[port].id;
    if id >= 0 && (id as usize) < pp.len() {
        pp[id as usize] = value;
    }
}

/// `otherEndOf(_:_:)`.
#[inline]
fn other_end_of(lg: &LGraphArena, edge: LEdgeId, from_port: LPortId) -> LPortId {
    let e = &lg[edge];
    if Some(from_port) == e.source {
        e.target.unwrap_or(from_port)
    } else {
        e.source.unwrap_or(from_port)
    }
}

/// `isInLayer(_:)`: both ends have layers and they are the same.
#[inline]
fn is_in_layer(lg: &LGraphArena, edge: LEdgeId) -> bool {
    let layer_of = |p: Option<LPortId>| p.and_then(|p| lg[p].owner).and_then(|n| lg[n].layer);
    match (layer_of(lg[edge].source), layer_of(lg[edge].target)) {
        (Some(s), Some(t)) => s == t,
        _ => false,
    }
}

/// `port.getConnectedEdges()` without allocating: incoming, then outgoing.
#[inline]
fn connected_edges(lg: &LGraphArena, port: LPortId) -> impl Iterator<Item = LEdgeId> + '_ {
    let p = &lg[port];
    p.incoming_edges.iter().chain(p.outgoing_edges.iter()).copied()
}

#[inline]
fn degree(lg: &LGraphArena, port: LPortId) -> i64 {
    (lg[port].incoming_edges.len() + lg[port].outgoing_edges.len()) as i64
}

impl CrossingsCounter {
    /// `CrossingsCounter()`.
    pub fn new() -> CrossingsCounter {
        CrossingsCounter::default()
    }

    /// `CrossingsCounter(_ portPositions: SharedIntArray)`: shares the array.
    pub fn with_shared(port_positions: SharedIntArray) -> CrossingsCounter {
        CrossingsCounter { port_positions, ..CrossingsCounter::default() }
    }

    /// `CrossingsCounter(_ portPositions: [Int])`: a private copy.
    pub fn from_values(port_positions: Vec<i64>) -> CrossingsCounter {
        CrossingsCounter::with_shared(SharedIntArray::from_values(port_positions))
    }

    // MARK: - Public API

    /// `countCrossingsBetweenLayers(_:_:)`.
    pub fn count_crossings_between_layers(&mut self, lg: &LGraphArena, left_layer_nodes: &[LNodeId], right_layer_nodes: &[LNodeId]) -> i64 {
        let shared = self.port_positions.clone();
        let mut pp = shared.values_mut();
        let ports = self.init_port_positions_counter_clockwise(lg, &mut pp, left_layer_nodes, right_layer_nodes);
        self.index_tree = Some(BinaryIndexedTree::new(ports.len()));
        self.count_crossings_on_ports(lg, &pp, &ports)
    }

    /// `countInLayerCrossingsOnSide(_:_:)`.
    pub fn count_in_layer_crossings_on_side(&mut self, lg: &LGraphArena, nodes: &[LNodeId], side: PortSide) -> i64 {
        let shared = self.port_positions.clone();
        let mut pp = shared.values_mut();
        let ports = self.init_port_positions_for_in_layer_crossings_in(lg, &mut pp, nodes, side);
        self.count_in_layer_crossings_on_ports(lg, &pp, &ports)
    }

    /// `countNorthSouthPortCrossingsInLayer(_:)`.
    pub fn count_north_south_port_crossings_in_layer(&mut self, lg: &LGraphArena, layer: &[LNodeId]) -> i64 {
        let shared = self.port_positions.clone();
        let mut pp = shared.values_mut();
        let ports = Self::init_positions_for_north_south_counting(lg, &mut pp, layer);
        self.index_tree = Some(BinaryIndexedTree::new(ports.len()));
        self.count_north_south_crossings_on_ports(lg, &pp, &ports)
    }

    /// `initForCountingBetween(_:_:)`.
    pub fn init_for_counting_between(&mut self, lg: &LGraphArena, left_layer_nodes: &[LNodeId], right_layer_nodes: &[LNodeId]) {
        let shared = self.port_positions.clone();
        let mut pp = shared.values_mut();
        let ports = self.init_port_positions_counter_clockwise(lg, &mut pp, left_layer_nodes, right_layer_nodes);
        self.index_tree = Some(BinaryIndexedTree::new(ports.len()));
    }

    /// `initPortPositionsForInLayerCrossings(_:_:)`.
    pub fn init_port_positions_for_in_layer_crossings(&mut self, lg: &LGraphArena, nodes: &[LNodeId], side: PortSide) -> Vec<LPortId> {
        let shared = self.port_positions.clone();
        let mut pp = shared.values_mut();
        self.init_port_positions_for_in_layer_crossings_in(lg, &mut pp, nodes, side)
    }

    fn init_port_positions_for_in_layer_crossings_in(&mut self, lg: &LGraphArena, pp: &mut [i64], nodes: &[LNodeId], side: PortSide) -> Vec<LPortId> {
        let mut ports = Vec::new();
        self.init_positions(lg, pp, nodes, &mut ports, side, true, true);
        self.index_tree = Some(BinaryIndexedTree::new(ports.len()));
        ports
    }

    /// `switchPorts(_:_:)` (unchecked indices: an out-of-range id traps).
    pub fn switch_ports(&mut self, lg: &LGraphArena, top_port: LPortId, bottom_port: LPortId) {
        let shared = self.port_positions.clone();
        let mut pp = shared.values_mut();
        Self::switch_ports_in(lg, &mut pp, top_port, bottom_port);
    }

    fn switch_ports_in(lg: &LGraphArena, pp: &mut [i64], top_port: LPortId, bottom_port: LPortId) {
        let top = lg[top_port].id as usize;
        let bottom = lg[bottom_port].id as usize;
        let top_port_pos = pp[top];
        pp[top] = pp[bottom];
        pp[bottom] = top_port_pos;
    }

    /// `switchNodes(_:_:_:)`.
    pub fn switch_nodes(&mut self, lg: &LGraphArena, was_upper_node: LNodeId, was_lower_node: LNodeId, side: PortSide) {
        let shared = self.port_positions.clone();
        let mut pp = shared.values_mut();
        self.switch_nodes_in(lg, &mut pp, was_upper_node, was_lower_node, side);
    }

    fn switch_nodes_in(&mut self, lg: &LGraphArena, pp: &mut [i64], was_upper_node: LNodeId, was_lower_node: LNodeId, side: PortSide) {
        for port in CrossMinUtil::in_north_south_east_west_order_iter(lg, was_upper_node, side) {
            let value = position_of(lg, pp, port) + self.node_cardinalities[lg[was_lower_node].id as usize];
            pp[lg[port].id as usize] = value;
        }
        for port in CrossMinUtil::in_north_south_east_west_order_iter(lg, was_lower_node, side) {
            let value = position_of(lg, pp, port) - self.node_cardinalities[lg[was_upper_node].id as usize];
            pp[lg[port].id as usize] = value;
        }
    }

    /// `countCrossingsBetweenPortsInBothOrders(_:_:)`: `(upperLower, lowerUpper)`.
    pub fn count_crossings_between_ports_in_both_orders(&mut self, lg: &LGraphArena, upper_port: LPortId, lower_port: LPortId) -> (i64, i64) {
        let shared = self.port_positions.clone();
        let mut pp = shared.values_mut();
        let ports = Self::connected_ports_sorted_by_position(lg, &pp, upper_port, lower_port);
        let upper_lower_crossings = self.count_crossings_on_ports(lg, &pp, &ports);
        if let Some(tree) = self.index_tree.as_mut() {
            tree.clear();
        }
        Self::switch_ports_in(lg, &mut pp, upper_port, lower_port);
        let sorted_ports = swift::sorted_by(ports.iter().copied(), |&a, &b| position_of(lg, &pp, a) < position_of(lg, &pp, b));
        let lower_upper_crossings = self.count_crossings_on_ports(lg, &pp, &sorted_ports);
        if let Some(tree) = self.index_tree.as_mut() {
            tree.clear();
        }
        Self::switch_ports_in(lg, &mut pp, lower_port, upper_port);
        (upper_lower_crossings, lower_upper_crossings)
    }

    /// `countInLayerCrossingsBetweenNodesInBothOrders(_:_:_:)`: `(upperLower, lowerUpper)`.
    pub fn count_in_layer_crossings_between_nodes_in_both_orders(
        &mut self,
        lg: &LGraphArena,
        upper_node: LNodeId,
        lower_node: LNodeId,
        side: PortSide,
    ) -> (i64, i64) {
        let shared = self.port_positions.clone();
        let mut pp = shared.values_mut();
        let ports = Self::connected_in_layer_ports_sorted_by_position(lg, &pp, upper_node, lower_node, side);
        let upper_lower_crossings = self.count_in_layer_crossings_on_ports(lg, &pp, &ports);
        self.switch_nodes_in(lg, &mut pp, upper_node, lower_node, side);
        if let Some(tree) = self.index_tree.as_mut() {
            tree.clear();
        }
        let sorted_ports = swift::sorted_by(ports.iter().copied(), |&a, &b| position_of(lg, &pp, a) < position_of(lg, &pp, b));
        let lower_upper_crossings = self.count_in_layer_crossings_on_ports(lg, &pp, &sorted_ports);
        self.switch_nodes_in(lg, &mut pp, lower_node, upper_node, side);
        if let Some(tree) = self.index_tree.as_mut() {
            tree.clear();
        }
        (upper_lower_crossings, lower_upper_crossings)
    }

    // MARK: - Private helpers

    /// `connectedPortsSortedByPosition(_:_:)`. The Swift `seen` set holds
    /// exactly the ports already in `result`, so a scan of `result` replaces it.
    fn connected_ports_sorted_by_position(lg: &LGraphArena, pp: &[i64], upper_port: LPortId, lower_port: LPortId) -> Vec<LPortId> {
        let mut result: Vec<(LPortId, i64)> = Vec::new();
        for port in [upper_port, lower_port] {
            if !result.iter().any(|&(p, _)| p == port) {
                result.push((port, position_of(lg, pp, port)));
            }
            for edge in connected_edges(lg, port) {
                if !Self::is_port_self_loop(lg, edge) {
                    let other = other_end_of(lg, edge, port);
                    if !result.iter().any(|&(p, _)| p == other) {
                        result.push((other, position_of(lg, pp, other)));
                    }
                }
            }
        }
        swift::sort_by(&mut result, |a, b| a.1 < b.1);
        result.into_iter().map(|(p, _)| p).collect()
    }

    /// `connectedInLayerPortsSortedByPosition(_:_:_:)`.
    fn connected_in_layer_ports_sorted_by_position(lg: &LGraphArena, pp: &[i64], upper_node: LNodeId, lower_node: LNodeId, side: PortSide) -> Vec<LPortId> {
        let mut result: Vec<(LPortId, i64)> = Vec::new();
        for node in [upper_node, lower_node] {
            for port in CrossMinUtil::in_north_south_east_west_order_iter(lg, node, side) {
                for edge in connected_edges(lg, port) {
                    if !lg.edge_is_self_loop(edge) {
                        if !result.iter().any(|&(p, _)| p == port) {
                            result.push((port, position_of(lg, pp, port)));
                        }
                        if is_in_layer(lg, edge) {
                            let other = other_end_of(lg, edge, port);
                            if !result.iter().any(|&(p, _)| p == other) {
                                result.push((other, position_of(lg, pp, other)));
                            }
                        }
                    }
                }
            }
        }
        swift::sort_by(&mut result, |a, b| a.1 < b.1);
        result.into_iter().map(|(p, _)| p).collect()
    }

    /// `isPortSelfLoop(_:)`: source and target are the same port (two nils
    /// count as the same, like Swift's optional `===`).
    fn is_port_self_loop(lg: &LGraphArena, edge: LEdgeId) -> bool {
        lg[edge].source == lg[edge].target
    }

    // MARK: - Private counting

    fn count_crossings_on_ports(&mut self, lg: &LGraphArena, pp: &[i64], ports: &[LPortId]) -> i64 {
        let Some(tree) = self.index_tree.as_mut() else { return 0 };
        let ends = &mut self.ends;
        let mut crossings = 0;
        for &port in ports {
            let port_position = position_of(lg, pp, port);
            tree.remove_all(port_position);
            for edge in connected_edges(lg, port) {
                let end_position = position_of(lg, pp, other_end_of(lg, edge, port));
                if end_position > port_position {
                    crossings += tree.rank(end_position);
                    ends.push(end_position);
                }
            }
            while let Some(end) = ends.pop() {
                tree.add(end);
            }
        }
        crossings
    }

    fn count_in_layer_crossings_on_ports(&mut self, lg: &LGraphArena, pp: &[i64], ports: &[LPortId]) -> i64 {
        let Some(tree) = self.index_tree.as_mut() else { return 0 };
        let ends = &mut self.ends;
        let mut crossings = 0;
        for &port in ports {
            let port_position = position_of(lg, pp, port);
            tree.remove_all(port_position);
            let mut num_between_layer_edges = 0;
            for edge in connected_edges(lg, port) {
                if is_in_layer(lg, edge) {
                    let end_position = position_of(lg, pp, other_end_of(lg, edge, port));
                    if end_position > port_position {
                        crossings += tree.rank(end_position);
                        ends.push(end_position);
                    }
                } else {
                    num_between_layer_edges += 1;
                }
            }
            crossings += tree.size() * num_between_layer_edges;
            while let Some(end) = ends.pop() {
                tree.add(end);
            }
        }
        crossings
    }

    fn count_north_south_crossings_on_ports(&mut self, lg: &LGraphArena, pp: &[i64], ports: &[LPortId]) -> i64 {
        let Some(tree) = self.index_tree.as_mut() else { return 0 };
        let ends = &mut self.ends;
        let mut crossings = 0;
        let mut targets_and_degrees: Vec<(LPortId, i64)> = Vec::new();

        for &port in ports {
            let port_position = position_of(lg, pp, port);
            tree.remove_all(port_position);
            targets_and_degrees.clear();

            match lg[port].owner.map(|n| lg[n].node_type) {
                Some(NodeType::NORMAL) => {
                    if let Some(dummy) = lg[port].props.get_as::<LNodeId>(&InternalProperties::PORT_DUMMY) {
                        for &p in &lg[dummy].ports {
                            targets_and_degrees.push((p, degree(lg, p)));
                        }
                    }
                }
                Some(NodeType::LONG_EDGE) => {
                    if let Some(node) = lg[port].owner {
                        if let Some(&other_port) = lg[node].ports.iter().find(|&&p| p != port) {
                            targets_and_degrees.push((other_port, degree(lg, other_port)));
                        }
                    }
                }
                Some(NodeType::NORTH_SOUTH_PORT) => {
                    if let Some(dummy_port) = lg[port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) {
                        targets_and_degrees.push((dummy_port, degree(lg, port)));
                    }
                }
                _ => {}
            }

            for &(target, deg) in &targets_and_degrees {
                let end_position = position_of(lg, pp, target);
                if end_position > port_position {
                    crossings += tree.rank(end_position) * deg;
                    ends.push(end_position);
                }
            }

            while let Some(end) = ends.pop() {
                tree.add(end);
            }
        }

        crossings
    }

    // MARK: - Port position initialization

    fn init_port_positions_counter_clockwise(&mut self, lg: &LGraphArena, pp: &mut [i64], left_layer_nodes: &[LNodeId], right_layer_nodes: &[LNodeId]) -> Vec<LPortId> {
        let mut ports = Vec::new();
        self.init_positions(lg, pp, left_layer_nodes, &mut ports, PortSide::EAST, true, false);
        self.init_positions(lg, pp, right_layer_nodes, &mut ports, PortSide::WEST, false, false);
        ports
    }

    #[allow(clippy::too_many_arguments)]
    fn init_positions(&mut self, lg: &LGraphArena, pp: &mut [i64], nodes: &[LNodeId], ports: &mut Vec<LPortId>, side: PortSide, top_down: bool, get_cardinalities: bool) {
        let mut num_ports = ports.len() as i64;
        if get_cardinalities {
            self.node_cardinalities.clear();
            self.node_cardinalities.resize(nodes.len(), 0);
        }

        let mut i: i64 = if top_down { 0 } else { nodes.len() as i64 - 1 };
        while if top_down { i < nodes.len() as i64 } else { i >= 0 } {
            let node = nodes[i as usize];
            let view = port_side_view(lg, node, side);
            // `getPorts(node, side, topDown)`: top-down, EAST ports in list
            // order and every other side reversed; bottom-up the opposite.
            let reversed = if side == PortSide::EAST { !top_down } else { top_down };
            let node_id = lg[node].id;
            if get_cardinalities && node_id >= 0 && (node_id as usize) < self.node_cardinalities.len() {
                self.node_cardinalities[node_id as usize] = view.len() as i64;
            }
            let start = ports.len();
            if reversed {
                ports.extend(view.iter().rev().copied());
            } else {
                ports.extend(view.iter().copied());
            }
            for &port in &ports[start..] {
                set_position_checked(lg, pp, port, num_ports);
                num_ports += 1;
            }
            i += if top_down { 1 } else { -1 };
        }
    }

    fn init_positions_for_north_south_counting(lg: &LGraphArena, pp: &mut [i64], nodes: &[LNodeId]) -> Vec<LPortId> {
        let mut ports: Vec<LPortId> = Vec::new();
        let mut stack: Vec<LNodeId> = Vec::new();

        let mut last_layout_unit: Option<LNodeId> = None;
        let mut index: i64 = 0;

        for &current in nodes {
            if Self::is_layout_unit_changed(lg, last_layout_unit, current) {
                index = Self::empty_stack(lg, pp, &mut stack, &mut ports, STACK_SIDE, index);
            }
            if lg[current].props.has(&InternalProperties::IN_LAYER_LAYOUT_UNIT) {
                last_layout_unit = lg[current].props.get_as::<LNodeId>(&InternalProperties::IN_LAYER_LAYOUT_UNIT);
            }

            match lg[current].node_type {
                NodeType::NORMAL => {
                    // `getNorthSouthPortsWithIncidentEdges(current, side)`.
                    for &p in port_side_view(lg, current, PortSide::NORTH) {
                        if lg[p].props.has(&InternalProperties::PORT_DUMMY) {
                            set_position_checked(lg, pp, p, index);
                            index += 1;
                            ports.push(p);
                        }
                    }

                    index = Self::empty_stack(lg, pp, &mut stack, &mut ports, STACK_SIDE, index);

                    for &p in port_side_view(lg, current, PortSide::SOUTH) {
                        if lg[p].props.has(&InternalProperties::PORT_DUMMY) {
                            set_position_checked(lg, pp, p, index);
                            index += 1;
                            ports.push(p);
                        }
                    }
                }
                NodeType::NORTH_SOUTH_PORT => {
                    if let Some(&p) = port_side_view(lg, current, INDEXING_SIDE).first() {
                        set_position_checked(lg, pp, p, index);
                        index += 1;
                        ports.push(p);
                    }
                    if !port_side_view(lg, current, STACK_SIDE).is_empty() {
                        stack.push(current);
                    }
                }
                NodeType::LONG_EDGE => {
                    for &p in port_side_view(lg, current, PortSide::WEST) {
                        set_position_checked(lg, pp, p, index);
                        index += 1;
                        ports.push(p);
                    }
                    for _ in port_side_view(lg, current, PortSide::EAST) {
                        stack.push(current);
                    }
                }
                _ => {}
            }
        }

        Self::empty_stack(lg, pp, &mut stack, &mut ports, STACK_SIDE, index);

        ports
    }

    fn empty_stack(lg: &LGraphArena, pp: &mut [i64], stack: &mut Vec<LNodeId>, ports: &mut Vec<LPortId>, side: PortSide, start_index: i64) -> i64 {
        let mut index = start_index;
        while let Some(dummy) = stack.pop() {
            let Some(&p) = port_side_view(lg, dummy, side).first() else { continue };
            set_position_checked(lg, pp, p, index);
            index += 1;
            ports.push(p);
        }
        index
    }

    /// `isLayoutUnitChanged(_:_:)`.
    fn is_layout_unit_changed(lg: &LGraphArena, last_unit: Option<LNodeId>, node: LNodeId) -> bool {
        let Some(last_unit) = last_unit else { return false };
        if last_unit == node {
            return false;
        }
        if !lg[node].props.has(&InternalProperties::IN_LAYER_LAYOUT_UNIT) {
            return false;
        }
        let unit = lg[node].props.get_as::<LNodeId>(&InternalProperties::IN_LAYER_LAYOUT_UNIT);
        unit != Some(last_unit)
    }
}
