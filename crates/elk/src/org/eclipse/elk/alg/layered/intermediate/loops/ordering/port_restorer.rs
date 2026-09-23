//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/ordering/org_eclipse_elk_alg_layered_intermediate_loops_ordering_PortRestorer.swift`.
//!
//! Puts hidden self loop ports back onto their node, in an order that keeps
//! the self loops from crossing.

use super::super::self_hyper_loop::{SelfHyperLoop, SlLoopId};
use super::super::self_loop_holder::SelfLoopHolder;
use super::super::self_loop_port::SlPortId;
use super::super::self_loop_type::SelfLoopType;
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::self_loop_ordering_strategy::SelfLoopOrderingStrategy;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::swift;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PortSideArea {
    START,
    MIDDLE,
    END,
}

impl PortSideArea {
    const ALL_CASES: [PortSideArea; 3] = [PortSideArea::START, PortSideArea::MIDDLE, PortSideArea::END];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AddMode {
    PREPEND,
    APPEND,
}

const ALL_SIDES: [PortSide; 5] = [PortSide::UNDEFINED, PortSide::NORTH, PortSide::EAST, PortSide::SOUTH, PortSide::WEST];

fn type_index(t: SelfLoopType) -> usize {
    t as usize
}

#[derive(Default)]
pub struct PortRestorer {
    /// `slLoopsByType` (a dictionary only read by key).
    sl_loops_by_type: [Vec<SlLoopId>; 5],
    /// `targetAreas[side][area]`, indexed by side ordinal and area.
    target_areas: [[Vec<SlPortId>; 3]; 5],
}

impl PortRestorer {
    pub fn new() -> PortRestorer {
        PortRestorer::default()
    }

    pub fn restore_ports(&mut self, lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder, _monitor: &mut dyn IElkProgressMonitor) {
        self.init_target_areas();
        self.sl_loops_by_type = gather_self_loops_by_type(sl_holder);

        let ordering = lg[sl_holder.get_l_node()]
            .props
            .get_as::<SelfLoopOrderingStrategy>(&LayeredOptions::EDGE_ROUTING_SELF_LOOP_ORDERING)
            .unwrap_or(SelfLoopOrderingStrategy::STACKED);
        self.process_one_side_loops(lg, sl_holder, ordering);
        self.process_two_side_corner_loops(sl_holder);
        self.process_three_side_loops(sl_holder);
        self.process_four_side_loops(sl_holder);
        self.process_two_side_opposing_loops(sl_holder);

        self.restore_ports_to_node(lg, sl_holder);

        // Un-hide all ports
        for side in ALL_SIDES {
            for area in PortSideArea::ALL_CASES {
                for &sl_port in &self.target_areas[side.ordinal()][area as usize] {
                    sl_holder.sl_port_mut(sl_port).set_hidden(false);
                }
            }
        }
        sl_holder.set_ports_hidden(false);

        self.sl_loops_by_type = Default::default();
    }

    fn init_target_areas(&mut self) {
        self.target_areas = Default::default();
    }

    // MARK: - One Side

    fn process_one_side_loops(&mut self, lg: &LGraphArena, sl_holder: &SelfLoopHolder, ordering: SelfLoopOrderingStrategy) {
        let mut loops = self.sl_loops_by_type[type_index(SelfLoopType::ONE_SIDE)].clone();
        if ordering == SelfLoopOrderingStrategy::REVERSE_STACKED {
            loops.reverse();
        }
        for sl_loop in loops {
            let l = sl_holder.sl_loop(sl_loop);
            let side = lg[sl_holder.sl_port(l.get_sl_ports()[0]).get_l_port()].side;
            let mut sorted_ports = l.get_sl_ports().to_vec();
            swift::sort_by(&mut sorted_ports, |a, b| sl_holder.sl_port(*a).get_sl_net_flow() < sl_holder.sl_port(*b).get_sl_net_flow());

            match ordering {
                SelfLoopOrderingStrategy::SEQUENCED => {
                    self.add_to_target_area(sl_holder, &sorted_ports, side, PortSideArea::MIDDLE, AddMode::APPEND);
                }
                SelfLoopOrderingStrategy::REVERSE_STACKED | SelfLoopOrderingStrategy::STACKED => {
                    let split_index = compute_port_list_split_index(sl_holder, &sorted_ports);
                    self.add_to_target_area(sl_holder, &sorted_ports[..split_index], side, PortSideArea::MIDDLE, AddMode::PREPEND);
                    self.add_to_target_area(sl_holder, &sorted_ports[split_index..], side, PortSideArea::MIDDLE, AddMode::APPEND);
                }
            }
        }
    }

    // MARK: - Two Sides Corner

    fn process_two_side_corner_loops(&mut self, sl_holder: &SelfLoopHolder) {
        for sl_loop in self.sl_loops_by_type[type_index(SelfLoopType::TWO_SIDES_CORNER)].clone() {
            let sides = PortRestorer::sorted_two_side_loop_port_sides(sl_holder.sl_loop(sl_loop));
            self.add_to_target_area_from_loop(sl_holder, sl_loop, sides[0], PortSideArea::END, AddMode::PREPEND);
            self.add_to_target_area_from_loop(sl_holder, sl_loop, sides[1], PortSideArea::START, AddMode::APPEND);
        }
    }

    fn process_two_side_opposing_loops(&mut self, sl_holder: &SelfLoopHolder) {
        for sl_loop in self.sl_loops_by_type[type_index(SelfLoopType::TWO_SIDES_OPPOSING)].clone() {
            let sides = PortRestorer::sorted_two_side_loop_port_sides(sl_holder.sl_loop(sl_loop));
            self.add_to_target_area_from_loop(sl_holder, sl_loop, sides[0], PortSideArea::END, AddMode::PREPEND);
            self.add_to_target_area_from_loop(sl_holder, sl_loop, sides[1], PortSideArea::START, AddMode::APPEND);
        }
    }

    /// `sortedTwoSideLoopPortSides(_:)`: the loop's port sides by ordinal
    /// (so independent of the dictionary's key order), with `[NORTH, WEST]`
    /// turned into `[WEST, NORTH]`.
    pub fn sorted_two_side_loop_port_sides(sl_loop: &SelfHyperLoop) -> Vec<PortSide> {
        let mut sides = swift::sorted_by(sl_loop.sl_ports_by_side_keys(), |a, b| a.ordinal() < b.ordinal());
        if sides.len() == 2 && sides[0] == PortSide::NORTH && sides[1] == PortSide::WEST {
            sides = vec![PortSide::WEST, PortSide::NORTH];
        }
        sides
    }

    // MARK: - Three Sides

    fn process_three_side_loops(&mut self, sl_holder: &SelfLoopHolder) {
        for sl_loop in self.sl_loops_by_type[type_index(SelfLoopType::THREE_SIDES)].clone() {
            let sides = determine_loop_constellation(sl_holder.sl_loop(sl_loop));
            self.add_to_target_area_from_loop(sl_holder, sl_loop, sides[0], PortSideArea::END, AddMode::PREPEND);
            self.add_to_target_area_from_loop(sl_holder, sl_loop, sides[1], PortSideArea::MIDDLE, AddMode::APPEND);
            self.add_to_target_area_from_loop(sl_holder, sl_loop, sides[2], PortSideArea::START, AddMode::APPEND);
        }
    }

    // MARK: - Four Sides

    fn process_four_side_loops(&mut self, sl_holder: &SelfLoopHolder) {
        for sl_loop in self.sl_loops_by_type[type_index(SelfLoopType::FOUR_SIDES)].clone() {
            // Iterates a dictionary's keys, but each side appends to its own
            // list, so the key order does not matter.
            for side in sl_holder.sl_loop(sl_loop).sl_ports_by_side_keys() {
                self.add_to_target_area_from_loop(sl_holder, sl_loop, side, PortSideArea::MIDDLE, AddMode::APPEND);
            }
        }
    }

    // MARK: - Placement Utilities

    fn add_to_target_area_from_loop(&mut self, sl_holder: &SelfLoopHolder, sl_loop: SlLoopId, port_side: PortSide, area: PortSideArea, add_mode: AddMode) {
        let ports = sl_holder.sl_loop(sl_loop).get_sl_ports_by_side(port_side).to_vec();
        self.add_to_target_area(sl_holder, &ports, port_side, area, add_mode);
    }

    fn add_to_target_area(&mut self, sl_holder: &SelfLoopHolder, sl_ports: &[SlPortId], port_side: PortSide, area: PortSideArea, add_mode: AddMode) {
        let mut hidden_ports: Vec<SlPortId> = sl_ports.iter().copied().filter(|&p| sl_holder.sl_port(p).is_hidden()).collect();
        hidden_ports.reverse();

        let target = &mut self.target_areas[port_side.ordinal()][area as usize];
        if add_mode == AddMode::PREPEND {
            target.splice(0..0, hidden_ports);
        } else {
            target.extend(hidden_ports);
        }
    }

    // MARK: - Port Restoring

    fn restore_ports_to_node(&self, lg: &mut LGraphArena, sl_holder: &SelfLoopHolder) {
        let l_node = sl_holder.get_l_node();

        let old_port_list: Vec<LPortId> = lg[l_node].ports.clone();
        let mut next_old_port_index = 0;

        // Clear and rebuild port list (directly, as Swift does: the old ports
        // keep their owner)
        lg[l_node].ports.clear();

        let area = |side: PortSide, area: PortSideArea| &self.target_areas[side.ordinal()][area as usize];

        add_all(lg, sl_holder, area(PortSide::NORTH, PortSideArea::START), l_node);
        next_old_port_index = add_all_that(
            lg,
            &old_port_list,
            next_old_port_index,
            |lg, p| lg[p].side == PortSide::NORTH && is_north_south_port_with_west_or_west_east_connections(lg, p),
            l_node,
        );
        add_all(lg, sl_holder, area(PortSide::NORTH, PortSideArea::MIDDLE), l_node);
        next_old_port_index = add_all_that(lg, &old_port_list, next_old_port_index, |lg, p| lg[p].side == PortSide::NORTH, l_node);
        add_all(lg, sl_holder, area(PortSide::NORTH, PortSideArea::END), l_node);

        add_all(lg, sl_holder, area(PortSide::EAST, PortSideArea::START), l_node);
        add_all(lg, sl_holder, area(PortSide::EAST, PortSideArea::MIDDLE), l_node);
        next_old_port_index = add_all_that(lg, &old_port_list, next_old_port_index, |lg, p| lg[p].side == PortSide::EAST, l_node);
        add_all(lg, sl_holder, area(PortSide::EAST, PortSideArea::END), l_node);

        add_all(lg, sl_holder, area(PortSide::SOUTH, PortSideArea::START), l_node);
        next_old_port_index = add_all_that(
            lg,
            &old_port_list,
            next_old_port_index,
            |lg, p| lg[p].side == PortSide::SOUTH && is_north_south_port_with_east_connections(lg, p),
            l_node,
        );
        add_all(lg, sl_holder, area(PortSide::SOUTH, PortSideArea::MIDDLE), l_node);
        next_old_port_index = add_all_that(lg, &old_port_list, next_old_port_index, |lg, p| lg[p].side == PortSide::SOUTH, l_node);
        add_all(lg, sl_holder, area(PortSide::SOUTH, PortSideArea::END), l_node);

        add_all(lg, sl_holder, area(PortSide::WEST, PortSideArea::START), l_node);
        next_old_port_index = add_all_that(lg, &old_port_list, next_old_port_index, |lg, p| lg[p].side == PortSide::WEST, l_node);
        add_all(lg, sl_holder, area(PortSide::WEST, PortSideArea::MIDDLE), l_node);
        add_all(lg, sl_holder, area(PortSide::WEST, PortSideArea::END), l_node);

        let _ = next_old_port_index;
    }
}

fn gather_self_loops_by_type(sl_holder: &SelfLoopHolder) -> [Vec<SlLoopId>; 5] {
    let mut loops: [Vec<SlLoopId>; 5] = Default::default();
    for sl_loop in sl_holder.sl_loop_ids() {
        if let Some(t) = sl_holder.sl_loop(sl_loop).get_self_loop_type() {
            loops[type_index(t)].push(sl_loop);
        }
    }
    loops
}

/// `computePortListSplitIndex(_:)`. The second search repeats the first
/// (`> 0`) and tests `positiveNetFlowIndex` again, exactly as the Swift does.
fn compute_port_list_split_index(sl_holder: &SelfLoopHolder, sorted_ports: &[SlPortId]) -> usize {
    let mut positive_net_flow_index = 0;
    while positive_net_flow_index < sorted_ports.len() {
        if sl_holder.sl_port(sorted_ports[positive_net_flow_index]).get_sl_net_flow() > 0 {
            break;
        }
        positive_net_flow_index += 1;
    }
    if positive_net_flow_index > 0 && (positive_net_flow_index as i64) < sorted_ports.len() as i64 - 1 {
        return positive_net_flow_index;
    }

    let mut non_negative_net_flow_index = 0;
    while non_negative_net_flow_index < sorted_ports.len() {
        if sl_holder.sl_port(sorted_ports[non_negative_net_flow_index]).get_sl_net_flow() > 0 {
            break;
        }
        non_negative_net_flow_index += 1;
    }
    if non_negative_net_flow_index > 0 && (positive_net_flow_index as i64) < sorted_ports.len() as i64 - 1 {
        return non_negative_net_flow_index;
    }

    sorted_ports.len() / 2
}

const NES: [PortSide; 3] = [PortSide::NORTH, PortSide::EAST, PortSide::SOUTH];
const ESW: [PortSide; 3] = [PortSide::EAST, PortSide::SOUTH, PortSide::WEST];
const SWN: [PortSide; 3] = [PortSide::SOUTH, PortSide::WEST, PortSide::NORTH];
const WNE: [PortSide; 3] = [PortSide::WEST, PortSide::NORTH, PortSide::EAST];

fn determine_loop_constellation(sl_loop: &SelfHyperLoop) -> [PortSide; 3] {
    let port_sides: EnumSet<PortSide> = sl_loop.sl_ports_by_side_keys().into_iter().collect();
    if !port_sides.contains(PortSide::NORTH) {
        return ESW;
    }
    if !port_sides.contains(PortSide::EAST) {
        return SWN;
    }
    if !port_sides.contains(PortSide::SOUTH) {
        return WNE;
    }
    if !port_sides.contains(PortSide::WEST) {
        return NES;
    }
    NES // shouldn't happen
}

fn add_all(lg: &mut LGraphArena, sl_holder: &SelfLoopHolder, sl_ports: &[SlPortId], l_node: LNodeId) {
    for &sl_port in sl_ports {
        lg.port_set_node(sl_holder.sl_port(sl_port).get_l_port(), Some(l_node));
    }
}

fn add_all_that(lg: &mut LGraphArena, l_ports: &[LPortId], from_index: usize, condition: impl Fn(&LGraphArena, LPortId) -> bool, l_node: LNodeId) -> usize {
    for (i, &l_port) in l_ports.iter().enumerate().skip(from_index) {
        if condition(lg, l_port) {
            lg[l_node].ports.push(l_port);
        } else {
            return i;
        }
    }
    l_ports.len()
}

fn is_north_south_port_with_west_or_west_east_connections(lg: &LGraphArena, l_port: LPortId) -> bool {
    let connections = north_south_port_connection_sides(lg, l_port);
    connections.contains(PortSide::WEST)
}

fn is_north_south_port_with_east_connections(lg: &LGraphArena, l_port: LPortId) -> bool {
    let connections = north_south_port_connection_sides(lg, l_port);
    connections.contains(PortSide::EAST)
}

fn north_south_port_connection_sides(lg: &LGraphArena, l_port: LPortId) -> EnumSet<PortSide> {
    let mut connection_sides = EnumSet::new();

    if let Some(port_dummy) = lg[l_port].props.get_as::<LNodeId>(&InternalProperties::PORT_DUMMY) {
        for &dummy_l_port in &lg[port_dummy].ports {
            if lg[dummy_l_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) == Some(l_port) && !lg.port_connected_edges(dummy_l_port).is_empty() {
                connection_sides.insert(lg[dummy_l_port].side);
            }
        }
    }

    connection_sides
}
