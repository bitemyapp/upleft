//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/counting/org_eclipse_elk_alg_layered_p3order_counting_AllCrossingsCounter.swift`.
//!
//! Counts all crossings of a node order: in-layer crossings on the outer
//! sides, between-layer crossings (with the hyperedge counter where ports
//! carry several edges) and north/south port crossings.
//!
//! Unlike Java, the two counters built in `initAfterTraversal` do not share a
//! position array: Swift passes the `[Int]` by value to both.

use super::crossings_counter::CrossingsCounter;
use super::hyperedge_crossings_counter::HyperedgeCrossingsCounter;
use super::i_initializable::IInitializable;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::core::options::port_side::PortSide;

#[derive(Clone, Debug)]
pub struct AllCrossingsCounter {
    pub crossing_counter: Option<CrossingsCounter>,
    pub has_hyper_edges_east_of_index: Vec<bool>,
    pub hyperedge_crossings_counter: Option<HyperedgeCrossingsCounter>,
    pub in_layer_edge_counts: Vec<i64>,
    pub has_north_south_ports: Vec<bool>,
    pub n_ports: i64,
}

impl IInitializable for AllCrossingsCounter {}

impl AllCrossingsCounter {
    pub fn new(graph: &[Vec<LNodeId>]) -> AllCrossingsCounter {
        AllCrossingsCounter {
            crossing_counter: None,
            has_hyper_edges_east_of_index: vec![false; graph.len()],
            hyperedge_crossings_counter: None,
            in_layer_edge_counts: vec![0; graph.len()],
            has_north_south_ports: vec![false; graph.len()],
            n_ports: 0,
        }
    }

    /// `countAllCrossings(_:)`.
    pub fn count_all_crossings(&mut self, lg: &LGraphArena, current_order: &[Vec<LNodeId>]) -> i64 {
        if current_order.is_empty() {
            return 0;
        }
        let Some(crossing_counter) = self.crossing_counter.as_mut() else { return 0 };

        let mut crossings = crossing_counter.count_in_layer_crossings_on_side(lg, &current_order[0], PortSide::WEST);
        let east_cross = crossing_counter.count_in_layer_crossings_on_side(lg, &current_order[current_order.len() - 1], PortSide::EAST);
        crossings += east_cross;

        for layer_index in 0..current_order.len() {
            let layer_cross = self.count_crossings_at(lg, layer_index, current_order);
            crossings += layer_cross;
        }

        crossings
    }

    /// `countCrossingsAt(_:_:)`.
    pub fn count_crossings_at(&mut self, lg: &LGraphArena, layer_index: usize, current_order: &[Vec<LNodeId>]) -> i64 {
        let mut total_crossings = 0;
        let left_layer = &current_order[layer_index];

        if (layer_index as i64) < current_order.len() as i64 - 1 {
            let right_layer = &current_order[layer_index + 1];
            if self.has_hyper_edges_east_of_index[layer_index] {
                total_crossings = match self.hyperedge_crossings_counter.as_mut() {
                    Some(h) => h.count_crossings(lg, left_layer, right_layer),
                    None => 0,
                };
                if let Some(crossing_counter) = self.crossing_counter.as_mut() {
                    total_crossings += crossing_counter.count_in_layer_crossings_on_side(lg, left_layer, PortSide::EAST);
                    total_crossings += crossing_counter.count_in_layer_crossings_on_side(lg, right_layer, PortSide::WEST);
                }
            } else {
                total_crossings = match self.crossing_counter.as_mut() {
                    Some(c) => c.count_crossings_between_layers(lg, left_layer, right_layer),
                    None => 0,
                };
            }
        }

        if self.has_north_south_ports[layer_index] {
            total_crossings += match self.crossing_counter.as_mut() {
                Some(c) => c.count_north_south_port_crossings_in_layer(lg, left_layer),
                None => 0,
            };
        }

        total_crossings
    }

    /// `initAtNodeLevel(_:_:_:)`.
    pub fn init_at_node_level(&mut self, lg: &LGraphArena, l: usize, n: usize, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() || n >= node_order[l].len() {
            return;
        }
        if lg[node_order[l][n]].node_type == NodeType::NORTH_SOUTH_PORT {
            self.has_north_south_ports[l] = true;
        }
    }

    /// `initAtPortLevel(_:_:_:_:)`: numbers the port.
    pub fn init_at_port_level(&mut self, lg: &mut LGraphArena, l: usize, n: usize, p: usize, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() || n >= node_order[l].len() {
            return;
        }
        let node = node_order[l][n];
        let Some(&port) = lg[node].ports.get(p) else { return };

        lg[port].id = self.n_ports as i32;
        self.n_ports += 1;

        if lg[port].outgoing_edges.len() + lg[port].incoming_edges.len() > 1 {
            if lg[port].side == PortSide::EAST {
                self.has_hyper_edges_east_of_index[l] = true;
            } else if lg[port].side == PortSide::WEST && l > 0 {
                self.has_hyper_edges_east_of_index[l - 1] = true;
            }
        }
    }

    /// `initAtEdgeLevel(_:_:_:_:_:_:)`.
    pub fn init_at_edge_level(&mut self, lg: &LGraphArena, l: usize, n: usize, p: usize, _e: usize, edge: LEdgeId, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() || n >= node_order[l].len() {
            return;
        }
        let node = node_order[l][n];
        let Some(&port) = lg[node].ports.get(p) else { return };

        if lg[edge].source == Some(port) {
            let source_layer = lg.edge_source_node(edge).and_then(|n| lg[n].layer);
            let target_layer = lg.edge_target_node(edge).and_then(|n| lg[n].layer);
            if let (Some(s), Some(t)) = (source_layer, target_layer) {
                if s == t {
                    self.in_layer_edge_counts[l] += 1;
                }
            }
        }
    }

    /// `initAfterTraversal()`.
    pub fn init_after_traversal(&mut self) {
        let port_pos = vec![0i64; self.n_ports as usize];
        self.hyperedge_crossings_counter =
            Some(HyperedgeCrossingsCounter::new(self.in_layer_edge_counts.clone(), self.has_north_south_ports.clone(), port_pos.clone()));
        self.crossing_counter = Some(CrossingsCounter::from_values(port_pos));
    }
}
