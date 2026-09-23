//! Port of `alg/layered/intermediate/LayerSizeAndGraphHeightCalculator.swift`.
//!
//! Computes each layer's size and the graph's height (moving the graph's
//! offset so that the topmost node starts at 0).

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::elk_margin::ElkMarginRef;
use crate::prelude::*;

#[derive(Default)]
pub struct LayerSizeAndGraphHeightCalculator;

impl LayerSizeAndGraphHeightCalculator {
    pub fn new() -> LayerSizeAndGraphHeightCalculator {
        LayerSizeAndGraphHeightCalculator
    }
}

impl ILayoutProcessor for LayerSizeAndGraphHeightCalculator {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Layer size calculation", 1.0);

        let mut min_y = f64::INFINITY;
        let mut max_y = -f64::INFINITY;

        let mut found_nodes = false;
        for li in 0..lg[layered_graph].layers.len() {
            let layer = lg[layered_graph].layers[li];
            lg[layer].size.x = 0.0;
            lg[layer].size.y = 0.0;

            if lg[layer].nodes.is_empty() {
                continue;
            }

            found_nodes = true;

            let mut layer_size_x = 0.0;
            for &node in &lg[layer].nodes {
                let n = &lg[node];
                layer_size_x = swift::max(layer_size_x, n.size.x + n.margin.left + n.margin.right);
            }
            lg[layer].size.x = layer_size_x;

            let first_node = lg[layer].nodes[0];
            let mut top = lg[first_node].position.y - lg[first_node].margin.top;
            if lg[first_node].node_type == NodeType::EXTERNAL_PORT {
                if let Some(surrounding) = lg[layered_graph].props.get_as::<ElkMarginRef>(&LayeredOptions::SPACING_PORTS_SURROUNDING) {
                    top -= surrounding.borrow().top;
                }
            }
            let last_node = *lg[layer].nodes.last().unwrap();
            let mut bottom = lg[last_node].position.y + lg[last_node].size.y + lg[last_node].margin.bottom;
            if lg[last_node].node_type == NodeType::EXTERNAL_PORT {
                if let Some(surrounding) = lg[layered_graph].props.get_as::<ElkMarginRef>(&LayeredOptions::SPACING_PORTS_SURROUNDING) {
                    bottom += surrounding.borrow().bottom;
                }
            }
            lg[layer].size.y = bottom - top;

            min_y = swift::min(min_y, top);
            max_y = swift::max(max_y, bottom);
        }

        if !found_nodes {
            min_y = 0.0;
            max_y = 0.0;
        }

        lg[layered_graph].size.y = max_y - min_y;
        lg[layered_graph].offset.y -= min_y;

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LayerSizeAndGraphHeightCalculator"
    }
}
