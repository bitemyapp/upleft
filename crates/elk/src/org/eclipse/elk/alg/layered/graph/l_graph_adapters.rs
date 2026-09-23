//! Port of `alg/layered/graph/LGraphAdapters.swift`: the `GraphAdapters`
//! protocols implemented on the layered graph. In elk-swift these are the only
//! live adapters (`ElkGraphAdapters.adapt` returns `nil`).
//!
//! An adapter is a small `Copy` handle: the element id plus the Swift flags.
//! Every method takes the arena. The Swift adapter caches (`nodeAdapters`,
//! `labelAdapters`, `portAdapters`) are not kept: within one adapter's lifetime
//! (one processor call) no reachable code adds or removes nodes, ports or
//! labels, so a cached list and a fresh one are the same.
//!
//! Semantics kept from the Swift:
//! * `getProperty<P>(prop) -> P?` is `element.getProperty(prop) as? P` (the
//!   stored value, else the property default, cast with `as?`):
//!   [`PropertyMap::get_as`].
//! * `getSize()`/`getPosition()` return the element's own `KVector`; callers
//!   in the ported code only read them, so the port returns copies.
//! * `setSize(v)`/`setPosition(v)` assign the caller's `KVector` object to the
//!   element (`element.size = size`), so element and caller share one vector
//!   afterwards. Every caller (`NodeContext.applyNodeSize`,
//!   `PortContext.applyPortPosition`, `LabelCell.apply*LabelLayout`) passes a
//!   vector that is never touched again after the call (the node/port context
//!   is dropped right after `applyStuff`, a label cell's `labelPos` is a fresh
//!   local), so copying the value is exact.
//! * `getPadding()`/`getMargin()` return copies; `setPadding`/`setMargin` copy
//!   the values into the element's own object.

use crate::org::eclipse::elk::core::math::elk_margin::ElkMargin;
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::org::eclipse::elk::core::options::label_side::{self, LabelSide};
use crate::prelude::*;

/// `(LNode) -> Bool` node filter of `LGraphAdapter`.
pub type NodeFilter = fn(&LGraphArena, LNodeId) -> bool;

/// `{ _ in true }`.
pub fn accept_all_nodes(_lg: &LGraphArena, _node: LNodeId) -> bool {
    true
}

/// `LGraphAdapters`: the `adapt` factory overloads.
pub struct LGraphAdapters;

impl LGraphAdapters {
    /// `adapt(_ graph:)`.
    pub fn adapt(graph: LGraphId) -> LGraphAdapter {
        LGraphAdapter::new(graph, false, false, accept_all_nodes)
    }

    /// `adapt(_ graph:, transparentNorthSouthEdges:)`.
    pub fn adapt_ns(graph: LGraphId, transparent_north_south_edges: bool) -> LGraphAdapter {
        LGraphAdapter::new(graph, transparent_north_south_edges, false, accept_all_nodes)
    }

    /// `adapt(_ graph:, transparentNorthSouthEdges:, transparentCommentNodes:, nodeFilter:)`.
    pub fn adapt_filtered(
        graph: LGraphId,
        transparent_north_south_edges: bool,
        transparent_comment_nodes: bool,
        node_filter: NodeFilter,
    ) -> LGraphAdapter {
        LGraphAdapter::new(graph, transparent_north_south_edges, transparent_comment_nodes, node_filter)
    }

    /// `adapt(_ node:, transparentNorthSouthEdges:)`: a node adapter without a
    /// parent graph adapter (its `getGraph()` is `nil`).
    pub fn adapt_node(node: LNodeId, transparent_north_south_edges: bool) -> LNodeAdapter {
        LNodeAdapter { parent_graph_adapter: None, element: node, transparent_north_south_edges }
    }

    /// `adapt(_ label:)`.
    pub fn adapt_label(label: LLabelId) -> LLabelAdapter {
        LLabelAdapter { element: label }
    }
}

// MARK: - LGraphAdapter

/// `LGraphAdapter`.
#[derive(Clone, Copy)]
pub struct LGraphAdapter {
    pub element: LGraphId,
    pub transparent_north_south_edges: bool,
    pub transparent_comment_nodes: bool,
    pub node_filter: NodeFilter,
}

impl std::fmt::Debug for LGraphAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LGraphAdapter({:?})", self.element)
    }
}

impl LGraphAdapter {
    pub fn new(
        element: LGraphId,
        transparent_north_south_edges: bool,
        transparent_comment_nodes: bool,
        node_filter: NodeFilter,
    ) -> LGraphAdapter {
        LGraphAdapter { element, transparent_north_south_edges, transparent_comment_nodes, node_filter }
    }

    pub fn get_size(&self, lg: &LGraphArena) -> KVector {
        lg[self.element].size
    }

    pub fn set_size(&self, lg: &mut LGraphArena, size: KVector) {
        lg[self.element].size = size;
    }

    /// The graph's `offset`.
    pub fn get_position(&self, lg: &LGraphArena) -> KVector {
        lg[self.element].offset
    }

    pub fn set_position(&self, lg: &mut LGraphArena, pos: KVector) {
        lg[self.element].offset = pos;
    }

    /// `getProperty<P>(_:) -> P?` (`as? P`).
    pub fn get_property<T: PropCast>(&self, lg: &LGraphArena, prop: &Property) -> Option<T> {
        lg[self.element].props.get_as::<T>(prop)
    }

    /// `getProperty(_:)` as `Any?`.
    pub fn get_property_value(&self, lg: &LGraphArena, prop: &Property) -> Option<PropValue> {
        lg[self.element].props.get(prop)
    }

    pub fn has_property(&self, lg: &LGraphArena, prop: &Property) -> bool {
        lg[self.element].props.has(prop)
    }

    pub fn get_volatile_id(&self, lg: &LGraphArena) -> i32 {
        lg[self.element].id
    }

    pub fn set_volatile_id(&self, lg: &mut LGraphArena, id: i32) {
        lg[self.element].id = id;
    }

    /// `getNodes()`: the filtered nodes of all layers, layer by layer.
    pub fn get_nodes(&self, lg: &LGraphArena) -> Vec<LNodeAdapter> {
        let mut computed = Vec::new();
        for &layer in &lg[self.element].layers {
            for &node in &lg[layer].nodes {
                if (self.node_filter)(lg, node) {
                    computed.push(LNodeAdapter {
                        parent_graph_adapter: Some(*self),
                        element: node,
                        transparent_north_south_edges: self.transparent_north_south_edges,
                    });
                }
            }
        }
        computed
    }
}

// MARK: - LNodeAdapter

/// `LNodeAdapter`. `parentGraphAdapter` is a weak reference in Swift; every
/// reachable use keeps the graph adapter alive for the node adapter's lifetime.
#[derive(Clone, Copy, Debug)]
pub struct LNodeAdapter {
    pub parent_graph_adapter: Option<LGraphAdapter>,
    pub element: LNodeId,
    pub transparent_north_south_edges: bool,
}

impl LNodeAdapter {
    pub fn get_size(&self, lg: &LGraphArena) -> KVector {
        lg[self.element].size
    }

    pub fn set_size(&self, lg: &mut LGraphArena, size: KVector) {
        lg[self.element].size = size;
    }

    pub fn get_position(&self, lg: &LGraphArena) -> KVector {
        lg[self.element].position
    }

    pub fn set_position(&self, lg: &mut LGraphArena, pos: KVector) {
        lg[self.element].position = pos;
    }

    /// `getProperty<P>(_:) -> P?` (`as? P`).
    pub fn get_property<T: PropCast>(&self, lg: &LGraphArena, prop: &Property) -> Option<T> {
        lg[self.element].props.get_as::<T>(prop)
    }

    /// `getProperty(_:)` as `Any?`.
    pub fn get_property_value(&self, lg: &LGraphArena, prop: &Property) -> Option<PropValue> {
        lg[self.element].props.get(prop)
    }

    pub fn has_property(&self, lg: &LGraphArena, prop: &Property) -> bool {
        lg[self.element].props.has(prop)
    }

    pub fn get_volatile_id(&self, lg: &LGraphArena) -> i32 {
        lg[self.element].id
    }

    pub fn set_volatile_id(&self, lg: &mut LGraphArena, id: i32) {
        lg[self.element].id = id;
    }

    /// `getGraph()`: the parent graph adapter, if any.
    pub fn get_graph(&self) -> Option<LGraphAdapter> {
        self.parent_graph_adapter
    }

    pub fn get_labels(&self, lg: &LGraphArena) -> Vec<LLabelAdapter> {
        lg[self.element].labels.iter().map(|&l| LLabelAdapter { element: l }).collect()
    }

    pub fn get_ports(&self, lg: &LGraphArena) -> Vec<LPortAdapter> {
        let tnse = self.transparent_north_south_edges;
        lg[self.element].ports.iter().map(|&p| LPortAdapter { element: p, transparent_north_south_edges: tnse }).collect()
    }

    /// Always empty in elk-swift.
    pub fn get_incoming_edges(&self, _lg: &LGraphArena) -> Vec<LEdgeAdapter> {
        Vec::new()
    }

    /// Always empty in elk-swift.
    pub fn get_outgoing_edges(&self, _lg: &LGraphArena) -> Vec<LEdgeAdapter> {
        Vec::new()
    }

    /// A no-op in elk-swift.
    pub fn sort_port_list(&self, _lg: &mut LGraphArena) {}

    pub fn is_compound_node(&self, lg: &LGraphArena) -> bool {
        lg[self.element].props.get_as::<bool>(&InternalProperties::COMPOUND_NODE).unwrap_or(false)
    }

    /// A copy of the node's padding.
    pub fn get_padding(&self, lg: &LGraphArena) -> ElkPadding {
        let p = lg[self.element].padding;
        ElkPadding::new(p.top, p.right, p.bottom, p.left)
    }

    /// Copies the values into the node's own padding.
    pub fn set_padding(&self, lg: &mut LGraphArena, padding: &ElkPadding) {
        lg[self.element].padding.set4(padding.top, padding.right, padding.bottom, padding.left);
    }

    /// A copy of the node's margin.
    pub fn get_margin(&self, lg: &LGraphArena) -> ElkMargin {
        let m = lg[self.element].margin;
        ElkMargin::new(m.top, m.right, m.bottom, m.left)
    }

    /// Copies the values into the node's own margin.
    pub fn set_margin(&self, lg: &mut LGraphArena, margin: &ElkMargin) {
        lg[self.element].margin.set4(margin.top, margin.right, margin.bottom, margin.left);
    }
}

// MARK: - LPortAdapter

/// `LPortAdapter`. `transparentNorthSouthEdges` is stored but, as in
/// elk-swift, not used by the edge accessors.
#[derive(Clone, Copy, Debug)]
pub struct LPortAdapter {
    pub element: LPortId,
    pub transparent_north_south_edges: bool,
}

impl LPortAdapter {
    pub fn get_size(&self, lg: &LGraphArena) -> KVector {
        lg[self.element].size
    }

    pub fn set_size(&self, lg: &mut LGraphArena, size: KVector) {
        lg[self.element].size = size;
    }

    pub fn get_position(&self, lg: &LGraphArena) -> KVector {
        lg[self.element].position
    }

    pub fn set_position(&self, lg: &mut LGraphArena, pos: KVector) {
        lg[self.element].position = pos;
    }

    /// `getProperty<P>(_:) -> P?` (`as? P`).
    pub fn get_property<T: PropCast>(&self, lg: &LGraphArena, prop: &Property) -> Option<T> {
        lg[self.element].props.get_as::<T>(prop)
    }

    pub fn get_property_value(&self, lg: &LGraphArena, prop: &Property) -> Option<PropValue> {
        lg[self.element].props.get(prop)
    }

    pub fn has_property(&self, lg: &LGraphArena, prop: &Property) -> bool {
        lg[self.element].props.has(prop)
    }

    pub fn get_volatile_id(&self, lg: &LGraphArena) -> i32 {
        lg[self.element].id
    }

    pub fn set_volatile_id(&self, lg: &mut LGraphArena, id: i32) {
        lg[self.element].id = id;
    }

    pub fn get_side(&self, lg: &LGraphArena) -> PortSide {
        lg[self.element].side
    }

    pub fn get_labels(&self, lg: &LGraphArena) -> Vec<LLabelAdapter> {
        lg[self.element].labels.iter().map(|&l| LLabelAdapter { element: l }).collect()
    }

    pub fn get_incoming_edges(&self, lg: &LGraphArena) -> Vec<LEdgeAdapter> {
        lg[self.element].incoming_edges.iter().map(|&e| LEdgeAdapter { element: e }).collect()
    }

    pub fn get_outgoing_edges(&self, lg: &LGraphArena) -> Vec<LEdgeAdapter> {
        lg[self.element].outgoing_edges.iter().map(|&e| LEdgeAdapter { element: e }).collect()
    }

    pub fn has_compound_connections(&self, lg: &LGraphArena) -> bool {
        lg[self.element].props.get_as::<bool>(&InternalProperties::INSIDE_CONNECTIONS).unwrap_or(false)
    }
}

// MARK: - LLabelAdapter

/// `LLabelAdapter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LLabelAdapter {
    pub element: LLabelId,
}

impl LLabelAdapter {
    pub fn get_size(&self, lg: &LGraphArena) -> KVector {
        lg[self.element].size
    }

    pub fn set_size(&self, lg: &mut LGraphArena, size: KVector) {
        lg[self.element].size = size;
    }

    pub fn get_position(&self, lg: &LGraphArena) -> KVector {
        lg[self.element].position
    }

    pub fn set_position(&self, lg: &mut LGraphArena, pos: KVector) {
        lg[self.element].position = pos;
    }

    /// `getProperty<P>(_:) -> P?` (`as? P`).
    pub fn get_property<T: PropCast>(&self, lg: &LGraphArena, prop: &Property) -> Option<T> {
        lg[self.element].props.get_as::<T>(prop)
    }

    pub fn get_property_value(&self, lg: &LGraphArena, prop: &Property) -> Option<PropValue> {
        lg[self.element].props.get(prop)
    }

    pub fn has_property(&self, lg: &LGraphArena, prop: &Property) -> bool {
        lg[self.element].props.has(prop)
    }

    pub fn get_volatile_id(&self, lg: &LGraphArena) -> i32 {
        lg[self.element].id
    }

    pub fn set_volatile_id(&self, lg: &mut LGraphArena, id: i32) {
        lg[self.element].id = id;
    }

    /// `LabelSide.LABEL_SIDE as? LabelSide ?? .UNKNOWN` (the core
    /// `org.eclipse.elk.labelSide` property, not the layered-internal one).
    pub fn get_side(&self, lg: &LGraphArena) -> LabelSide {
        lg[self.element].props.get_as::<LabelSide>(&label_side::LABEL_SIDE).unwrap_or(LabelSide::UNKNOWN)
    }

    pub fn get_text(&self, lg: &LGraphArena) -> String {
        lg[self.element].text.clone()
    }
}

// MARK: - LEdgeAdapter

/// `LEdgeAdapter`: size and position are always `(0, 0)` and cannot be set.
#[derive(Clone, Copy, Debug)]
pub struct LEdgeAdapter {
    pub element: LEdgeId,
}

impl LEdgeAdapter {
    pub fn get_size(&self, _lg: &LGraphArena) -> KVector {
        KVector::default()
    }

    pub fn set_size(&self, _lg: &mut LGraphArena, _size: KVector) {}

    pub fn get_position(&self, _lg: &LGraphArena) -> KVector {
        KVector::default()
    }

    pub fn set_position(&self, _lg: &mut LGraphArena, _pos: KVector) {}

    /// `getProperty<P>(_:) -> P?` (`as? P`).
    pub fn get_property<T: PropCast>(&self, lg: &LGraphArena, prop: &Property) -> Option<T> {
        lg[self.element].props.get_as::<T>(prop)
    }

    pub fn get_property_value(&self, lg: &LGraphArena, prop: &Property) -> Option<PropValue> {
        lg[self.element].props.get(prop)
    }

    pub fn has_property(&self, lg: &LGraphArena, prop: &Property) -> bool {
        lg[self.element].props.has(prop)
    }

    pub fn get_volatile_id(&self, lg: &LGraphArena) -> i32 {
        lg[self.element].id
    }

    pub fn set_volatile_id(&self, lg: &mut LGraphArena, id: i32) {
        lg[self.element].id = id;
    }

    pub fn get_labels(&self, lg: &LGraphArena) -> Vec<LLabelAdapter> {
        lg[self.element].labels.iter().map(|&l| LLabelAdapter { element: l }).collect()
    }
}
