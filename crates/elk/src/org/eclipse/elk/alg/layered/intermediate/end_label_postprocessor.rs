//! Port of `alg/layered/intermediate/EndLabelPostprocessor.swift`.
//!
//! After the `EndLabelPreprocessor` has done all the major work, each node may
//! have a list of label cells full of edge end labels associated with it,
//! along with proper label cell coordinates. This processor offsets them by
//! the node position and places the labels.
//!
//! Swift iterates the `[LPort: LabelCell]` values in hash order; each cell
//! moves its own rectangle and positions its own labels (no label is in two
//! cells of one node), so the order is unobservable. The port uses port order.

use super::end_label_preprocessor::EndLabelCells;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Default)]
pub struct EndLabelPostprocessor;

impl EndLabelPostprocessor {
    pub fn new() -> EndLabelPostprocessor {
        EndLabelPostprocessor
    }

    pub fn process_node(&self, lg: &mut LGraphArena, node: LNodeId) {
        // The node should have a non-empty list of label cells, or something went TERRIBLY WRONG!!!
        let Some(end_label_cells) = lg[node].props.get_object::<EndLabelCells>(&InternalProperties::END_LABELS) else { return };
        if end_label_cells.is_empty() {
            return;
        }

        let node_pos = lg[node].position;

        for label_cell in end_label_cells.values() {
            label_cell.borrow_mut().cell.cell_rectangle.move_by(node_pos);

            label_cell.borrow().apply_label_layout(lg);
        }

        // Remove label cells
        lg[node].props.set_opt(&InternalProperties::END_LABELS, None);
    }
}

impl ILayoutProcessor for EndLabelPostprocessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("End label post-processing", 1.0);

        // We iterate over each node's label cells and offset and place them
        let nodes: Vec<LNodeId> = lg[layered_graph]
            .layers
            .iter()
            .flat_map(|&l| lg[l].nodes.iter().copied())
            .filter(|&node| {
                (lg[node].node_type == NodeType::NORMAL || lg[node].node_type == NodeType::EXTERNAL_PORT)
                    && lg[node].props.has(&InternalProperties::END_LABELS)
            })
            .collect();
        for node in nodes {
            self.process_node(lg, node);
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "EndLabelPostprocessor"
    }
}
