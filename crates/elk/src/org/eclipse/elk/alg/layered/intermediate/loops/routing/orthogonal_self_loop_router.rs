//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/routing/org_eclipse_elk_alg_layered_intermediate_loops_routing_OrthogonalSelfLoopRouter.swift`.
//!
//! Routes self loops orthogonally around their node, in the routing slots
//! computed by `RoutingSlotAssigner`, and grows the node's margins to include
//! the loops and their labels.

use super::abstract_self_loop_router::AbstractSelfLoopRouter;
use super::super::self_hyper_loop::SlLoopId;
use super::super::self_hyper_loop_labels::SelfHyperLoopLabels;
use super::super::self_loop_edge::SlEdgeId;
use super::super::self_loop_holder::SelfLoopHolder;
use super::super::self_loop_port::SlPortId;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena;
use crate::org::eclipse::elk::alg::layered::graph::l_margin::LMargin;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChain;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

/// `OrthogonalSelfLoopRouter.EdgeRoutingDirection`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeRoutingDirection {
    CLOCKWISE,
    COUNTER_CLOCKWISE,
}

/// `modifyBendPoints(_:_:_:)`, the hook `PolylineSelfLoopRouter` overrides.
pub(crate) type ModifyBendPoints = fn(&LGraphArena, &SelfLoopHolder, SlEdgeId, EdgeRoutingDirection, KVectorChain) -> KVectorChain;

#[derive(Default)]
pub struct OrthogonalSelfLoopRouter;

impl OrthogonalSelfLoopRouter {
    pub fn new() -> OrthogonalSelfLoopRouter {
        OrthogonalSelfLoopRouter
    }

    /// The orthogonal router's `modifyBendPoints`: returns the bend points
    /// unchanged.
    pub(crate) fn modify_bend_points(_lg: &LGraphArena, _sl_holder: &SelfLoopHolder, _sl_edge: SlEdgeId, _routing_direction: EdgeRoutingDirection, bend_points: KVectorChain) -> KVectorChain {
        bend_points
    }
}

impl AbstractSelfLoopRouter for OrthogonalSelfLoopRouter {
    fn route_self_loops(&self, lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder) {
        route_self_loops_with(lg, sl_holder, OrthogonalSelfLoopRouter::modify_bend_points);
    }
}

/// `routeSelfLoops(_:)`, dispatching `modifyBendPoints` to `modify`.
pub(crate) fn route_self_loops_with(lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder, modify: ModifyBendPoints) {
    let l_node = sl_holder.get_l_node();
    let node_size = lg[l_node].size;
    let node_margins: LMargin = lg[l_node].margin;

    let edge_edge_distance = lg.get_individual_or_inherited(l_node, &LayeredOptions::SPACING_EDGE_EDGE);
    let edge_label_distance = lg.get_individual_or_inherited(l_node, &LayeredOptions::SPACING_EDGE_LABEL);
    let node_sl_distance = lg.get_individual_or_inherited(l_node, &LayeredOptions::SPACING_NODE_SELF_LOOP);

    let mut new_node_margins = LMargin::default();
    new_node_margins.set(&node_margins);

    let routing_slot_positions = compute_routing_slot_positions(lg, sl_holder, edge_edge_distance, edge_label_distance, node_sl_distance);

    for sl_loop in sl_holder.sl_loop_ids() {
        // NONDETERMINISTIC IN SWIFT: `getSLEdges()` is a `Set`; see
        // `SelfHyperLoop.sl_edges` (insertion order).
        for sl_edge in sl_holder.sl_loop(sl_loop).get_sl_edges().to_vec() {
            let l_edge = sl_holder.sl_edge(sl_edge).get_l_edge();
            let routing_direction = compute_edge_routing_direction(lg, sl_holder, sl_edge);

            let mut bend_points = compute_orthogonal_bend_points(lg, sl_holder, sl_edge, routing_direction, &routing_slot_positions);
            bend_points = modify(lg, sl_holder, sl_edge, routing_direction, bend_points);

            lg[l_edge].bend_points.clear();
            for bp in bend_points.iter() {
                lg[l_edge].bend_points.add(*bp);
                update_new_node_margins(node_size, &mut new_node_margins, *bp);
            }
        }

        if sl_holder.sl_loop(sl_loop).get_sl_labels().is_some() {
            place_labels(lg, sl_holder, sl_loop, &routing_slot_positions, edge_label_distance);
            if let Some(sl_labels) = sl_holder.sl_loop(sl_loop).get_sl_labels() {
                update_new_node_margins_for_labels(node_size, &mut new_node_margins, sl_labels);
            }
        }
    }

    lg[l_node].margin.set(&new_node_margins);
}

fn compute_edge_routing_direction(lg: &LGraphArena, sl_holder: &SelfLoopHolder, sl_edge: SlEdgeId) -> EdgeRoutingDirection {
    let e = sl_holder.sl_edge(sl_edge);
    let source_l_port = sl_holder.sl_port(e.get_sl_source()).get_l_port();
    let source_port_side = lg[source_l_port].side;
    let target_l_port = sl_holder.sl_port(e.get_sl_target()).get_l_port();
    let target_port_side = lg[target_l_port].side;

    if source_port_side == target_port_side {
        if lg[source_l_port].id < lg[target_l_port].id {
            EdgeRoutingDirection::CLOCKWISE
        } else {
            EdgeRoutingDirection::COUNTER_CLOCKWISE
        }
    } else if source_port_side.right() == target_port_side {
        EdgeRoutingDirection::CLOCKWISE
    } else if source_port_side.left() == target_port_side {
        EdgeRoutingDirection::COUNTER_CLOCKWISE
    } else {
        let Some(sl_loop) = e.get_sl_hyper_loop() else { return EdgeRoutingDirection::CLOCKWISE };
        if sl_holder.sl_loop(sl_loop).get_occupied_port_sides().contains(source_port_side.right()) {
            EdgeRoutingDirection::CLOCKWISE
        } else {
            EdgeRoutingDirection::COUNTER_CLOCKWISE
        }
    }
}

fn place_labels(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, sl_loop: SlLoopId, routing_slot_positions: &[Vec<f64>], edge_label_distance: f64) {
    let l = sl_holder.sl_loop(sl_loop);
    let Some(sl_labels) = l.get_sl_labels() else { return };
    let label_side = sl_labels.get_side();
    let mut label_position = routing_slot_positions[label_side.ordinal()][l.get_routing_slot(label_side) as usize];
    let inline = sl_labels.get_l_labels().iter().any(|&label| lg[label].props.get_as::<bool>(&LayeredOptions::EDGE_LABELS_INLINE) == Some(true));
    let mut effective_edge_label_distance = edge_label_distance;
    if inline {
        effective_edge_label_distance = 0.0;
    }
    let size = sl_labels.get_size();

    let Some(sl_labels) = sl_holder.sl_loop_mut(sl_loop).get_sl_labels_mut() else { return };
    match label_side {
        PortSide::NORTH => {
            label_position -= effective_edge_label_distance + size.y;
            sl_labels.position_mut().y = label_position;
        }
        PortSide::SOUTH => {
            label_position += effective_edge_label_distance;
            sl_labels.position_mut().y = label_position;
        }
        PortSide::WEST => {
            label_position -= effective_edge_label_distance + size.x;
            sl_labels.position_mut().x = label_position;
        }
        PortSide::EAST => {
            label_position += effective_edge_label_distance;
            sl_labels.position_mut().x = label_position;
        }
        _ => {}
    }
}

fn update_new_node_margins(node_size: KVector, new_node_margins: &mut LMargin, bend_point: KVector) {
    new_node_margins.left = swift::max(new_node_margins.left, -bend_point.x);
    new_node_margins.right = swift::max(new_node_margins.right, bend_point.x - node_size.x);
    new_node_margins.top = swift::max(new_node_margins.top, -bend_point.y);
    new_node_margins.bottom = swift::max(new_node_margins.bottom, bend_point.y - node_size.y);
}

fn update_new_node_margins_for_labels(node_size: KVector, new_node_margins: &mut LMargin, sl_labels: &SelfHyperLoopLabels) {
    let mut pos = sl_labels.get_position();
    update_new_node_margins(node_size, new_node_margins, pos);
    pos.add(sl_labels.get_size());
    update_new_node_margins(node_size, new_node_margins, pos);
}

// MARK: - Routing Slot Positions

fn compute_routing_slot_positions(lg: &LGraphArena, sl_holder: &SelfLoopHolder, edge_edge_distance: f64, edge_label_distance: f64, node_sl_distance: f64) -> Vec<Vec<f64>> {
    let side_count = 5; // PortSide enum count
    let mut positions: Vec<Vec<f64>> = vec![Vec::new(); side_count];
    for side in [PortSide::UNDEFINED, PortSide::NORTH, PortSide::EAST, PortSide::SOUTH, PortSide::WEST] {
        let slot_count = sl_holder.get_routing_slot_count()[side.ordinal()];
        positions[side.ordinal()] = vec![0.0; slot_count as usize];
    }

    initialize_with_max_label_height(&mut positions, sl_holder, PortSide::NORTH);
    initialize_with_max_label_height(&mut positions, sl_holder, PortSide::SOUTH);

    compute_positions(lg, &mut positions, sl_holder, PortSide::NORTH, edge_edge_distance, edge_label_distance, node_sl_distance);
    compute_positions(lg, &mut positions, sl_holder, PortSide::EAST, edge_edge_distance, edge_label_distance, node_sl_distance);
    compute_positions(lg, &mut positions, sl_holder, PortSide::SOUTH, edge_edge_distance, edge_label_distance, node_sl_distance);
    compute_positions(lg, &mut positions, sl_holder, PortSide::WEST, edge_edge_distance, edge_label_distance, node_sl_distance);

    positions
}

fn initialize_with_max_label_height(positions: &mut [Vec<f64>], sl_holder: &SelfLoopHolder, port_side: PortSide) {
    for l in sl_holder.get_sl_hyper_loops() {
        if let Some(sl_labels) = l.get_sl_labels() {
            if sl_labels.get_side() == port_side {
                let routing_slot = l.get_routing_slot(port_side);
                let side_positions = &mut positions[port_side.ordinal()];
                if routing_slot < side_positions.len() as i64 {
                    let slot = routing_slot as usize;
                    side_positions[slot] = swift::max(side_positions[slot], sl_labels.get_size().y);
                }
            }
        }
    }
}

fn compute_positions(
    lg: &LGraphArena,
    positions: &mut [Vec<f64>],
    sl_holder: &SelfLoopHolder,
    port_side: PortSide,
    edge_edge_distance: f64,
    edge_label_distance: f64,
    node_self_loop_distance: f64,
) {
    let mut curr_pos = compute_baseline_position(lg, sl_holder, port_side, node_self_loop_distance);
    let factor: f64 = if port_side == PortSide::NORTH || port_side == PortSide::WEST { -1.0 } else { 1.0 };

    for slot in 0..positions[port_side.ordinal()].len() {
        let mut largest_label_size = positions[port_side.ordinal()][slot];
        if largest_label_size > 0.0 {
            largest_label_size += edge_label_distance;
        }
        positions[port_side.ordinal()][slot] = curr_pos;
        curr_pos += factor * (largest_label_size + edge_edge_distance);
    }
}

fn compute_baseline_position(lg: &LGraphArena, sl_holder: &SelfLoopHolder, port_side: PortSide, node_self_loop_distance: f64) -> f64 {
    let l_node = sl_holder.get_l_node();
    let l_margins = lg[l_node].margin;

    match port_side {
        PortSide::NORTH => -l_margins.top - node_self_loop_distance,
        PortSide::EAST => lg[l_node].size.x + l_margins.right + node_self_loop_distance,
        PortSide::SOUTH => lg[l_node].size.y + l_margins.bottom + node_self_loop_distance,
        PortSide::WEST => -l_margins.left - node_self_loop_distance,
        _ => -1.0,
    }
}

// MARK: - Bend Point Computation

fn compute_orthogonal_bend_points(
    lg: &LGraphArena,
    sl_holder: &SelfLoopHolder,
    sl_edge: SlEdgeId,
    routing_direction: EdgeRoutingDirection,
    routing_slot_positions: &[Vec<f64>],
) -> KVectorChain {
    let mut bend_points = KVectorChain::new();
    let e = sl_holder.sl_edge(sl_edge);
    add_outer_bend_point(lg, sl_holder, sl_edge, e.get_sl_source(), routing_slot_positions, &mut bend_points);
    add_corner_bend_points(lg, sl_holder, sl_edge, routing_direction, routing_slot_positions, &mut bend_points);
    add_outer_bend_point(lg, sl_holder, sl_edge, e.get_sl_target(), routing_slot_positions, &mut bend_points);
    bend_points
}

fn add_outer_bend_point(
    lg: &LGraphArena,
    sl_holder: &SelfLoopHolder,
    sl_edge: SlEdgeId,
    sl_port: SlPortId,
    routing_slot_positions: &[Vec<f64>],
    bend_points: &mut KVectorChain,
) {
    let Some(sl_loop) = sl_holder.sl_edge(sl_edge).get_sl_hyper_loop() else { return };
    let l_port = sl_holder.sl_port(sl_port).get_l_port();
    let port_side = lg[l_port].side;

    let mut result = get_base_vector(port_side, sl_holder.sl_loop(sl_loop).get_routing_slot(port_side), routing_slot_positions);

    let mut anchor = lg[l_port].position;
    anchor.add(lg[l_port].anchor);
    match lg[l_port].side {
        PortSide::NORTH | PortSide::SOUTH => result.x += anchor.x,
        PortSide::EAST | PortSide::WEST => result.y += anchor.y,
        _ => {}
    }

    bend_points.add(result);
}

fn add_corner_bend_points(
    lg: &LGraphArena,
    sl_holder: &SelfLoopHolder,
    sl_edge: SlEdgeId,
    routing_direction: EdgeRoutingDirection,
    routing_slot_positions: &[Vec<f64>],
    bend_points: &mut KVectorChain,
) {
    let e = sl_holder.sl_edge(sl_edge);
    let l_source_port = sl_holder.sl_port(e.get_sl_source()).get_l_port();
    let l_target_port = sl_holder.sl_port(e.get_sl_target()).get_l_port();

    if lg[l_source_port].side == lg[l_target_port].side {
        return;
    }

    let Some(sl_loop) = e.get_sl_hyper_loop() else { return };
    let l = sl_holder.sl_loop(sl_loop);
    let mut label_side: Option<PortSide> = None;
    let mut l_size: Option<KVector> = None;
    let inline = e.is_inline(lg);
    if inline {
        if let Some(labels) = l.get_sl_labels() {
            label_side = Some(sl_holder.sl_edge_label_side(sl_edge));
            l_size = Some(labels.get_size());
        }
    }

    let mut curr_port_side = lg[l_source_port].side;

    while curr_port_side != lg[l_target_port].side {
        let next_port_side = if routing_direction == EdgeRoutingDirection::CLOCKWISE { curr_port_side.right() } else { curr_port_side.left() };

        let mut curr_port_side_component = get_base_vector(curr_port_side, l.get_routing_slot(curr_port_side), routing_slot_positions);
        let mut next_port_side_component = get_base_vector(next_port_side, l.get_routing_slot(next_port_side), routing_slot_positions);

        if inline {
            if let (Some(ls), Some(sz)) = (label_side, l_size) {
                if curr_port_side == ls {
                    adjust_vector_for_label_side(&mut curr_port_side_component, ls, sz);
                } else if next_port_side == ls {
                    adjust_vector_for_label_side(&mut next_port_side_component, ls, sz);
                }
            }
        }

        curr_port_side_component.add(next_port_side_component);
        bend_points.add(curr_port_side_component);
        curr_port_side = next_port_side;
    }
}

fn get_base_vector(port_side: PortSide, routing_slot: i64, routing_slot_positions: &[Vec<f64>]) -> KVector {
    let position = routing_slot_positions[port_side.ordinal()][routing_slot as usize];

    match port_side {
        PortSide::NORTH | PortSide::SOUTH => KVector::new(0.0, position),
        PortSide::EAST | PortSide::WEST => KVector::new(position, 0.0),
        _ => KVector::default(),
    }
}

fn adjust_vector_for_label_side(port_side_component: &mut KVector, label_side: PortSide, label_size: KVector) {
    match label_side {
        PortSide::NORTH => port_side_component.y -= label_size.y / 2.0,
        PortSide::SOUTH => port_side_component.y += label_size.y / 2.0,
        PortSide::WEST => port_side_component.x -= label_size.x / 2.0,
        PortSide::EAST => port_side_component.x += label_size.x / 2.0,
        _ => {}
    }
}
