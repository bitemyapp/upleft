//! Port of `alg/layered/graph/LEdge.swift`.

use super::l_graph::{LEdgeId, LGraphArena, LGraphId, LLabelId, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChain;
use crate::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;

#[derive(Clone, Debug)]
pub struct LEdgeData {
    pub props: PropertyMap,
    pub id: i32,
    pub bend_points: KVectorChain,
    pub source: Option<LPortId>,
    pub target: Option<LPortId>,
    pub labels: Vec<LLabelId>,
}

impl LGraphArena {
    /// `LEdge()`.
    pub fn new_edge(&mut self) -> LEdgeId {
        let id = LEdgeId(self.edges.len() as u32);
        self.edges.push(LEdgeData { props: PropertyMap::new(), id: 0, bend_points: KVectorChain::new(), source: None, target: None, labels: Vec::new() });
        id
    }

    /// `reverse(_:_:)`.
    pub fn edge_reverse(&mut self, edge: LEdgeId, layered_graph: LGraphId, adapt_ports: bool) {
        let (Some(old_source), Some(old_target)) = (self[edge].source, self[edge].target) else { return };
        self.edge_set_source(edge, None);
        self.edge_set_target(edge, None);

        let input_collect: Option<bool> = self[old_target].props.get_typed(&InternalProperties::INPUT_COLLECT);
        if adapt_ports && input_collect == Some(true) && self[old_target].owner.is_some() {
            let node = self[old_target].owner.unwrap();
            let port = self.provide_collector_port(layered_graph, node, PortType::OUTPUT, PortSide::EAST);
            self.edge_set_source(edge, Some(port));
        } else {
            self.edge_set_source(edge, Some(old_target));
        }

        let output_collect: Option<bool> = self[old_source].props.get_typed(&InternalProperties::OUTPUT_COLLECT);
        if adapt_ports && output_collect == Some(true) && self[old_source].owner.is_some() {
            let node = self[old_source].owner.unwrap();
            let port = self.provide_collector_port(layered_graph, node, PortType::INPUT, PortSide::WEST);
            self.edge_set_target(edge, Some(port));
        } else {
            self.edge_set_target(edge, Some(old_source));
        }

        for label in self[edge].labels.clone() {
            let placement: Option<EdgeLabelPlacement> = self[label].props.get_typed(&LayeredOptions::EDGE_LABELS_PLACEMENT);
            if let Some(placement) = placement {
                if placement == EdgeLabelPlacement::TAIL {
                    self[label].props.set(&LayeredOptions::EDGE_LABELS_PLACEMENT, EdgeLabelPlacement::HEAD);
                } else if placement == EdgeLabelPlacement::HEAD {
                    self[label].props.set(&LayeredOptions::EDGE_LABELS_PLACEMENT, EdgeLabelPlacement::TAIL);
                }
            }
        }

        let reversed: bool = self[edge].props.get_typed(&InternalProperties::REVERSED).unwrap_or(false);
        self[edge].props.set(&InternalProperties::REVERSED, !reversed);

        self[edge].bend_points = self[edge].bend_points.reversed();
    }

    /// `setSource(_:)`: moves the edge to the end of the new source's outgoing edges.
    pub fn edge_set_source(&mut self, edge: LEdgeId, source: Option<LPortId>) {
        if let Some(old) = self[edge].source {
            self[old].outgoing_edges.retain(|&e| e != edge);
        }
        self[edge].source = source;
        if let Some(new) = source {
            self[new].outgoing_edges.push(edge);
        }
    }

    /// `setTarget(_:)`: moves the edge to the end of the new target's incoming edges.
    pub fn edge_set_target(&mut self, edge: LEdgeId, target: Option<LPortId>) {
        if let Some(old) = self[edge].target {
            self[old].incoming_edges.retain(|&e| e != edge);
        }
        self[edge].target = target;
        if let Some(new) = target {
            self[new].incoming_edges.push(edge);
        }
    }

    /// `setTargetAndInsertAtIndex(_:_:)`.
    pub fn edge_set_target_and_insert_at_index(&mut self, edge: LEdgeId, target: Option<LPortId>, index: usize) {
        if let Some(old) = self[edge].target {
            self[old].incoming_edges.retain(|&e| e != edge);
        }
        self[edge].target = target;
        if let Some(new) = target {
            self[new].incoming_edges.insert(index, edge);
        }
    }

    /// `isSelfLoop()`.
    pub fn edge_is_self_loop(&self, edge: LEdgeId) -> bool {
        let (Some(src), Some(tgt)) = (self[edge].source, self[edge].target) else { return false };
        self[src].owner.is_some() && self[src].owner == self[tgt].owner
    }

    /// `isInLayerEdge()`: not a self loop, and both ends' layers are the same
    /// (including both missing, as in Swift's optional `===`).
    pub fn edge_is_in_layer_edge(&self, edge: LEdgeId) -> bool {
        let layer_of = |p: Option<LPortId>| p.and_then(|p| self[p].owner).and_then(|n| self[n].layer);
        !self.edge_is_self_loop(edge) && layer_of(self[edge].source) == layer_of(self[edge].target)
    }

    /// `getOther(_ port:)`.
    pub fn edge_other_port(&self, edge: LEdgeId, port: LPortId) -> LPortId {
        let e = &self[edge];
        if Some(port) == e.source {
            if let Some(t) = e.target {
                return t;
            }
        }
        if Some(port) == e.target {
            if let Some(s) = e.source {
                return s;
            }
        }
        port
    }

    /// `getOther(_ node:)`.
    pub fn edge_other_node(&self, edge: LEdgeId, node: LNodeId) -> LNodeId {
        let e = &self[edge];
        let src_node = e.source.and_then(|p| self[p].owner);
        let tgt_node = e.target.and_then(|p| self[p].owner);
        if Some(node) == src_node {
            if let Some(t) = tgt_node {
                return t;
            }
        }
        if Some(node) == tgt_node {
            if let Some(s) = src_node {
                return s;
            }
        }
        node
    }

    /// The source port's owner.
    pub fn edge_source_node(&self, edge: LEdgeId) -> Option<LNodeId> {
        self[edge].source.and_then(|p| self[p].owner)
    }

    /// The target port's owner.
    pub fn edge_target_node(&self, edge: LEdgeId) -> Option<LNodeId> {
        self[edge].target.and_then(|p| self[p].owner)
    }
}
