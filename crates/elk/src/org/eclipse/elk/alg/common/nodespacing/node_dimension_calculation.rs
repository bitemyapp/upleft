//! Port of `alg/common/nodespacing/NodeDimensionCalculation.swift`.
//!
//! Entry points to apply several methods for node dimension calculation,
//! including positioning of labels, ports, etc.

use super::node_label_and_size_calculator::NodeLabelAndSizeCalculator;
use super::node_margin_calculator::NodeMarginCalculator;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LGraphAdapter;
use crate::prelude::*;

pub struct NodeDimensionCalculation;

impl NodeDimensionCalculation {
    /// Calculates label sizes and node sizes also considering ports. Make sure
    /// that the port lists are sorted properly.
    pub fn calculate_label_and_node_sizes(lg: &mut LGraphArena, adapter: &LGraphAdapter) {
        NodeLabelAndSizeCalculator::process_graph(lg, adapter);
    }

    /// Calculates node margins for the nodes of the passed graph.
    pub fn calculate_node_margins(lg: &mut LGraphArena, adapter: &LGraphAdapter) {
        let calculator = NodeMarginCalculator::new(*adapter);
        calculator.process(lg);
    }

    /// Returns a configurable `NodeMarginCalculator` that can be executed
    /// using its `process` methods.
    pub fn get_node_margin_calculator(adapter: LGraphAdapter) -> NodeMarginCalculator {
        NodeMarginCalculator::new(adapter)
    }

    /// Sorts the port lists of all nodes of the graph clockwise (a no-op for
    /// the layered adapters).
    pub fn sort_port_lists(lg: &mut LGraphArena, adapter: &LGraphAdapter) {
        for node in adapter.get_nodes(lg) {
            node.sort_port_list(lg);
        }
    }
}
