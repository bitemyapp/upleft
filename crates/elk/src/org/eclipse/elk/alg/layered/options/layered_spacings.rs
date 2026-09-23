//! Port of `alg/layered/options/LayeredSpacings.swift` — a stub in
//! elk-swift: `withBaseValue(_:).apply(_:)` does nothing.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};

pub struct LayeredSpacings;

impl LayeredSpacings {
    pub fn with_base_value(_base_value: f64) -> LayeredSpacings {
        LayeredSpacings
    }

    pub fn apply(&self, _lg: &mut LGraphArena, _graph: LGraphId) {}
}
