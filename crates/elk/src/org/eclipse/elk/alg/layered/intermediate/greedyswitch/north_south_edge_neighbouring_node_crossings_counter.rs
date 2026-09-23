//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/greedyswitch/org_eclipse_elk_alg_layered_intermediate_greedyswitch_NorthSouthEdgeNeighbouringNodeCrossingsCounter.swift`.
//!
//! Counts crossings caused by north/south port dummies next to each other or
//! next to long-edge dummies, for two neighbouring nodes in both orders.

use std::collections::HashMap;

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::p3order::counting::cross_min_util::{port_side_view, CrossMinUtil};
use crate::org::eclipse::elk::core::options::port_side::PortSide;

#[derive(Clone, Debug)]
pub struct NorthSouthEdgeNeighbouringNodeCrossingsCounter {
    upper_lower_crossings: i64,
    lower_upper_crossings: i64,
    port_positions: HashMap<LPortId, i64>,
    layer: Vec<LNodeId>,
}

impl NorthSouthEdgeNeighbouringNodeCrossingsCounter {
    pub fn new(lg: &LGraphArena, nodes: &[LNodeId]) -> NorthSouthEdgeNeighbouringNodeCrossingsCounter {
        let mut counter = NorthSouthEdgeNeighbouringNodeCrossingsCounter {
            upper_lower_crossings: 0,
            lower_upper_crossings: 0,
            port_positions: HashMap::new(),
            layer: nodes.to_vec(),
        };
        counter.initialize_port_positions(lg);
        counter
    }

    fn initialize_port_positions(&mut self, lg: &LGraphArena) {
        for i in 0..self.layer.len() {
            let node = self.layer[i];
            self.set_port_ids_on(lg, node, PortSide::SOUTH);
            self.set_port_ids_on(lg, node, PortSide::NORTH);
        }
    }

    fn set_port_ids_on(&mut self, lg: &LGraphArena, node: LNodeId, side: PortSide) {
        let mut port_id = 0;
        for port in CrossMinUtil::in_north_south_east_west_order_iter(lg, node, side) {
            self.port_positions.insert(port, port_id);
            port_id += 1;
        }
    }

    /// `countCrossings(_:_:)`.
    pub fn count_crossings(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        self.upper_lower_crossings = 0;
        self.lower_upper_crossings = 0;

        self.process_if_two_north_south_nodes(lg, upper_node, lower_node);
        self.process_if_north_south_long_edge_dummy_crossing(lg, upper_node, lower_node);
        self.process_if_normal_node_with_ns_ports_and_long_edge_dummy(lg, upper_node, lower_node);
    }

    fn process_if_two_north_south_nodes(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        if Self::is_north_south(lg, upper_node) && Self::is_north_south(lg, lower_node) && !Self::have_different_origins(lg, upper_node, lower_node) {
            if Self::is_north_of_normal_node(lg, upper_node) {
                self.count_crossings_of_two_north_south_dummies(lg, upper_node, lower_node);
            } else {
                self.count_crossings_of_two_north_south_dummies(lg, lower_node, upper_node);
            }
        }
    }

    fn count_crossings_of_two_north_south_dummies(&mut self, lg: &LGraphArena, further_from_normal_node: LNodeId, closer_to_normal_node: LNodeId) {
        let first_degree = |node: LNodeId, side: PortSide| -> i64 {
            match port_side_view(lg, node, side).first() {
                Some(&p) => (lg[p].incoming_edges.len() + lg[p].outgoing_edges.len()) as i64,
                None => 0,
            }
        };
        if self.origin_port_position_of(lg, further_from_normal_node) > self.origin_port_position_of(lg, closer_to_normal_node) {
            self.upper_lower_crossings = first_degree(closer_to_normal_node, PortSide::EAST);
            self.lower_upper_crossings = first_degree(further_from_normal_node, PortSide::WEST);
        } else {
            self.upper_lower_crossings = first_degree(closer_to_normal_node, PortSide::WEST);
            self.lower_upper_crossings = first_degree(further_from_normal_node, PortSide::EAST);
        }
    }

    fn process_if_north_south_long_edge_dummy_crossing(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        if Self::is_north_south(lg, upper_node) && Self::is_long_edge_dummy(lg, lower_node) {
            if Self::is_north_of_normal_node(lg, upper_node) {
                self.upper_lower_crossings = 1;
            } else {
                self.lower_upper_crossings = 1;
            }
        } else if Self::is_north_south(lg, lower_node) && Self::is_long_edge_dummy(lg, upper_node) {
            if Self::is_north_of_normal_node(lg, lower_node) {
                self.lower_upper_crossings = 1;
            } else {
                self.upper_lower_crossings = 1;
            }
        }
    }

    fn process_if_normal_node_with_ns_ports_and_long_edge_dummy(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        if Self::is_normal(lg, upper_node) && Self::is_long_edge_dummy(lg, lower_node) {
            self.upper_lower_crossings = Self::number_of_north_south_edges(lg, upper_node, PortSide::SOUTH);
            self.lower_upper_crossings = Self::number_of_north_south_edges(lg, upper_node, PortSide::NORTH);
        }
        if Self::is_normal(lg, lower_node) && Self::is_long_edge_dummy(lg, upper_node) {
            self.upper_lower_crossings = Self::number_of_north_south_edges(lg, lower_node, PortSide::NORTH);
            self.lower_upper_crossings = Self::number_of_north_south_edges(lg, lower_node, PortSide::SOUTH);
        }
    }

    fn number_of_north_south_edges(lg: &LGraphArena, node: LNodeId, side: PortSide) -> i64 {
        port_side_view(lg, node, side).iter().filter(|&&p| Self::has_connected_north_south_edge(lg, p)).count() as i64
    }

    fn has_connected_north_south_edge(lg: &LGraphArena, port: LPortId) -> bool {
        lg[port].props.get(&InternalProperties::PORT_DUMMY).is_some()
    }

    fn have_different_origins(lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) -> bool {
        Self::origin_of(lg, upper_node) != Self::origin_of(lg, lower_node)
    }

    fn origin_port_position_of(&self, lg: &LGraphArena, node: LNodeId) -> i64 {
        let origin = Self::origin_port_of(lg, node);
        self.port_positions.get(&origin).copied().unwrap_or(0)
    }

    /// `originPortOf(_:)`: the first port's origin port, else the port
    /// (traps for a node without ports).
    fn origin_port_of(lg: &LGraphArena, node: LNodeId) -> LPortId {
        let port = lg[node].ports[0];
        lg[port].props.get_as::<LPortId>(&InternalProperties::ORIGIN).unwrap_or(port)
    }

    fn is_north_of_normal_node(lg: &LGraphArena, node: LNodeId) -> bool {
        lg[Self::origin_port_of(lg, node)].side == PortSide::NORTH
    }

    fn origin_of(lg: &LGraphArena, node: LNodeId) -> Option<LNodeId> {
        lg[node].props.get_as::<LNodeId>(&InternalProperties::ORIGIN)
    }

    fn is_long_edge_dummy(lg: &LGraphArena, node: LNodeId) -> bool {
        lg[node].node_type == NodeType::LONG_EDGE
    }

    fn is_north_south(lg: &LGraphArena, node: LNodeId) -> bool {
        lg[node].node_type == NodeType::NORTH_SOUTH_PORT
    }

    fn is_normal(lg: &LGraphArena, node: LNodeId) -> bool {
        lg[node].node_type == NodeType::NORMAL
    }

    /// `getUpperLowerCrossings()`.
    pub fn get_upper_lower_crossings(&self) -> i64 {
        self.upper_lower_crossings
    }

    /// `getLowerUpperCrossings()`.
    pub fn get_lower_upper_crossings(&self) -> i64 {
        self.lower_upper_crossings
    }
}
