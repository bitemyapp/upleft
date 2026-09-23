//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/greedyswitch/org_eclipse_elk_alg_layered_intermediate_greedyswitch_BetweenLayerEdgeTwoNodeCrossingsCounter.swift`.
//!
//! Counts the crossings between the edges of two nodes of the free layer and
//! the neighbouring layers, in both relative orders of the two nodes.
//!
//! The Swift keeps a value copy of the whole node order but only ever reads
//! the neighbouring layers (in the initializer) and the free layer (to build
//! the adjacency lists lazily), so the port keeps a copy of the free layer.
//! Each `AdjacencyList` gets a value copy of the position dictionary that it
//! only reads while being built.

use std::collections::HashMap;

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::p3order::counting::cross_min_util::CrossMinUtil;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

#[derive(Clone, Debug)]
pub struct BetweenLayerEdgeTwoNodeCrossingsCounter {
    upper_lower_crossings: i64,
    lower_upper_crossings: i64,
    /// `currentNodeOrder[freeLayerIndex]` of the order given at creation
    /// (`None` when that index is out of range, which traps on first use).
    free_layer: Option<Vec<LNodeId>>,
    free_layer_index: i64,
    port_positions: HashMap<LPortId, i64>,
    eastern_adjacencies: Adjacencies,
    western_adjacencies: Adjacencies,
}

/// `[ObjectIdentifier: AdjacencyList]`, built lazily for the whole free layer.
#[derive(Clone, Debug, Default)]
struct Adjacencies {
    index: HashMap<LNodeId, usize>,
    lists: Vec<AdjacencyList>,
}

impl BetweenLayerEdgeTwoNodeCrossingsCounter {
    pub fn new(lg: &LGraphArena, current_node_order: &[Vec<LNodeId>], free_layer_index: i64) -> BetweenLayerEdgeTwoNodeCrossingsCounter {
        let mut counter = BetweenLayerEdgeTwoNodeCrossingsCounter {
            upper_lower_crossings: 0,
            lower_upper_crossings: 0,
            free_layer: usize::try_from(free_layer_index).ok().and_then(|i| current_node_order.get(i)).cloned(),
            free_layer_index,
            port_positions: HashMap::new(),
            eastern_adjacencies: Adjacencies::default(),
            western_adjacencies: Adjacencies::default(),
        };
        counter.set_port_positions_for_neighbouring_layers(lg, current_node_order);
        counter
    }

    fn set_port_positions_for_neighbouring_layers(&mut self, lg: &LGraphArena, current_node_order: &[Vec<LNodeId>]) {
        if self.free_layer_is_not_first_layer() {
            self.set_port_positions_for_layer(lg, &current_node_order[(self.free_layer_index - 1) as usize], PortSide::EAST);
        }
        if self.free_layer_is_not_last_layer(current_node_order.len()) {
            self.set_port_positions_for_layer(lg, &current_node_order[(self.free_layer_index + 1) as usize], PortSide::WEST);
        }
    }

    fn free_layer_is_not_first_layer(&self) -> bool {
        self.free_layer_index > 0
    }

    fn free_layer_is_not_last_layer(&self, count: usize) -> bool {
        self.free_layer_index < count as i64 - 1
    }

    fn set_port_positions_for_layer(&mut self, lg: &LGraphArena, layer: &[LNodeId], port_side: PortSide) {
        let mut port_id = 0;
        for &node in layer {
            for port in CrossMinUtil::in_north_south_east_west_order_iter(lg, node, port_side) {
                self.port_positions.insert(port, port_id);
                port_id += 1;
            }
        }
    }

    /// `countEasternEdgeCrossings(_:_:)`.
    pub fn count_eastern_edge_crossings(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        self.reset_crossing_count();
        if upper_node == lower_node {
            return;
        }
        self.add_eastern_crossings(lg, upper_node, lower_node);
    }

    /// `countWesternEdgeCrossings(_:_:)`.
    pub fn count_western_edge_crossings(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        self.reset_crossing_count();
        if upper_node == lower_node {
            return;
        }
        self.add_western_crossings(lg, upper_node, lower_node);
    }

    /// `countBothSideCrossings(_:_:)`.
    pub fn count_both_side_crossings(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        self.reset_crossing_count();
        if upper_node == lower_node {
            return;
        }
        self.add_western_crossings(lg, upper_node, lower_node);
        self.add_eastern_crossings(lg, upper_node, lower_node);
    }

    fn reset_crossing_count(&mut self) {
        self.upper_lower_crossings = 0;
        self.lower_upper_crossings = 0;
    }

    fn add_eastern_crossings(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        let mut adjacencies = std::mem::take(&mut self.eastern_adjacencies);
        self.add_crossings(lg, upper_node, lower_node, PortSide::EAST, &mut adjacencies);
        self.eastern_adjacencies = adjacencies;
    }

    fn add_western_crossings(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        let mut adjacencies = std::mem::take(&mut self.western_adjacencies);
        self.add_crossings(lg, upper_node, lower_node, PortSide::WEST, &mut adjacencies);
        self.western_adjacencies = adjacencies;
    }

    fn add_crossings(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId, side: PortSide, adjacencies: &mut Adjacencies) {
        let upper = self.get_adjacency_for(lg, upper_node, side, adjacencies);
        let lower = self.get_adjacency_for(lg, lower_node, side, adjacencies);
        // `upperNode !== lowerNode` here, so the two lists are distinct.
        let (upper_adj, lower_adj) = two_mut(&mut adjacencies.lists, upper, lower);
        if upper_adj.size() == 0 || lower_adj.size() == 0 {
            return;
        }
        self.count_crossings_by_merging_adjacency_lists(upper_adj, lower_adj);
    }

    /// `getAdjacencyFor(_:_:_:)`: builds every free-layer node's list on
    /// first use, then resets and returns the node's (traps if the node is not
    /// in the free layer).
    fn get_adjacency_for(&self, lg: &LGraphArena, node: LNodeId, side: PortSide, adjacencies: &mut Adjacencies) -> usize {
        if adjacencies.index.is_empty() {
            let free_layer = self.free_layer.as_ref().expect("Index out of range");
            for &n in free_layer {
                let list = AdjacencyList::new(lg, n, side, &self.port_positions);
                match adjacencies.index.get(&n) {
                    Some(&i) => adjacencies.lists[i] = list,
                    None => {
                        adjacencies.index.insert(n, adjacencies.lists.len());
                        adjacencies.lists.push(list);
                    }
                }
            }
        }
        let i = *adjacencies.index.get(&node).expect("Unexpectedly found nil while unwrapping an Optional value");
        adjacencies.lists[i].reset();
        i
    }

    fn count_crossings_by_merging_adjacency_lists(&mut self, upper_adj: &mut AdjacencyList, lower_adj: &mut AdjacencyList) {
        while !upper_adj.is_empty() && !lower_adj.is_empty() {
            if Self::is_below(upper_adj.first(), lower_adj.first()) {
                self.upper_lower_crossings += upper_adj.size();
                lower_adj.remove_first();
            } else if Self::is_below(lower_adj.first(), upper_adj.first()) {
                self.lower_upper_crossings += lower_adj.size();
                upper_adj.remove_first();
            } else {
                self.upper_lower_crossings += upper_adj.count_adjacencies_below_node_of_first_port();
                self.lower_upper_crossings += lower_adj.count_adjacencies_below_node_of_first_port();
                upper_adj.remove_first();
                lower_adj.remove_first();
            }
        }
    }

    fn is_below(first_port: i64, second_port: i64) -> bool {
        first_port > second_port
    }

    /// `getUpperLowerCrossings()`.
    pub fn get_upper_lower_crossings(&self) -> i64 {
        self.upper_lower_crossings
    }

    /// `getLowerUpperCrossings()`.
    pub fn get_lower_upper_crossings(&self) -> i64 {
        self.lower_upper_crossings
    }
}

fn two_mut<T>(v: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    assert!(a != b);
    if a < b {
        let (x, y) = v.split_at_mut(b);
        (&mut x[a], &mut y[0])
    } else {
        let (x, y) = v.split_at_mut(a);
        (&mut y[0], &mut x[b])
    }
}

// MARK: - AdjacencyList

#[derive(Clone, Debug)]
struct AdjacencyList {
    adjacency_list: Vec<Adjacency>,
    total_size: i64,
    current_size: i64,
    current_index: usize,
}

impl AdjacencyList {
    fn new(lg: &LGraphArena, node: LNodeId, side: PortSide, port_positions: &HashMap<LPortId, i64>) -> AdjacencyList {
        let mut list = AdjacencyList { adjacency_list: Vec::new(), total_size: 0, current_size: 0, current_index: 0 };
        list.get_adjacencies_sorted_by_position(lg, node, side, port_positions);
        list
    }

    fn get_adjacencies_sorted_by_position(&mut self, lg: &LGraphArena, node: LNodeId, side: PortSide, port_positions: &HashMap<LPortId, i64>) {
        self.iterate_through_edges_collecting_adjacencies(lg, node, side, port_positions);
        swift::sort_by(&mut self.adjacency_list, |a, b| a.position < b.position);
    }

    fn iterate_through_edges_collecting_adjacencies(&mut self, lg: &LGraphArena, node: LNodeId, side: PortSide, port_positions: &HashMap<LPortId, i64>) {
        for port in CrossMinUtil::in_north_south_east_west_order_iter(lg, node, side) {
            // `getEdgesConnectedTo(_:)`.
            let edges = if side == PortSide::WEST { &lg[port].incoming_edges } else { &lg[port].outgoing_edges };
            for &edge in edges {
                if !lg.edge_is_self_loop(edge) && Self::is_not_in_layer(lg, edge) {
                    self.add_adjacency_of(lg, edge, side, port_positions);
                    self.total_size += 1;
                    self.current_size += 1;
                }
            }
        }
    }

    /// `isNotInLayer(_:)`: the end layers differ (optional identity).
    fn is_not_in_layer(lg: &LGraphArena, edge: LEdgeId) -> bool {
        lg.edge_source_node(edge).and_then(|n| lg[n].layer) != lg.edge_target_node(edge).and_then(|n| lg[n].layer)
    }

    fn add_adjacency_of(&mut self, lg: &LGraphArena, edge: LEdgeId, side: PortSide, port_positions: &HashMap<LPortId, i64>) {
        // `adjacentPortOf(_:_:)`.
        let adjacent_port = if side == PortSide::WEST { lg[edge].source } else { lg[edge].target };
        let Some(adjacent_port) = adjacent_port else { return };
        let adjacent_port_position = port_positions.get(&adjacent_port).copied().unwrap_or(0);
        match self.adjacency_list.last_mut() {
            Some(last) if last.position == adjacent_port_position => {
                last.cardinality += 1;
                last.current_cardinality += 1;
            }
            _ => self.adjacency_list.push(Adjacency::new(adjacent_port_position)),
        }
    }

    fn reset(&mut self) {
        self.current_index = 0;
        self.current_size = self.total_size;
        if !self.is_empty() {
            self.current_adjacency().reset();
        }
    }

    fn count_adjacencies_below_node_of_first_port(&mut self) -> i64 {
        self.current_size - self.current_adjacency().current_cardinality
    }

    fn remove_first(&mut self) {
        if self.is_empty() {
            return;
        }
        let current_entry = self.current_adjacency();
        if current_entry.current_cardinality == 1 {
            self.increment_current_index();
        } else {
            current_entry.current_cardinality -= 1;
        }
        self.current_size -= 1;
    }

    fn increment_current_index(&mut self) {
        self.current_index += 1;
        if self.current_index < self.adjacency_list.len() {
            self.current_adjacency().reset();
        }
    }

    fn is_empty(&self) -> bool {
        self.current_size == 0
    }

    fn first(&mut self) -> i64 {
        self.current_adjacency().position
    }

    fn size(&self) -> i64 {
        self.current_size
    }

    /// `currentAdjacency()` (traps past the end).
    fn current_adjacency(&mut self) -> &mut Adjacency {
        &mut self.adjacency_list[self.current_index]
    }
}

// MARK: - Adjacency

#[derive(Clone, Debug)]
struct Adjacency {
    position: i64,
    cardinality: i64,
    current_cardinality: i64,
}

impl Adjacency {
    fn new(adjacent_port_position: i64) -> Adjacency {
        Adjacency { position: adjacent_port_position, cardinality: 1, current_cardinality: 1 }
    }

    fn reset(&mut self) {
        self.current_cardinality = self.cardinality;
    }
}
