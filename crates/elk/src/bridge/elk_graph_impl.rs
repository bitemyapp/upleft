//! Port of `Bridge/ElkGraphImpl.swift`: the ELK graph model (nodes, ports,
//! labels, edges, edge sections, bend points) the JSON importer builds and
//! the layout writes its results into.
//!
//! As with the layered graph, elements live in an arena ([`ElkGraph`]) and are
//! addressed by typed indices.

use std::ops::{Index, IndexMut};

use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;

macro_rules! ids {
    ($($name:ident),*) => {$(
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
        pub struct $name(pub u32);
        impl $name {
            #[inline]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    )*};
}

ids!(ElkNodeId, ElkPortId, ElkLabelId, ElkEdgeId, ElkEdgeSectionId);

/// `ElkConnectableShape`: a node or a port.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ElkShape {
    Node(ElkNodeId),
    Port(ElkPortId),
}

/// `ElkGraphElement`: anything that can carry labels.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ElkElement {
    Node(ElkNodeId),
    Port(ElkPortId),
    Edge(ElkEdgeId),
    Label(ElkLabelId),
}

#[derive(Clone, Debug, Default)]
pub struct ElkNodeData {
    pub props: PropertyMap,
    pub identifier: Option<String>,
    pub labels: Vec<ElkLabelId>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub outgoing_edges: Vec<ElkEdgeId>,
    pub incoming_edges: Vec<ElkEdgeId>,
    pub ports: Vec<ElkPortId>,
    pub children: Vec<ElkNodeId>,
    pub parent: Option<ElkNodeId>,
    pub contained_edges: Vec<ElkEdgeId>,
}

#[derive(Clone, Debug, Default)]
pub struct ElkPortData {
    pub props: PropertyMap,
    pub identifier: Option<String>,
    pub labels: Vec<ElkLabelId>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub outgoing_edges: Vec<ElkEdgeId>,
    pub incoming_edges: Vec<ElkEdgeId>,
    pub parent: Option<ElkNodeId>,
}

#[derive(Clone, Debug, Default)]
pub struct ElkLabelData {
    pub props: PropertyMap,
    pub identifier: Option<String>,
    pub labels: Vec<ElkLabelId>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub parent: Option<ElkElement>,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct ElkEdgeData {
    pub props: PropertyMap,
    pub identifier: Option<String>,
    pub labels: Vec<ElkLabelId>,
    pub containing_node: Option<ElkNodeId>,
    pub sources: Vec<ElkShape>,
    pub targets: Vec<ElkShape>,
    pub sections: Vec<ElkEdgeSectionId>,
}

/// `ElkBendPoint` (a class in Swift; never shared between sections).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ElkBendPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Default)]
pub struct ElkEdgeSectionData {
    pub props: PropertyMap,
    pub identifier: Option<String>,
    pub start_x: f64,
    pub start_y: f64,
    pub end_x: f64,
    pub end_y: f64,
    pub bend_points: Vec<ElkBendPoint>,
    pub parent: Option<ElkEdgeId>,
    pub outgoing_shape: Option<ElkShape>,
    pub incoming_shape: Option<ElkShape>,
    pub outgoing_sections: Vec<ElkEdgeSectionId>,
    pub incoming_sections: Vec<ElkEdgeSectionId>,
}

/// Every element of one ELK graph.
#[derive(Clone, Debug, Default)]
pub struct ElkGraph {
    pub nodes: Vec<ElkNodeData>,
    pub ports: Vec<ElkPortData>,
    pub labels: Vec<ElkLabelData>,
    pub edges: Vec<ElkEdgeData>,
    pub sections: Vec<ElkEdgeSectionData>,
}

macro_rules! arena_index {
    ($($id:ident => $field:ident: $data:ident),*) => {$(
        impl Index<$id> for ElkGraph {
            type Output = $data;
            #[inline]
            fn index(&self, id: $id) -> &$data {
                &self.$field[id.0 as usize]
            }
        }
        impl IndexMut<$id> for ElkGraph {
            #[inline]
            fn index_mut(&mut self, id: $id) -> &mut $data {
                &mut self.$field[id.0 as usize]
            }
        }
    )*};
}

arena_index!(
    ElkNodeId => nodes: ElkNodeData,
    ElkPortId => ports: ElkPortData,
    ElkLabelId => labels: ElkLabelData,
    ElkEdgeId => edges: ElkEdgeData,
    ElkEdgeSectionId => sections: ElkEdgeSectionData
);

impl ElkGraph {
    pub fn new() -> ElkGraph {
        ElkGraph::default()
    }

    pub fn new_node(&mut self) -> ElkNodeId {
        self.nodes.push(ElkNodeData::default());
        ElkNodeId(self.nodes.len() as u32 - 1)
    }

    pub fn new_port(&mut self) -> ElkPortId {
        self.ports.push(ElkPortData::default());
        ElkPortId(self.ports.len() as u32 - 1)
    }

    pub fn new_label(&mut self) -> ElkLabelId {
        self.labels.push(ElkLabelData::default());
        ElkLabelId(self.labels.len() as u32 - 1)
    }

    pub fn new_edge(&mut self) -> ElkEdgeId {
        self.edges.push(ElkEdgeData::default());
        ElkEdgeId(self.edges.len() as u32 - 1)
    }

    pub fn new_section(&mut self) -> ElkEdgeSectionId {
        self.sections.push(ElkEdgeSectionData::default());
        ElkEdgeSectionId(self.sections.len() as u32 - 1)
    }

    /// The property map of any labelled element.
    pub fn element_props(&self, e: ElkElement) -> &PropertyMap {
        match e {
            ElkElement::Node(n) => &self[n].props,
            ElkElement::Port(p) => &self[p].props,
            ElkElement::Edge(ed) => &self[ed].props,
            ElkElement::Label(l) => &self[l].props,
        }
    }

    pub fn element_props_mut(&mut self, e: ElkElement) -> &mut PropertyMap {
        match e {
            ElkElement::Node(n) => &mut self[n].props,
            ElkElement::Port(p) => &mut self[p].props,
            ElkElement::Edge(ed) => &mut self[ed].props,
            ElkElement::Label(l) => &mut self[l].props,
        }
    }

    pub fn element_labels(&self, e: ElkElement) -> &[ElkLabelId] {
        match e {
            ElkElement::Node(n) => &self[n].labels,
            ElkElement::Port(p) => &self[p].labels,
            ElkElement::Edge(ed) => &self[ed].labels,
            ElkElement::Label(l) => &self[l].labels,
        }
    }

    pub fn shape_props(&self, s: ElkShape) -> &PropertyMap {
        match s {
            ElkShape::Node(n) => &self[n].props,
            ElkShape::Port(p) => &self[p].props,
        }
    }

    pub fn shape_outgoing_edges(&self, s: ElkShape) -> &[ElkEdgeId] {
        match s {
            ElkShape::Node(n) => &self[n].outgoing_edges,
            ElkShape::Port(p) => &self[p].outgoing_edges,
        }
    }

    pub fn shape_incoming_edges(&self, s: ElkShape) -> &[ElkEdgeId] {
        match s {
            ElkShape::Node(n) => &self[n].incoming_edges,
            ElkShape::Port(p) => &self[p].incoming_edges,
        }
    }

    /// `(x, y, width, height)` of a node or port.
    pub fn shape_bounds(&self, s: ElkShape) -> (f64, f64, f64, f64) {
        match s {
            ElkShape::Node(n) => (self[n].x, self[n].y, self[n].width, self[n].height),
            ElkShape::Port(p) => (self[p].x, self[p].y, self[p].width, self[p].height),
        }
    }

    /// `isHierarchical()` for nodes: has children.
    pub fn node_is_hierarchical(&self, n: ElkNodeId) -> bool {
        !self[n].children.is_empty()
    }

    /// `ElkEdge.isHyperedge()`.
    pub fn edge_is_hyperedge(&self, e: ElkEdgeId) -> bool {
        self[e].sources.len() > 1 || self[e].targets.len() > 1
    }

    fn shape_node_of(&self, s: ElkShape) -> Option<ElkNodeId> {
        match s {
            ElkShape::Node(n) => Some(n),
            ElkShape::Port(p) => self[p].parent,
        }
    }

    /// `ElkEdge.isHierarchical()`.
    pub fn edge_is_hierarchical(&self, e: ElkEdgeId) -> bool {
        let Some(containing) = self[e].containing_node else { return false };
        for &shape in self[e].sources.iter().chain(self[e].targets.iter()) {
            let node = self.shape_node_of(shape);
            if node == Some(containing) {
                return true;
            }
            if node.and_then(|n| self[n].parent) != Some(containing) {
                return true;
            }
        }
        false
    }

    /// `ElkEdge.isSelfloop()`: the sets of incident nodes (a port counting as
    /// its node) of sources and targets are equal.
    pub fn edge_is_selfloop(&self, e: ElkEdgeId) -> bool {
        let edge = &self[e];
        if edge.sources.is_empty() || edge.targets.is_empty() {
            return false;
        }
        // Swift maps a parentless port to the port object itself.
        #[derive(PartialEq, Eq, Hash)]
        enum Key {
            Node(ElkNodeId),
            Port(ElkPortId),
        }
        let key = |s: ElkShape| match s {
            ElkShape::Node(n) => Key::Node(n),
            ElkShape::Port(p) => match self[p].parent {
                Some(n) => Key::Node(n),
                None => Key::Port(p),
            },
        };
        let sources: std::collections::HashSet<Key> = edge.sources.iter().map(|&s| key(s)).collect();
        let targets: std::collections::HashSet<Key> = edge.targets.iter().map(|&s| key(s)).collect();
        sources == targets
    }

    /// `ElkEdge.isConnected()`.
    pub fn edge_is_connected(&self, e: ElkEdgeId) -> bool {
        !self[e].sources.is_empty() && !self[e].targets.is_empty()
    }
}
