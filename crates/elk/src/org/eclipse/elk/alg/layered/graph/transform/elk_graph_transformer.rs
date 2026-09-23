//! Port of `alg/layered/graph/transform/ElkGraphTransformer.swift`.

use super::elk_graph_importer::ElkGraphImporter;
use super::elk_graph_layout_transferrer::ElkGraphLayoutTransferrer;
use crate::bridge::elk::ElkError;
use crate::bridge::elk_graph_impl::{ElkGraph, ElkNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};

#[derive(Default)]
pub struct ElkGraphTransformer;

impl ElkGraphTransformer {
    pub fn new() -> ElkGraphTransformer {
        ElkGraphTransformer
    }

    /// `importGraph(_:)`: a fresh arena holding the imported layered graph.
    pub fn import_graph(&mut self, graph: &mut ElkGraph, elk_node: ElkNodeId) -> Result<Option<(LGraphArena, LGraphId)>, ElkError> {
        let mut lg = LGraphArena::new();
        let lgraph = ElkGraphImporter::new().import_graph(graph, &mut lg, elk_node)?;
        Ok(Some((lg, lgraph)))
    }

    /// `applyLayout(_:)`.
    pub fn apply_layout(&mut self, graph: &mut ElkGraph, lg: &mut LGraphArena, layered_graph: LGraphId) {
        ElkGraphLayoutTransferrer::new().apply(graph, lg, layered_graph);
    }
}
