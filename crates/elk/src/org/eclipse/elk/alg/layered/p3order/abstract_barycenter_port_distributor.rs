//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_AbstractBarycenterPortDistributor.swift`.
//!
//! The Swift abstract class has two subclasses that only override the
//! per-node `calculatePortRanks(_:_:_:)`: `NodeRelativePortDistributor` and
//! `LayerTotalPortDistributor`. The port is one struct with a
//! [`BarycenterPortDistributorKind`] tag; the overrides live in their own
//! modules.
//!
//! One instance is shared between `GraphInfoHolder` (which sweeps with it)
//! and `BarycenterHeuristic` (which asks it for port ranks), so it is held as
//! `Rc<RefCell<AbstractBarycenterPortDistributor>>`.

use super::counting::i_initializable::IInitializable;
use super::i_sweep_port_distributor::ISweepPortDistributor;
use super::layer_total_port_distributor::LayerTotalPortDistributor;
use super::node_relative_port_distributor::NodeRelativePortDistributor;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::graph::properties::keys;
use crate::org::eclipse::elk::graph::properties::property::Property;
use crate::swift;

/// `PORT_CONSTRAINTS_KEY`: `Property<PortConstraints>("org.eclipse.elk.portConstraints")`, no default.
pub static PORT_CONSTRAINTS_KEY: Property = Property::new(keys::ELK_PORT_CONSTRAINTS);
/// `SORTED_PORTS_KEY`: `Property<[LPort]>("org.eclipse.elk.alg.layered.p3order.sortedPorts.todo")`.
pub static SORTED_PORTS_KEY: Property = Property::new(keys::ELK_ALG_LAYERED_P3ORDER_SORTED_PORTS_TODO);
// `PORT_DUMMY_KEY` (`"portDummy"`) and `ORIGIN_KEY` (`"origin"`) have the ids
// of `InternalProperties.PORT_DUMMY` and `.ORIGIN` and no default, so those
// are used directly.

/// Which subclass an [`AbstractBarycenterPortDistributor`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarycenterPortDistributorKind {
    NodeRelative,
    LayerTotal,
}

#[derive(Clone, Debug)]
pub struct AbstractBarycenterPortDistributor {
    pub kind: BarycenterPortDistributorKind,
    pub port_ranks: Vec<f32>,
    pub min_barycenter: f32,
    pub max_barycenter: f32,
    pub node_positions: Vec<Vec<i64>>,
    pub port_barycenter: Vec<f32>,
    pub in_layer_ports: Vec<LPortId>,
    pub n_ports: i64,
}

impl IInitializable for AbstractBarycenterPortDistributor {}

impl AbstractBarycenterPortDistributor {
    /// `init(_ numLayers:)`.
    pub fn new(kind: BarycenterPortDistributorKind, num_layers: i64) -> AbstractBarycenterPortDistributor {
        AbstractBarycenterPortDistributor {
            kind,
            port_ranks: Vec::new(),
            min_barycenter: 0.0,
            max_barycenter: 0.0,
            node_positions: vec![Vec::new(); swift::max(0, num_layers) as usize],
            port_barycenter: Vec::new(),
            in_layer_ports: Vec::new(),
            n_ports: 0,
        }
    }

    /// `getPortRanks()`.
    pub fn get_port_ranks(&self) -> &[f32] {
        &self.port_ranks
    }

    /// `setPortRank(_:_:)`.
    pub fn set_port_rank(&mut self, port_id: i64, rank: f32) {
        self.ensure_port_arrays_contain(port_id);
        self.port_ranks[port_id as usize] = rank;
    }

    /// `calculatePortRanks(_ layer:, _ portType:)` (final in Swift).
    pub fn calculate_port_ranks(&mut self, lg: &LGraphArena, layer: &[LNodeId], port_type: PortType) {
        let mut consumed_rank: f32 = 0.0;
        for &node in layer {
            consumed_rank += self.calculate_port_ranks_for_node(lg, node, consumed_rank, port_type);
        }
    }

    /// `calculatePortRanks(_ node:, _ rankSum:, _ type:)`: the subclass override.
    pub fn calculate_port_ranks_for_node(&mut self, lg: &LGraphArena, node: LNodeId, rank_sum: f32, port_type: PortType) -> f32 {
        match self.kind {
            BarycenterPortDistributorKind::NodeRelative => NodeRelativePortDistributor::calculate_port_ranks(self, lg, node, rank_sum, port_type),
            BarycenterPortDistributorKind::LayerTotal => LayerTotalPortDistributor::calculate_port_ranks(self, lg, node, rank_sum, port_type),
        }
    }

    /// `distributePorts(_ node:, _ side:)`.
    pub fn distribute_ports(&mut self, lg: &mut LGraphArena, node: LNodeId, side: PortSide) {
        let pc = self.port_constraints_of(lg, node);
        if !pc.is_order_fixed() {
            let ports = lg.node_ports_on_side(node, side);
            self.distribute_ports_list(lg, node, &ports);
            let ports = lg.node_ports_on_side(node, PortSide::SOUTH);
            self.distribute_ports_list(lg, node, &ports);
            let ports = lg.node_ports_on_side(node, PortSide::NORTH);
            self.distribute_ports_list(lg, node, &ports);
            self.sort_ports(lg, node);
        }
    }

    /// `distributePorts(_ node:, _ ports:)`.
    pub fn distribute_ports_list(&mut self, lg: &LGraphArena, node: LNodeId, ports: &[LPortId]) {
        self.in_layer_ports.clear();
        self.iterate_ports_and_collect_in_layer_ports(lg, node, ports);
        if !self.in_layer_ports.is_empty() {
            self.calculate_in_layer_ports_barycenter_values(lg, node);
        }
    }

    /// `iteratePortsAndCollectInLayerPorts(_:_:)`.
    pub fn iterate_ports_and_collect_in_layer_ports(&mut self, lg: &LGraphArena, node: LNodeId, ports: &[LPortId]) {
        self.min_barycenter = 0.0;
        self.max_barycenter = 0.0;

        let node_layer = lg[node].layer;
        let layer_size = node_layer.map_or(0, |l| lg[l].nodes.len() as i64);
        let absurdly_large_float = (2 * layer_size + 1) as f32;

        'port_loop: for &port in ports {
            let north_south_port = lg[port].side == PortSide::NORTH || lg[port].side == PortSide::SOUTH;
            let mut sum: f32 = 0.0;

            if north_south_port {
                let Some(port_dummy) = lg[port].props.get_typed::<LNodeId>(&InternalProperties::PORT_DUMMY) else {
                    continue;
                };
                let ns_result = self.deal_with_north_south_ports(lg, absurdly_large_float, port, port_dummy);
                sum += ns_result;
            } else {
                for &outgoing_edge in &lg[port].outgoing_edges {
                    let Some(connected_port) = lg[outgoing_edge].target else { continue };
                    let Some(connected_node) = lg[connected_port].owner else { continue };
                    let Some(connected_layer) = lg[connected_node].layer else { continue };
                    let Some(node_layer) = node_layer else { continue };
                    let same_layer = connected_layer == node_layer;
                    let rank = self.rank_of_port(lg, connected_port);
                    if same_layer {
                        self.in_layer_ports.push(port);
                        continue 'port_loop;
                    } else {
                        sum += rank;
                    }
                }

                for &incoming_edge in &lg[port].incoming_edges {
                    let Some(connected_port) = lg[incoming_edge].source else { continue };
                    let Some(connected_node) = lg[connected_port].owner else { continue };
                    let Some(connected_layer) = lg[connected_node].layer else { continue };
                    let Some(node_layer) = node_layer else { continue };
                    let same_layer = connected_layer == node_layer;
                    let rank = self.rank_of_port(lg, connected_port);
                    if same_layer {
                        self.in_layer_ports.push(port);
                        continue 'port_loop;
                    } else {
                        sum -= rank;
                    }
                }
            }

            let degree = (lg[port].incoming_edges.len() + lg[port].outgoing_edges.len()) as i64;
            let port_id = lg[port].id as i64;
            self.ensure_port_arrays_contain(port_id);
            if degree > 0 {
                self.port_barycenter[port_id as usize] = sum / degree as f32;
                self.min_barycenter = swift::min(self.min_barycenter, self.port_barycenter[port_id as usize]);
                self.max_barycenter = swift::max(self.max_barycenter, self.port_barycenter[port_id as usize]);
            } else if north_south_port {
                self.port_barycenter[port_id as usize] = sum;
            }
        }
    }

    /// `calculateInLayerPortsBarycenterValues(_:)`.
    pub fn calculate_in_layer_ports_barycenter_values(&mut self, lg: &LGraphArena, node: LNodeId) {
        let node_index_in_layer = self.position_of(lg, node) + 1;
        let node_layer = lg[node].layer;
        let layer_size = node_layer.map_or(0, |l| lg[l].nodes.len() as i64) + 1;

        for i in 0..self.in_layer_ports.len() {
            let in_layer_port = self.in_layer_ports[i];
            let mut sum: i64 = 0;
            let mut in_layer_connections: i64 = 0;
            // `getConnectedPorts()`: predecessors, then successors.
            let p = &lg[in_layer_port];
            let connected = p.incoming_edges.iter().filter_map(|&e| lg[e].source).chain(p.outgoing_edges.iter().filter_map(|&e| lg[e].target));
            for connected_port in connected {
                let Some(connected_node) = lg[connected_port].owner else { continue };
                let Some(connected_layer) = lg[connected_node].layer else { continue };
                let Some(node_layer) = node_layer else { continue };
                if connected_layer == node_layer {
                    sum += self.position_of(lg, connected_node) + 1;
                    in_layer_connections += 1;
                }
            }

            if in_layer_connections <= 0 {
                continue;
            }
            let barycenter = sum as f32 / in_layer_connections as f32;
            let id = lg[in_layer_port].id as i64;
            self.ensure_port_arrays_contain(id);

            let port_side = lg[in_layer_port].side;
            if port_side == PortSide::EAST {
                if barycenter < node_index_in_layer as f32 {
                    self.port_barycenter[id as usize] = self.min_barycenter - barycenter;
                } else {
                    self.port_barycenter[id as usize] = self.max_barycenter + (layer_size as f32 - barycenter);
                }
            } else if port_side == PortSide::WEST {
                if barycenter < node_index_in_layer as f32 {
                    self.port_barycenter[id as usize] = self.max_barycenter + barycenter;
                } else {
                    self.port_barycenter[id as usize] = self.min_barycenter - (layer_size as f32 - barycenter);
                }
            }
        }
    }

    /// `dealWithNorthSouthPorts(_:_:_:)`.
    pub fn deal_with_north_south_ports(&self, lg: &LGraphArena, absurdly_large_float: f32, port: LPortId, port_dummy: LNodeId) -> f32 {
        let mut input = false;
        let mut output = false;

        for &port_dummy_port in &lg[port_dummy].ports {
            let origin = lg[port_dummy_port].props.get_typed::<LPortId>(&InternalProperties::ORIGIN);
            if origin == Some(port) {
                if !lg[port_dummy_port].outgoing_edges.is_empty() {
                    output = true;
                } else if !lg[port_dummy_port].incoming_edges.is_empty() {
                    input = true;
                }
            }
        }

        if input && (input != output) {
            let pos = self.position_of(lg, port_dummy) as f32;
            return if lg[port].side == PortSide::NORTH { -pos } else { absurdly_large_float - pos };
        } else if output && (input != output) {
            return self.position_of(lg, port_dummy) as f32 + 1.0;
        } else if input && output {
            return if lg[port].side == PortSide::NORTH { 0.0 } else { absurdly_large_float / 2.0 };
        }
        0.0
    }

    /// `positionOf(_:)`: the node's position in its layer as last recorded.
    pub fn position_of(&self, lg: &LGraphArena, node: LNodeId) -> i64 {
        let Some(layer) = lg[node].layer else { return 0 };
        let layer_id = lg[layer].id;
        if layer_id < 0 || layer_id as usize >= self.node_positions.len() {
            return 0;
        }
        let row = &self.node_positions[layer_id as usize];
        let node_id = lg[node].id;
        if node_id < 0 || node_id as usize >= row.len() {
            return 0;
        }
        row[node_id as usize]
    }

    /// `updateNodePositions(_:_:)`.
    pub fn update_node_positions(&mut self, lg: &LGraphArena, node_order: &[Vec<LNodeId>], current_index: usize) {
        let layer = &node_order[current_index];
        for (i, &node) in layer.iter().enumerate() {
            let Some(l) = lg[node].layer else { continue };
            let layer_id = lg[l].id as i64;
            let node_id = lg[node].id as i64;
            self.ensure_node_position_row(layer_id, swift::max(node_id + 1, layer.len() as i64));
            // Traps for a negative id, like the Swift subscript.
            self.node_positions[layer_id as usize][node_id as usize] = i as i64;
        }
    }

    /// `hasNestedGraph(_:)`.
    pub fn has_nested_graph(&self, lg: &LGraphArena, node: LNodeId) -> bool {
        lg[node].nested_graph.is_some()
    }

    /// `isNotFirstLayer(_:_:_:)`.
    pub fn is_not_first_layer(&self, length: usize, current_index: usize, is_forward_sweep: bool) -> bool {
        if is_forward_sweep {
            current_index != 0
        } else {
            current_index as i64 != swift::max(0, length as i64 - 1)
        }
    }

    /// `portTypeFor(_:)`.
    pub fn port_type_for(&self, is_forward_sweep: bool) -> PortType {
        if is_forward_sweep { PortType::OUTPUT } else { PortType::INPUT }
    }

    /// `sortPorts(_:)`: orders the ports by side, then by barycenter.
    pub fn sort_ports(&mut self, lg: &mut LGraphArena, node: LNodeId) {
        let sorted = swift::sorted_by(lg[node].ports.iter().copied(), |&port1, &port2| {
            let side1 = lg[port1].side;
            let side2 = lg[port2].side;
            if side1 != side2 {
                return Self::side_ordinal(side1) < Self::side_ordinal(side2);
            }
            // Match Java's Float.compare semantics: straightforward numeric comparison
            let p1 = self.barycenter_of(lg, port1);
            let p2 = self.barycenter_of(lg, port2);
            p1 < p2
        });

        lg[node].ports = sorted.clone();
        lg[node].props.set(&SORTED_PORTS_KEY, sorted);
    }

    /// `sideOrdinal(_:)`.
    pub fn side_ordinal(side: PortSide) -> i64 {
        match side {
            PortSide::UNDEFINED => 0,
            PortSide::NORTH => 1,
            PortSide::EAST => 2,
            PortSide::SOUTH => 3,
            PortSide::WEST => 4,
        }
    }

    /// `initAtLayerLevel(_:_:)` (the Swift `GraphInfoHolder` never calls it).
    pub fn init_at_layer_level(&mut self, l: usize, node_order: &[Vec<LNodeId>]) {
        let node_count = if l < node_order.len() { node_order[l].len() } else { 0 };
        self.ensure_node_position_row(l as i64, node_count as i64);
    }

    /// `initAtNodeLevel(_:_:_:)`: numbers the node.
    pub fn init_at_node_level(&mut self, lg: &mut LGraphArena, l: usize, n: usize, node_order: &[Vec<LNodeId>]) {
        let node = node_order[l][n];
        lg[node].id = n as i32;
        self.ensure_node_position_row(l as i64, node_order[l].len() as i64);
        self.node_positions[l][n] = n as i64;
    }

    /// `initAtPortLevel(_:_:_:_:)`: numbers the port.
    pub fn init_at_port_level(&mut self, lg: &mut LGraphArena, l: usize, n: usize, p: usize, node_order: &[Vec<LNodeId>]) {
        let node = node_order[l][n];
        let Some(&port) = lg[node].ports.get(p) else { return };
        lg[port].id = self.n_ports as i32;
        self.n_ports += 1;
    }

    /// `initAfterTraversal()`.
    pub fn init_after_traversal(&mut self) {
        self.port_ranks = vec![0.0; self.n_ports as usize];
        self.port_barycenter = vec![0.0; self.n_ports as usize];
    }

    /// `ensureNodePositionRow(layerId:nodeCount:)`.
    pub fn ensure_node_position_row(&mut self, layer_id: i64, node_count: i64) {
        if layer_id < 0 {
            return;
        }
        let layer_id = layer_id as usize;
        if layer_id >= self.node_positions.len() {
            self.node_positions.resize(layer_id + 1, Vec::new());
        }
        let row = &mut self.node_positions[layer_id];
        if (row.len() as i64) < node_count {
            row.resize(node_count as usize, 0);
        }
    }

    /// `ensurePortArraysContain(_:)`.
    pub fn ensure_port_arrays_contain(&mut self, port_id: i64) {
        if port_id < 0 {
            return;
        }
        let needed = port_id as usize + 1;
        if self.port_ranks.len() < needed {
            self.port_ranks.resize(needed, 0.0);
        }
        if self.port_barycenter.len() < needed {
            self.port_barycenter.resize(needed, 0.0);
        }
    }

    /// `rankOfPort(_:)`.
    pub fn rank_of_port(&self, lg: &LGraphArena, port: LPortId) -> f32 {
        let id = lg[port].id;
        if id < 0 || id as usize >= self.port_ranks.len() {
            return 0.0;
        }
        self.port_ranks[id as usize]
    }

    /// `barycenterOf(_:)`.
    pub fn barycenter_of(&self, lg: &LGraphArena, port: LPortId) -> f32 {
        let id = lg[port].id;
        if id < 0 || id as usize >= self.port_barycenter.len() {
            return 0.0;
        }
        self.port_barycenter[id as usize]
    }

    /// `portConstraintsOf(_:)`.
    pub fn port_constraints_of(&self, lg: &LGraphArena, node: LNodeId) -> PortConstraints {
        lg[node].props.get_as::<PortConstraints>(&PORT_CONSTRAINTS_KEY).unwrap_or(PortConstraints::UNDEFINED)
    }
}

impl ISweepPortDistributor for AbstractBarycenterPortDistributor {
    /// `distributePortsWhileSweeping(_:_:_:)`.
    fn distribute_ports_while_sweeping(&mut self, lg: &mut LGraphArena, node_order: &[Vec<LNodeId>], current_index: usize, is_forward_sweep: bool) -> bool {
        self.update_node_positions(lg, node_order, current_index);
        let free_layer = &node_order[current_index];
        let side = if is_forward_sweep { PortSide::WEST } else { PortSide::EAST };

        if self.is_not_first_layer(node_order.len(), current_index, is_forward_sweep) {
            let fixed_layer = &node_order[if is_forward_sweep { current_index - 1 } else { current_index + 1 }];
            let port_type = self.port_type_for(is_forward_sweep);
            self.calculate_port_ranks(lg, fixed_layer, port_type);
            for &node in free_layer {
                self.distribute_ports(lg, node, side);
            }

            let port_type = self.port_type_for(!is_forward_sweep);
            self.calculate_port_ranks(lg, free_layer, port_type);
            for &node in fixed_layer {
                if !self.has_nested_graph(lg, node) {
                    self.distribute_ports(lg, node, side.opposed());
                }
            }
        } else {
            for &node in free_layer {
                self.distribute_ports(lg, node, side);
            }
        }

        // Java behavior: barycenter port distributors do not guarantee monotonic improvement.
        false
    }
}
