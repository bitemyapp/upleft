//! Port of `alg/layered/intermediate/InnermostNodeMarginCalculator.swift`.
//!
//! Computes and sets the inner node margins: the space around a node that
//! must not overlap with other diagram elements. This processor only computes
//! the space required for ports and port labels (and node labels). The margins
//! are extended by `SelfLoopRouter`, `CommentNodeMarginCalculator`, and
//! `EndLabelPreprocessor`.

use crate::org::eclipse::elk::alg::common::nodespacing::node_dimension_calculation::NodeDimensionCalculation;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LGraphAdapters;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Default)]
pub struct InnermostNodeMarginCalculator;

impl InnermostNodeMarginCalculator {
    pub fn new() -> InnermostNodeMarginCalculator {
        InnermostNodeMarginCalculator
    }
}

impl ILayoutProcessor for InnermostNodeMarginCalculator {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Node margin calculation", 1.0);

        // Calculate the margins using ELK's utility methods
        NodeDimensionCalculation::get_node_margin_calculator(LGraphAdapters::adapt_ns(layered_graph, false))
            .exclude_edge_head_tail_labels()
            .process(lg);

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "InnermostNodeMarginCalculator"
    }
}
