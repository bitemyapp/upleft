//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/greedyswitch/org_eclipse_elk_alg_layered_intermediate_greedyswitch_SwitchDecider.swift`.
//!
//! Decides whether switching two neighbouring nodes of the free layer
//! reduces crossings.
//!
//! In Java, `freeLayer` is the same array the heuristic swaps nodes in; the
//! Swift keeps a copy and `notifyOfSwitch` swaps it too. The two in-layer
//! counters share the heuristic's port-position array, and the parent
//! counter shares the parent `GraphInfoHolder`'s (all `SharedIntArray`s).
//! The Swift keeps a reference to its `GraphInfoHolder`; it only reads it in
//! the initializer, so the port takes a [`GraphDataView`] there.

use super::crossing_matrix_filler::CrossingMatrixFiller;
use super::north_south_edge_neighbouring_node_crossings_counter::NorthSouthEdgeNeighbouringNodeCrossingsCounter;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::p3order::counting::cross_min_util::port_side_view;
use crate::org::eclipse::elk::alg::layered::p3order::counting::crossings_counter::CrossingsCounter;
use crate::org::eclipse::elk::alg::layered::p3order::counting::shared_int_array::SharedIntArray;
use crate::org::eclipse::elk::alg::layered::p3order::graph_info_holder::GraphDataView;
use crate::org::eclipse::elk::core::options::port_side::PortSide;

/// `SwitchDecider.CrossingCountSide`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrossingCountSide {
    WEST,
    EAST,
}

#[derive(Clone, Debug)]
pub struct SwitchDecider {
    free_layer: Vec<LNodeId>,
    left_in_layer_counter: CrossingsCounter,
    right_in_layer_counter: CrossingsCounter,
    north_south_counter: NorthSouthEdgeNeighbouringNodeCrossingsCounter,
    crossing_matrix_filler: CrossingMatrixFiller,
    parent_cross_counter: Option<CrossingsCounter>,
    count_crossings_caused_by_port_switch: bool,
}

impl SwitchDecider {
    pub fn new(
        lg: &LGraphArena,
        free_layer_index: i64,
        graph: &[Vec<LNodeId>],
        crossing_matrix_filler: CrossingMatrixFiller,
        port_positions: SharedIntArray,
        graph_data: &GraphDataView,
        one_sided: bool,
    ) -> SwitchDecider {
        if free_layer_index >= graph.len() as i64 {
            // assertionFailure("Greedy SwitchDecider: Free layer not in graph.") — a no-op in release.
            return SwitchDecider {
                free_layer: Vec::new(),
                left_in_layer_counter: CrossingsCounter::with_shared(port_positions.clone()),
                right_in_layer_counter: CrossingsCounter::with_shared(port_positions),
                north_south_counter: NorthSouthEdgeNeighbouringNodeCrossingsCounter::new(lg, &[]),
                crossing_matrix_filler,
                parent_cross_counter: None,
                count_crossings_caused_by_port_switch: false,
            };
        }
        let free_layer = graph[free_layer_index as usize].clone();

        let mut left_in_layer_counter = CrossingsCounter::with_shared(port_positions.clone());
        left_in_layer_counter.init_port_positions_for_in_layer_crossings(lg, &free_layer, PortSide::WEST);
        let mut right_in_layer_counter = CrossingsCounter::with_shared(port_positions);
        right_in_layer_counter.init_port_positions_for_in_layer_crossings(lg, &free_layer, PortSide::EAST);
        let north_south_counter = NorthSouthEdgeNeighbouringNodeCrossingsCounter::new(lg, &free_layer);

        let count_crossings_caused_by_port_switch = !one_sided
            && graph_data.has_parent
            && !graph_data.dont_sweep_into
            && !free_layer.is_empty()
            && lg[free_layer[0]].node_type == NodeType::EXTERNAL_PORT;

        let mut decider = SwitchDecider {
            free_layer,
            left_in_layer_counter,
            right_in_layer_counter,
            north_south_counter,
            crossing_matrix_filler,
            parent_cross_counter: None,
            count_crossings_caused_by_port_switch,
        };
        if count_crossings_caused_by_port_switch {
            decider.init_parent_crossings_counters(lg, free_layer_index, graph.len(), graph_data);
        }
        decider
    }

    fn init_parent_crossings_counters(&mut self, lg: &LGraphArena, free_layer_index: i64, length: usize, graph_data: &GraphDataView) {
        let Some(parent_graph_data) = graph_data.parent_graph_data else { return };
        let parent_node_order = &parent_graph_data.current_node_order;
        let port_pos = parent_graph_data.port_positions();
        let mut parent_cross_counter = CrossingsCounter::with_shared(port_pos);
        // `graphData.parent()` is a fresh layerless node when there is no parent.
        let parent_node_layer_pos: i64 = graph_data.parent.and_then(|p| lg[p].layer).map_or(0, |l| lg[l].id as i64);
        let empty: Vec<LNodeId> = Vec::new();
        let left_layer = if parent_node_layer_pos > 0 { &parent_node_order[(parent_node_layer_pos - 1) as usize] } else { &empty };
        let middle_layer = &parent_node_order[parent_node_layer_pos as usize];
        let right_layer = if parent_node_layer_pos < parent_node_order.len() as i64 - 1 {
            &parent_node_order[(parent_node_layer_pos + 1) as usize]
        } else {
            &empty
        };
        let right_most_layer = free_layer_index == length as i64 - 1;
        if right_most_layer {
            parent_cross_counter.init_for_counting_between(lg, middle_layer, right_layer);
        } else {
            parent_cross_counter.init_for_counting_between(lg, left_layer, middle_layer);
        }
        self.parent_cross_counter = Some(parent_cross_counter);
    }

    /// `notifyOfSwitch(_:_:)`.
    pub fn notify_of_switch(&mut self, lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) {
        // Update freeLayer to reflect the swap (Java's LNode[] is a reference type,
        // so swaps in GreedySwitchHeuristic.exchangeNodes are visible there automatically).
        if let (Some(upper_idx), Some(lower_idx)) =
            (self.free_layer.iter().position(|&n| n == upper_node), self.free_layer.iter().position(|&n| n == lower_node))
        {
            self.free_layer.swap(upper_idx, lower_idx);
        }
        self.left_in_layer_counter.switch_nodes(lg, upper_node, lower_node, PortSide::WEST);
        self.right_in_layer_counter.switch_nodes(lg, upper_node, lower_node, PortSide::EAST);
        if self.count_crossings_caused_by_port_switch {
            if let (Some(upper_port), Some(lower_port)) = (Self::origin_port(lg, upper_node), Self::origin_port(lg, lower_node)) {
                if let Some(parent) = self.parent_cross_counter.as_mut() {
                    parent.switch_ports(lg, upper_port, lower_port);
                }
            }
        }
    }

    fn origin_port(lg: &LGraphArena, node: LNodeId) -> Option<LPortId> {
        lg[node].props.get_as::<LPortId>(&InternalProperties::ORIGIN)
    }

    /// `doesSwitchReduceCrossings(_:_:)`.
    pub fn does_switch_reduce_crossings(&mut self, lg: &LGraphArena, upper_node_index: usize, lower_node_index: usize) -> bool {
        if self.constraints_prevent_switch(lg, upper_node_index, lower_node_index) {
            return false;
        }

        let upper_node = self.free_layer[upper_node_index];
        let lower_node = self.free_layer[lower_node_index];

        let left_inlayer = self.left_in_layer_counter.count_in_layer_crossings_between_nodes_in_both_orders(lg, upper_node, lower_node, PortSide::WEST);
        let right_inlayer = self.right_in_layer_counter.count_in_layer_crossings_between_nodes_in_both_orders(lg, upper_node, lower_node, PortSide::EAST);
        self.north_south_counter.count_crossings(lg, upper_node, lower_node);

        let mut upper_lower_crossings = self.crossing_matrix_filler.get_crossing_matrix_entry(lg, upper_node, lower_node)
            + left_inlayer.0
            + right_inlayer.0
            + self.north_south_counter.get_upper_lower_crossings();
        let mut lower_upper_crossings = self.crossing_matrix_filler.get_crossing_matrix_entry(lg, lower_node, upper_node)
            + left_inlayer.1
            + right_inlayer.1
            + self.north_south_counter.get_lower_upper_crossings();

        if self.count_crossings_caused_by_port_switch {
            if let (Some(upper_port), Some(lower_port)) = (Self::origin_port(lg, upper_node), Self::origin_port(lg, lower_node)) {
                if let Some(parent_cc) = self.parent_cross_counter.as_mut() {
                    let crossing_numbers = parent_cc.count_crossings_between_ports_in_both_orders(lg, upper_port, lower_port);
                    upper_lower_crossings += crossing_numbers.0;
                    lower_upper_crossings += crossing_numbers.1;
                }
            }
        }

        upper_lower_crossings > lower_upper_crossings
    }

    fn constraints_prevent_switch(&self, lg: &LGraphArena, node_index: usize, lower_node_index: usize) -> bool {
        let upper_node = self.free_layer[node_index];
        let lower_node = self.free_layer[lower_node_index];

        Self::have_successor_constraints(lg, upper_node, lower_node)
            || Self::have_layout_unit_constraints(lg, upper_node, lower_node)
            || Self::are_normal_and_north_south_port_dummy(lg, upper_node, lower_node)
    }

    fn have_successor_constraints(lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) -> bool {
        let Some(constraints) = lg[upper_node].props.get_as::<std::rc::Rc<Vec<LNodeId>>>(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS) else {
            return false;
        };
        !constraints.is_empty() && constraints.contains(&lower_node)
    }

    fn have_layout_unit_constraints(lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) -> bool {
        let neither_node_is_long_edge_dummy = lg[upper_node].node_type != NodeType::LONG_EDGE && lg[lower_node].node_type != NodeType::LONG_EDGE;

        let upper_layout_unit = lg[upper_node].props.get_as::<LNodeId>(&InternalProperties::IN_LAYER_LAYOUT_UNIT);
        let lower_layout_unit = lg[lower_node].props.get_as::<LNodeId>(&InternalProperties::IN_LAYER_LAYOUT_UNIT);

        let are_in_different_layout_units = upper_layout_unit != lower_layout_unit;

        let mut nodes_have_layout_units =
            Self::part_of_multi_node_layout_unit(upper_node, upper_layout_unit) || Self::part_of_multi_node_layout_unit(lower_node, lower_layout_unit);

        let upper_node_has_northern_edges = Self::has_edges_on_side(lg, upper_node, PortSide::NORTH);
        let lower_node_has_southern_edges = Self::has_edges_on_side(lg, lower_node, PortSide::SOUTH);

        // hotfix for #162
        nodes_have_layout_units = nodes_have_layout_units || Self::has_edges_on_side(lg, upper_node, PortSide::SOUTH) || Self::has_edges_on_side(lg, lower_node, PortSide::NORTH);

        let has_layout_unit_constraint = (nodes_have_layout_units && are_in_different_layout_units) || (upper_node_has_northern_edges || lower_node_has_southern_edges);

        neither_node_is_long_edge_dummy && has_layout_unit_constraint
    }

    fn has_edges_on_side(lg: &LGraphArena, node: LNodeId, side: PortSide) -> bool {
        for &port in port_side_view(lg, node, side) {
            if lg[port].props.get(&InternalProperties::PORT_DUMMY).is_some() || !lg[port].incoming_edges.is_empty() || !lg[port].outgoing_edges.is_empty() {
                return true;
            }
        }
        false
    }

    fn part_of_multi_node_layout_unit(node: LNodeId, layout_unit: Option<LNodeId>) -> bool {
        layout_unit.is_some() && layout_unit != Some(node)
    }

    fn are_normal_and_north_south_port_dummy(lg: &LGraphArena, upper_node: LNodeId, lower_node: LNodeId) -> bool {
        let is_ns = |n: LNodeId| lg[n].node_type == NodeType::NORTH_SOUTH_PORT;
        let is_normal = |n: LNodeId| lg[n].node_type == NodeType::NORMAL;
        (is_ns(upper_node) && is_normal(lower_node)) || (is_ns(lower_node) && is_normal(upper_node))
    }
}
