//! Port of `alg/layered/intermediate/PortSideProcessor.swift`.
//!
//! Assigns port sides to ports whose side is not fixed yet: ports with more
//! outgoing than incoming edges go east, others west; ports represented by an
//! external port dummy take the dummy's side.

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Default)]
pub struct PortSideProcessor;

impl PortSideProcessor {
    pub fn new() -> PortSideProcessor {
        PortSideProcessor
    }

    fn process_node(lg: &mut LGraphArena, node: LNodeId) {
        let constraints = lg[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED);
        if constraints.is_side_fixed() {
            for port in lg[node].ports.clone() {
                if lg[port].side == PortSide::UNDEFINED {
                    Self::set_port_side(lg, port);
                }
            }
        } else {
            for port in lg[node].ports.clone() {
                Self::set_port_side(lg, port);
            }
            lg[node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_SIDE);
        }
    }

    /// `setPortSide(_:)` (assigns `port.side` directly, not through `setSide`).
    pub fn set_port_side(lg: &mut LGraphArena, port: LPortId) {
        let port_dummy: Option<LNodeId> = lg[port].props.get_typed::<LNodeId>(&InternalProperties::PORT_DUMMY);
        if let Some(port_dummy) = port_dummy {
            lg[port].side = lg[port_dummy].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE).unwrap_or(PortSide::UNDEFINED);
        } else if lg.port_net_flow(port) < 0 {
            lg[port].side = PortSide::EAST;
        } else {
            lg[port].side = PortSide::WEST;
        }
    }
}

impl ILayoutProcessor for PortSideProcessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Port side processing", 1.0);

        // IF USED BEFORE PHASE 1
        for node in lg[layered_graph].layerless_nodes.clone() {
            Self::process_node(lg, node);
        }

        // IF USED BEFORE PHASE 3
        for layer in lg[layered_graph].layers.clone() {
            for node in lg[layer].nodes.clone() {
                Self::process_node(lg, node);
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "PortSideProcessor"
    }
}
