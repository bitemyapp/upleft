//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/routing/org_eclipse_elk_alg_layered_intermediate_loops_routing_PolylineSelfLoopRouter.swift`.
//!
//! An `OrthogonalSelfLoopRouter` whose `modifyBendPoints` adds the port
//! anchors and cuts the corners.

use super::abstract_self_loop_router::AbstractSelfLoopRouter;
use super::orthogonal_self_loop_router::{route_self_loops_with, EdgeRoutingDirection};
use super::super::self_loop_edge::SlEdgeId;
use super::super::self_loop_holder::SelfLoopHolder;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChain;
use crate::swift;

const CORNER_DISTANCE: f64 = 10.0;
const TOLERANCE: f64 = 0.01;

#[derive(Default)]
pub struct PolylineSelfLoopRouter;

impl PolylineSelfLoopRouter {
    pub fn new() -> PolylineSelfLoopRouter {
        PolylineSelfLoopRouter
    }

    fn modify_bend_points(lg: &LGraphArena, sl_holder: &SelfLoopHolder, sl_edge: SlEdgeId, _routing_direction: EdgeRoutingDirection, bend_points: KVectorChain) -> KVectorChain {
        let mut bend_points = bend_points;
        let e = sl_holder.sl_edge(sl_edge);
        let l_source_port = sl_holder.sl_port(e.get_sl_source()).get_l_port();
        let mut source = lg[l_source_port].position;
        source.add(lg[l_source_port].anchor);
        bend_points.insert(0, source);

        let l_target_port = sl_holder.sl_port(e.get_sl_target()).get_l_port();
        let mut target = lg[l_target_port].position;
        target.add(lg[l_target_port].anchor);
        bend_points.add(target);

        cut_corners(&bend_points, CORNER_DISTANCE)
    }
}

impl AbstractSelfLoopRouter for PolylineSelfLoopRouter {
    fn route_self_loops(&self, lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder) {
        route_self_loops_with(lg, sl_holder, PolylineSelfLoopRouter::modify_bend_points);
    }
}

/// `cutCorners(_:_:)`.
pub fn cut_corners(bend_points: &KVectorChain, distance: f64) -> KVectorChain {
    let mut result = KVectorChain::new();

    let mut corner = bend_points.get(0);
    let mut next = bend_points.get(1);

    for second_bp_index in 2..bend_points.size() {
        let previous = corner;
        corner = next;
        next = bend_points.get(second_bp_index);

        let mut offset1 = previous;
        offset1.sub(corner);
        near_zero_to_zero(&mut offset1);
        let mut offset2 = next;
        offset2.sub(corner);
        near_zero_to_zero(&mut offset2);

        let mut effective_distance = distance;
        effective_distance = swift::min(effective_distance, (offset1.x + offset1.y).abs() / 2.0);
        effective_distance = swift::min(effective_distance, (offset2.x + offset2.y).abs() / 2.0);

        offset1.x = effective_distance.copysign(offset1.x) * if offset1.x == 0.0 { 0.0 } else { 1.0 };
        offset1.y = effective_distance.copysign(offset1.y) * if offset1.y == 0.0 { 0.0 } else { 1.0 };
        offset2.x = effective_distance.copysign(offset2.x) * if offset2.x == 0.0 { 0.0 } else { 1.0 };
        offset2.y = effective_distance.copysign(offset2.y) * if offset2.y == 0.0 { 0.0 } else { 1.0 };

        offset1.add(corner);
        result.add(offset1);
        offset2.add(corner);
        result.add(offset2);
    }

    result
}

fn near_zero_to_zero(vector: &mut KVector) {
    if vector.x >= -TOLERANCE && vector.x <= TOLERANCE {
        vector.x = 0.0;
    }
    if vector.y >= -TOLERANCE && vector.y <= TOLERANCE {
        vector.y = 0.0;
    }
}
