//! Port of `alg/layered/intermediate/PortListSorter.swift`.
//!
//! Sorts each node's port list by side (clockwise from north) and, for fixed
//! order constraints, by position/index within a side; west and south sides
//! are then reversed so that ports appear in clockwise order. Finally caches
//! each node's port side ranges.

use crate::org::eclipse::elk::alg::layered::options::port_sorting_strategy::PortSortingStrategy;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Default)]
pub struct PortListSorter;

impl PortListSorter {
    pub fn new() -> PortListSorter {
        PortListSorter
    }

    /// `CMP_PORT_SIDE`.
    pub fn cmp_port_side(lg: &LGraphArena, p1: LPortId, p2: LPortId) -> i64 {
        lg[p1].side.ordinal() as i64 - lg[p2].side.ordinal() as i64
    }

    /// `CMP_PORT_DEGREE_EAST_WEST`.
    pub fn cmp_port_degree_east_west(lg: &LGraphArena, p1: LPortId, p2: LPortId) -> i64 {
        let ordinal_difference = lg[p1].side.ordinal() as i64 - lg[p2].side.ordinal() as i64;
        if ordinal_difference != 0 {
            return 0;
        }
        match lg[p1].side {
            PortSide::EAST => Self::real_degree(lg, &lg[p2].outgoing_edges) - Self::real_degree(lg, &lg[p1].outgoing_edges),
            PortSide::WEST => Self::real_degree(lg, &lg[p1].incoming_edges) - Self::real_degree(lg, &lg[p2].incoming_edges),
            _ => 0,
        }
    }

    /// `CMP_FIXED_ORDER_AND_FIXED_POS`.
    pub fn cmp_fixed_order_and_fixed_pos(lg: &LGraphArena, p1: LPortId, p2: LPortId) -> i64 {
        let port_constraints = lg[p1]
            .owner
            .and_then(|n| lg[n].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS))
            .unwrap_or(PortConstraints::UNDEFINED);

        let ordinal_difference = lg[p1].side.ordinal() as i64 - lg[p2].side.ordinal() as i64;
        if ordinal_difference != 0 || !port_constraints.is_order_fixed() {
            return 0;
        }

        if port_constraints == PortConstraints::FIXED_ORDER {
            let index1 = lg[p1].props.get_as::<i64>(&LayeredOptions::PORT_INDEX);
            let index2 = lg[p2].props.get_as::<i64>(&LayeredOptions::PORT_INDEX);
            if let (Some(i1), Some(i2)) = (index1, index2) {
                let index_difference = i1 - i2;
                if index_difference != 0 {
                    return index_difference;
                }
            }
        }

        let (a, b) = (lg[p1].position, lg[p2].position);
        match lg[p1].side {
            PortSide::NORTH => Self::compare_double(a.x, b.x),
            PortSide::EAST => Self::compare_double(a.y, b.y),
            PortSide::SOUTH => Self::compare_double(b.x, a.x),
            PortSide::WEST => Self::compare_double(b.y, a.y),
            _ => 0,
        }
    }

    /// `CMP_COMBINED`.
    pub fn cmp_combined(lg: &LGraphArena, p1: LPortId, p2: LPortId) -> i64 {
        let side_compare = Self::cmp_port_side(lg, p1, p2);
        if side_compare != 0 {
            return side_compare;
        }
        Self::cmp_fixed_order_and_fixed_pos(lg, p1, p2)
    }

    fn reverse_west_and_south_side(lg: &LGraphArena, ports: &mut [LPortId]) {
        if ports.len() <= 1 {
            return;
        }
        let south_indices = Self::find_port_side_range(lg, ports, PortSide::SOUTH);
        Self::reverse_range(ports, south_indices.0, south_indices.1);

        let west_indices = Self::find_port_side_range(lg, ports, PortSide::WEST);
        Self::reverse_range(ports, west_indices.0, west_indices.1);
    }

    fn find_port_side_range(lg: &LGraphArena, ports: &[LPortId], side: PortSide) -> (usize, usize) {
        if ports.is_empty() {
            return (0, 0);
        }
        let mut current_side = lg[ports[0]].side;
        let mut low_idx = 0;
        let lb = side.ordinal();
        let hb = side.ordinal() + 1;

        while low_idx < ports.len() - 1 && current_side.ordinal() < lb {
            low_idx += 1;
            current_side = lg[ports[low_idx]].side;
        }
        let mut high_idx = low_idx;
        while high_idx < ports.len() - 1 && current_side.ordinal() < hb {
            high_idx += 1;
            current_side = lg[ports[high_idx]].side;
        }
        (low_idx, high_idx)
    }

    fn reverse_range(ports: &mut [LPortId], low_idx: usize, high_idx: usize) {
        if high_idx <= low_idx + 2 {
            return;
        }
        let n = (high_idx - low_idx) / 2;
        for i in 0..n {
            ports.swap(low_idx + i, high_idx - i - 1);
        }
    }

    fn real_degree(lg: &LGraphArena, edges: &[LEdgeId]) -> i64 {
        let mut count = 0;
        for &e in edges {
            if !lg[e].props.get_as::<bool>(&InternalProperties::REVERSED).unwrap_or(false) {
                count += 1;
            }
        }
        count
    }

    /// `compareDouble(_:_:)`.
    pub fn compare_double(lhs: f64, rhs: f64) -> i64 {
        if lhs < rhs {
            return -1;
        }
        if lhs > rhs {
            return 1;
        }
        0
    }
}

impl ILayoutProcessor for PortListSorter {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Port order processing", 1.0);

        let pss = lg[layered_graph].props.get_as::<PortSortingStrategy>(&LayeredOptions::PORT_SORTING_STRATEGY);

        for layer in lg[layered_graph].layers.clone() {
            for node in lg[layer].nodes.clone() {
                let port_constraints = lg[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED);

                if port_constraints.is_order_fixed() {
                    let mut ports = lg[node].ports.clone();
                    swift::sort_by(&mut ports, |&a, &b| Self::cmp_combined(lg, a, b) < 0);
                    lg[node].ports = ports;
                } else if port_constraints.is_side_fixed() {
                    let mut ports = lg[node].ports.clone();
                    swift::sort_by(&mut ports, |&a, &b| Self::cmp_port_side(lg, a, b) < 0);

                    Self::reverse_west_and_south_side(lg, &mut ports);

                    if pss == Some(PortSortingStrategy::PORT_DEGREE) {
                        swift::sort_by(&mut ports, |&a, &b| Self::cmp_port_degree_east_west(lg, a, b) < 0);
                    }
                    lg[node].ports = ports;
                }
                lg.node_cache_port_sides(node);
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "PortListSorter"
    }
}
