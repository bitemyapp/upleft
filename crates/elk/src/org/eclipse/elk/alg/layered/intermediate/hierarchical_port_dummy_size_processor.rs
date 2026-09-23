//! Port of `alg/layered/intermediate/HierarchicalPortDummySizeProcessor.swift`.
//!
//! Sets the width of hierarchical port dummies and sets the layer alignment of
//! North/South port dummies to Center. Runs before phase 4.

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::alignment::Alignment;
use crate::prelude::*;

#[derive(Default)]
pub struct HierarchicalPortDummySizeProcessor;

impl HierarchicalPortDummySizeProcessor {
    pub fn new() -> HierarchicalPortDummySizeProcessor {
        HierarchicalPortDummySizeProcessor
    }

    /// `setWidths(_:topDown:delta:)`: sets the widths of the given nodes and
    /// their layer alignment.
    fn set_widths(lg: &mut LGraphArena, nodes: &[LNodeId], top_down: bool, delta: f64) {
        let mut current_width = 0.0;
        let mut step = delta;

        if !top_down {
            // Start with the widest node, decreasing node size
            current_width = delta * (nodes.len() as i64 - 1) as f64;
            step *= -1.0;
        }

        for &node in nodes {
            lg[node].props.set(&LayeredOptions::ALIGNMENT, Alignment::CENTER);
            lg[node].size.x = current_width;

            // Move eastern ports to the node's right border. (An external
            // port dummy's `PORT_ANCHOR` is its port's position; the arena
            // keeps that alias, so nothing else needs updating.)
            for port in lg.node_ports_on_side(node, PortSide::EAST) {
                lg[port].position.x = current_width;
            }

            current_width += step;
        }
    }
}

impl ILayoutProcessor for HierarchicalPortDummySizeProcessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Hierarchical port dummy size processing", 1.0);

        let mut northern_dummies: Vec<LNodeId> = Vec::new();
        let mut southern_dummies: Vec<LNodeId> = Vec::new();

        // Calculate the width difference (this assumes CENTER node alignment).
        // Ports are stacked on top of each other at the center of the node.
        // By iteratively increasing their size by twice the edgeEdge spacing between layers,
        // vertical edge segments are spaced by that amount.
        let edge_spacing = lg[layered_graph].props.get_as::<f64>(&LayeredOptions::SPACING_EDGE_EDGE_BETWEEN_LAYERS).unwrap_or(0.0);
        let delta = edge_spacing * 2.0;

        // Iterate through the layers
        for layer in lg[layered_graph].layers.clone() {
            northern_dummies.clear();
            southern_dummies.clear();

            // Collect northern and southern hierarchical port dummies
            for &node in &lg[layer].nodes {
                if lg[node].node_type == NodeType::EXTERNAL_PORT {
                    let side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE);

                    if side == Some(PortSide::NORTH) {
                        northern_dummies.push(node);
                    } else if side == Some(PortSide::SOUTH) {
                        southern_dummies.push(node);
                    }
                }
            }

            // Set widths
            Self::set_widths(lg, &northern_dummies, true, delta);
            Self::set_widths(lg, &southern_dummies, false, delta);
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "HierarchicalPortDummySizeProcessor"
    }
}
