//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_GreedyPortDistributor.swift`.
//!
//! Orders the ports of each node side by greedily switching neighbouring
//! ports while that reduces crossings.

use super::counting::cross_min_util::port_side_view;
use super::counting::crossings_counter::CrossingsCounter;
use super::counting::i_initializable::IInitializable;
use super::i_sweep_port_distributor::ISweepPortDistributor;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::intermediate::greedyswitch::between_layer_edge_two_node_crossings_counter::BetweenLayerEdgeTwoNodeCrossingsCounter;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

const PORT_CONSTRAINTS_KEY: &str = "org.eclipse.elk.portConstraints";
const PORT_DUMMY_KEY: &str = "portDummy";

#[derive(Clone, Debug, Default)]
pub struct GreedyPortDistributor {
    pub crossings_counter: CrossingsCounter,
    pub n_ports: i64,
    pub port_pos: Vec<i64>,
    pub hierarchical_crossings_counter: Option<BetweenLayerEdgeTwoNodeCrossingsCounter>,
}

impl IInitializable for GreedyPortDistributor {}

impl GreedyPortDistributor {
    pub fn new() -> GreedyPortDistributor {
        GreedyPortDistributor::default()
    }

    /// `distributePortsInLayer(_:_:_:)`.
    pub fn distribute_ports_in_layer(&mut self, lg: &mut LGraphArena, node_order: &[Vec<LNodeId>], current_index: usize, is_forward_sweep: bool) -> bool {
        if current_index >= node_order.len() {
            return false;
        }

        let side = if is_forward_sweep { PortSide::WEST } else { PortSide::EAST };
        let mut improved = false;

        for &node in &node_order[current_index] {
            // (The Swift computes unused debug labels here.)
            let pc = self.port_constraints(lg, node);
            if pc.is_order_fixed() {
                continue;
            }

            let nested_graph = lg[node].nested_graph;
            let use_hierarchical_cross_counter = !port_side_view(lg, node, side).is_empty() && nested_graph.is_some();

            if use_hierarchical_cross_counter {
                let nested_graph = nested_graph.unwrap();
                let inner_graph = lg.graph_to_node_array(nested_graph);
                let relevant_layer = if is_forward_sweep { 0 } else { swift::max(0, inner_graph.len() as i64 - 1) };
                self.hierarchical_crossings_counter = Some(BetweenLayerEdgeTwoNodeCrossingsCounter::new(lg, &inner_graph, relevant_layer));
            }

            improved = self.distribute_ports_on_node(lg, node, side, use_hierarchical_cross_counter) || improved;
        }

        improved
    }

    /// `distributePortsOnNode(_:_:_:)`.
    pub fn distribute_ports_on_node(&mut self, lg: &mut LGraphArena, node: LNodeId, side: PortSide, use_hierarchical_crosscounter: bool) -> bool {
        // In Java, getPortSideView returns a live subList view; the Swift copy
        // is written back after the greedy swap loop.
        let mut ports: Vec<LPortId> = port_side_view(lg, node, side).to_vec();
        let reversed = side == PortSide::SOUTH || side == PortSide::WEST;
        if reversed {
            ports.reverse();
        }

        let mut improved = false;
        loop {
            let mut continue_switching = false;
            if ports.len() > 1 {
                let mut i = 0;
                while i < ports.len() - 1 {
                    let upper_port = ports[i];
                    let lower_port = ports[i + 1];

                    if self.switching_decreases_crossings(lg, upper_port, lower_port, node, use_hierarchical_crosscounter) {
                        improved = true;
                        self.switch_ports(lg, &mut ports, node, i as i64, (i + 1) as i64);
                        continue_switching = true;
                    }

                    i += 1;
                }
            }
            if !continue_switching {
                break;
            }
        }

        // Write back: propagate reordered ports to the node's actual port list.
        if improved {
            if reversed {
                ports.reverse();
            }
            lg.node_set_port_side_view(node, side, &ports);
        }

        improved
    }

    /// `initForLayers(_:_:)`.
    pub fn init_for_layers(&mut self, lg: &LGraphArena, left_layer: &[LNodeId], right_layer: &[LNodeId]) {
        self.crossings_counter.init_for_counting_between(lg, left_layer, right_layer);
    }

    /// `switchingDecreasesCrossings(_:_:_:_:)`.
    pub fn switching_decreases_crossings(&mut self, lg: &LGraphArena, upper_port: LPortId, lower_port: LPortId, _node: LNodeId, use_hierarchical_crosscounter: bool) -> bool {
        let original_and_switched = self.crossings_counter.count_crossings_between_ports_in_both_orders(lg, upper_port, lower_port);
        let mut upper_lower_crossings = original_and_switched.0;
        let mut lower_upper_crossings = original_and_switched.1;

        if use_hierarchical_crosscounter {
            let upper_node = lg[upper_port].props.get_by_id(PORT_DUMMY_KEY).and_then(|v| v.cast::<LNodeId>());
            let lower_node = lg[lower_port].props.get_by_id(PORT_DUMMY_KEY).and_then(|v| v.cast::<LNodeId>());

            if let (Some(upper_node), Some(lower_node), Some(counter)) = (upper_node, lower_node, self.hierarchical_crossings_counter.as_mut()) {
                counter.count_both_side_crossings(lg, upper_node, lower_node);
                upper_lower_crossings += counter.get_upper_lower_crossings();
                lower_upper_crossings += counter.get_lower_upper_crossings();
            }
        }

        upper_lower_crossings > lower_upper_crossings
    }

    /// `switchPorts(_:_:_:_:)`.
    pub fn switch_ports(&mut self, lg: &LGraphArena, ports: &mut [LPortId], _node: LNodeId, top_port: i64, bottom_port: i64) {
        if top_port < 0 || bottom_port < 0 || top_port as usize >= ports.len() || bottom_port as usize >= ports.len() {
            return;
        }
        let (top, bottom) = (top_port as usize, bottom_port as usize);
        self.crossings_counter.switch_ports(lg, ports[top], ports[bottom]);
        ports.swap(top, bottom);
    }

    /// `initialize(_:_:_:)`.
    pub fn initialize(&mut self, lg: &LGraphArena, node_order: &[Vec<LNodeId>], current_index: usize, is_forward_sweep: bool) {
        if is_forward_sweep && current_index > 0 {
            self.init_for_layers(lg, &node_order[current_index - 1], &node_order[current_index]);
        } else if !is_forward_sweep && (current_index as i64) < node_order.len() as i64 - 1 {
            self.init_for_layers(lg, &node_order[current_index], &node_order[current_index + 1]);
        } else if current_index < node_order.len() {
            self.crossings_counter.init_port_positions_for_in_layer_crossings(
                lg,
                &node_order[current_index],
                if is_forward_sweep { PortSide::WEST } else { PortSide::EAST },
            );
        }
    }

    /// `initAtNodeLevel(_:_:_:)`.
    pub fn init_at_node_level(&mut self, lg: &LGraphArena, l: usize, n: usize, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() || n >= node_order[l].len() {
            return;
        }
        let node = node_order[l][n];
        self.n_ports += lg[node].ports.len() as i64;
    }

    /// `initAfterTraversal()`.
    pub fn init_after_traversal(&mut self) {
        self.port_pos = vec![0; self.n_ports as usize];
        self.crossings_counter = CrossingsCounter::from_values(self.port_pos.clone());
    }

    /// `portConstraints(of:)`.
    pub fn port_constraints(&self, lg: &LGraphArena, node: LNodeId) -> PortConstraints {
        lg[node].props.get_by_id(PORT_CONSTRAINTS_KEY).and_then(|v| v.cast::<PortConstraints>()).unwrap_or(PortConstraints::UNDEFINED)
    }

    /// `portConstraintsKey()`.
    pub fn port_constraints_key(&self) -> &'static str {
        PORT_CONSTRAINTS_KEY
    }

    /// `portDummyKey()`.
    pub fn port_dummy_key(&self) -> &'static str {
        PORT_DUMMY_KEY
    }
}

impl ISweepPortDistributor for GreedyPortDistributor {
    /// `distributePortsWhileSweeping(_:_:_:)`.
    fn distribute_ports_while_sweeping(&mut self, lg: &mut LGraphArena, node_order: &[Vec<LNodeId>], current_index: usize, is_forward_sweep: bool) -> bool {
        self.initialize(lg, node_order, current_index, is_forward_sweep);
        self.distribute_ports_in_layer(lg, node_order, current_index, is_forward_sweep)
    }
}
