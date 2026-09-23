//! Port of `alg/layered/graph/LPort.swift`.

use super::l_graph::{LEdgeId, LGraphArena, LLabelId, LNodeId, LPortId};
use super::l_margin::LMargin;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;

#[derive(Clone, Debug)]
pub struct LPortData {
    pub props: PropertyMap,
    pub id: i32,
    pub position: KVector,
    pub size: KVector,
    /// `owner` (a weak reference in Swift).
    pub owner: Option<LNodeId>,
    pub side: PortSide,
    pub anchor: KVector,
    pub explicitly_supplied_port_anchor: bool,
    pub margin: LMargin,
    pub labels: Vec<LLabelId>,
    pub incoming_edges: Vec<LEdgeId>,
    pub outgoing_edges: Vec<LEdgeId>,
    pub connected_to_external_nodes: bool,
}

impl LGraphArena {
    /// `LPort()`.
    pub fn new_port(&mut self) -> LPortId {
        let id = LPortId(self.ports.len() as u32);
        self.ports.push(LPortData {
            props: PropertyMap::new(),
            id: 0,
            position: KVector::default(),
            size: KVector::default(),
            owner: None,
            side: PortSide::UNDEFINED,
            anchor: KVector::default(),
            explicitly_supplied_port_anchor: false,
            margin: LMargin::default(),
            labels: Vec::new(),
            incoming_edges: Vec::new(),
            outgoing_edges: Vec::new(),
            connected_to_external_nodes: true,
        });
        id
    }

    /// `setNode(_:)`: moves the port to the end of `node`'s port list.
    pub fn port_set_node(&mut self, port: LPortId, node: Option<LNodeId>) {
        if let Some(old) = self[port].owner {
            self[old].ports.retain(|&p| p != port);
        }
        self[port].owner = node;
        if let Some(new) = node {
            self[new].ports.push(port);
        }
    }

    /// `setSide(_:)`: ignores `UNDEFINED`; recomputes the anchor unless it
    /// was supplied explicitly.
    pub fn port_set_side(&mut self, port: LPortId, side: PortSide) {
        if side == PortSide::UNDEFINED {
            return;
        }
        let p = &mut self[port];
        p.side = side;
        if !p.explicitly_supplied_port_anchor {
            let size = p.size;
            match side {
                PortSide::NORTH => {
                    p.anchor.x = size.x / 2.0;
                    p.anchor.y = 0.0;
                }
                PortSide::EAST => {
                    p.anchor.x = size.x;
                    p.anchor.y = size.y / 2.0;
                }
                PortSide::SOUTH => {
                    p.anchor.x = size.x / 2.0;
                    p.anchor.y = size.y;
                }
                PortSide::WEST => {
                    p.anchor.x = 0.0;
                    p.anchor.y = size.y / 2.0;
                }
                _ => {}
            }
        }
    }

    /// `getAbsoluteAnchor()` / `absoluteAnchor`: owner position + port
    /// position + anchor (`(0, 0)` without an owner).
    pub fn port_absolute_anchor(&self, port: LPortId) -> KVector {
        let p = &self[port];
        let Some(owner) = p.owner else { return KVector::default() };
        let owner_pos = self[owner].position;
        KVector::new(owner_pos.x + p.position.x + p.anchor.x, owner_pos.y + p.position.y + p.anchor.y)
    }

    /// `getDegree()` / `degree`.
    pub fn port_degree(&self, port: LPortId) -> i32 {
        (self[port].incoming_edges.len() + self[port].outgoing_edges.len()) as i32
    }

    /// `getNetFlow()`.
    pub fn port_net_flow(&self, port: LPortId) -> i32 {
        self[port].incoming_edges.len() as i32 - self[port].outgoing_edges.len() as i32
    }

    /// `getConnectedEdges()`: incoming then outgoing.
    pub fn port_connected_edges(&self, port: LPortId) -> Vec<LEdgeId> {
        let p = &self[port];
        p.incoming_edges.iter().chain(p.outgoing_edges.iter()).copied().collect()
    }

    /// `getPredecessorPorts()`.
    pub fn port_predecessor_ports(&self, port: LPortId) -> Vec<LPortId> {
        self[port].incoming_edges.iter().filter_map(|&e| self[e].source).collect()
    }

    /// `getSuccessorPorts()`.
    pub fn port_successor_ports(&self, port: LPortId) -> Vec<LPortId> {
        self[port].outgoing_edges.iter().filter_map(|&e| self[e].target).collect()
    }

    /// `getConnectedPorts()`.
    pub fn port_connected_ports(&self, port: LPortId) -> Vec<LPortId> {
        let mut v = self.port_predecessor_ports(port);
        v.extend(self.port_successor_ports(port));
        v
    }

    /// `getIndex()`: position in the owner's port list, or -1.
    pub fn port_index(&self, port: LPortId) -> i32 {
        match self[port].owner {
            Some(owner) => self[owner].ports.iter().position(|&p| p == port).map_or(-1, |i| i as i32),
            None => -1,
        }
    }

    /// `getName()`: first label's text.
    pub fn port_name(&self, port: LPortId) -> Option<String> {
        self[port].labels.first().map(|&l| self[l].text.clone())
    }

    /// `getDesignation()`.
    pub fn port_designation(&self, port: LPortId) -> String {
        if let Some(&first) = self[port].labels.first() {
            if !self[first].text.is_empty() {
                return self[first].text.clone();
            }
        }
        self.port_index(port).to_string()
    }
}
