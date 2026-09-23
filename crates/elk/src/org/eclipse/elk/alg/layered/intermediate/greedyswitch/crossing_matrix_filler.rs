//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/greedyswitch/org_eclipse_elk_alg_layered_intermediate_greedyswitch_CrossingMatrixFiller.swift`.
//!
//! Lazily fills the matrix of between-layer crossings of every pair of nodes
//! of the free layer, indexed by node id.

use super::between_layer_edge_two_node_crossings_counter::BetweenLayerEdgeTwoNodeCrossingsCounter;
use super::switch_decider::CrossingCountSide;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};
use crate::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::CrossMinType;

#[derive(Clone, Debug)]
pub struct CrossingMatrixFiller {
    is_crossing_matrix_filled: Vec<Vec<bool>>,
    crossing_matrix: Vec<Vec<i64>>,
    in_between_layer_crossing_counter: BetweenLayerEdgeTwoNodeCrossingsCounter,
    direction: CrossingCountSide,
    one_sided: bool,
}

impl CrossingMatrixFiller {
    pub fn new(lg: &LGraphArena, greedy_switch_type: CrossMinType, graph: &[Vec<LNodeId>], free_layer_index: i64, direction: CrossingCountSide) -> CrossingMatrixFiller {
        let one_sided = greedy_switch_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH;

        // Traps for an index outside the order, like the Swift subscript.
        let free_layer_length = graph[free_layer_index as usize].len();
        CrossingMatrixFiller {
            is_crossing_matrix_filled: vec![vec![false; free_layer_length]; free_layer_length],
            crossing_matrix: vec![vec![0; free_layer_length]; free_layer_length],
            in_between_layer_crossing_counter: BetweenLayerEdgeTwoNodeCrossingsCounter::new(lg, graph, free_layer_index),
            direction,
            one_sided,
        }
    }

    /// `getCrossingMatrixEntry(_:_:)`.
    pub fn get_crossing_matrix_entry(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) -> i64 {
        let (u, l) = (lg[upper_node].id as usize, lg[lower_node].id as usize);
        if !self.is_crossing_matrix_filled[u][l] {
            self.fill_crossing_matrix(lg, upper_node, lower_node);
            self.is_crossing_matrix_filled[u][l] = true;
            self.is_crossing_matrix_filled[l][u] = true;
        }
        self.crossing_matrix[u][l]
    }

    fn fill_crossing_matrix(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        if self.one_sided {
            match self.direction {
                CrossingCountSide::EAST => self.in_between_layer_crossing_counter.count_eastern_edge_crossings(lg, upper_node, lower_node),
                CrossingCountSide::WEST => self.in_between_layer_crossing_counter.count_western_edge_crossings(lg, upper_node, lower_node),
            }
        } else {
            self.in_between_layer_crossing_counter.count_both_side_crossings(lg, upper_node, lower_node);
        }
        let (u, l) = (lg[upper_node].id as usize, lg[lower_node].id as usize);
        self.crossing_matrix[u][l] = self.in_between_layer_crossing_counter.get_upper_lower_crossings();
        self.crossing_matrix[l][u] = self.in_between_layer_crossing_counter.get_lower_upper_crossings();
    }
}
