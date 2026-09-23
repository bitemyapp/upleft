//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/routing/org_eclipse_elk_alg_layered_intermediate_loops_routing_LabelPlacer.swift`.
//!
//! Decides on which side of the node each hyper loop's labels go, how they
//! are aligned, and computes their coordinate along that side.

use super::super::self_hyper_loop::SlLoopId;
use super::super::self_hyper_loop_labels::Alignment;
use super::super::self_loop_holder::SelfLoopHolder;
use super::super::self_loop_port::SlPortId;
use super::super::self_loop_type::SelfLoopType;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::self_loop_ordering_strategy::SelfLoopOrderingStrategy;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::swift;

#[derive(Default)]
pub struct LabelPlacer;

impl LabelPlacer {
    pub fn new() -> LabelPlacer {
        LabelPlacer
    }

    /// `placeLabels(_:_:_:)`. elk-swift passes
    /// `graph.getProperty(LABEL_MANAGER) as? ILabelManager`, which is always
    /// `nil` (no layout option value can be an `ILabelManager`), so the
    /// `manageLabels` branch is never taken and is not ported.
    pub fn place_labels(&self, lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder, _monitor: &mut dyn IElkProgressMonitor) {
        assign_side_and_alignment(lg, sl_holder);

        for sl_loop in sl_holder.sl_loop_ids() {
            if sl_holder.sl_loop(sl_loop).get_sl_labels().is_some() {
                compute_coordinates(lg, sl_holder, sl_loop);
            }
        }
    }
}

// MARK: - Side and Alignment

fn assign_side_and_alignment(lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder) {
    let mut northern_one_sided_sl_loops: Option<Vec<SlLoopId>> = None;
    let mut southern_one_sided_sl_loops: Option<Vec<SlLoopId>> = None;

    let ordering_strategy = lg[sl_holder.get_l_node()]
        .props
        .get_as::<SelfLoopOrderingStrategy>(&LayeredOptions::EDGE_ROUTING_SELF_LOOP_ORDERING)
        .unwrap_or(SelfLoopOrderingStrategy::STACKED);
    if ordering_strategy == SelfLoopOrderingStrategy::SEQUENCED {
        northern_one_sided_sl_loops = Some(Vec::new());
        southern_one_sided_sl_loops = Some(Vec::new());
    }

    for sl_loop in sl_holder.sl_loop_ids() {
        let l = sl_holder.sl_loop(sl_loop);
        let Some(sl_labels) = l.get_sl_labels() else { continue };
        if sl_labels.get_l_labels().is_empty() {
            continue;
        }
        let Some(loop_type) = l.get_self_loop_type() else { continue };

        match loop_type {
            SelfLoopType::ONE_SIDE => {
                // `getOccupiedPortSides().first`: a one-sided loop occupies
                // exactly one side, so the set's order does not matter.
                let Some(loop_side) = l.get_occupied_port_sides().iter().next() else { continue };

                if ordering_strategy == SelfLoopOrderingStrategy::SEQUENCED && loop_side == PortSide::NORTH {
                    if let Some(v) = northern_one_sided_sl_loops.as_mut() {
                        v.push(sl_loop);
                    }
                } else if ordering_strategy == SelfLoopOrderingStrategy::SEQUENCED && loop_side == PortSide::SOUTH {
                    if let Some(v) = southern_one_sided_sl_loops.as_mut() {
                        v.push(sl_loop);
                    }
                } else {
                    assign_one_sided_simple_side_and_alignment(lg, sl_holder, sl_loop, loop_side);
                }
            }
            SelfLoopType::TWO_SIDES_CORNER => assign_two_sides_corner_side_and_alignment(lg, sl_holder, sl_loop),
            SelfLoopType::TWO_SIDES_OPPOSING | SelfLoopType::THREE_SIDES => {
                assign_two_sides_opposing_and_three_sides_side_and_alignment(lg, sl_holder, sl_loop)
            }
            SelfLoopType::FOUR_SIDES => assign_four_sides_side_and_alignment(lg, sl_holder, sl_loop),
        }
    }

    if let Some(north_loops) = northern_one_sided_sl_loops {
        if !north_loops.is_empty() {
            assign_one_sided_sequenced_side_and_alignment(lg, sl_holder, north_loops, PortSide::NORTH);
        }
    }
    if let Some(south_loops) = southern_one_sided_sl_loops {
        if !south_loops.is_empty() {
            assign_one_sided_sequenced_side_and_alignment(lg, sl_holder, south_loops, PortSide::SOUTH);
        }
    }
}

/// `label.setProperty(EDGE_LABELS_INLINE, nil)` for every label of the loop.
fn clear_inline(lg: &mut LGraphArena, sl_holder: &SelfLoopHolder, sl_loop: SlLoopId) {
    if let Some(sl_labels) = sl_holder.sl_loop(sl_loop).get_sl_labels() {
        for &label in sl_labels.get_l_labels() {
            lg[label].props.remove(&LayeredOptions::EDGE_LABELS_INLINE);
        }
    }
}

fn assign_one_sided_simple_side_and_alignment(lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId, loop_side: PortSide) {
    clear_inline(lg, sl_holder, sl_loop);

    match loop_side {
        PortSide::EAST | PortSide::WEST => {
            let l = sl_holder.sl_loop(sl_loop);
            let Some(mut topmost_port) = l.get_leftmost_port() else { return };
            if let Some(rightmost) = l.get_rightmost_port() {
                let y = |p: SlPortId| lg[sl_holder.sl_port(p).get_l_port()].position.y;
                if y(rightmost) < y(topmost_port) {
                    topmost_port = rightmost;
                }
            }
            assign_side_and_alignment_helper(sl_holder, sl_loop, loop_side, Alignment::TOP, Some(topmost_port));
        }
        PortSide::NORTH | PortSide::SOUTH => {
            assign_side_and_alignment_helper(sl_holder, sl_loop, loop_side, Alignment::CENTER, None);
        }
        _ => {}
    }
}

fn assign_one_sided_sequenced_side_and_alignment(lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loops: Vec<SlLoopId>, port_side: PortSide) {
    let mut sl_loops = sl_loops;
    if sl_loops.is_empty() {
        return;
    }

    for (id, port) in lg[sl_holder.get_l_node()].ports.clone().into_iter().enumerate() {
        lg[port].id = id as i32;
    }

    let leftmost_id = |l: SlLoopId| sl_holder.sl_loop(l).get_leftmost_port().map_or(0, |p| lg[sl_holder.sl_port(p).get_l_port()].id);
    if port_side == PortSide::NORTH {
        swift::sort_by(&mut sl_loops, |a, b| leftmost_id(*a) < leftmost_id(*b));
    } else {
        swift::sort_by(&mut sl_loops, |a, b| leftmost_id(*a) > leftmost_id(*b));
    }

    let mut left_idx: i64 = 0;
    let mut right_idx: i64 = sl_loops.len() as i64 - 1;

    while left_idx < right_idx {
        let left_sl_loop = sl_loops[left_idx as usize];
        let right_sl_loop = sl_loops[right_idx as usize];

        let left_loop_alignment_ref = if port_side == PortSide::NORTH {
            sl_holder.sl_loop(left_sl_loop).get_rightmost_port()
        } else {
            sl_holder.sl_loop(left_sl_loop).get_leftmost_port()
        };
        let right_loop_alignment_ref = if port_side == PortSide::NORTH {
            sl_holder.sl_loop(right_sl_loop).get_leftmost_port()
        } else {
            sl_holder.sl_loop(right_sl_loop).get_rightmost_port()
        };
        let (Some(left_ref), Some(right_ref)) = (left_loop_alignment_ref, right_loop_alignment_ref) else {
            left_idx += 1;
            right_idx -= 1;
            continue;
        };

        assign_side_and_alignment_helper(sl_holder, left_sl_loop, port_side, Alignment::RIGHT, Some(left_ref));
        assign_side_and_alignment_helper(sl_holder, right_sl_loop, port_side, Alignment::LEFT, Some(right_ref));

        left_idx += 1;
        right_idx -= 1;
    }

    if left_idx == right_idx {
        assign_side_and_alignment_helper(sl_holder, sl_loops[left_idx as usize], port_side, Alignment::CENTER, None);
    }
}

fn assign_two_sides_corner_side_and_alignment(lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
    let l = sl_holder.sl_loop(sl_loop);
    let (Some(leftmost_port), Some(rightmost_port)) = (l.get_leftmost_port(), l.get_rightmost_port()) else { return };
    let leftmost_port_side = lg[sl_holder.sl_port(leftmost_port).get_l_port()].side;
    let rightmost_port_side = lg[sl_holder.sl_port(rightmost_port).get_l_port()].side;

    clear_inline(lg, sl_holder, sl_loop);

    if leftmost_port_side == PortSide::NORTH {
        assign_side_and_alignment_helper(sl_holder, sl_loop, PortSide::NORTH, Alignment::LEFT, Some(leftmost_port));
    } else if rightmost_port_side == PortSide::NORTH {
        assign_side_and_alignment_helper(sl_holder, sl_loop, PortSide::NORTH, Alignment::RIGHT, Some(rightmost_port));
    } else if leftmost_port_side == PortSide::SOUTH {
        assign_side_and_alignment_helper(sl_holder, sl_loop, PortSide::SOUTH, Alignment::RIGHT, Some(leftmost_port));
    } else if rightmost_port_side == PortSide::SOUTH {
        assign_side_and_alignment_helper(sl_holder, sl_loop, PortSide::SOUTH, Alignment::LEFT, Some(rightmost_port));
    }
}

fn assign_two_sides_opposing_and_three_sides_side_and_alignment(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
    let l = sl_holder.sl_loop(sl_loop);
    let occupied_sides = l.get_occupied_port_sides();
    let Some(sl_labels_for_inline) = l.get_sl_labels() else { return };
    let has_inline_labels = sl_labels_for_inline
        .get_l_labels()
        .iter()
        .any(|&label| lg[label].props.get_as::<bool>(&LayeredOptions::EDGE_LABELS_INLINE) == Some(true));
    let leftmost = l.get_leftmost_port();
    let rightmost = l.get_rightmost_port();

    if !occupied_sides.contains(PortSide::NORTH) {
        assign_side_and_alignment_helper(sl_holder, sl_loop, PortSide::SOUTH, Alignment::CENTER, None);
    } else if !occupied_sides.contains(PortSide::SOUTH) {
        assign_side_and_alignment_helper(sl_holder, sl_loop, PortSide::NORTH, Alignment::CENTER, None);
    } else if !occupied_sides.contains(PortSide::WEST) {
        assign_side_and_alignment_helper(
            sl_holder,
            sl_loop,
            if has_inline_labels { PortSide::EAST } else { PortSide::NORTH },
            if has_inline_labels { Alignment::CENTER } else { Alignment::LEFT },
            if has_inline_labels { None } else { leftmost },
        );
    } else if !occupied_sides.contains(PortSide::EAST) {
        assign_side_and_alignment_helper(
            sl_holder,
            sl_loop,
            if has_inline_labels { PortSide::WEST } else { PortSide::NORTH },
            if has_inline_labels { Alignment::CENTER } else { Alignment::RIGHT },
            if has_inline_labels { None } else { rightmost },
        );
    }
}

fn assign_four_sides_side_and_alignment(lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
    let Some(leftmost_port) = sl_holder.sl_loop(sl_loop).get_leftmost_port() else { return };
    let leftmost_port_side = lg[sl_holder.sl_port(leftmost_port).get_l_port()].side;
    let rightmost_port_side = lg[sl_holder.sl_port(leftmost_port).get_l_port()].side; // Note: Java uses getLeftmostPort() for both

    clear_inline(lg, sl_holder, sl_loop);

    if leftmost_port_side == PortSide::NORTH || rightmost_port_side == PortSide::NORTH {
        assign_side_and_alignment_helper(sl_holder, sl_loop, PortSide::SOUTH, Alignment::CENTER, None);
    } else {
        assign_side_and_alignment_helper(sl_holder, sl_loop, PortSide::NORTH, Alignment::CENTER, None);
    }
}

fn assign_side_and_alignment_helper(sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId, side: PortSide, alignment: Alignment, alignment_reference: Option<SlPortId>) {
    let Some(sl_labels) = sl_holder.sl_loop_mut(sl_loop).get_sl_labels_mut() else { return };
    sl_labels.set_side(side);
    sl_labels.set_alignment(alignment);
    sl_labels.set_alignment_reference_sl_port(alignment_reference);
}

// MARK: - Coordinate Computation

fn compute_coordinates(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId) {
    let node_size_x = lg[sl_holder.get_l_node()].size.x;
    let Some(sl_labels) = sl_holder.sl_loop(sl_loop).get_sl_labels() else { return };
    let align_ref = sl_labels.get_alignment_reference_sl_port().map(|p| sl_holder.sl_port(p).get_l_port());
    let size = sl_labels.get_size();
    let alignment = sl_labels.get_alignment();
    let Some(sl_labels) = sl_holder.sl_loop_mut(sl_loop).get_sl_labels_mut() else { return };
    let pos = sl_labels.position_mut();

    match alignment {
        Alignment::CENTER => {
            pos.x = (node_size_x - size.x) / 2.0;
        }
        Alignment::LEFT => {
            if let Some(r) = align_ref {
                pos.x = lg[r].position.x + lg[r].anchor.x;
            }
        }
        Alignment::RIGHT => {
            if let Some(r) = align_ref {
                pos.x = lg[r].position.x + lg[r].anchor.x - size.x;
            }
        }
        Alignment::TOP => {
            if let Some(r) = align_ref {
                pos.y = lg[r].position.y + lg[r].anchor.y;
            }
        }
    }
}
