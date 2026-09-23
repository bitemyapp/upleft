//! Port of `alg/layered/graph/transform/ElkGraphImporter.swift`.
//!
//! Not ported yet.

use crate::bridge::elk::ElkError;
use crate::bridge::elk_graph_impl::{ElkGraph, ElkNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};

#[derive(Default)]
pub struct ElkGraphImporter;

impl ElkGraphImporter {
    pub fn new() -> ElkGraphImporter {
        ElkGraphImporter
    }

    pub fn import_graph(&mut self, _graph: &mut ElkGraph, _lg: &mut LGraphArena, _elkgraph: ElkNodeId) -> Result<LGraphId, ElkError> {
        unimplemented!()
    }
}
