//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/compound/org_eclipse_elk_alg_layered_compound_CrossHierarchyEdgeComparator.swift`.
//!
//! Compares cross-hierarchy edge segments such that they can be sorted from
//! the start to the end segment.

use std::cmp::Ordering;

use super::cross_hierarchy_edge::CrossHierarchyEdge;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;

pub struct CrossHierarchyEdgeComparator {
    pub graph: LGraphId,
}

impl CrossHierarchyEdgeComparator {
    /// Creates a new comparator for sorting cross-hierarchy edge segments for
    /// the given top-level compound graph.
    pub fn new(graph: LGraphId) -> CrossHierarchyEdgeComparator {
        CrossHierarchyEdgeComparator { graph }
    }

    /// `compare(_:_:)`: `Less` is `.orderedAscending`.
    pub fn compare(&self, lg: &LGraphArena, edge1: &CrossHierarchyEdge, edge2: &CrossHierarchyEdge) -> Ordering {
        if edge1.port_type == PortType::OUTPUT && edge2.port_type == PortType::INPUT {
            return Ordering::Less;
        } else if edge1.port_type == PortType::INPUT && edge2.port_type == PortType::OUTPUT {
            return Ordering::Greater;
        }

        let level1 = CrossHierarchyEdgeComparator::hierarchy_level(lg, edge1.graph, self.graph);
        let level2 = CrossHierarchyEdgeComparator::hierarchy_level(lg, edge2.graph, self.graph);

        let diff = if edge1.port_type == PortType::OUTPUT {
            // from deeper level to higher level
            level2 - level1
        } else {
            // from higher level to deeper level
            level1 - level2
        };

        diff.cmp(&0)
    }

    /// Compute the hierarchy level of the given nested graph (higher number
    /// means the node is nested deeper).
    pub fn hierarchy_level(lg: &LGraphArena, nested_graph: LGraphId, top_level_graph: LGraphId) -> i64 {
        let mut current_graph = nested_graph;
        let mut level = 0;

        loop {
            if current_graph == top_level_graph {
                return level;
            }

            // assertionFailure (a no-op in release builds), then return.
            let Some(current_node) = lg[current_graph].parent_node else { return level };
            let Some(graph) = lg[current_node].graph else { return level };
            current_graph = graph;
            level += 1;
        }
    }
}
