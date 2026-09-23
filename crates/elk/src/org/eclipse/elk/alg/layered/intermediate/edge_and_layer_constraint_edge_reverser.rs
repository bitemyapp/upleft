//! Port of `alg/layered/intermediate/EdgeAndLayerConstraintEdgeReverser.swift`.
//!
//! Makes sure nodes with edge or layer constraints have only incoming or only
//! outgoing edges, as appropriate, and reverses the edges of feedback nodes
//! (fixed port sides with every port pointing the wrong way).
//!
//! Precondition: an unlayered graph. Slot: before phase 1.

use crate::org::eclipse::elk::alg::layered::options::edge_constraint::EdgeConstraint;
use crate::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Default)]
pub struct EdgeAndLayerConstraintEdgeReverser;

fn layer_constraint_of(lg: &LGraphArena, node: LNodeId) -> LayerConstraint {
    lg[node].props.get_as::<LayerConstraint>(&LayeredOptions::LAYERING_LAYER_CONSTRAINT).unwrap_or(LayerConstraint::NONE)
}

fn edge_constraint_for(layer_constraint: LayerConstraint) -> Option<EdgeConstraint> {
    match layer_constraint {
        LayerConstraint::FIRST | LayerConstraint::FIRST_SEPARATE => Some(EdgeConstraint::OUTGOING_ONLY),
        LayerConstraint::LAST | LayerConstraint::LAST_SEPARATE => Some(EdgeConstraint::INCOMING_ONLY),
        _ => None,
    }
}

impl EdgeAndLayerConstraintEdgeReverser {
    pub fn new() -> EdgeAndLayerConstraintEdgeReverser {
        EdgeAndLayerConstraintEdgeReverser
    }

    /// `handleOuterNodes(_:)`: nodes with a FIRST/LAST(_SEPARATE) constraint;
    /// returns the other nodes.
    pub fn handle_outer_nodes(&self, lg: &mut LGraphArena, layered_graph: LGraphId) -> Vec<LNodeId> {
        let nodes = lg[layered_graph].layerless_nodes.clone();
        let mut remaining_nodes = Vec::with_capacity(nodes.len());

        for node in nodes {
            let layer_constraint = layer_constraint_of(lg, node);
            if let Some(edge_constraint) = edge_constraint_for(layer_constraint) {
                lg[node].props.set(&InternalProperties::EDGE_CONSTRAINT, edge_constraint);
                if edge_constraint == EdgeConstraint::INCOMING_ONLY {
                    self.reverse_edges(lg, layered_graph, node, layer_constraint, PortType::INPUT);
                } else if edge_constraint == EdgeConstraint::OUTGOING_ONLY {
                    self.reverse_edges(lg, layered_graph, node, layer_constraint, PortType::OUTPUT);
                }
            } else {
                remaining_nodes.push(node);
            }
        }
        remaining_nodes
    }

    /// `handleInnerNodes(_:_:)`.
    pub fn handle_inner_nodes(&self, lg: &mut LGraphArena, layered_graph: LGraphId, remaining_nodes: &[LNodeId]) {
        for &node in remaining_nodes {
            let layer_constraint = layer_constraint_of(lg, node);
            if let Some(edge_constraint) = edge_constraint_for(layer_constraint) {
                lg[node].props.set(&InternalProperties::EDGE_CONSTRAINT, edge_constraint);
                if edge_constraint == EdgeConstraint::INCOMING_ONLY {
                    self.reverse_edges(lg, layered_graph, node, layer_constraint, PortType::INPUT);
                } else if edge_constraint == EdgeConstraint::OUTGOING_ONLY {
                    self.reverse_edges(lg, layered_graph, node, layer_constraint, PortType::OUTPUT);
                }
            } else {
                // If the port sides are fixed, but all ports are reversed, that probably means that we
                // have a feedback node.
                let port_constraints = lg[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED);
                if port_constraints.is_side_fixed() && !lg[node].ports.is_empty() {
                    let mut all_ports_reversed = true;

                    for &port in &lg[node].ports {
                        let side = lg[port].side;
                        let net_flow = lg.port_net_flow(port);
                        if !(side == PortSide::EAST && net_flow > 0 || side == PortSide::WEST && net_flow < 0) {
                            all_ports_reversed = false;
                            break;
                        }

                        // no LAST or LAST_SEPARATE allowed for the target of outgoing edges
                        for &edge in &lg[port].outgoing_edges {
                            let lc = lg.edge_target_node(edge).map_or(LayerConstraint::NONE, |n| layer_constraint_of(lg, n));
                            if lc == LayerConstraint::LAST || lc == LayerConstraint::LAST_SEPARATE {
                                all_ports_reversed = false;
                                break;
                            }
                        }
                        // no FIRST or FIRST_SEPARATE allowed for the source of incoming edges
                        for &edge in &lg[port].incoming_edges {
                            let lc = lg.edge_source_node(edge).map_or(LayerConstraint::NONE, |n| layer_constraint_of(lg, n));
                            if lc == LayerConstraint::FIRST || lc == LayerConstraint::FIRST_SEPARATE {
                                all_ports_reversed = false;
                                break;
                            }
                        }
                    }

                    if all_ports_reversed {
                        self.reverse_edges(lg, layered_graph, node, layer_constraint, PortType::UNDEFINED);
                    }
                }
            }
        }
    }

    /// `reverseEdges(_:node:nodeLayerConstraint:targetPortType:)`.
    pub fn reverse_edges(&self, lg: &mut LGraphArena, layered_graph: LGraphId, node: LNodeId, node_layer_constraint: LayerConstraint, target_port_type: PortType) {
        for port in lg[node].ports.clone() {
            // Only incoming edges
            if target_port_type == PortType::INPUT || target_port_type == PortType::UNDEFINED {
                for edge in lg[port].outgoing_edges.clone() {
                    if self.can_reverse_outgoing_edge(lg, node_layer_constraint, edge) {
                        lg.edge_reverse(edge, layered_graph, true);
                    }
                }
            }

            // Only outgoing edges
            if target_port_type == PortType::OUTPUT || target_port_type == PortType::UNDEFINED {
                for edge in lg[port].incoming_edges.clone() {
                    if self.can_reverse_incoming_edge(lg, node_layer_constraint, edge) {
                        lg.edge_reverse(edge, layered_graph, true);
                    }
                }
            }
        }
    }

    /// `canReverseOutgoingEdge(_:_:)`.
    pub fn can_reverse_outgoing_edge(&self, lg: &LGraphArena, source_node_layer_constraint: LayerConstraint, edge: LEdgeId) -> bool {
        if lg[edge].props.get_as::<bool>(&InternalProperties::REVERSED).unwrap_or(false) {
            return false;
        }
        let Some(target_node) = lg.edge_target_node(edge) else { return false };
        if source_node_layer_constraint == LayerConstraint::LAST && lg[target_node].node_type == NodeType::LABEL {
            return false;
        }
        if layer_constraint_of(lg, target_node) == LayerConstraint::LAST_SEPARATE {
            return false;
        }
        true
    }

    /// `canReverseIncomingEdge(_:_:)`.
    pub fn can_reverse_incoming_edge(&self, lg: &LGraphArena, target_node_layer_constraint: LayerConstraint, edge: LEdgeId) -> bool {
        if lg[edge].props.get_as::<bool>(&InternalProperties::REVERSED).unwrap_or(false) {
            return false;
        }
        let Some(source_node) = lg.edge_source_node(edge) else { return false };
        if target_node_layer_constraint == LayerConstraint::FIRST && lg[source_node].node_type == NodeType::LABEL {
            return false;
        }
        if layer_constraint_of(lg, source_node) == LayerConstraint::FIRST_SEPARATE {
            return false;
        }
        true
    }
}

impl ILayoutProcessor for EdgeAndLayerConstraintEdgeReverser {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Edge and layer constraint edge reversal", 1.0);
        let remaining_nodes = self.handle_outer_nodes(lg, layered_graph);
        self.handle_inner_nodes(lg, layered_graph, &remaining_nodes);
        monitor.done();
    }

    fn name(&self) -> &'static str {
        "EdgeAndLayerConstraintEdgeReverser"
    }
}
