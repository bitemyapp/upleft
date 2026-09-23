//! Port of `alg/layered/intermediate/ConstraintsPostprocessor.swift`.

use crate::prelude::*;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;

#[derive(Default)]
pub struct ConstraintsPostprocessor;

impl ConstraintsPostprocessor {
    pub fn new() -> ConstraintsPostprocessor {
        ConstraintsPostprocessor
    }
}

impl ILayoutProcessor for ConstraintsPostprocessor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Constraints Postprocessor", 1.0);
        let mut layer_index: i64 = 0;
        for layer in lg[graph].layers.clone() {
            let mut pos_index: i64 = 0;
            let mut node_layer = false;
            for node in lg[layer].nodes.clone() {
                if lg[node].node_type == NodeType::NORMAL {
                    node_layer = true;
                    lg[node].props.set(&LayeredOptions::LAYERING_LAYER_ID, layer_index);
                    lg[node].props.set(&LayeredOptions::CROSSING_MINIMIZATION_POSITION_ID, pos_index);
                    pos_index += 1;
                }
            }
            if node_layer {
                layer_index += 1;
            }
        }
        monitor.done();
    }

    fn name(&self) -> &'static str {
        "ConstraintsPostprocessor"
    }
}
