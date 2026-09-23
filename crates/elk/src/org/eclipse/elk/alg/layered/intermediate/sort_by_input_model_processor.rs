//! Port of `alg/layered/intermediate/SortByInputModelProcessor.swift`.
//!
//! Before crossing minimization, sorts the nodes of every layer (twice,
//! around a port sort) and the ports of every node without fixed port order
//! by the model order of the input graph. elk-swift's version is its own
//! rewrite of the Java processor, with its own node and port comparators;
//! both remember each decision transitively and are used with a custom
//! insertion sort (not Swift's `sort`).
//!
//! Several options are read through the string-key `getProperty(_:)`
//! overload, which returns the stored value only (no default).

use std::collections::HashMap;
use std::rc::Rc;

use super::preserveorder::cm_group_model_order_calculator::CMGroupModelOrderCalculator;
use super::preserveorder::model_order_node_comparator::{IdMap, IdSet};
use crate::org::eclipse::elk::alg::layered::graph::l_graph_element::LElement;
use crate::org::eclipse::elk::alg::layered::options::group_order_strategy::GroupOrderStrategy;
use crate::org::eclipse::elk::alg::layered::options::long_edge_ordering_strategy::LongEdgeOrderingStrategy;
use crate::org::eclipse::elk::alg::layered::options::ordering_strategy::OrderingStrategy;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

/// `_Keys`.
pub mod _Keys {
    pub const portConstraints: &str = "org.eclipse.elk.portConstraints";
    pub const considerModelOrderStrategy: &str = "org.eclipse.elk.layered.considerModelOrder.strategy";
    pub const considerModelOrderLongEdgeStrategy: &str = "org.eclipse.elk.layered.considerModelOrder.longEdgeStrategy";
    pub const considerModelOrderPortModelOrder: &str = "org.eclipse.elk.layered.considerModelOrder.portModelOrder";
    pub const longEdgeTargetNode: &str = "longEdgeTargetNode";
    pub const targetNodeModelOrder: &str = "targetNode.modelOrder";
}

/// `[ObjectIdentifier: Int]`: target node → minimal model order of the
/// edges leading to it. Stored on nodes under `"targetNode.modelOrder"` as a
/// `PropValue::Object`.
pub type TargetNodeModelOrder = HashMap<LNodeId, i64>;

#[derive(Default)]
pub struct SortByInputModelProcessor;

impl SortByInputModelProcessor {
    pub fn new() -> SortByInputModelProcessor {
        SortByInputModelProcessor
    }

    /// `longEdgeTargetNodePreprocessing(_:)`: for each port with outgoing
    /// edges, stores the (long-edge) target node on the port as
    /// `"longEdgeTargetNode"`; returns (and caches on the node) the minimal
    /// model order of the non-reversed edges per target node.
    pub fn long_edge_target_node_preprocessing(lg: &mut LGraphArena, node: LNodeId) -> Rc<TargetNodeModelOrder> {
        if let Some(existing) = lg[node].props.get_by_id(_Keys::targetNodeModelOrder).and_then(|v| v.downcast::<TargetNodeModelOrder>()) {
            return existing;
        }

        let mut target_node_model_order: TargetNodeModelOrder = HashMap::new();
        for port in lg[node].ports.clone() {
            if lg[port].outgoing_edges.is_empty() {
                continue;
            }
            let target_node = Self::get_target_node(lg, port);
            lg[port].props.set_by_id(_Keys::longEdgeTargetNode, target_node.map(PropValue::LNode));

            let Some(target_node) = target_node else { continue };
            let previous_order = target_node_model_order.get(&target_node).copied().unwrap_or(i64::MAX);
            let edge = lg[port].outgoing_edges[0];
            let reversed = lg[edge].props.get_as::<bool>(&InternalProperties::REVERSED).unwrap_or(false);
            if reversed {
                continue;
            }
            if let Some(model_order) = lg[edge].props.get_as::<i64>(&InternalProperties::MODEL_ORDER) {
                target_node_model_order.insert(target_node, swift::min(previous_order, model_order));
            }
        }

        let result = Rc::new(target_node_model_order);
        lg[node].props.set_by_id(_Keys::targetNodeModelOrder, Some(PropValue::Object(result.clone())));
        result
    }

    /// `getTargetNode(_:)`: follows the first outgoing edge through dummy
    /// nodes to a normal node (or a long edge's recorded target).
    pub fn get_target_node(lg: &LGraphArena, port: LPortId) -> Option<LNodeId> {
        let mut edge = *lg[port].outgoing_edges.first()?;
        loop {
            let node = lg.edge_target_node(edge)?;
            if let Some(long_edge_target_port) = lg[node].props.get_as::<LPortId>(&InternalProperties::LONG_EDGE_TARGET) {
                return lg[long_edge_target_port].owner;
            }
            if lg[node].node_type != NodeType::NORMAL {
                edge = *lg.node_outgoing_edges(node).first()?;
            }
            if lg[node].node_type == NodeType::NORMAL {
                return Some(node);
            }
        }
    }

    /// `insertionSort(_:_:)` for nodes.
    pub fn insertion_sort(lg: &LGraphArena, layer: &mut [LNodeId], comparator: &mut _NodeComparator) {
        if layer.len() <= 1 {
            comparator.clear_transitive_ordering();
            return;
        }
        for i in 1..layer.len() {
            let temp = layer[i];
            let mut j = i;
            while j > 0 && comparator.compare(lg, layer[j - 1], temp) > 0 {
                layer[j] = layer[j - 1];
                j -= 1;
            }
            layer[j] = temp;
        }
        comparator.clear_transitive_ordering();
    }

    /// `insertionSortPort(_:_:)`.
    pub fn insertion_sort_port(lg: &LGraphArena, layer: &mut [LPortId], comparator: &mut _PortComparator) {
        if layer.len() <= 1 {
            comparator.clear_transitive_ordering();
            return;
        }
        for i in 1..layer.len() {
            let temp = layer[i];
            let mut j = i;
            while j > 0 && comparator.compare(lg, layer[j - 1], temp) > 0 {
                layer[j] = layer[j - 1];
                j -= 1;
            }
            layer[j] = temp;
        }
        comparator.clear_transitive_ordering();
    }
}

impl ILayoutProcessor for SortByInputModelProcessor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, progress_monitor: &mut dyn IElkProgressMonitor) {
        let ordering = lg[graph].props.get_by_id(_Keys::considerModelOrderStrategy).and_then(|v| v.cast::<OrderingStrategy>()).unwrap_or(OrderingStrategy::NONE);
        progress_monitor.begin(&format!("Sort By Input Model {}", ordering.name()), 1.0);

        let strategy = ordering;
        let long_edge_strategy = lg[graph]
            .props
            .get_by_id(_Keys::considerModelOrderLongEdgeStrategy)
            .and_then(|v| v.cast::<LongEdgeOrderingStrategy>())
            .unwrap_or(LongEdgeOrderingStrategy::EQUAL);
        let group_strategy = lg[graph]
            .props
            .get_as::<GroupOrderStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CM_GROUP_ORDER_STRATEGY)
            .unwrap_or(GroupOrderStrategy::ONLY_WITHIN_GROUP);
        let port_model_order = lg[graph].props.get_by_id(_Keys::considerModelOrderPortModelOrder).and_then(|v| v.cast::<bool>()).unwrap_or(false);

        let mut layer_index: usize = 0;
        for layer in lg[graph].layers.clone() {
            lg[layer].id = layer_index as i32;
            let previous_layer_index = if layer_index == 0 { 0 } else { layer_index - 1 };
            let previous_layer = lg[graph].layers[previous_layer_index];

            let mut nodes = lg[layer].nodes.clone();
            let mut first_comparator = _NodeComparator::new(lg, graph, previous_layer, strategy, long_edge_strategy, group_strategy, true);
            SortByInputModelProcessor::insertion_sort(lg, &mut nodes, &mut first_comparator);
            lg.layer_set_nodes(layer, nodes);

            for node in lg[layer].nodes.clone() {
                let constraints = lg[node].props.get_by_id(_Keys::portConstraints).and_then(|v| v.cast::<PortConstraints>()).unwrap_or(PortConstraints::UNDEFINED);
                if constraints != PortConstraints::FIXED_ORDER && constraints != PortConstraints::FIXED_POS {
                    let target_node_model_order = Self::long_edge_target_node_preprocessing(lg, node);
                    let mut ports = lg[node].ports.clone();
                    let mut port_comparator = _PortComparator::new(lg, graph, previous_layer, strategy, target_node_model_order, port_model_order);
                    Self::insertion_sort_port(lg, &mut ports, &mut port_comparator);
                    lg[node].ports = ports;
                }
            }

            let mut nodes = lg[layer].nodes.clone();
            let mut second_comparator = _NodeComparator::new(lg, graph, previous_layer, strategy, long_edge_strategy, group_strategy, false);
            SortByInputModelProcessor::insertion_sort(lg, &mut nodes, &mut second_comparator);
            lg.layer_set_nodes(layer, nodes);

            layer_index += 1;
        }
        progress_monitor.done();
    }

    fn name(&self) -> &'static str {
        "SortByInputModelProcessor"
    }
}

/// `SortByInputModelProcessor._NodeComparator`.
pub struct _NodeComparator {
    pub graph: LGraphId,
    pub previous_layer: Vec<LNodeId>,
    pub ordering_strategy: OrderingStrategy,
    pub group_order_strategy: GroupOrderStrategy,
    pub long_edge_node_order: LongEdgeOrderingStrategy,
    pub before_ports: bool,
    bigger_than: IdMap<LNodeId, IdSet<LNodeId>>,
    smaller_than: IdMap<LNodeId, IdSet<LNodeId>>,
}

/// `biggerThan[k] == nil` creates the entry, otherwise checks membership.
fn entry_or_contains<K: std::hash::Hash + Eq + Copy>(map: &mut IdMap<K, IdSet<K>>, key: K, value: K) -> bool {
    match map.get(&key) {
        None => {
            map.insert(key, IdSet::default());
            false
        }
        Some(set) => set.contains(&value),
    }
}

fn contains<K: std::hash::Hash + Eq + Copy>(map: &IdMap<K, IdSet<K>>, key: K, value: K) -> bool {
    map.get(&key).is_some_and(|s| s.contains(&value))
}

impl _NodeComparator {
    pub fn new(
        lg: &LGraphArena,
        graph: LGraphId,
        previous_layer: LayerId,
        ordering_strategy: OrderingStrategy,
        long_edge_ordering_strategy: LongEdgeOrderingStrategy,
        group_order_strategy: GroupOrderStrategy,
        before_ports: bool,
    ) -> _NodeComparator {
        _NodeComparator {
            graph,
            previous_layer: lg[previous_layer].nodes.clone(),
            ordering_strategy,
            group_order_strategy,
            long_edge_node_order: long_edge_ordering_strategy,
            before_ports,
            bigger_than: IdMap::default(),
            smaller_than: IdMap::default(),
        }
    }

    pub fn clear_transitive_ordering(&mut self) {
        self.bigger_than.clear();
        self.smaller_than.clear();
    }

    pub fn compare(&mut self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> i64 {
        if entry_or_contains(&mut self.bigger_than, n1, n2) {
            return 1;
        }
        if entry_or_contains(&mut self.bigger_than, n2, n1) {
            return -1;
        }
        if entry_or_contains(&mut self.smaller_than, n1, n2) {
            return -1;
        }
        // Swift checks `biggerThan[n2]` again here (after creating
        // `smallerThan[n2]`); unreachable, kept as written.
        if self.smaller_than.contains_key(&n2) {
            if contains(&self.bigger_than, n2, n1) {
                return 1;
            }
        } else {
            self.smaller_than.insert(n2, IdSet::default());
        }

        let n1_has_model_order = lg[n1].props.has(&InternalProperties::MODEL_ORDER);
        let n2_has_model_order = lg[n2].props.has(&InternalProperties::MODEL_ORDER);
        if self.ordering_strategy == OrderingStrategy::PREFER_EDGES || !n1_has_model_order || !n2_has_model_order {
            let p1_source_port = Self::source_port_connected_to_previous_layer(lg, n1);
            let p2_source_port = Self::source_port_connected_to_previous_layer(lg, n2);

            if let (Some(p1_source_port), Some(p2_source_port)) = (p1_source_port, p2_source_port) {
                let p1_node = lg[p1_source_port].owner;
                let p2_node = lg[p2_source_port].owner;
                if let (Some(p1_node), Some(p2_node)) = (p1_node, p2_node) {
                    if p1_node == p2_node {
                        for &port in &lg[p1_node].ports {
                            if port == p1_source_port {
                                self.update_bigger_and_smaller_associations(n2, n1);
                                return -1;
                            } else if port == p2_source_port {
                                self.update_bigger_and_smaller_associations(n1, n2);
                                return 1;
                            }
                        }
                        let n1_edge_order = self.get_model_order_from_connected_edges(lg, n1);
                        let n2_edge_order = self.get_model_order_from_connected_edges(lg, n2);
                        if n1_edge_order > n2_edge_order {
                            self.update_bigger_and_smaller_associations(n1, n2);
                            return 1;
                        } else {
                            self.update_bigger_and_smaller_associations(n2, n1);
                            return -1;
                        }
                    }

                    for i in 0..self.previous_layer.len() {
                        let previous_node = self.previous_layer[i];
                        if previous_node == p1_node {
                            self.update_bigger_and_smaller_associations(n2, n1);
                            return -1;
                        } else if previous_node == p2_node {
                            self.update_bigger_and_smaller_associations(n1, n2);
                            return 1;
                        }
                    }
                }
            }

            if p1_source_port.is_some() != p2_source_port.is_some() {
                let helper = self.handle_helper_dummy_nodes(lg, n1, n2);
                if helper != 0 {
                    if helper > 0 {
                        self.update_bigger_and_smaller_associations(n1, n2);
                    } else {
                        self.update_bigger_and_smaller_associations(n2, n1);
                    }
                    return helper;
                }
                if !n1_has_model_order || !n2_has_model_order {
                    let n1_model_order = self.get_model_order_from_connected_edges(lg, n1);
                    let n2_model_order = self.get_model_order_from_connected_edges(lg, n2);
                    if n1_model_order > n2_model_order {
                        self.update_bigger_and_smaller_associations(n1, n2);
                        return 1;
                    } else {
                        self.update_bigger_and_smaller_associations(n2, n1);
                        return -1;
                    }
                }
            }

            if p1_source_port.is_none() && p2_source_port.is_none() {
                let helper = self.handle_helper_dummy_nodes(lg, n1, n2);
                if helper != 0 {
                    if helper > 0 {
                        self.update_bigger_and_smaller_associations(n1, n2);
                    } else {
                        self.update_bigger_and_smaller_associations(n2, n1);
                    }
                    return helper;
                }
            }
        }

        if n1_has_model_order && n2_has_model_order {
            let max_model_order_nodes = lg[self.graph].props.get_as::<i64>(&InternalProperties::MAX_MODEL_ORDER_NODES).unwrap_or(0);
            let n1_model_order =
                CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Node(n1), LElement::Node(n2), self.graph, max_model_order_nodes);
            let n2_model_order =
                CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Node(n2), LElement::Node(n1), self.graph, max_model_order_nodes);
            // Both group order strategies take the same branch in elk-swift.
            if n1_model_order > n2_model_order {
                self.update_bigger_and_smaller_associations(n1, n2);
                return 1;
            } else {
                self.update_bigger_and_smaller_associations(n2, n1);
                return -1;
            }
        }

        self.update_bigger_and_smaller_associations(n2, n1);
        -1
    }

    /// `sourcePortConnectedToPreviousLayer(of:)`.
    pub fn source_port_connected_to_previous_layer(lg: &LGraphArena, node: LNodeId) -> Option<LPortId> {
        for &p in &lg[node].ports {
            let incoming = &lg[p].incoming_edges;
            if incoming.is_empty() {
                continue;
            }
            let edge = incoming[0];
            let source_layer_id = lg.edge_source_node(edge).and_then(|n| lg[n].layer).map(|l| lg[l].id);
            let node_layer_id = lg[node].layer.map(|l| lg[l].id);
            if let (Some(source_layer_id), Some(node_layer_id)) = (source_layer_id, node_layer_id) {
                if source_layer_id == node_layer_id - 1 {
                    return lg[edge].source;
                }
            }
        }
        None
    }

    /// `getModelOrderFromConnectedEdges(_:)`: the model order of the first
    /// incoming edge (of the first port with incoming edges) that has one.
    pub fn get_model_order_from_connected_edges(&self, lg: &LGraphArena, n: LNodeId) -> i64 {
        for &source_port in &lg[n].ports {
            let incoming = &lg[source_port].incoming_edges;
            if incoming.is_empty() {
                continue;
            }
            if let Some(order) = lg[incoming[0]].props.get_as::<i64>(&InternalProperties::MODEL_ORDER) {
                return order;
            }
        }
        self.long_edge_node_order.return_value()
    }

    /// `updateBiggerAndSmallerAssociations(_:_:)`.
    pub fn update_bigger_and_smaller_associations(&mut self, bigger: LNodeId, smaller: LNodeId) {
        let smaller_node_bigger_than = self.bigger_than.get(&smaller).cloned().unwrap_or_default();
        let bigger_node_smaller_than = self.smaller_than.get(&bigger).cloned().unwrap_or_default();

        let mut new_bigger_node_bigger_than = self.bigger_than.get(&bigger).cloned().unwrap_or_default();
        new_bigger_node_bigger_than.insert(smaller);
        self.bigger_than.insert(bigger, new_bigger_node_bigger_than);

        let mut new_smaller_node_smaller_than = self.smaller_than.get(&smaller).cloned().unwrap_or_default();
        new_smaller_node_smaller_than.insert(bigger);
        self.smaller_than.insert(smaller, new_smaller_node_smaller_than);

        for &very_small in &smaller_node_bigger_than {
            self.bigger_than.entry(bigger).or_default().insert(very_small);
            let s = self.smaller_than.entry(very_small).or_default();
            s.insert(bigger);
            s.extend(bigger_node_smaller_than.iter().copied());
        }

        for &very_big in &bigger_node_smaller_than {
            self.smaller_than.entry(smaller).or_default().insert(very_big);
            let b = self.bigger_than.entry(very_big).or_default();
            b.insert(smaller);
            b.extend(smaller_node_bigger_than.iter().copied());
        }
    }

    /// `handleHelperDummyNodes(_:_:)`.
    pub fn handle_helper_dummy_nodes(&mut self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> i64 {
        let layer_id = |n: LNodeId| lg[n].layer.map(|l| lg[l].id);
        let (t1, t2) = (lg[n1].node_type, lg[n2].node_type);
        if t1 == NodeType::LONG_EDGE && t2 == NodeType::NORMAL {
            let Some(dummy_source_port) = Self::get_first_incoming_source_port_of_node(lg, n1) else { return 0 };
            let Some(dummy_source_node) = lg[dummy_source_port].owner else { return 0 };
            let Some(dummy_target_port) = Self::get_first_outgoing_target_port_of_node(lg, n1) else { return 0 };
            let Some(dummy_target_node) = lg[dummy_target_port].owner else { return 0 };
            let Some(dummy_layer_id) = layer_id(n1) else { return 0 };

            if layer_id(dummy_source_node) != Some(dummy_layer_id) && layer_id(dummy_target_node) != Some(dummy_layer_id) {
                return 0;
            }
            if dummy_source_node == n2 || dummy_target_node == n2 {
                self.update_bigger_and_smaller_associations(n1, n2);
                return 1;
            }
            self.compare(lg, dummy_source_node, n2)
        } else if t1 == NodeType::NORMAL && t2 == NodeType::LONG_EDGE {
            let Some(dummy_source_port) = Self::get_first_incoming_source_port_of_node(lg, n2) else { return 0 };
            let Some(dummy_source_node) = lg[dummy_source_port].owner else { return 0 };
            let Some(dummy_target_port) = Self::get_first_outgoing_target_port_of_node(lg, n2) else { return 0 };
            let Some(dummy_target_node) = lg[dummy_target_port].owner else { return 0 };
            let Some(dummy_layer_id) = layer_id(n1) else { return 0 };

            if layer_id(dummy_source_node) != Some(dummy_layer_id) && layer_id(dummy_target_node) != Some(dummy_layer_id) {
                return 0;
            }
            if dummy_source_node == n1 || dummy_target_node == n1 {
                self.update_bigger_and_smaller_associations(n2, n1);
                return -1;
            }
            self.compare(lg, n1, dummy_source_node)
        } else if t1 == NodeType::LONG_EDGE && t2 == NodeType::LONG_EDGE {
            if self.before_ports {
                return self.compare_long_edge_sources_by_model_order(lg, n1, n2);
            }
            let (Some(n1_ref_node), Some(n2_ref_node)) = (Self::get_reference_node_in_current_layer(lg, n1), Self::get_reference_node_in_current_layer(lg, n2)) else {
                return 0;
            };
            self.compare(lg, n1_ref_node, n2_ref_node)
        } else {
            0
        }
    }

    /// `getReferenceNodeInCurrentLayer(_:)`.
    pub fn get_reference_node_in_current_layer(lg: &LGraphArena, dummy: LNodeId) -> Option<LNodeId> {
        let layer_id = |n: LNodeId| lg[n].layer.map(|l| lg[l].id);
        let current_layer_id = layer_id(dummy);
        for edge in lg.node_incoming_edges(dummy) {
            if let Some(source_node) = lg.edge_source_node(edge) {
                if layer_id(source_node) == current_layer_id {
                    return Some(source_node);
                }
            }
        }
        for edge in lg.node_outgoing_edges(dummy) {
            if let Some(target_node) = lg.edge_target_node(edge) {
                if layer_id(target_node) == current_layer_id {
                    return Some(target_node);
                }
            }
        }
        None
    }

    /// `compareLongEdgeSourcesByModelOrder(_:_:)`.
    pub fn compare_long_edge_sources_by_model_order(&mut self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> i64 {
        let n1_mo = self.get_model_order_from_connected_edges(lg, n1);
        let n2_mo = self.get_model_order_from_connected_edges(lg, n2);
        if n1_mo > n2_mo {
            self.update_bigger_and_smaller_associations(n1, n2);
            return 1;
        }
        self.update_bigger_and_smaller_associations(n2, n1);
        -1
    }

    /// `getFirstIncomingSourcePortOfNode(_:)`.
    pub fn get_first_incoming_source_port_of_node(lg: &LGraphArena, n: LNodeId) -> Option<LPortId> {
        let p = lg[n].ports.iter().copied().find(|&p| !lg[p].incoming_edges.is_empty())?;
        lg[lg[p].incoming_edges[0]].source
    }

    /// `getFirstOutgoingTargetPortOfNode(_:)`.
    pub fn get_first_outgoing_target_port_of_node(lg: &LGraphArena, n: LNodeId) -> Option<LPortId> {
        let p = lg[n].ports.iter().copied().find(|&p| !lg[p].outgoing_edges.is_empty())?;
        lg[lg[p].outgoing_edges[0]].target
    }
}

/// `SortByInputModelProcessor._PortComparator`.
pub struct _PortComparator {
    pub graph: LGraphId,
    pub previous_layer: Vec<LNodeId>,
    pub strategy: OrderingStrategy,
    pub target_node_model_order: Rc<TargetNodeModelOrder>,
    pub port_model_order: bool,
    bigger_than: IdMap<LPortId, IdSet<LPortId>>,
    smaller_than: IdMap<LPortId, IdSet<LPortId>>,
}

impl _PortComparator {
    pub fn new(
        lg: &LGraphArena,
        graph: LGraphId,
        previous_layer: LayerId,
        strategy: OrderingStrategy,
        target_node_model_order: Rc<TargetNodeModelOrder>,
        port_model_order: bool,
    ) -> _PortComparator {
        _PortComparator {
            graph,
            previous_layer: lg[previous_layer].nodes.clone(),
            strategy,
            target_node_model_order,
            port_model_order,
            bigger_than: IdMap::default(),
            smaller_than: IdMap::default(),
        }
    }

    pub fn clear_transitive_ordering(&mut self) {
        self.bigger_than.clear();
        self.smaller_than.clear();
    }

    /// `updateBiggerAndSmallerAssociations(_:_:_:)`: records `bigger >
    /// smaller` (swapped for a negative `reverse_order`) and its transitive
    /// consequences.
    fn update_bigger_and_smaller_associations(&mut self, bigger_ori: LPortId, smaller_ori: LPortId, reverse_order: i64) {
        let (bigger, smaller) = if reverse_order < 0 { (smaller_ori, bigger_ori) } else { (bigger_ori, smaller_ori) };

        self.bigger_than.entry(bigger).or_default();
        self.bigger_than.entry(smaller).or_default();
        self.smaller_than.entry(bigger).or_default();
        self.smaller_than.entry(smaller).or_default();

        self.bigger_than.entry(bigger).or_default().insert(smaller);
        self.smaller_than.entry(smaller).or_default().insert(bigger);

        // Transitive closure: everything smaller than `smaller` is also smaller than `bigger`
        let smaller_node_bigger_than = self.bigger_than.get(&smaller).cloned().unwrap_or_default();
        let bigger_node_smaller_than = self.smaller_than.get(&bigger).cloned().unwrap_or_default();
        for &very_small in &smaller_node_bigger_than {
            self.bigger_than.entry(bigger).or_default().insert(very_small);
            let s = self.smaller_than.entry(very_small).or_default();
            s.insert(bigger);
            s.extend(bigger_node_smaller_than.iter().copied());
        }

        // Transitive closure: everything bigger than `bigger` is also bigger than `smaller`
        let smaller_node_bigger_than2 = self.bigger_than.get(&smaller).cloned().unwrap_or_default();
        for &very_big in &bigger_node_smaller_than {
            self.smaller_than.entry(smaller).or_default().insert(very_big);
            let b = self.bigger_than.entry(very_big).or_default();
            b.insert(smaller);
            b.extend(smaller_node_bigger_than2.iter().copied());
        }
    }

    fn check_reference_layer(layer: &[LNodeId], p1_node: LNodeId, p2_node: LNodeId) -> i64 {
        for &node in layer {
            if node == p1_node {
                return -1;
            } else if node == p2_node {
                return 1;
            }
        }
        0
    }

    pub fn compare(&mut self, lg: &LGraphArena, p1: LPortId, p2: LPortId) -> i64 {
        // Check transitive ordering first (Java lines 106-125)
        if entry_or_contains(&mut self.bigger_than, p1, p2) {
            return 1;
        }
        if entry_or_contains(&mut self.bigger_than, p2, p1) {
            return -1;
        }
        if entry_or_contains(&mut self.smaller_than, p1, p2) {
            return -1;
        }
        if self.smaller_than.contains_key(&p2) {
            if contains(&self.bigger_than, p2, p1) {
                return 1;
            }
        } else {
            self.smaller_than.insert(p2, IdSet::default());
        }

        let (s1, s2) = (lg[p1].side, lg[p2].side);

        // Sort by port side NORTH < EAST < SOUTH < WEST
        if s1 != s2 {
            let result = Self::side_ordinal(s1) - Self::side_ordinal(s2);
            if result > 0 {
                self.update_bigger_and_smaller_associations(p1, p2, 1);
            } else {
                self.update_bigger_and_smaller_associations(p2, p1, 1);
            }
            return result;
        }
        let mut reverse_order: i64 = 1;
        let both = |side: PortSide| s1 == side && s2 == side;

        // Sort incoming edges by the order of source nodes in the previous layer
        if !lg[p1].incoming_edges.is_empty() && !lg[p2].incoming_edges.is_empty() {
            if both(PortSide::WEST) || both(PortSide::NORTH) || both(PortSide::SOUTH) {
                reverse_order = -reverse_order;
            }

            let p1_source_port = lg[lg[p1].incoming_edges[0]].source;
            let p2_source_port = lg[lg[p2].incoming_edges[0]].source;
            let p1_node = p1_source_port.and_then(|p| lg[p].owner);
            let p2_node = p2_source_port.and_then(|p| lg[p].owner);

            // If both connect to the same node, check port occurrence order
            if let (Some(p1_node), Some(p2_node)) = (p1_node, p2_node) {
                if p1_node == p2_node {
                    for &port in &lg[p1_node].ports {
                        if Some(port) == p1_source_port {
                            self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
                            return -reverse_order;
                        } else if Some(port) == p2_source_port {
                            self.update_bigger_and_smaller_associations(p1, p2, reverse_order);
                            return reverse_order;
                        }
                    }
                }
            }

            // If both connect to long edges in the same layer (Java lines 166-193)
            if let (Some(p1_node), Some(p2_node)) = (p1_node, p2_node) {
                let layer_id = |n: LNodeId| lg[n].layer.map(|l| lg[l].id);
                let p1_owner_layer_id = lg[p1].owner.and_then(|n| layer_id(n));
                if lg[p1_node].node_type == NodeType::LONG_EDGE
                    && lg[p2_node].node_type == NodeType::LONG_EDGE
                    && layer_id(p1_node) == layer_id(p2_node)
                    && layer_id(p1_node) == p1_owner_layer_id
                {
                    let same_layer_nodes: &[LNodeId] = match lg[p1_node].layer {
                        Some(l) => &lg[l].nodes,
                        None => &[],
                    };
                    let in_previous_layer = Self::check_reference_layer(same_layer_nodes, p1_node, p2_node);
                    if in_previous_layer != 0 {
                        let mut local_reverse = reverse_order;
                        if both(PortSide::EAST) {
                            local_reverse = -local_reverse;
                        }
                        if in_previous_layer > 0 {
                            self.update_bigger_and_smaller_associations(p1, p2, local_reverse);
                            return local_reverse;
                        } else {
                            self.update_bigger_and_smaller_associations(p2, p1, local_reverse);
                            return -local_reverse;
                        }
                    }
                }
            }

            // Check which node appears first in the previous layer
            if let (Some(p1_node), Some(p2_node)) = (p1_node, p2_node) {
                let in_previous_layer = Self::check_reference_layer(&self.previous_layer, p1_node, p2_node);
                if in_previous_layer != 0 {
                    if in_previous_layer > 0 {
                        self.update_bigger_and_smaller_associations(p1, p2, reverse_order);
                        return reverse_order;
                    } else {
                        self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
                        return -reverse_order;
                    }
                }
            }

            if self.port_model_order {
                let result = self.compare_by_port_model_order(lg, p1, p2);
                if result != 0 {
                    if result > 0 {
                        self.update_bigger_and_smaller_associations(p1, p2, reverse_order);
                        return reverse_order;
                    } else {
                        self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
                        return -reverse_order;
                    }
                }
            }
        }

        // Sort outgoing edges by model order
        if !lg[p1].outgoing_edges.is_empty() && !lg[p2].outgoing_edges.is_empty() {
            if both(PortSide::WEST) || both(PortSide::SOUTH) {
                reverse_order = -reverse_order;
            }

            let p1_target_node = lg[p1].props.get_by_id(_Keys::longEdgeTargetNode).and_then(|v| v.cast::<LNodeId>());
            let p2_target_node = lg[p2].props.get_by_id(_Keys::longEdgeTargetNode).and_then(|v| v.cast::<LNodeId>());

            if self.strategy == OrderingStrategy::PREFER_NODES {
                if let (Some(p1_target_node), Some(p2_target_node)) = (p1_target_node, p2_target_node) {
                    if lg[p1_target_node].props.has(&InternalProperties::MODEL_ORDER) && lg[p2_target_node].props.has(&InternalProperties::MODEL_ORDER) {
                        let max_model_order_nodes = lg[self.graph].props.get_as::<i64>(&InternalProperties::MAX_MODEL_ORDER_NODES).unwrap_or(0);
                        let p1_mo = CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(
                            lg,
                            LElement::Node(p1_target_node),
                            LElement::Node(p2_target_node),
                            self.graph,
                            max_model_order_nodes,
                        );
                        let p2_mo = CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(
                            lg,
                            LElement::Node(p2_target_node),
                            LElement::Node(p1_target_node),
                            self.graph,
                            max_model_order_nodes,
                        );
                        if p1_mo > p2_mo {
                            self.update_bigger_and_smaller_associations(p1, p2, reverse_order);
                            return reverse_order;
                        } else {
                            self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
                            return -reverse_order;
                        }
                    }
                }
            }

            if self.port_model_order {
                let result = self.compare_by_port_model_order(lg, p1, p2);
                if result != 0 {
                    if result > 0 {
                        self.update_bigger_and_smaller_associations(p1, p2, reverse_order);
                        return reverse_order;
                    } else {
                        self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
                        return -reverse_order;
                    }
                }
            }

            let mut p1_order: i64 = 0;
            let mut p2_order: i64 = 0;
            let p1_first = lg[p1].outgoing_edges[0];
            let p2_first = lg[p2].outgoing_edges[0];
            if lg[p1_first].props.has(&InternalProperties::MODEL_ORDER) {
                let offset = (lg[p1].outgoing_edges.len() + lg[p1].incoming_edges.len()) as i64;
                p1_order =
                    CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Edge(p1_first), LElement::Edge(p2_first), self.graph, offset);
            }
            if lg[p2_first].props.has(&InternalProperties::MODEL_ORDER) {
                let offset = (lg[p2].outgoing_edges.len() + lg[p2].incoming_edges.len()) as i64;
                p2_order =
                    CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Edge(p2_first), LElement::Edge(p1_first), self.graph, offset);
            }

            if let (Some(t1), Some(t2)) = (p1_target_node, p2_target_node) {
                if t1 == t2 {
                    if p1_order > p2_order {
                        self.update_bigger_and_smaller_associations(p1, p2, reverse_order);
                        return reverse_order;
                    } else {
                        self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
                        return -reverse_order;
                    }
                }
            }

            if let Some(t1) = p1_target_node {
                p1_order = self.target_node_model_order.get(&t1).copied().unwrap_or(p1_order);
            }
            if let Some(t2) = p2_target_node {
                p2_order = self.target_node_model_order.get(&t2).copied().unwrap_or(p2_order);
            }
            if p1_order > p2_order {
                self.update_bigger_and_smaller_associations(p1, p2, reverse_order);
                return reverse_order;
            } else {
                self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
                return -reverse_order;
            }
        }

        // Sort outgoing ports before incoming ports
        if !lg[p1].incoming_edges.is_empty() && !lg[p2].outgoing_edges.is_empty() {
            self.update_bigger_and_smaller_associations(p1, p2, reverse_order);
            1
        } else if !lg[p1].outgoing_edges.is_empty() && !lg[p2].incoming_edges.is_empty() {
            self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
            -1
        } else if lg[p1].props.has(&InternalProperties::MODEL_ORDER) && lg[p2].props.has(&InternalProperties::MODEL_ORDER) {
            let number_of_ports = lg[p1].owner.map_or(0, |n| lg[n].ports.len()) as i64;
            let p1_mo = CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Port(p1), LElement::Port(p2), self.graph, number_of_ports);
            let p2_mo = CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Port(p2), LElement::Port(p1), self.graph, number_of_ports);
            if both(PortSide::WEST) || both(PortSide::SOUTH) {
                reverse_order = -reverse_order;
            }
            if p1_mo > p2_mo {
                self.update_bigger_and_smaller_associations(p1, p2, reverse_order);
                reverse_order
            } else {
                self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
                -reverse_order
            }
        } else {
            self.update_bigger_and_smaller_associations(p2, p1, reverse_order);
            -reverse_order
        }
    }

    /// `compareByPortModelOrder(_:_:)`.
    pub fn compare_by_port_model_order(&self, lg: &LGraphArena, p1: LPortId, p2: LPortId) -> i64 {
        let number_of_ports = lg[p1].owner.map_or(0, |n| lg[n].ports.len()) as i64;
        if lg[p1].props.has(&InternalProperties::MODEL_ORDER) && lg[p2].props.has(&InternalProperties::MODEL_ORDER) {
            let p1_mo = CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Port(p1), LElement::Port(p2), self.graph, number_of_ports);
            let p2_mo = CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Port(p2), LElement::Port(p1), self.graph, number_of_ports);
            if p1_mo == p2_mo {
                return 0;
            }
            return if p1_mo > p2_mo { 1 } else { -1 };
        }
        0
    }

    /// `sideOrdinal(_:)`.
    pub fn side_ordinal(side: PortSide) -> i64 {
        match side {
            PortSide::UNDEFINED => 0,
            PortSide::NORTH => 1,
            PortSide::EAST => 2,
            PortSide::SOUTH => 3,
            PortSide::WEST => 4,
        }
    }
}
