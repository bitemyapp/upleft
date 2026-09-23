//! Shared helpers for the ports of elk-swift's `Tests/ElkSwiftTests`
//! (`TestHelpers/`). Not every test file uses every helper.
#![allow(dead_code, unused_imports)]

pub mod in_layer_edge_test_graph_creator;
pub mod north_south_edge_test_graph_creator;
pub mod test_graph_creator;

pub use test_graph_creator::{MockRandom, TestGraphCreator};

use upleft_elk::prelude::*;

/// `nodes.map { ObjectIdentifier($0) }` comparisons become plain id lists.
pub fn ids(nodes: &[LNodeId]) -> Vec<LNodeId> {
    nodes.to_vec()
}
