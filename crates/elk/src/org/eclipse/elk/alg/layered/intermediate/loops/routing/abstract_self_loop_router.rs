//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/routing/org_eclipse_elk_alg_layered_intermediate_loops_routing_AbstractSelfLoopRouter.swift`.

use super::super::self_loop_holder::SelfLoopHolder;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena;

/// `AbstractSelfLoopRouter`: routes the self loops of one holder.
pub trait AbstractSelfLoopRouter {
    fn route_self_loops(&self, lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder);
}
