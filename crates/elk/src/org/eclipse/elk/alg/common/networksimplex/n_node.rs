//! Port of `alg/common/networksimplex/NNode.swift`.
//!
//! A node used by the network simplex algorithm. Nodes live in their
//! [`NGraph`]'s node store and are addressed by [`NNodeId`] (a Swift object
//! reference; `===` is `==`).
//!
//! Swift keeps the incoming and outgoing edges in `ChangeAwareArrayList`s and
//! caches their concatenation (`getConnectedEdges()`) by modification count.
//! Every mutation bumps the count, so the cache always equals
//! `incoming ++ outgoing`; the port computes that concatenation directly.

use super::n_edge::NEdgeId;
use super::n_graph::NGraph;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_element::LElement;

/// An `NNode` reference.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NNodeId(pub u32);

impl NNodeId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug)]
pub struct NNode {
    /// A package id, unused internally.
    pub id: i64,
    /// An object from which this node is derived (`Any?`).
    pub origin: Option<LElement>,
    /// Debug label, no semantic meaning.
    pub type_: String,
    /// The layer the node is currently assigned to.
    pub layer: i64,
    /// Internally set and used id to index arrays.
    pub internal_id: usize,
    /// Whether the node is part of the spanning tree determined by `tightTree()`.
    pub tree_node: bool,
    /// Edges whose cut values are still unknown.
    pub unknown_cutvalues: Vec<NEdgeId>,
    pub outgoing_edges: Vec<NEdgeId>,
    pub incoming_edges: Vec<NEdgeId>,
}

impl Default for NNode {
    fn default() -> Self {
        NNode {
            id: 0,
            origin: None,
            type_: String::new(),
            layer: 0,
            internal_id: 0,
            tree_node: false,
            unknown_cutvalues: Vec::new(),
            outgoing_edges: Vec::new(),
            incoming_edges: Vec::new(),
        }
    }
}

impl NNode {
    /// `NNode.of()`.
    pub fn of() -> NNodeBuilder {
        NNodeBuilder { node: NNode::default() }
    }

    /// `getOutgoingEdges()`.
    pub fn get_outgoing_edges(&self) -> &[NEdgeId] {
        &self.outgoing_edges
    }

    /// `getIncomingEdges()`.
    pub fn get_incoming_edges(&self) -> &[NEdgeId] {
        &self.incoming_edges
    }

    /// `getConnectedEdges()` / `connectedEdges`: incoming, then outgoing.
    pub fn get_connected_edges(&self) -> Vec<NEdgeId> {
        let mut all = Vec::with_capacity(self.incoming_edges.len() + self.outgoing_edges.len());
        all.extend_from_slice(&self.incoming_edges);
        all.extend_from_slice(&self.outgoing_edges);
        all
    }

    /// `connectedEdges.count`.
    #[inline]
    pub fn connected_edge_count(&self) -> usize {
        self.incoming_edges.len() + self.outgoing_edges.len()
    }

    /// `connectedEdges[i]` without building the list.
    #[inline]
    pub fn connected_edge(&self, i: usize) -> NEdgeId {
        if i < self.incoming_edges.len() {
            self.incoming_edges[i]
        } else {
            self.outgoing_edges[i - self.incoming_edges.len()]
        }
    }
}

/// `NNode.NNodeBuilder`.
pub struct NNodeBuilder {
    pub node: NNode,
}

impl NNodeBuilder {
    pub fn id(mut self, id: i64) -> NNodeBuilder {
        self.node.id = id;
        self
    }

    pub fn origin(mut self, origin: LElement) -> NNodeBuilder {
        self.node.origin = Some(origin);
        self
    }

    pub fn type_(mut self, type_: &str) -> NNodeBuilder {
        self.node.type_ = type_.to_string();
        self
    }

    /// `create(_ graph:)`: stores the node and appends it to `graph.nodes`.
    pub fn create(self, graph: &mut NGraph) -> NNodeId {
        let id = NNodeId(graph.node_store.len() as u32);
        graph.node_store.push(self.node);
        graph.nodes.push(id);
        id
    }
}
