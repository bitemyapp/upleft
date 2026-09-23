//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/routing/org_eclipse_elk_alg_layered_intermediate_loops_routing_RoutingDirector.swift`.
//!
//! Decides, for every hyper loop, its leftmost and rightmost port (the
//! stretch of the node's border the loop runs along).

use super::super::ordering::port_restorer::PortRestorer;
use super::super::self_hyper_loop::SlLoopId;
use super::super::self_loop_holder::SelfLoopHolder;
use super::super::self_loop_port::SlPortId;
use super::super::self_loop_type::SelfLoopType;
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LPortId};
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

const UNCONNECTED_PORT_PENALTY: i64 = 1;
const CONNECTED_PORT_PENALTY: i64 = 3;

#[derive(Default)]
pub struct RoutingDirector {
    port_penalties: Option<Vec<i64>>,
}

impl RoutingDirector {
    pub fn new() -> RoutingDirector {
        RoutingDirector::default()
    }

    pub fn determine_loop_routes(&mut self, lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder) {
        let ports = lg[sl_holder.get_l_node()].ports.clone();
        assign_port_ids(lg, &ports);
        sort_hyper_loop_port_lists(lg, sl_holder);

        for sl_loop in sl_holder.sl_loop_ids() {
            let Some(loop_type) = sl_holder.sl_loop(sl_loop).get_self_loop_type() else { continue };
            match loop_type {
                SelfLoopType::ONE_SIDE => determine_one_side_loop_routes(lg, sl_holder, sl_loop),
                SelfLoopType::TWO_SIDES_CORNER => determine_two_side_corner_loop_routes(lg, sl_holder, sl_loop),
                SelfLoopType::TWO_SIDES_OPPOSING => self.determine_two_side_opposing_loop_routes(lg, sl_holder, sl_loop),
                SelfLoopType::THREE_SIDES => determine_three_side_loop_routes(lg, sl_holder, sl_loop),
                SelfLoopType::FOUR_SIDES => self.determine_four_side_loop_routes(lg, sl_holder, sl_loop),
            }
            compute_occupied_port_sides(lg, sl_holder, sl_loop);
        }

        self.port_penalties = None;
    }

    // MARK: - Two Sides Opposing

    fn determine_two_side_opposing_loop_routes(&mut self, lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
        // `Array(slLoop.getSLPortsBySide().keys)`: see the NONDETERMINISTIC
        // note on `SelfHyperLoop.sl_ports_by_side` for the order used.
        let sides = sl_holder.sl_loop(sl_loop).sl_ports_by_side_keys();
        if sides.len() != 2 {
            return;
        }

        let option1_leftmost_port = lowest_port_on_side(lg, sl_holder, sl_loop, sides[0]);
        let option1_rightmost_port = highest_port_on_side(lg, sl_holder, sl_loop, sides[1]);
        let option1_penalty = self.compute_edge_penalty(lg, sl_holder, option1_leftmost_port, option1_rightmost_port);

        let option2_leftmost_port = lowest_port_on_side(lg, sl_holder, sl_loop, sides[1]);
        let option2_rightmost_port = highest_port_on_side(lg, sl_holder, sl_loop, sides[0]);
        let option2_penalty = self.compute_edge_penalty(lg, sl_holder, option2_leftmost_port, option2_rightmost_port);

        let l = sl_holder.sl_loop_mut(sl_loop);
        if option1_penalty <= option2_penalty {
            l.set_leftmost_port(option1_leftmost_port);
            l.set_rightmost_port(option1_rightmost_port);
        } else {
            l.set_leftmost_port(option2_leftmost_port);
            l.set_rightmost_port(option2_rightmost_port);
        }
    }

    // MARK: - Four Sides

    fn determine_four_side_loop_routes(&mut self, lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
        let sorted_sl_ports = sl_holder.sl_loop(sl_loop).get_sl_ports().to_vec();

        let mut worst_left_port = sorted_sl_ports[sorted_sl_ports.len().wrapping_sub(1)];
        let mut worst_right_port = sorted_sl_ports[0];
        let mut worst_penalty = self.compute_edge_penalty(lg, sl_holder, worst_left_port, worst_right_port);

        for right_port_index in 1..sorted_sl_ports.len() {
            let curr_left_port = sorted_sl_ports[right_port_index - 1];
            let curr_right_port = sorted_sl_ports[right_port_index];
            let curr_penalty = self.compute_edge_penalty(lg, sl_holder, curr_left_port, curr_right_port);

            if curr_penalty > worst_penalty {
                worst_left_port = curr_left_port;
                worst_right_port = curr_right_port;
                worst_penalty = curr_penalty;
            }
        }

        let l = sl_holder.sl_loop_mut(sl_loop);
        l.set_leftmost_port(worst_right_port);
        l.set_rightmost_port(worst_left_port);
    }

    fn compute_edge_penalty(&mut self, lg: &LGraphArena, sl_holder: &SelfLoopHolder, leftmost_port: SlPortId, rightmost_port: SlPortId) -> i64 {
        if self.port_penalties.is_none() {
            self.compute_penalties(lg, sl_holder);
        }

        let port_count = lg[sl_holder.get_l_node()].ports.len() as i64;
        let leftmost_port_id = lg[sl_holder.sl_port(leftmost_port).get_l_port()].id as i64;
        let rightmost_port_id = lg[sl_holder.sl_port(rightmost_port).get_l_port()].id as i64;
        let mut left_of_rightmost_port_id = rightmost_port_id - 1;

        if left_of_rightmost_port_id < 0 {
            left_of_rightmost_port_id = port_count - 1;
        }

        let Some(penalties) = &self.port_penalties else { return 0 };
        let at = |i: i64| penalties[i as usize];

        if leftmost_port_id <= left_of_rightmost_port_id {
            at(left_of_rightmost_port_id) - at(leftmost_port_id)
        } else {
            at(port_count - 1) - at(leftmost_port_id) + at(left_of_rightmost_port_id)
        }
    }

    fn compute_penalties(&mut self, lg: &LGraphArena, sl_holder: &SelfLoopHolder) {
        let ports = &lg[sl_holder.get_l_node()].ports;
        let mut penalties = vec![0i64; ports.len()];
        let mut penalty_sum = 0;

        for (i, &curr_port) in ports.iter().enumerate() {
            if lg[curr_port].incoming_edges.is_empty() && lg[curr_port].outgoing_edges.is_empty() {
                penalty_sum += UNCONNECTED_PORT_PENALTY;
            } else {
                penalty_sum += CONNECTED_PORT_PENALTY;
            }
            penalties[i] = penalty_sum;
        }
        self.port_penalties = Some(penalties);
    }
}

fn assign_port_ids(lg: &mut LGraphArena, l_ports: &[LPortId]) {
    for (i, &port) in l_ports.iter().enumerate() {
        lg[port].id = i as i32;
    }
}

fn sort_hyper_loop_port_lists(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder) {
    let port_ids: Vec<i32> = sl_holder.get_sl_port_values().map(|p| lg[sl_holder.sl_port(p).get_l_port()].id).collect();
    for sl_loop in sl_holder.sl_loop_ids() {
        sl_holder.sl_loop_mut(sl_loop).sort_sl_ports(|a, b| port_ids[a.index()] < port_ids[b.index()]);
    }
}

fn compute_occupied_port_sides(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
    let l = sl_holder.sl_loop(sl_loop);
    let (Some(leftmost), Some(rightmost)) = (l.get_leftmost_port(), l.get_rightmost_port()) else { return };
    let mut curr_port_side = lg[sl_holder.sl_port(leftmost).get_l_port()].side;
    let target_side = lg[sl_holder.sl_port(rightmost).get_l_port()].side;

    let l = sl_holder.sl_loop_mut(sl_loop);
    while curr_port_side != target_side {
        l.add_occupied_port_side(curr_port_side);
        curr_port_side = curr_port_side.right();
    }
    l.add_occupied_port_side(curr_port_side);
}

// MARK: - One Side

fn determine_one_side_loop_routes(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
    let first = sl_holder.sl_loop(sl_loop).get_sl_ports()[0];
    let side = lg[sl_holder.sl_port(first).get_l_port()].side;
    assign_leftmost_rightmost_ports(lg, sl_holder, sl_loop, side, side);
}

// MARK: - Two Sides Corner

fn determine_two_side_corner_loop_routes(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
    let sides = PortRestorer::sorted_two_side_loop_port_sides(sl_holder.sl_loop(sl_loop));
    assign_leftmost_rightmost_ports(lg, sl_holder, sl_loop, sides[0], sides[1]);
}

// MARK: - Three Sides

fn determine_three_side_loop_routes(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
    let mut leftmost_side = PortSide::UNDEFINED;
    let mut rightmost_side = PortSide::UNDEFINED;

    match compute_missing_port_side(sl_holder, sl_loop) {
        PortSide::NORTH => {
            leftmost_side = PortSide::EAST;
            rightmost_side = PortSide::WEST;
        }
        PortSide::EAST => {
            leftmost_side = PortSide::SOUTH;
            rightmost_side = PortSide::NORTH;
        }
        PortSide::SOUTH => {
            leftmost_side = PortSide::WEST;
            rightmost_side = PortSide::EAST;
        }
        PortSide::WEST => {
            leftmost_side = PortSide::NORTH;
            rightmost_side = PortSide::SOUTH;
        }
        _ => {}
    }

    assign_leftmost_rightmost_ports(lg, sl_holder, sl_loop, leftmost_side, rightmost_side);
}

fn compute_missing_port_side(sl_holder: &SelfLoopHolder, sl_loop: SlLoopId) -> PortSide {
    let sides: EnumSet<PortSide> = sl_holder.sl_loop(sl_loop).sl_ports_by_side_keys().into_iter().collect();
    for side in [PortSide::NORTH, PortSide::EAST, PortSide::SOUTH, PortSide::WEST] {
        if !sides.contains(side) {
            return side;
        }
    }
    PortSide::UNDEFINED
}

// MARK: - Utility Methods

fn assign_leftmost_rightmost_ports(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId, leftmost_side: PortSide, rightmost_side: PortSide) {
    let leftmost = lowest_port_on_side(lg, sl_holder, sl_loop, leftmost_side);
    sl_holder.sl_loop_mut(sl_loop).set_leftmost_port(leftmost);
    let rightmost = highest_port_on_side(lg, sl_holder, sl_loop, rightmost_side);
    sl_holder.sl_loop_mut(sl_loop).set_rightmost_port(rightmost);
}

fn lowest_port_on_side(lg: &LGraphArena, sl_holder: &SelfLoopHolder, sl_loop: SlLoopId, side: PortSide) -> SlPortId {
    let id = |p: &SlPortId| lg[sl_holder.sl_port(*p).get_l_port()].id;
    swift::seq_min_by(sl_holder.sl_loop(sl_loop).get_sl_ports_by_side(side).iter().copied(), |a, b| id(a) < id(b)).unwrap()
}

fn highest_port_on_side(lg: &LGraphArena, sl_holder: &SelfLoopHolder, sl_loop: SlLoopId, side: PortSide) -> SlPortId {
    let id = |p: &SlPortId| lg[sl_holder.sl_port(*p).get_l_port()].id;
    swift::seq_max_by(sl_holder.sl_loop(sl_loop).get_sl_ports_by_side(side).iter().copied(), |a, b| id(a) < id(b)).unwrap()
}
