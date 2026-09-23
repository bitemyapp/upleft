//! Port of `alg/layered/graph/LNode.swift`.

use super::l_graph::{LEdgeId, LGraphArena, LGraphId, LLabelId, LNodeId, LPortId, LayerId};
use super::l_margin::LMargin;
use super::l_padding::LPadding;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum NodeType {
    NORMAL,
    LONG_EDGE,
    EXTERNAL_PORT,
    NORTH_SOUTH_PORT,
    LABEL,
    BREAKING_POINT,
    PLACEHOLDER,
    NONSHIFTING_PLACEHOLDER,
}

impl NodeType {
    pub const ALL: [NodeType; 8] = [
        NodeType::NORMAL,
        NodeType::LONG_EDGE,
        NodeType::EXTERNAL_PORT,
        NodeType::NORTH_SOUTH_PORT,
        NodeType::LABEL,
        NodeType::BREAKING_POINT,
        NodeType::PLACEHOLDER,
        NodeType::NONSHIFTING_PLACEHOLDER,
    ];

    pub fn ordinal(self) -> usize {
        self as usize
    }

    /// The Swift raw value.
    pub fn raw_value(self) -> &'static str {
        match self {
            NodeType::NORMAL => "NORMAL",
            NodeType::LONG_EDGE => "LONG_EDGE",
            NodeType::EXTERNAL_PORT => "EXTERNAL_PORT",
            NodeType::NORTH_SOUTH_PORT => "NORTH_SOUTH_PORT",
            NodeType::LABEL => "LABEL",
            NodeType::BREAKING_POINT => "BREAKING_POINT",
            NodeType::PLACEHOLDER => "PLACEHOLDER",
            NodeType::NONSHIFTING_PLACEHOLDER => "NONSHIFTING_PLACEHOLDER",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LNodeData {
    pub props: PropertyMap,
    pub id: i32,
    pub position: KVector,
    pub size: KVector,
    pub graph: Option<LGraphId>,
    pub layer: Option<LayerId>,
    pub node_type: NodeType,
    pub ports: Vec<LPortId>,
    pub labels: Vec<LLabelId>,
    pub nested_graph: Option<LGraphId>,
    pub margin: LMargin,
    pub padding: LPadding,
    /// `portSideIndices`, indexed by `PortSide` ordinal.
    pub port_side_indices: Option<[Option<(usize, usize)>; 5]>,
    pub port_sides_cached: bool,
    /// Set on external port dummies: their `PORT_ANCHOR` property is, in
    /// Swift, the same `KVector` object as this port's `position` (see
    /// `LGraphArena::create_external_port_dummy`).
    pub port_anchor_alias: Option<LPortId>,
}

impl LGraphArena {
    /// `LNode(graph)`: a node belonging to `graph` (not added to any list).
    pub fn new_node(&mut self, graph: Option<LGraphId>) -> LNodeId {
        let id = LNodeId(self.nodes.len() as u32);
        self.nodes.push(LNodeData {
            props: PropertyMap::new(),
            id: 0,
            position: KVector::default(),
            size: KVector::default(),
            graph,
            layer: None,
            node_type: NodeType::NORMAL,
            ports: Vec::new(),
            labels: Vec::new(),
            nested_graph: None,
            margin: LMargin::default(),
            padding: LPadding::default(),
            port_side_indices: None,
            port_sides_cached: false,
            port_anchor_alias: None,
        });
        id
    }

    /// `setLayer(_:)`: moves the node to the end of `layer` (or out of any layer).
    pub fn node_set_layer(&mut self, node: LNodeId, layer: Option<LayerId>) {
        if let Some(old) = self[node].layer {
            self[old].nodes.retain(|&n| n != node);
        }
        self[node].layer = layer;
        if let Some(new) = layer {
            self[new].nodes.push(node);
        }
    }

    /// `setLayer(_ index:, _ layer:)`: inserts at `min(index, count)`.
    pub fn node_set_layer_at(&mut self, node: LNodeId, index: usize, layer: LayerId) {
        if let Some(old) = self[node].layer {
            self[old].nodes.retain(|&n| n != node);
        }
        self[node].layer = Some(layer);
        let at = index.min(self[layer].nodes.len());
        self[layer].nodes.insert(at, node);
    }

    /// `getGraph()`: the node's graph, else its layer's graph.
    pub fn node_graph(&self, node: LNodeId) -> Option<LGraphId> {
        let n = &self[node];
        if n.graph.is_none() {
            if let Some(layer) = n.layer {
                return Some(self[layer].owner);
            }
        }
        n.graph
    }

    /// `getPorts(_ portType:)`.
    pub fn node_ports_of_type(&self, node: LNodeId, port_type: PortType) -> Vec<LPortId> {
        match port_type {
            PortType::INPUT => self[node].ports.iter().copied().filter(|&p| !self[p].incoming_edges.is_empty()).collect(),
            PortType::OUTPUT => self[node].ports.iter().copied().filter(|&p| !self[p].outgoing_edges.is_empty()).collect(),
            _ => self[node].ports.clone(),
        }
    }

    /// `getPorts(_ side:)`.
    pub fn node_ports_on_side(&self, node: LNodeId, side: PortSide) -> Vec<LPortId> {
        self[node].ports.iter().copied().filter(|&p| self[p].side == side).collect()
    }

    /// `getPorts(_ portType:, _ side:)`.
    pub fn node_ports_of_type_on_side(&self, node: LNodeId, port_type: PortType, side: PortSide) -> Vec<LPortId> {
        self[node]
            .ports
            .iter()
            .copied()
            .filter(|&p| self[p].side == side)
            .filter(|&p| match port_type {
                PortType::INPUT => !self[p].incoming_edges.is_empty(),
                PortType::OUTPUT => !self[p].outgoing_edges.is_empty(),
                _ => true,
            })
            .collect()
    }

    /// `getPortSideView(_:)`: the ports of one side, from the cached indices.
    pub fn node_port_side_view(&mut self, node: LNodeId, side: PortSide) -> Vec<LPortId> {
        if !self[node].port_sides_cached {
            self.node_find_port_indices(node);
        }
        match self[node].port_side_indices.and_then(|i| i[side.ordinal()]) {
            Some((a, b)) => self[node].ports[a..b].to_vec(),
            None => Vec::new(),
        }
    }

    /// `setPortSideView(_:_:)`: writes a reordered side back into `ports`.
    pub fn node_set_port_side_view(&mut self, node: LNodeId, side: PortSide, reordered: &[LPortId]) {
        if !self[node].port_sides_cached {
            self.node_find_port_indices(node);
        }
        let Some((a, b)) = self[node].port_side_indices.and_then(|i| i[side.ordinal()]) else { return };
        if b - a != reordered.len() {
            return;
        }
        self[node].ports[a..b].copy_from_slice(reordered);
    }

    /// `getIncomingEdges()`.
    pub fn node_incoming_edges(&self, node: LNodeId) -> Vec<LEdgeId> {
        self[node].ports.iter().flat_map(|&p| self[p].incoming_edges.iter().copied()).collect()
    }

    /// `getOutgoingEdges()`.
    pub fn node_outgoing_edges(&self, node: LNodeId) -> Vec<LEdgeId> {
        self[node].ports.iter().flat_map(|&p| self[p].outgoing_edges.iter().copied()).collect()
    }

    /// `getConnectedEdges()`: per port, incoming then outgoing.
    pub fn node_connected_edges(&self, node: LNodeId) -> Vec<LEdgeId> {
        self[node]
            .ports
            .iter()
            .flat_map(|&p| self[p].incoming_edges.iter().copied().chain(self[p].outgoing_edges.iter().copied()))
            .collect()
    }

    /// `borderToContentAreaCoordinates(_:_:)`.
    pub fn node_border_to_content_area_coordinates(&mut self, node: LNodeId, horizontal: bool, vertical: bool) {
        let Some(g) = self.node_graph(node) else { return };
        let padding = self[g].padding;
        let offset = self[g].offset;
        let pos = &mut self[node].position;
        if horizontal {
            pos.x = pos.x - padding.left - offset.x;
        }
        if vertical {
            pos.y = pos.y - padding.top - offset.y;
        }
    }

    /// `getIndex()`: position in the layer, or -1.
    pub fn node_index(&self, node: LNodeId) -> i32 {
        match self[node].layer {
            Some(layer) => self[layer].nodes.iter().position(|&n| n == node).map_or(-1, |i| i as i32),
            None => -1,
        }
    }

    /// `cachePortSides()`.
    pub fn node_cache_port_sides(&mut self, node: LNodeId) {
        self[node].port_sides_cached = true;
        self.node_find_port_indices(node);
    }

    /// `findPortIndices()`.
    pub fn node_find_port_indices(&mut self, node: LNodeId) {
        let mut indices: [Option<(usize, usize)>; 5] = [None; 5];
        let mut first_index_for_current_side = 0;
        let mut current_side = PortSide::NORTH;
        let mut current_index = 0;
        for &port in &self[node].ports {
            let side = self[port].side;
            if side != current_side {
                if first_index_for_current_side != current_index {
                    indices[current_side.ordinal()] = Some((first_index_for_current_side, current_index));
                }
                current_side = side;
                first_index_for_current_side = current_index;
            }
            current_index += 1;
        }
        indices[current_side.ordinal()] = Some((first_index_for_current_side, current_index));
        self[node].port_side_indices = Some(indices);
    }

    /// `getInteractiveReferencePoint()`.
    pub fn node_interactive_reference_point(&self, node: LNodeId) -> KVector {
        let n = &self[node];
        KVector::new(n.position.x + n.size.x / 2.0, n.position.y + n.size.y / 2.0)
    }

    /// `isInlineEdgeLabel()`.
    pub fn node_is_inline_edge_label(&self, node: LNodeId) -> bool {
        if self[node].node_type != NodeType::LABEL {
            return false;
        }
        let represented: Vec<LLabelId> = self[node].props.get_as(&InternalProperties::REPRESENTED_LABELS).unwrap_or_default();
        represented.iter().all(|&l| self[l].props.get_as::<bool>(&LayeredOptions::EDGE_LABELS_INLINE).unwrap_or(false))
    }

    /// `getDesignation()`: first label's text, else the index in the layer.
    pub fn node_designation(&self, node: LNodeId) -> String {
        if let Some(&first) = self[node].labels.first() {
            if !self[first].text.is_empty() {
                return self[first].text.clone();
            }
        }
        self.node_index(node).to_string()
    }
}

crate::enum_ordinal!(NodeType);
