//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/ordering/org_eclipse_elk_alg_layered_intermediate_loops_ordering_PortSideAssigner.swift`.

use super::super::self_hyper_loop::SlLoopId;
use super::super::self_loop_holder::SelfLoopHolder;
use super::super::self_loop_port::SlPortId;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::self_loop_distribution_strategy::SelfLoopDistributionStrategy;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

#[derive(Default)]
pub struct PortSideAssigner;

/// `PortSideAssigner.Target`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    NORTH,
    SOUTH,
    EAST,
    WEST,
    NORTH_WEST_CORNER,
    NORTH_EAST_CORNER,
    SOUTH_WEST_CORNER,
    SOUTH_EAST_CORNER,
}

impl Target {
    const ALL_CASES: [Target; 8] = [
        Target::NORTH,
        Target::SOUTH,
        Target::EAST,
        Target::WEST,
        Target::NORTH_WEST_CORNER,
        Target::NORTH_EAST_CORNER,
        Target::SOUTH_WEST_CORNER,
        Target::SOUTH_EAST_CORNER,
    ];

    fn first_side(self) -> PortSide {
        match self {
            Target::NORTH => PortSide::NORTH,
            Target::SOUTH => PortSide::SOUTH,
            Target::EAST => PortSide::EAST,
            Target::WEST => PortSide::WEST,
            Target::NORTH_WEST_CORNER => PortSide::WEST,
            Target::NORTH_EAST_CORNER => PortSide::NORTH,
            Target::SOUTH_WEST_CORNER => PortSide::SOUTH,
            Target::SOUTH_EAST_CORNER => PortSide::EAST,
        }
    }

    fn second_side(self) -> PortSide {
        match self {
            Target::NORTH => PortSide::NORTH,
            Target::SOUTH => PortSide::SOUTH,
            Target::EAST => PortSide::EAST,
            Target::WEST => PortSide::WEST,
            Target::NORTH_WEST_CORNER => PortSide::NORTH,
            Target::NORTH_EAST_CORNER => PortSide::EAST,
            Target::SOUTH_WEST_CORNER => PortSide::WEST,
            Target::SOUTH_EAST_CORNER => PortSide::SOUTH,
        }
    }

    fn is_corner_target(self) -> bool {
        self.first_side() != self.second_side()
    }
}

impl PortSideAssigner {
    pub fn new() -> PortSideAssigner {
        PortSideAssigner
    }

    pub fn assign_port_sides(&self, lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder) {
        let dist = lg[sl_holder.get_l_node()]
            .props
            .get_as::<SelfLoopDistributionStrategy>(&LayeredOptions::EDGE_ROUTING_SELF_LOOP_DISTRIBUTION)
            .unwrap_or(SelfLoopDistributionStrategy::NORTH);

        match dist {
            SelfLoopDistributionStrategy::NORTH => assign_to_north_side(lg, sl_holder),
            SelfLoopDistributionStrategy::NORTH_SOUTH => assign_to_north_or_south_side(lg, sl_holder),
            SelfLoopDistributionStrategy::EQUALLY => assign_to_all_sides(lg, sl_holder),
        }
    }
}

// MARK: - North

fn assign_to_north_side(lg: &mut LGraphArena, sl_holder: &SelfLoopHolder) {
    for sl_loop in sl_holder.get_sl_hyper_loops() {
        for &sl_port in sl_loop.get_sl_ports() {
            if sl_holder.sl_port(sl_port).is_hidden() {
                lg.port_set_side(sl_holder.sl_port(sl_port).get_l_port(), PortSide::NORTH);
            }
        }
    }
}

// MARK: - North, South

fn assign_to_north_or_south_side(lg: &mut LGraphArena, sl_holder: &SelfLoopHolder) {
    let mut north_ports = 0usize;
    let mut south_ports = 0usize;

    for sl_loop in sl_holder.get_sl_hyper_loops() {
        let sl_hidden_ports: Vec<SlPortId> = sl_loop.get_sl_ports().iter().copied().filter(|&p| sl_holder.sl_port(p).is_hidden()).collect();
        let new_port_side;
        if north_ports <= south_ports {
            new_port_side = PortSide::NORTH;
            north_ports += sl_hidden_ports.len();
        } else {
            new_port_side = PortSide::SOUTH;
            south_ports += sl_hidden_ports.len();
        }
        for sl_port in sl_hidden_ports {
            lg.port_set_side(sl_holder.sl_port(sl_port).get_l_port(), new_port_side);
        }
    }
}

// MARK: - Equal Distribution

fn assign_to_all_sides(lg: &mut LGraphArena, sl_holder: &SelfLoopHolder) {
    let mut sl_sorted_loops: Vec<SlLoopId> = sl_holder.sl_loop_ids().collect();
    swift::sort_by(&mut sl_sorted_loops, |a, b| sl_holder.sl_loop(*a).get_sl_ports().len() > sl_holder.sl_loop(*b).get_sl_ports().len());

    let assignment_targets = Target::ALL_CASES;
    for (index, &sl_loop) in sl_sorted_loops.iter().enumerate() {
        let curr_target = assignment_targets[index % assignment_targets.len()];
        assign_to_target(lg, sl_holder, sl_loop, curr_target);
    }
}

fn assign_to_target(lg: &mut LGraphArena, sl_holder: &SelfLoopHolder, sl_loop: SlLoopId, target: Target) {
    let mut sl_ports: Vec<SlPortId> = sl_holder.sl_loop(sl_loop).get_sl_ports().to_vec();

    if target.is_corner_target() {
        swift::sort_by(&mut sl_ports, |a, b| {
            lg.port_net_flow(sl_holder.sl_port(*a).get_l_port()) < lg.port_net_flow(sl_holder.sl_port(*b).get_l_port())
        });
    }

    let second_half_start_index = sl_ports.len() / 2;

    for &sl_port in &sl_ports[..second_half_start_index] {
        if sl_holder.sl_port(sl_port).is_hidden() {
            lg.port_set_side(sl_holder.sl_port(sl_port).get_l_port(), target.first_side());
        }
    }

    for &sl_port in &sl_ports[second_half_start_index..] {
        if sl_holder.sl_port(sl_port).is_hidden() {
            lg.port_set_side(sl_holder.sl_port(sl_port).get_l_port(), target.second_side());
        }
    }
}
