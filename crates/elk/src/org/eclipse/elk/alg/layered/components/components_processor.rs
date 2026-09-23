//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/components/org_eclipse_elk_alg_layered_components_ComponentsProcessor.swift`.
//!
//! Not ported yet.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};

#[derive(Default)]
pub struct ComponentsProcessor;

impl ComponentsProcessor {
    pub fn new() -> ComponentsProcessor {
        ComponentsProcessor
    }

    /// `split(_:)`.
    pub fn split(&mut self, _lg: &mut LGraphArena, _graph: LGraphId) -> Vec<LGraphId> {
        unimplemented!("ComponentsProcessor is not ported yet")
    }

    /// `combine(_:target:)`.
    pub fn combine(&mut self, _lg: &mut LGraphArena, _components: &[LGraphId], _target: LGraphId) {
        unimplemented!("ComponentsProcessor is not ported yet")
    }
}
