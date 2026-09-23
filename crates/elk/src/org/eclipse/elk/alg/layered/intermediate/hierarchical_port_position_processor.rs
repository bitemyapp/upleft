//! Port of `alg/layered/intermediate/HierarchicalPortPositionProcessor.swift`.
//!
//! Sets the y coordinate of external node dummies representing eastern or
//! western hierarchical ports. Runs before phase 5.

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Default)]
pub struct HierarchicalPortPositionProcessor;

impl HierarchicalPortPositionProcessor {
    pub fn new() -> HierarchicalPortPositionProcessor {
        HierarchicalPortPositionProcessor
    }

    /// `fixCoordinates(_:_:)`: fixes the y coordinates of external port dummies in the given layer.
    fn fix_coordinates(lg: &mut LGraphArena, layer: LayerId, layered_graph: LGraphId) {
        let port_constraints = lg[layered_graph].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
        if !(port_constraints.is_ratio_fixed() || port_constraints.is_pos_fixed()) {
            // If coordinates are free to be set, we're done
            return;
        }

        let graph_height = lg.graph_actual_size(layered_graph).y;

        // Iterate over the layer's nodes
        for node in lg[layer].nodes.clone() {
            // We only care about external port dummies...
            if lg[node].node_type != NodeType::EXTERNAL_PORT {
                continue;
            }

            // ...representing eastern or western ports.
            let ext_port_side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE);
            if ext_port_side != Some(PortSide::EAST) && ext_port_side != Some(PortSide::WEST) {
                continue;
            }

            let mut final_y_coordinate = lg[node].props.get_as::<f64>(&InternalProperties::PORT_RATIO_OR_POSITION).unwrap_or(0.0);

            if port_constraints == PortConstraints::FIXED_RATIO {
                // finalYCoordinate is a ratio that must be multiplied with the graph's height
                final_y_coordinate *= graph_height;
            }

            // Apply the node's new Y coordinate (PORT_ANCHOR aliases the dummy port's position)
            let anchor_y = lg.node_port_anchor(node).map_or(0.0, |a| a.y);
            lg[node].position.y = final_y_coordinate - anchor_y;
            lg.node_border_to_content_area_coordinates(node, false, true);
        }
    }
}

impl ILayoutProcessor for HierarchicalPortPositionProcessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Hierarchical port position processing", 1.0);

        let layers = lg[layered_graph].layers.clone();

        // We're interested in EAST and WEST external port dummies only; since they can only be in
        // the first or last layer, only fix coordinates of nodes in those two layers
        if !layers.is_empty() {
            Self::fix_coordinates(lg, layers[0], layered_graph);
        }

        if layers.len() > 1 {
            Self::fix_coordinates(lg, layers[layers.len() - 1], layered_graph);
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "HierarchicalPortPositionProcessor"
    }
}
