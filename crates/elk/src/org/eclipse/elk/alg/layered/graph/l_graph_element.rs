//! Port of `alg/layered/graph/LGraphElement.swift`.
//!
//! Every element record carries `props` (the `MapPropertyHolder`) and `id`
//! (the scratch integer algorithms use). Identity and hashing are the typed
//! arena indices.

use super::l_graph::{LEdgeId, LGraphId, LLabelId, LNodeId, LPortId, LayerId};

/// Any layered-graph element (Swift code often handles `LGraphElement`s
/// generically, e.g. as property owners or `ORIGIN` values).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LElement {
    Graph(LGraphId),
    Layer(LayerId),
    Node(LNodeId),
    Port(LPortId),
    Edge(LEdgeId),
    Label(LLabelId),
}
