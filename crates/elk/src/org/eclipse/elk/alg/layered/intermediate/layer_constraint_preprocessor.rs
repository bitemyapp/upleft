//! Port of `alg/layered/intermediate/LayerConstraintPreprocessor.swift`.
//!
//! Hides `FIRST_SEPARATE` and `LAST_SEPARATE` nodes from the layerer: they
//! are removed from the graph with their edges detached at the far end
//! (remembered in `ORIGINAL_OPPOSITE_PORT`), and restored by the
//! `LayerConstraintPostprocessor`. A node left without edges that was only
//! connected to one kind of separate node gets a `FIRST`/`LAST` constraint.

use std::rc::Rc;

use crate::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::graph::properties::keys;
use crate::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HiddenNodeConnections {
    NONE,
    FIRST_SEPARATE,
    LAST_SEPARATE,
    BOTH,
}

impl HiddenNodeConnections {
    fn combine(self, layer_constraint: LayerConstraint) -> HiddenNodeConnections {
        match self {
            HiddenNodeConnections::NONE => {
                if layer_constraint == LayerConstraint::FIRST_SEPARATE {
                    HiddenNodeConnections::FIRST_SEPARATE
                } else {
                    HiddenNodeConnections::LAST_SEPARATE
                }
            }
            HiddenNodeConnections::FIRST_SEPARATE => {
                if layer_constraint == LayerConstraint::FIRST_SEPARATE {
                    HiddenNodeConnections::FIRST_SEPARATE
                } else {
                    HiddenNodeConnections::BOTH
                }
            }
            HiddenNodeConnections::LAST_SEPARATE => {
                if layer_constraint == LayerConstraint::FIRST_SEPARATE {
                    HiddenNodeConnections::BOTH
                } else {
                    HiddenNodeConnections::LAST_SEPARATE
                }
            }
            HiddenNodeConnections::BOTH => HiddenNodeConnections::BOTH,
        }
    }
}

/// `HIDDEN_NODE_CONNECTIONS` (`Property<Any>("separateLayerConnections")`).
static HIDDEN_NODE_CONNECTIONS: Property = Property::new(keys::SEPARATE_LAYER_CONNECTIONS);

#[derive(Default)]
pub struct LayerConstraintPreprocessor;

fn layer_constraint_of(lg: &LGraphArena, node: LNodeId) -> LayerConstraint {
    lg[node].props.get_as::<LayerConstraint>(&LayeredOptions::LAYERING_LAYER_CONSTRAINT).unwrap_or(LayerConstraint::NONE)
}

impl LayerConstraintPreprocessor {
    pub fn new() -> LayerConstraintPreprocessor {
        LayerConstraintPreprocessor
    }

    fn is_relevant_node(lg: &LGraphArena, l_node: LNodeId) -> bool {
        let constraint = layer_constraint_of(lg, l_node);
        constraint == LayerConstraint::FIRST_SEPARATE || constraint == LayerConstraint::LAST_SEPARATE
    }

    fn hide(lg: &mut LGraphArena, l_node: LNodeId) {
        Self::ensure_no_inacceptable_edges(lg, l_node);
        for l_edge in lg.node_connected_edges(l_node) {
            Self::hide_edge(lg, l_node, l_edge);
        }
    }

    fn hide_edge(lg: &mut LGraphArena, l_node: LNodeId, l_edge: LEdgeId) {
        let is_outgoing = lg.edge_source_node(l_edge) == Some(l_node);
        let opposite = if is_outgoing { lg[l_edge].target } else { lg[l_edge].source };
        let Some(opposite_port) = opposite else { return };

        if is_outgoing {
            lg.edge_set_target(l_edge, None);
        } else {
            lg.edge_set_source(l_edge, None);
        }

        lg[l_edge].props.set(&InternalProperties::ORIGINAL_OPPOSITE_PORT, opposite_port);

        if let Some(opposite_node) = lg[opposite_port].owner {
            Self::update_opposite_node_layer_constraints(lg, l_node, opposite_node);
        }
    }

    fn update_opposite_node_layer_constraints(lg: &mut LGraphArena, hidden_node: LNodeId, opposite_node: LNodeId) {
        if lg[opposite_node].props.has(&LayeredOptions::LAYERING_LAYER_CONSTRAINT) {
            return;
        }

        let hidden_constraint = layer_constraint_of(lg, hidden_node);
        let current = lg[opposite_node]
            .props
            .get_object::<HiddenNodeConnections>(&HIDDEN_NODE_CONNECTIONS)
            .map_or(HiddenNodeConnections::NONE, |c| *c);
        let connections = current.combine(hidden_constraint);
        lg[opposite_node].props.set(&HIDDEN_NODE_CONNECTIONS, PropValue::object(Rc::new(connections)));

        if lg[opposite_node].ports.iter().any(|&p| !lg[p].incoming_edges.is_empty() || !lg[p].outgoing_edges.is_empty()) {
            return;
        }

        match connections {
            HiddenNodeConnections::FIRST_SEPARATE => {
                lg[opposite_node].props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::FIRST);
            }
            HiddenNodeConnections::LAST_SEPARATE => {
                lg[opposite_node].props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::LAST);
            }
            _ => {}
        }
    }

    /// `ensureNoInacceptableEdges(_:)`: Java throws on an unacceptable edge;
    /// elk-swift only returns, so this has no effect.
    fn ensure_no_inacceptable_edges(lg: &LGraphArena, l_node: LNodeId) {
        let layer_constraint = layer_constraint_of(lg, l_node);
        if layer_constraint == LayerConstraint::FIRST_SEPARATE {
            for in_edge in lg.node_incoming_edges(l_node) {
                if !Self::is_acceptable_incident_edge(lg, in_edge) {
                    return;
                }
            }
        } else if layer_constraint == LayerConstraint::LAST_SEPARATE {
            for out_edge in lg.node_outgoing_edges(l_node) {
                if !Self::is_acceptable_incident_edge(lg, out_edge) {
                    return;
                }
            }
        }
    }

    fn is_acceptable_incident_edge(lg: &LGraphArena, edge: LEdgeId) -> bool {
        let (Some(source_node), Some(target_node)) = (lg.edge_source_node(edge), lg.edge_target_node(edge)) else { return false };
        lg[source_node].node_type == NodeType::EXTERNAL_PORT && lg[target_node].node_type == NodeType::EXTERNAL_PORT
    }
}

impl ILayoutProcessor for LayerConstraintPreprocessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Layer constraint preprocessing", 1.0);

        let mut hidden_nodes: Vec<LNodeId> = Vec::new();

        let mut i = 0;
        while i < lg[layered_graph].layerless_nodes.len() {
            let l_node = lg[layered_graph].layerless_nodes[i];
            if Self::is_relevant_node(lg, l_node) {
                Self::hide(lg, l_node);
                hidden_nodes.push(l_node);
                lg[layered_graph].layerless_nodes.remove(i);
            } else {
                i += 1;
            }
        }

        if !hidden_nodes.is_empty() {
            lg[layered_graph].props.set(&InternalProperties::HIDDEN_NODES, hidden_nodes);
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LayerConstraintPreprocessor"
    }
}
