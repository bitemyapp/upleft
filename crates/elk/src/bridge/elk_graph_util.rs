//! Port of `Bridge/ElkGraphUtil.swift`.

use super::elk_graph_impl::{ElkBendPoint, ElkEdgeId, ElkEdgeSectionId, ElkElement, ElkGraph, ElkLabelId, ElkNodeId, ElkPortId, ElkShape};

impl ElkGraph {
    /// `createNode(parent)`: sets the parent only (not added to its children).
    pub fn create_node(&mut self, parent: Option<ElkNodeId>) -> ElkNodeId {
        let n = self.new_node();
        self[n].parent = parent;
        n
    }

    /// `createLabel(text, parent)`: sets the parent only.
    pub fn create_label(&mut self, text: &str, parent: Option<ElkElement>) -> ElkLabelId {
        let l = self.new_label();
        self[l].parent = parent;
        self[l].text = text.to_string();
        l
    }

    /// `createEdgeSection(edge)`: appends to the edge's sections (without
    /// setting the section's parent, as in elk-swift).
    pub fn create_edge_section(&mut self, edge: Option<ElkEdgeId>) -> ElkEdgeSectionId {
        let s = self.new_section();
        if let Some(e) = edge {
            self[e].sections.push(s);
        }
        s
    }

    /// `firstEdgeSection(edge, resetSection, removeOtherSections)`.
    pub fn first_edge_section(&mut self, edge: ElkEdgeId, reset_section: bool, remove_other_sections: bool) -> ElkEdgeSectionId {
        if self[edge].sections.is_empty() {
            return self.create_edge_section(Some(edge));
        }
        let section = self[edge].sections[0];
        if reset_section {
            let s = &mut self[section];
            s.bend_points.clear();
            s.start_x = 0.0;
            s.start_y = 0.0;
            s.end_x = 0.0;
            s.end_y = 0.0;
        }
        if remove_other_sections {
            self[edge].sections.truncate(1);
        }
        section
    }

    /// `createBendPoint(section, x, y)`.
    pub fn create_bend_point(&mut self, section: ElkEdgeSectionId, x: f64, y: f64) {
        self[section].bend_points.push(ElkBendPoint { x, y });
    }

    /// `findLowestCommonAncestor(_:_:)`.
    pub fn find_lowest_common_ancestor(&self, node1: ElkNodeId, node2: ElkNodeId) -> Option<ElkNodeId> {
        let chain = |mut c: Option<ElkNodeId>| {
            let mut v = Vec::new();
            while let Some(n) = c {
                v.push(n);
                c = self[n].parent;
            }
            v
        };
        let a1 = chain(Some(node1));
        let a2 = chain(Some(node2));
        let mut common = None;
        let (mut i1, mut i2) = (a1.len() as isize - 1, a2.len() as isize - 1);
        while i1 >= 0 && i2 >= 0 && a1[i1 as usize] == a2[i2 as usize] {
            common = Some(a1[i1 as usize]);
            i1 -= 1;
            i2 -= 1;
        }
        common
    }

    /// `allIncomingEdges(node)`: the node's, then each port's.
    pub fn all_incoming_edges(&self, node: ElkNodeId) -> Vec<ElkEdgeId> {
        let mut edges = self[node].incoming_edges.clone();
        for &p in &self[node].ports {
            edges.extend_from_slice(&self[p].incoming_edges);
        }
        edges
    }

    /// `allOutgoingEdges(node)`: the node's, then each port's.
    pub fn all_outgoing_edges(&self, node: ElkNodeId) -> Vec<ElkEdgeId> {
        let mut edges = self[node].outgoing_edges.clone();
        for &p in &self[node].ports {
            edges.extend_from_slice(&self[p].outgoing_edges);
        }
        edges
    }

    /// `allIncidentEdges(node)`: outgoing then incoming.
    pub fn all_incident_edges(&self, node: ElkNodeId) -> Vec<ElkEdgeId> {
        let mut v = self.all_outgoing_edges(node);
        v.extend(self.all_incoming_edges(node));
        v
    }

    /// `allIncidentShapes(edge)`: sources then targets.
    pub fn all_incident_shapes(&self, edge: ElkEdgeId) -> Vec<ElkShape> {
        let mut v = self[edge].sources.clone();
        v.extend_from_slice(&self[edge].targets);
        v
    }

    /// `isDescendant(child, ancestor)`: a strict descendant.
    pub fn is_descendant(&self, child: ElkNodeId, ancestor: ElkNodeId) -> bool {
        let mut current = child;
        while let Some(parent) = self[current].parent {
            if parent == ancestor {
                return true;
            }
            current = parent;
        }
        false
    }

    /// `connectableShapeToNode(_:)`: a node, or a port's parent. A parentless
    /// port asserts in Swift (and then yields a fresh node); here it panics.
    pub fn connectable_shape_to_node(&self, shape: ElkShape) -> ElkNodeId {
        match shape {
            ElkShape::Node(n) => n,
            ElkShape::Port(p) => self[p].parent.expect("connectableShapeToNode: port without a parent"),
        }
    }

    /// `connectableShapeToPort(_:)`.
    pub fn connectable_shape_to_port(&self, shape: ElkShape) -> Option<ElkPortId> {
        match shape {
            ElkShape::Port(p) => Some(p),
            ElkShape::Node(_) => None,
        }
    }

    /// `findBestEdgeContainment(_:)`.
    pub fn find_best_edge_containment(&self, edge: ElkEdgeId) -> Option<ElkNodeId> {
        let e = &self[edge];
        let incident_count = e.sources.len() + e.targets.len();
        match incident_count {
            0 => None,
            1 => {
                if e.sources.is_empty() {
                    self[self.connectable_shape_to_node(e.targets[0])].parent
                } else {
                    self[self.connectable_shape_to_node(e.sources[0])].parent
                }
            }
            _ => {
                if e.sources.len() == 1 && e.targets.len() == 1 {
                    let source_node = self.connectable_shape_to_node(e.sources[0]);
                    let target_node = self.connectable_shape_to_node(e.targets[0]);
                    if self[source_node].parent == self[target_node].parent {
                        return self[source_node].parent;
                    } else if Some(source_node) == self[target_node].parent {
                        return Some(source_node);
                    } else if Some(target_node) == self[source_node].parent {
                        return Some(target_node);
                    }
                }
                let shapes = self.all_incident_shapes(edge);
                let first = *shapes.first()?;
                let mut common = self.connectable_shape_to_node(first);
                for &shape in &shapes[1..] {
                    let incident = self.connectable_shape_to_node(shape);
                    if incident != common && !self.is_descendant(incident, common) {
                        if self[incident].parent == self[common].parent {
                            if let Some(p) = self[incident].parent {
                                common = p;
                            }
                        } else {
                            common = self.find_lowest_common_ancestor(common, incident).or(self[common].parent).unwrap_or(common);
                        }
                    }
                }
                Some(common)
            }
        }
    }

    /// `containingGraph(_:)`.
    pub fn containing_graph(&self, element: ElkElement) -> Option<ElkNodeId> {
        match element {
            ElkElement::Edge(e) => self[e].containing_node,
            ElkElement::Node(n) => self[n].parent,
            ElkElement::Port(p) => self[p].parent,
            ElkElement::Label(l) => match self[l].parent {
                Some(ElkElement::Node(n)) => Some(n),
                Some(ElkElement::Port(p)) => self[p].parent,
                _ => None,
            },
        }
    }
}
