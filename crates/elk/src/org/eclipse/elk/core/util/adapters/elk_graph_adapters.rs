//! Port of `core/util/adapters/ElkGraphAdapters.swift`: the adapter protocols
//! (`GraphElementAdapter`, `GraphAdapter`, `NodeAdapter`, `PortAdapter`,
//! `LabelAdapter`, `EdgeAdapter`) and the `ElkGraphAdapters` stub.
//!
//! The protocols' only live implementation in elk-swift is the layered-graph
//! one (`LGraphAdapters`); `ElkGraphAdapters.adapt` returns `nil`. The port
//! therefore names each protocol as the concrete layered adapter type (methods
//! take the `LGraphArena`), and `ElkGraphAdapters` returns `None` with an
//! uninhabited result type.
//!
//! The protocol extension `getProperty<P>(_:) -> P` (non-optional: the value
//! `as? P`, else the property default `as? P`, else a zero value) is not used
//! by any reachable call site that could observe the difference from the
//! optional form; call sites use `get_property` (`as? P`) with `unwrap_or`.

use crate::bridge::elk_graph_impl::ElkNodeId;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters;

/// `GraphAdapter`.
pub type GraphAdapter = l_graph_adapters::LGraphAdapter;
/// `NodeAdapter`.
pub type NodeAdapter = l_graph_adapters::LNodeAdapter;
/// `PortAdapter`.
pub type PortAdapter = l_graph_adapters::LPortAdapter;
/// `LabelAdapter`.
pub type LabelAdapter = l_graph_adapters::LLabelAdapter;
/// `EdgeAdapter`.
pub type EdgeAdapter = l_graph_adapters::LEdgeAdapter;

/// The (never produced) adapter of an `ElkNode` graph.
pub enum ElkGraphAdapter {}

/// The (never produced) adapter of a single `ElkNode`.
pub enum ElkNodeAdapter {}

/// `ElkGraphAdapters` (stub implementation in elk-swift).
pub struct ElkGraphAdapters;

impl ElkGraphAdapters {
    /// `adapt(_ node:)`: always `nil` in elk-swift.
    pub fn adapt(_node: ElkNodeId) -> Option<ElkGraphAdapter> {
        None
    }

    /// `adaptSingleNode(_ node:)`: always `nil` in elk-swift.
    pub fn adapt_single_node(_node: ElkNodeId) -> Option<ElkNodeAdapter> {
        None
    }
}
