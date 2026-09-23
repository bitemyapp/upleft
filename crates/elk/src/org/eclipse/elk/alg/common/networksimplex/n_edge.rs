//! Port of `alg/common/networksimplex/NEdge.swift`.
//!
//! An edge of the network simplex graph: source and target `NNode`, a weight
//! and a minimum length (`delta`). Edges live in their [`NGraph`]'s edge store.

use super::n_graph::NGraph;
use super::n_node::NNodeId;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_element::LElement;

/// An `NEdge` reference.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NEdgeId(pub u32);

impl NEdgeId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug)]
pub struct NEdge {
    /// A package id, unused internally.
    pub id: i64,
    /// Internally set and used id to index arrays.
    pub internal_id: usize,
    /// An object from which this edge is derived (`Any?`).
    pub origin: Option<LElement>,
    pub source: Option<NNodeId>,
    pub target: Option<NNodeId>,
    pub weight: f64,
    /// The minimum length of this edge.
    pub delta: i64,
    /// Whether the edge is part of the spanning tree determined by `tightTree()`.
    pub tree_edge: bool,
}

impl Default for NEdge {
    fn default() -> Self {
        NEdge { id: 0, internal_id: 0, origin: None, source: None, target: None, weight: 0.0, delta: 1, tree_edge: false }
    }
}

impl NEdge {
    /// `NEdge.of()`.
    pub fn of() -> NEdgeBuilder {
        NEdgeBuilder { edge: NEdge::default() }
    }

    /// `NEdge.of(_ origin:)`: elk-swift ignores the origin here (it returns a
    /// plain builder), so this is `of()`.
    pub fn of_origin(_origin: Option<LElement>) -> NEdgeBuilder {
        NEdgeBuilder { edge: NEdge::default() }
    }

    /// `getOther(_:)`: the opposite end; `some` itself if the edge is not
    /// fully connected or `some` is not an end (an `assertionFailure` in Swift).
    #[inline]
    pub fn get_other(&self, some: NNodeId) -> NNodeId {
        let (Some(src), Some(tgt)) = (self.source, self.target) else { return some };
        if some == src {
            tgt
        } else if some == tgt {
            src
        } else {
            some
        }
    }
}

impl NGraph {
    /// `NEdge.reverse()`: swaps source and target and moves the edge between
    /// the nodes' edge lists (removed from its old list, appended to the new).
    pub fn edge_reverse(&mut self, edge: NEdgeId) -> NEdgeId {
        let e = &mut self[edge];
        std::mem::swap(&mut e.source, &mut e.target);
        let (Some(src), Some(tgt)) = (e.source, e.target) else { return edge };

        remove_first(&mut self[tgt].outgoing_edges, edge);
        self[tgt].incoming_edges.push(edge);

        remove_first(&mut self[src].incoming_edges, edge);
        self[src].outgoing_edges.push(edge);

        edge
    }
}

/// `ChangeAwareArrayList.remove(_:)`: removes the first identical element.
pub(crate) fn remove_first(list: &mut Vec<NEdgeId>, edge: NEdgeId) -> bool {
    if let Some(i) = list.iter().position(|&e| e == edge) {
        list.remove(i);
        true
    } else {
        false
    }
}

/// `NEdge.NEdgeBuilder`.
pub struct NEdgeBuilder {
    pub edge: NEdge,
}

impl NEdgeBuilder {
    pub fn id(mut self, id: i64) -> NEdgeBuilder {
        self.edge.id = id;
        self
    }

    pub fn origin(mut self, origin: Option<LElement>) -> NEdgeBuilder {
        self.edge.origin = origin;
        self
    }

    pub fn weight(mut self, weight: f64) -> NEdgeBuilder {
        self.edge.weight = weight;
        self
    }

    pub fn delta(mut self, delta: i64) -> NEdgeBuilder {
        self.edge.delta = delta;
        self
    }

    pub fn source(mut self, source: NNodeId) -> NEdgeBuilder {
        self.edge.source = Some(source);
        self
    }

    pub fn target(mut self, target: NNodeId) -> NEdgeBuilder {
        self.edge.target = Some(target);
        self
    }

    /// `create()`: stores the edge and appends it to the source's outgoing
    /// and the target's incoming edges. Without both ends, or for a self
    /// loop, Swift asserts (a no-op in release) and returns the unattached edge.
    pub fn create(self, graph: &mut NGraph) -> NEdgeId {
        let id = NEdgeId(graph.edge_store.len() as u32);
        let (source, target) = (self.edge.source, self.edge.target);
        graph.edge_store.push(self.edge);
        let (Some(src), Some(tgt)) = (source, target) else { return id };
        if src == tgt {
            return id;
        }
        graph[src].outgoing_edges.push(id);
        graph[tgt].incoming_edges.push(id);
        id
    }
}
