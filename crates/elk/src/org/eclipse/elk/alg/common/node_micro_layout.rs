//! Port of `alg/common/NodeMicroLayout.swift`.
//!
//! Utility class to execute "node micro layout" - automatically computing
//! node dimensions, positioning ports, positioning labels, etc. Not used by
//! elk-swift's layered pipeline; `forGraph(_ elkGraph: ElkNode)` always gives
//! `nil` because `ElkGraphAdapters.adapt` does.

use crate::bridge::elk_graph_impl::ElkNodeId;
use crate::org::eclipse::elk::alg::common::nodespacing::node_dimension_calculation::NodeDimensionCalculation;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LGraphAdapter;
use crate::org::eclipse::elk::core::util::adapters::elk_graph_adapters::ElkGraphAdapters;
use crate::prelude::*;

pub struct NodeMicroLayout {
    pub adapter: LGraphAdapter,
}

impl NodeMicroLayout {
    /// `forGraph(_ elkGraph: ElkNode)`: `nil` (see the module docs).
    pub fn for_elk_graph(elk_graph: ElkNodeId) -> Option<NodeMicroLayout> {
        match ElkGraphAdapters::adapt(elk_graph) {
            None => None,
            Some(never) => match never {},
        }
    }

    /// `forGraph(_ adapter:)`.
    pub fn for_graph(adapter: LGraphAdapter) -> NodeMicroLayout {
        NodeMicroLayout { adapter }
    }

    /// Perform the actual layout.
    pub fn execute(&self, lg: &mut LGraphArena) {
        NodeDimensionCalculation::sort_port_lists(lg, &self.adapter);
        NodeDimensionCalculation::calculate_label_and_node_sizes(lg, &self.adapter);
        NodeDimensionCalculation::calculate_node_margins(lg, &self.adapter);
    }
}
