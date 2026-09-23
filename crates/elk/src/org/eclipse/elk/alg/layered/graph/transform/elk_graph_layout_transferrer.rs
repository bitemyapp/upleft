//! Port of `alg/layered/graph/transform/ElkGraphLayoutTransferrer.swift`.
//!
//! Not ported yet.

use crate::bridge::elk_graph_impl::ElkGraph;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};

#[derive(Default)]
pub struct ElkGraphLayoutTransferrer;

impl ElkGraphLayoutTransferrer {
    pub fn new() -> ElkGraphLayoutTransferrer {
        ElkGraphLayoutTransferrer
    }

    pub fn apply(&mut self, _graph: &mut ElkGraph, _lg: &mut LGraphArena, _layered_graph: LGraphId) {
        unimplemented!()
    }
}
