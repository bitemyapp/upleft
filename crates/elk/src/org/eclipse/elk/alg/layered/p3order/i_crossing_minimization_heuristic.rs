//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_ICrossingMinimizationHeuristic.swift`.
//!
//! Heuristics only read the layered graph while sweeping. The greedy-switch
//! heuristic reads its owning `GraphInfoHolder` (and that holder's parent);
//! the holder cannot lend itself to a heuristic it owns, so every call gets a
//! [`GraphDataView`] of it.

use super::counting::i_initializable::IInitializable;
use super::graph_info_holder::GraphDataView;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};

pub trait ICrossingMinimizationHeuristic: IInitializable {
    /// `alwaysImproves()`.
    fn always_improves(&self) -> bool;

    /// `setFirstLayerOrder(_:_:)`.
    fn set_first_layer_order(&mut self, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, forward_sweep: bool, graph_data: &GraphDataView) -> bool;

    /// `minimizeCrossings(_:_:_:_:)`.
    fn minimize_crossings(
        &mut self,
        lg: &LGraphArena,
        order: &mut Vec<Vec<LNodeId>>,
        free_layer_index: i64,
        forward_sweep: bool,
        is_first_sweep: bool,
        graph_data: &GraphDataView,
    ) -> bool;

    /// `isDeterministic()`.
    fn is_deterministic(&self) -> bool;
}
