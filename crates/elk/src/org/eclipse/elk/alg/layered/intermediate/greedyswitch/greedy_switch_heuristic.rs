//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/greedyswitch/org_eclipse_elk_alg_layered_intermediate_greedyswitch_GreedySwitchHeuristic.swift`.
//!
//! Switches neighbouring nodes of the free layer while that reduces
//! crossings. The Swift keeps a reference to its `GraphInfoHolder` (which owns
//! this heuristic); the port receives a [`GraphDataView`] of it on each call.

use super::crossing_matrix_filler::CrossingMatrixFiller;
use super::switch_decider::{CrossingCountSide, SwitchDecider};
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};
use crate::org::eclipse::elk::alg::layered::p3order::counting::i_initializable::IInitializable;
use crate::org::eclipse::elk::alg::layered::p3order::counting::shared_int_array::SharedIntArray;
use crate::org::eclipse::elk::alg::layered::p3order::graph_info_holder::GraphDataView;
use crate::org::eclipse::elk::alg::layered::p3order::i_crossing_minimization_heuristic::ICrossingMinimizationHeuristic;
use crate::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::CrossMinType;

#[derive(Clone, Debug)]
pub struct GreedySwitchHeuristic {
    greedy_switch_type: CrossMinType,
    current_node_order: Vec<Vec<LNodeId>>,
    switch_decider: Option<SwitchDecider>,
    port_positions: SharedIntArray,
    n_ports: i64,
}

impl IInitializable for GreedySwitchHeuristic {}

impl GreedySwitchHeuristic {
    pub fn new(greedy_type: CrossMinType) -> GreedySwitchHeuristic {
        GreedySwitchHeuristic {
            greedy_switch_type: greedy_type,
            current_node_order: Vec::new(),
            switch_decider: None,
            port_positions: SharedIntArray::new(),
            n_ports: 0,
        }
    }

    /// `setUp(_:_:_:)`: takes over the order (the caller gets it back when
    /// the call ends, as the Swift writes `currentNodeOrder` back).
    fn set_up(&mut self, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, free_layer_index: i64, forward_sweep: bool, graph_data: &GraphDataView) {
        self.current_node_order = std::mem::take(order);
        let side = if forward_sweep { CrossingCountSide::WEST } else { CrossingCountSide::EAST };
        self.switch_decider = Some(self.get_new_switch_decider(lg, free_layer_index, side, graph_data));
    }

    fn get_new_switch_decider(&self, lg: &LGraphArena, free_layer_index: i64, side: CrossingCountSide, graph_data: &GraphDataView) -> SwitchDecider {
        let crossing_matrix_filler = CrossingMatrixFiller::new(lg, self.greedy_switch_type, &self.current_node_order, free_layer_index, side);
        SwitchDecider::new(
            lg,
            free_layer_index,
            &self.current_node_order,
            crossing_matrix_filler,
            self.port_positions.clone(),
            graph_data,
            self.greedy_switch_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH,
        )
    }

    fn continue_switching_until_no_improvement_in_layer(&mut self, lg: &LGraphArena, free_layer_index: i64) -> bool {
        let mut improved = false;
        loop {
            let continue_switching = self.sweep_downward_in_layer(lg, free_layer_index);
            improved = improved || continue_switching;
            if !continue_switching {
                break;
            }
        }
        improved
    }

    fn sweep_downward_in_layer(&mut self, lg: &LGraphArena, layer_index: i64) -> bool {
        let mut continue_switching = false;
        let length_of_free_layer = self.current_node_order[layer_index as usize].len() as i64;
        // `0..<(length - 1)` traps for an empty layer.
        assert!(length_of_free_layer >= 1, "Range requires lowerBound <= upperBound");
        for upper_node_index in 0..(length_of_free_layer - 1) as usize {
            let lower_node_index = upper_node_index + 1;
            continue_switching = self.switch_if_improves(lg, layer_index as usize, upper_node_index, lower_node_index) || continue_switching;
        }
        continue_switching
    }

    fn switch_if_improves(&mut self, lg: &LGraphArena, layer_index: usize, upper_node_index: usize, lower_node_index: usize) -> bool {
        let Some(switch_decider) = self.switch_decider.as_mut() else { return false };
        if switch_decider.does_switch_reduce_crossings(lg, upper_node_index, lower_node_index) {
            self.exchange_nodes(lg, upper_node_index, lower_node_index, layer_index);
            return true;
        }
        false
    }

    fn exchange_nodes(&mut self, lg: &LGraphArena, index_one: usize, index_two: usize, layer_index: usize) {
        let upper = self.current_node_order[layer_index][index_one];
        let lower = self.current_node_order[layer_index][index_two];
        if let Some(switch_decider) = self.switch_decider.as_mut() {
            switch_decider.notify_of_switch(lg, upper, lower);
        }
        self.current_node_order[layer_index].swap(index_one, index_two);
    }

    fn start_index(is_forward_sweep: bool, length: usize) -> i64 {
        if is_forward_sweep { 0 } else { length as i64 - 1 }
    }

    /// `initAtPortLevel(_:_:_:_:)`.
    pub fn init_at_port_level(&mut self, _l: usize, _n: usize, _p: usize, _node_order: &[Vec<LNodeId>]) {
        self.n_ports += 1;
    }

    /// `initAtLayerLevel(_:_:)`: numbers the layer (traps for an empty one).
    pub fn init_at_layer_level(&mut self, lg: &mut LGraphArena, l: usize, node_order: &[Vec<LNodeId>]) {
        if let Some(layer) = lg[node_order[l][0]].layer {
            lg[layer].id = l as i32;
        }
    }

    /// `initAfterTraversal()`.
    pub fn init_after_traversal(&mut self) {
        self.port_positions = SharedIntArray::repeating(0, self.n_ports as usize);
    }
}

impl ICrossingMinimizationHeuristic for GreedySwitchHeuristic {
    fn minimize_crossings(&mut self, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, free_layer_index: i64, forward_sweep: bool, _is_first_sweep: bool, graph_data: &GraphDataView) -> bool {
        self.set_up(lg, order, free_layer_index, forward_sweep, graph_data);
        let result = self.continue_switching_until_no_improvement_in_layer(lg, free_layer_index);
        // Write back: in Java, currentNodeOrder = order is a reference copy,
        // but in Swift arrays are value types so swaps in exchangeNodes only
        // modify self.currentNodeOrder. Propagate changes back to the caller.
        *order = std::mem::take(&mut self.current_node_order);
        result
    }

    fn set_first_layer_order(&mut self, lg: &LGraphArena, current_order: &mut Vec<Vec<LNodeId>>, is_forward_sweep: bool, graph_data: &GraphDataView) -> bool {
        let start_index = Self::start_index(is_forward_sweep, current_order.len());
        self.set_up(lg, current_order, start_index, is_forward_sweep, graph_data);
        let result = self.sweep_downward_in_layer(lg, start_index);
        *current_order = std::mem::take(&mut self.current_node_order);
        result
    }

    fn always_improves(&self) -> bool {
        !(self.greedy_switch_type == CrossMinType::ONE_SIDED_GREEDY_SWITCH)
    }

    fn is_deterministic(&self) -> bool {
        true
    }
}
