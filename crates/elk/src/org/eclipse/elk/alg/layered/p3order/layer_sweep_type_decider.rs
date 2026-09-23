//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_LayerSweepTypeDecider.swift`.
//!
//! Decides whether a nested graph is laid out bottom-up (on its own) or
//! swept into from its parent.
//!
//! The Swift holds a reference to its `GraphInfoHolder`; here the holder is
//! passed to [`LayerSweepTypeDecider::use_bottom_up`]. `NodeInfo` is a class:
//! [`NodeInfoRef`] is either a slot of the `nodeInfo` table (the shared
//! object) or a throwaway object for nodes outside the table.

use super::counting::cross_min_util::port_side_view;
use super::counting::i_initializable::IInitializable;
use super::graph_info_holder::GraphInfoHolder;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::options::port_side::PortSide;

const CROSSING_MINIMIZATION_HIERARCHICAL_SWEEPINESS: &str = "org.eclipse.elk.layered.crossingMinimization.hierarchicalSweepiness";
const PORT_CONSTRAINTS: &str = "org.eclipse.elk.portConstraints";
const PORT_DUMMY: &str = "portDummy";
/// Not the id `InternalProperties.ORIGIN` uses (`"origin"`), so this lookup
/// finds nothing on graphs built by the pipeline.
const ORIGIN: &str = "org.eclipse.elk.layered.origin";

/// `LayerSweepTypeDecider.NodeInfo`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeInfo {
    pub connected_edges: i64,
    pub hierarchical_influence: i64,
    pub random_influence: i64,
}

impl NodeInfo {
    /// `transfer(_:)`.
    pub fn transfer(&mut self, node_info: NodeInfo) {
        self.hierarchical_influence += node_info.hierarchical_influence;
        self.random_influence += node_info.random_influence;
        self.connected_edges += node_info.connected_edges;
    }
}

/// A `NodeInfo` object: a table slot, or a throwaway instance.
#[derive(Clone, Copy, Debug)]
pub enum NodeInfoRef {
    Slot(usize, usize),
    Temp(NodeInfo),
}

#[derive(Clone, Debug, Default)]
pub struct LayerSweepTypeDecider {
    pub node_info: Vec<Vec<Option<NodeInfo>>>,
}

impl IInitializable for LayerSweepTypeDecider {}

impl LayerSweepTypeDecider {
    /// `LayerSweepTypeDecider(_ graphData:)`.
    pub fn new(graph_data: &GraphInfoHolder) -> LayerSweepTypeDecider {
        LayerSweepTypeDecider { node_info: vec![Vec::new(); graph_data.current_node_order.len()] }
    }

    /// `useBottomUp()`.
    pub fn use_bottom_up(&mut self, lg: &LGraphArena, graph_data: &GraphInfoHolder) -> bool {
        // Java default is 0.1 (from Layered.melk), NOT 0
        let boundary = lg[graph_data.l_graph]
            .props
            .get_by_id(CROSSING_MINIMIZATION_HIERARCHICAL_SWEEPINESS)
            .and_then(|v| v.cast::<f64>())
            .unwrap_or(0.1);

        if self.bottom_up_forced(boundary) || self.root_node(graph_data) || self.fixed_port_order(lg, graph_data) || self.fewer_than_two_in_out_edges(lg, graph_data) {
            return true;
        }

        if graph_data.cross_min_deterministic() {
            return false;
        }

        let mut paths_to_random: i64 = 0;
        let mut paths_to_hierarchical: i64 = 0;

        let mut ns_port_dummies: Vec<LNodeId> = Vec::new();

        for layer in &graph_data.current_node_order {
            for &node in layer {
                if self.is_north_south_dummy(lg, node) {
                    ns_port_dummies.push(node);
                    continue;
                }

                let mut current_node = self.node_info_for(lg, node);

                if self.is_external_port_dummy(lg, node) {
                    self.info_mut(&mut current_node, |i| i.hierarchical_influence = 1);
                    if self.is_eastern_dummy(lg, node) {
                        paths_to_hierarchical += self.info(current_node).connected_edges;
                    }
                } else if self.has_no_western_ports(lg, node) {
                    self.info_mut(&mut current_node, |i| i.random_influence = 1);
                } else if self.has_no_eastern_ports(lg, node) {
                    paths_to_random += self.info(current_node).connected_edges;
                }

                for edge in lg.node_outgoing_edges(node) {
                    paths_to_random += self.info(current_node).random_influence;
                    paths_to_hierarchical += self.info(current_node).hierarchical_influence;
                    self.transfer_info_to_target(lg, current_node, edge);
                }

                let mut north_south_ports: Vec<LPortId> = port_side_view(lg, node, PortSide::NORTH).to_vec();
                north_south_ports.extend_from_slice(port_side_view(lg, node, PortSide::SOUTH));
                for port in north_south_ports {
                    if let Some(ns_dummy) = lg[port].props.get_by_id(PORT_DUMMY).and_then(|v| v.cast::<LNodeId>()) {
                        paths_to_random += self.info(current_node).random_influence;
                        paths_to_hierarchical += self.info(current_node).hierarchical_influence;
                        self.transfer_info_to(lg, current_node, Some(ns_dummy));
                    }
                }
            }

            for &node in &ns_port_dummies {
                let current_node = self.node_info_for(lg, node);
                for edge in lg.node_outgoing_edges(node) {
                    paths_to_random += self.info(current_node).random_influence;
                    paths_to_hierarchical += self.info(current_node).hierarchical_influence;
                    self.transfer_info_to_target(lg, current_node, edge);
                }
            }
            ns_port_dummies.clear();
        }

        let all_paths = (paths_to_random + paths_to_hierarchical) as f64;
        let normalized = if all_paths == 0.0 { f64::INFINITY } else { (paths_to_random - paths_to_hierarchical) as f64 / all_paths };
        normalized >= boundary
    }

    /// Reads a `NodeInfo` object.
    pub fn info(&self, r: NodeInfoRef) -> NodeInfo {
        match r {
            NodeInfoRef::Slot(l, n) => self.node_info[l][n].unwrap_or_default(),
            NodeInfoRef::Temp(i) => i,
        }
    }

    /// Mutates a `NodeInfo` object.
    fn info_mut(&mut self, r: &mut NodeInfoRef, f: impl FnOnce(&mut NodeInfo)) {
        match r {
            NodeInfoRef::Slot(l, n) => {
                if let Some(info) = self.node_info[*l][*n].as_mut() {
                    f(info);
                }
            }
            NodeInfoRef::Temp(info) => f(info),
        }
    }

    /// `fixedPortOrder()`.
    pub fn fixed_port_order(&self, lg: &LGraphArena, graph_data: &GraphInfoHolder) -> bool {
        // `graphData.parent()` is a fresh, property-less node without a parent.
        let constraints = graph_data
            .parent()
            .and_then(|p| lg[p].props.get_by_id(PORT_CONSTRAINTS))
            .and_then(|v| v.cast::<PortConstraints>())
            .unwrap_or(PortConstraints::UNDEFINED);
        constraints.is_order_fixed()
    }

    /// `transferInfoToTarget(_:_:)`.
    pub fn transfer_info_to_target(&mut self, lg: &LGraphArena, current_node: NodeInfoRef, edge: LEdgeId) {
        let target = self.target_node(lg, edge);
        self.transfer_info_to(lg, current_node, target);
    }

    /// `transferInfoTo(_:_:)` (`None` stands for the fresh node Swift makes
    /// for an edge without a target).
    pub fn transfer_info_to(&mut self, lg: &LGraphArena, current_node: NodeInfoRef, target: Option<LNodeId>) {
        let mut target_node_info = match target {
            Some(t) => self.node_info_for(lg, t),
            None => NodeInfoRef::Temp(NodeInfo::default()),
        };
        // `targetNodeInfo.transfer(currentNode)` reads `currentNode` as it is
        // now: when both are the same object the values double.
        let current = self.info(current_node);
        // (A throwaway target object is dropped afterwards, as in Swift.)
        self.info_mut(&mut target_node_info, |t| {
            t.transfer(current);
            t.connected_edges += 1;
        });
    }

    /// `fewerThanTwoInOutEdges()`.
    pub fn fewer_than_two_in_out_edges(&self, lg: &LGraphArena, graph_data: &GraphInfoHolder) -> bool {
        let count = |side: PortSide| graph_data.parent().map_or(0, |p| port_side_view(lg, p, side).len());
        count(PortSide::EAST) < 2 && count(PortSide::WEST) < 2
    }

    /// `rootNode()`.
    pub fn root_node(&self, graph_data: &GraphInfoHolder) -> bool {
        !graph_data.has_parent
    }

    /// `bottomUpForced(_:)`.
    pub fn bottom_up_forced(&self, boundary: f64) -> bool {
        boundary < -1.0
    }

    /// `targetNode(_:)`.
    pub fn target_node(&self, lg: &LGraphArena, edge: LEdgeId) -> Option<LNodeId> {
        lg.edge_target_node(edge)
    }

    /// `hasNoEasternPorts(_:)`.
    pub fn has_no_eastern_ports(&self, lg: &LGraphArena, node: LNodeId) -> bool {
        let east_ports = port_side_view(lg, node, PortSide::EAST);
        east_ports.is_empty() || !east_ports.iter().any(|&p| !lg[p].incoming_edges.is_empty() || !lg[p].outgoing_edges.is_empty())
    }

    /// `hasNoWesternPorts(_:)`.
    pub fn has_no_western_ports(&self, lg: &LGraphArena, node: LNodeId) -> bool {
        let west_ports = port_side_view(lg, node, PortSide::WEST);
        west_ports.is_empty() || !west_ports.iter().any(|&p| !lg[p].incoming_edges.is_empty() || !lg[p].outgoing_edges.is_empty())
    }

    /// `isExternalPortDummy(_:)`.
    pub fn is_external_port_dummy(&self, lg: &LGraphArena, node: LNodeId) -> bool {
        lg[node].node_type == NodeType::EXTERNAL_PORT
    }

    /// `isNorthSouthDummy(_:)`.
    pub fn is_north_south_dummy(&self, lg: &LGraphArena, node: LNodeId) -> bool {
        lg[node].node_type == NodeType::NORTH_SOUTH_PORT
    }

    /// `isEasternDummy(_:)`.
    pub fn is_eastern_dummy(&self, lg: &LGraphArena, node: LNodeId) -> bool {
        self.origin_port(lg, node).map(|p| lg[p].side) == Some(PortSide::EAST)
    }

    /// `originPort(_:)`: reads the string key `"org.eclipse.elk.layered.origin"`.
    pub fn origin_port(&self, lg: &LGraphArena, node: LNodeId) -> Option<LPortId> {
        lg[node].props.get_by_id(ORIGIN).and_then(|v| v.cast::<LPortId>())
    }

    /// `nodeInfoFor(_:)`: the table slot of the node, else a throwaway.
    pub fn node_info_for(&mut self, lg: &LGraphArena, node: LNodeId) -> NodeInfoRef {
        let Some(layer) = lg[node].layer else { return NodeInfoRef::Temp(NodeInfo::default()) };
        let layer_index = lg[layer].id;
        let node_id = lg[node].id;
        if layer_index < 0 || layer_index as usize >= self.node_info.len() || node_id < 0 || node_id as usize >= self.node_info[layer_index as usize].len() {
            return NodeInfoRef::Temp(NodeInfo::default());
        }
        let (l, n) = (layer_index as usize, node_id as usize);
        if self.node_info[l][n].is_none() {
            self.node_info[l][n] = Some(NodeInfo::default());
        }
        NodeInfoRef::Slot(l, n)
    }

    /// `initAtLayerLevel(_:_:)`: numbers the layer.
    pub fn init_at_layer_level(&mut self, lg: &mut LGraphArena, l: usize, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() {
            return;
        }
        if let Some(&first) = node_order[l].first() {
            if let Some(layer) = lg[first].layer {
                lg[layer].id = l as i32;
            }
        }
        self.node_info[l] = vec![None; node_order[l].len()];
    }

    /// `initAtNodeLevel(_:_:_:)`: numbers the node.
    pub fn init_at_node_level(&mut self, lg: &mut LGraphArena, l: usize, n: usize, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() || n >= node_order[l].len() {
            return;
        }
        let node = node_order[l][n];
        lg[node].id = n as i32;
        self.node_info[l][n] = Some(NodeInfo::default());
    }
}
