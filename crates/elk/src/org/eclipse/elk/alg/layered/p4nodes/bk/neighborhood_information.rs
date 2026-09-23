//! Port of `alg/layered/p4nodes/bk/NeighborhoodInformation.swift`.
//!
//! Precomputed neighbourhood data for the Brandes & Köpf node placer.
//! Building it renumbers the graph: every layer's and node's `id` becomes its
//! running index, which the BK arrays are indexed by.

use crate::prelude::*;

/// A `Pair<LNode, LEdge>`: the neighbour and the edge leading to it.
pub type Neighbor = (LNodeId, LEdgeId);

#[derive(Clone, Debug, Default)]
pub struct NeighborhoodInformation {
    pub node_count: usize,
    /// Layer position, indexed by the layer's `id`.
    pub layer_index: Vec<i64>,
    /// Position within its layer, indexed by the node's `id`.
    pub node_index: Vec<i64>,
    pub left_neighbors: Vec<Vec<Neighbor>>,
    pub right_neighbors: Vec<Vec<Neighbor>>,
}

impl NeighborhoodInformation {
    pub fn new() -> NeighborhoodInformation {
        NeighborhoodInformation::default()
    }

    pub fn cleanup(&mut self) {
        self.layer_index = Vec::new();
        self.node_index = Vec::new();
        self.left_neighbors = Vec::new();
        self.right_neighbors = Vec::new();
    }

    /// `buildFor(_:)`.
    pub fn build_for(lg: &mut LGraphArena, graph: LGraphId) -> NeighborhoodInformation {
        let mut ni = NeighborhoodInformation::new();

        ni.node_count = 0;
        for &layer in &lg[graph].layers {
            ni.node_count += lg[layer].nodes.len();
        }

        let mut layer_id = 0i32;
        let mut layer_pos = 0i64;
        ni.layer_index = vec![0; lg[graph].layers.len()];

        let mut node_id = 0i32;
        ni.node_index = vec![0; ni.node_count];
        for li in 0..lg[graph].layers.len() {
            let layer = lg[graph].layers[li];
            lg[layer].id = layer_id;
            ni.layer_index[layer_id as usize] = layer_pos;
            layer_id += 1;
            layer_pos += 1;

            let mut node_pos = 0i64;
            for k in 0..lg[layer].nodes.len() {
                let node = lg[layer].nodes[k];
                lg[node].id = node_id;
                ni.node_index[node_id as usize] = node_pos;
                node_id += 1;
                node_pos += 1;
            }
        }

        ni.left_neighbors = vec![Vec::new(); ni.node_count];
        Self::determine_all_left_neighbors(&mut ni, lg, graph);
        ni.right_neighbors = vec![Vec::new(); ni.node_count];
        Self::determine_all_right_neighbors(&mut ni, lg, graph);

        ni
    }

    /// `determineAllRightNeighbors(_:_:)`.
    pub fn determine_all_right_neighbors(ni: &mut NeighborhoodInformation, lg: &LGraphArena, graph: LGraphId) {
        let mut result: Vec<Neighbor> = Vec::new();
        for &layer in &lg[graph].layers {
            for &node in &lg[layer].nodes {
                result.clear();
                let mut max_priority = 0i64;

                // `node.getOutgoingEdges()`: every port's outgoing edges in port order.
                for &port in &lg[node].ports {
                    for &edge in &lg[port].outgoing_edges {
                        if lg.edge_is_self_loop(edge) || lg.edge_is_in_layer_edge(edge) {
                            continue;
                        }

                        let edge_priority: i64 = lg[edge].props.get_typed::<i64>(&LayeredOptions::PRIORITY_STRAIGHTNESS).unwrap_or(0);

                        if edge_priority > max_priority {
                            max_priority = edge_priority;
                            result.clear();
                        }
                        if edge_priority == max_priority {
                            if let Some(target_node) = lg.edge_target_node(edge) {
                                result.push((target_node, edge));
                            }
                        }
                    }
                }

                ni.right_neighbors[lg[node].id as usize] = Self::sort_neighbors(lg, &result, &ni.node_index);
            }
        }
    }

    /// `determineAllLeftNeighbors(_:_:)`.
    pub fn determine_all_left_neighbors(ni: &mut NeighborhoodInformation, lg: &LGraphArena, graph: LGraphId) {
        let mut result: Vec<Neighbor> = Vec::new();
        for &layer in &lg[graph].layers {
            for &node in &lg[layer].nodes {
                result.clear();
                let mut max_priority = 0i64;

                for &port in &lg[node].ports {
                    for &edge in &lg[port].incoming_edges {
                        if lg.edge_is_self_loop(edge) || lg.edge_is_in_layer_edge(edge) {
                            continue;
                        }

                        let edge_priority: i64 = lg[edge].props.get_typed::<i64>(&LayeredOptions::PRIORITY_STRAIGHTNESS).unwrap_or(0);

                        if edge_priority > max_priority {
                            max_priority = edge_priority;
                            result.clear();
                        }
                        if edge_priority == max_priority {
                            if let Some(source_node) = lg.edge_source_node(edge) {
                                result.push((source_node, edge));
                            }
                        }
                    }
                }

                ni.left_neighbors[lg[node].id as usize] = Self::sort_neighbors(lg, &result, &ni.node_index);
            }
        }
    }

    /// `sortNeighbors(_:_:)`: by position in the layer, ties by list order.
    pub fn sort_neighbors(lg: &LGraphArena, list: &[Neighbor], node_index: &[i64]) -> Vec<Neighbor> {
        let indexed: Vec<(usize, Neighbor)> = list.iter().copied().enumerate().collect();
        swift::sorted_by(indexed, |lhs, rhs| {
            let lhs_pos = node_index[lg[lhs.1 .0].id as usize];
            let rhs_pos = node_index[lg[rhs.1 .0].id as usize];
            if lhs_pos == rhs_pos {
                return lhs.0 < rhs.0;
            }
            lhs_pos < rhs_pos
        })
        .into_iter()
        .map(|(_, e)| e)
        .collect()
    }
}
