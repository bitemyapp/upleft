//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/compound/org_eclipse_elk_alg_layered_compound_CrossHierarchyEdge.swift`.
//!
//! A data holder used to pass information on hierarchy crossing edges from
//! the `CompoundGraphPreprocessor` to the `CompoundGraphPostprocessor`.
//! Instances are held in the [`CrossHierarchyMap`] attached to the top-level
//! graph via the `CROSS_HIERARCHY_MAP` property.
//!
//! The Swift class is never mutated after construction and never compared by
//! identity, so it is a `Copy` value here.

use std::collections::HashMap;

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LGraphId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossHierarchyEdge {
    /// The edge used in the layered graph to compute a layout.
    pub new_edge: LEdgeId,
    /// The layered graph in which the layout was computed.
    pub graph: LGraphId,
    /// The flow direction: input or output.
    pub port_type: PortType,
}

impl CrossHierarchyEdge {
    pub fn new(new_edge: LEdgeId, graph: LGraphId, port_type: PortType) -> CrossHierarchyEdge {
        CrossHierarchyEdge { new_edge, graph, port_type }
    }

    /// Return the dummy edge used to compute a layout in one segment of the
    /// cross-hierarchy edge.
    pub fn get_edge(&self) -> LEdgeId {
        self.new_edge
    }

    /// Return the graph in which the dummy edge `get_edge()` is used.
    pub fn get_graph(&self) -> LGraphId {
        self.graph
    }

    /// Return the type of cross-hierarchy edge segment (input or output). An
    /// input segment is one that points to deeper hierarchy levels, while an
    /// output segment is one that points to shallower hierarchy levels.
    pub fn get_type(&self) -> PortType {
        self.port_type
    }

    /// Return the actual source port of the edge. In case the source port of
    /// the dummy edge is an external port, the corresponding port of the
    /// containing node is returned. Without a source, Swift returns a fresh
    /// `LPort()`; so does this (a new, unattached port in the arena).
    pub fn get_actual_source(&self, lg: &mut LGraphArena) -> LPortId {
        let Some(src) = lg[self.new_edge].source else { return lg.new_port() };
        actual_port(lg, src)
    }

    /// Return the actual target port of the edge (see `get_actual_source`).
    pub fn get_actual_target(&self, lg: &mut LGraphArena) -> LPortId {
        let Some(tgt) = lg[self.new_edge].target else { return lg.new_port() };
        actual_port(lg, tgt)
    }
}

fn actual_port(lg: &LGraphArena, port: LPortId) -> LPortId {
    if let Some(node) = lg[port].owner {
        if lg[node].node_type == NodeType::EXTERNAL_PORT {
            if let Some(origin) = lg[node].props.get_as::<LPortId>(&InternalProperties::ORIGIN) {
                return origin;
            }
        }
    }
    port
}

/// Swift's `[LEdge: [CrossHierarchyEdge]]` (the `CROSS_HIERARCHY_MAP` value),
/// keyed by edge identity.
///
/// NONDETERMINISTIC IN SWIFT: both compound processors iterate this
/// dictionary (`for (origEdge, segments) in crossHierarchyMap`), and its
/// order is the hash order of `ObjectIdentifier`s (heap addresses, per-process
/// seed). What depends on it: the order in which labels of different
/// original edges are appended to a shared (merged) hierarchy segment, and
/// the order in which the postprocessor re-adds original edges to their
/// ports' edge lists. Java ELK used a `HashMultimap`-like map as well. The
/// port iterates in insertion order (the order in which the preprocessor
/// first recorded each original edge).
#[derive(Clone, Debug, Default)]
pub struct CrossHierarchyMap {
    entries: Vec<(LEdgeId, Vec<CrossHierarchyEdge>)>,
    index: HashMap<LEdgeId, usize>,
}

impl CrossHierarchyMap {
    pub fn new() -> CrossHierarchyMap {
        CrossHierarchyMap::default()
    }

    /// `map[origEdge, default: []].append(che)`.
    pub fn append(&mut self, orig_edge: LEdgeId, che: CrossHierarchyEdge) {
        match self.index.get(&orig_edge) {
            Some(&i) => self.entries[i].1.push(che),
            None => {
                self.index.insert(orig_edge, self.entries.len());
                self.entries.push((orig_edge, vec![che]));
            }
        }
    }

    pub fn get(&self, orig_edge: LEdgeId) -> Option<&[CrossHierarchyEdge]> {
        self.index.get(&orig_edge).map(|&i| self.entries[i].1.as_slice())
    }

    /// The entries, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (LEdgeId, &[CrossHierarchyEdge])> {
        self.entries.iter().map(|(e, v)| (*e, v.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
