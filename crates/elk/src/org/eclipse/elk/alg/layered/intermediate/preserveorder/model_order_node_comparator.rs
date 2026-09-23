//! Port of `alg/layered/intermediate/preserveorder/ModelOrderNodeComparator.swift`.
//!
//! Compares nodes of one layer by the model order of the nodes (or of the
//! edges connecting them to the previous layer), remembering every decision
//! transitively so that the comparator stays consistent within one sort.
//!
//! The transitive caches are `[ObjectIdentifier: Set<ObjectIdentifier>]` in
//! Swift. They are only ever queried for membership and grown by insertions
//! and unions, so their hash order is unobservable.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

use super::cm_group_model_order_calculator::CMGroupModelOrderCalculator;
use super::model_order_port_comparator::ModelOrderPortComparator;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_element::LElement;
use crate::org::eclipse::elk::alg::layered::options::group_order_strategy::GroupOrderStrategy;
use crate::org::eclipse::elk::alg::layered::options::long_edge_ordering_strategy::LongEdgeOrderingStrategy;
use crate::org::eclipse::elk::alg::layered::options::ordering_strategy::OrderingStrategy;
use crate::prelude::*;

/// A fast hasher for the arena's `u32` ids (FxHash's mixing step). Only for
/// maps and sets whose iteration order is never observed.
#[derive(Default, Clone, Copy)]
pub(crate) struct IdHasher(u64);

impl Hasher for IdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

pub(crate) type IdBuildHasher = BuildHasherDefault<IdHasher>;
pub(crate) type IdMap<K, V> = HashMap<K, V, IdBuildHasher>;
pub(crate) type IdSet<K> = HashSet<K, IdBuildHasher>;

pub struct ModelOrderNodeComparator {
    pub previous_layer: Vec<LNodeId>,
    pub graph: LGraphId,
    pub ordering_strategy: OrderingStrategy,
    pub group_order_strategy: GroupOrderStrategy,
    /// Node → nodes it must come after.
    pub(crate) bigger_than: IdMap<LNodeId, IdSet<LNodeId>>,
    /// Node → nodes it must come before.
    pub(crate) smaller_than: IdMap<LNodeId, IdSet<LNodeId>>,
    pub long_edge_node_order: LongEdgeOrderingStrategy,
    pub before_ports: bool,
}

impl ModelOrderNodeComparator {
    /// `init(_:_:_:_:_:_:)` with the previous layer's node list.
    pub fn new(
        graph: LGraphId,
        previous_layer: Vec<LNodeId>,
        ordering_strategy: OrderingStrategy,
        long_edge_ordering_strategy: LongEdgeOrderingStrategy,
        group_order_strategy: GroupOrderStrategy,
        before_ports: bool,
    ) -> ModelOrderNodeComparator {
        ModelOrderNodeComparator {
            previous_layer,
            graph,
            ordering_strategy,
            group_order_strategy,
            bigger_than: IdMap::default(),
            smaller_than: IdMap::default(),
            long_edge_node_order: long_edge_ordering_strategy,
            before_ports,
        }
    }

    /// The convenience `init` taking the previous `Layer`.
    pub fn with_layer(
        lg: &LGraphArena,
        graph: LGraphId,
        the_previous_layer: LayerId,
        ordering_strategy: OrderingStrategy,
        long_edge_ordering_strategy: LongEdgeOrderingStrategy,
        group_order_strategy: GroupOrderStrategy,
        before_ports: bool,
    ) -> ModelOrderNodeComparator {
        Self::new(graph, lg[the_previous_layer].nodes.clone(), ordering_strategy, long_edge_ordering_strategy, group_order_strategy, before_ports)
    }

    fn contains(map: &IdMap<LNodeId, IdSet<LNodeId>>, key: LNodeId, value: LNodeId) -> bool {
        map.get(&key).is_some_and(|s| s.contains(&value))
    }

    /// The first incoming edge's source port of the first port with incoming
    /// edges whose source node is in the layer right before `n`'s.
    fn source_port_in_previous_layer(lg: &LGraphArena, n: LNodeId) -> Option<LPortId> {
        for &p in &lg[n].ports {
            let incoming = &lg[p].incoming_edges;
            if incoming.is_empty() {
                continue;
            }
            let source_layer_id = lg.edge_source_node(incoming[0]).and_then(|s| lg[s].layer).map(|l| lg[l].id);
            let n_layer_id = lg[n].layer.map(|l| lg[l].id);
            if let (Some(source_layer_id), Some(n_layer_id)) = (source_layer_id, n_layer_id) {
                if source_layer_id == n_layer_id - 1 {
                    return lg[incoming[0]].source;
                }
            }
        }
        None
    }

    pub fn compare(&mut self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> i64 {
        self.ensure_node_in_maps(n1);
        self.ensure_node_in_maps(n2);

        if Self::contains(&self.bigger_than, n1, n2) {
            return 1;
        }
        if Self::contains(&self.bigger_than, n2, n1) {
            return -1;
        }
        if Self::contains(&self.smaller_than, n1, n2) {
            return -1;
        }
        // Java implementation checks biggerThan(n2) here; keep behavior identical.
        if Self::contains(&self.bigger_than, n2, n1) {
            return 1;
        }

        let n1_has_mo = lg[n1].props.has(&InternalProperties::MODEL_ORDER);
        let n2_has_mo = lg[n2].props.has(&InternalProperties::MODEL_ORDER);
        if self.ordering_strategy == OrderingStrategy::PREFER_EDGES || !n1_has_mo || !n2_has_mo {
            let p1_source_port = Self::source_port_in_previous_layer(lg, n1);
            let p2_source_port = Self::source_port_in_previous_layer(lg, n2);

            if let (Some(p1_source_port), Some(p2_source_port)) = (p1_source_port, p2_source_port) {
                if let (Some(p1_node), Some(p2_node)) = (lg[p1_source_port].owner, lg[p2_source_port].owner) {
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
                        }
                        self.update_bigger_and_smaller_associations(n2, n1);
                        return -1;
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
                let compared_with_long_edge_feedback = self.handle_helper_dummy_nodes(lg, n1, n2);
                if compared_with_long_edge_feedback != 0 {
                    if compared_with_long_edge_feedback > 0 {
                        self.update_bigger_and_smaller_associations(n1, n2);
                    } else {
                        self.update_bigger_and_smaller_associations(n2, n1);
                    }
                    return compared_with_long_edge_feedback;
                }

                if !lg[n1].props.has(&InternalProperties::MODEL_ORDER) || !lg[n2].props.has(&InternalProperties::MODEL_ORDER) {
                    let n1_model_order = self.get_model_order_from_connected_edges(lg, n1);
                    let n2_model_order = self.get_model_order_from_connected_edges(lg, n2);
                    if n1_model_order > n2_model_order {
                        self.update_bigger_and_smaller_associations(n1, n2);
                        return 1;
                    }
                    self.update_bigger_and_smaller_associations(n2, n1);
                    return -1;
                }
            }

            if p1_source_port.is_none() && p2_source_port.is_none() {
                let compared_with_long_edge_feedback = self.handle_helper_dummy_nodes(lg, n1, n2);
                if compared_with_long_edge_feedback != 0 {
                    if compared_with_long_edge_feedback > 0 {
                        self.update_bigger_and_smaller_associations(n1, n2);
                    } else {
                        self.update_bigger_and_smaller_associations(n2, n1);
                    }
                    return compared_with_long_edge_feedback;
                }
            }
        }

        if lg[n1].props.has(&InternalProperties::MODEL_ORDER) && lg[n2].props.has(&InternalProperties::MODEL_ORDER) {
            let max_model_order = lg[self.graph].props.get_as::<i64>(&InternalProperties::MAX_MODEL_ORDER_NODES).unwrap_or(0);
            let n1_model_order =
                CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Node(n1), LElement::Node(n2), self.graph, max_model_order);
            let n2_model_order =
                CMGroupModelOrderCalculator::calculate_model_order_or_group_model_order(lg, LElement::Node(n2), LElement::Node(n1), self.graph, max_model_order);
            if n1_model_order > n2_model_order {
                self.update_bigger_and_smaller_associations(n1, n2);
                return 1;
            }
            self.update_bigger_and_smaller_associations(n2, n1);
            return -1;
        }

        self.update_bigger_and_smaller_associations(n2, n1);
        -1
    }

    /// `getModelOrderFromConnectedEdges(_:)`.
    pub fn get_model_order_from_connected_edges(&self, lg: &LGraphArena, n: LNodeId) -> i64 {
        let source_port = lg[n].ports.iter().copied().find(|&p| !lg[p].incoming_edges.is_empty());
        if let Some(source_port) = source_port {
            let incoming = &lg[source_port].incoming_edges;
            if !incoming.is_empty() {
                if let Some(model_order) = lg[incoming[0]].props.get_as::<i64>(&InternalProperties::MODEL_ORDER) {
                    return model_order;
                }
            }
        }
        self.long_edge_node_order.return_value()
    }

    /// `updateBiggerAndSmallerAssociations(_:_:)`. The four sets are Swift
    /// value copies taken up front and written back at the end, so the final
    /// writes overwrite whatever the loops did to the same entries.
    pub fn update_bigger_and_smaller_associations(&mut self, bigger: LNodeId, smaller: LNodeId) {
        self.ensure_node_in_maps(bigger);
        self.ensure_node_in_maps(smaller);

        let smaller_node_bigger_than = self.bigger_than.get(&smaller).cloned().unwrap_or_default();
        let bigger_node_smaller_than = self.smaller_than.get(&bigger).cloned().unwrap_or_default();
        let mut bigger_node_bigger_than = self.bigger_than.get(&bigger).cloned().unwrap_or_default();
        let mut smaller_node_smaller_than = self.smaller_than.get(&smaller).cloned().unwrap_or_default();

        bigger_node_bigger_than.insert(smaller);
        smaller_node_smaller_than.insert(bigger);

        for &very_small in &smaller_node_bigger_than {
            bigger_node_bigger_than.insert(very_small);
            let very_small_smaller = self.smaller_than.entry(very_small).or_default();
            very_small_smaller.insert(bigger);
            very_small_smaller.extend(bigger_node_smaller_than.iter().copied());
        }

        for &very_big in &bigger_node_smaller_than {
            smaller_node_smaller_than.insert(very_big);
            let very_big_bigger = self.bigger_than.entry(very_big).or_default();
            very_big_bigger.insert(smaller);
            very_big_bigger.extend(smaller_node_bigger_than.iter().copied());
        }

        self.bigger_than.insert(bigger, bigger_node_bigger_than);
        self.bigger_than.insert(smaller, smaller_node_bigger_than);
        self.smaller_than.insert(bigger, bigger_node_smaller_than);
        self.smaller_than.insert(smaller, smaller_node_smaller_than);
    }

    /// `handleHelperDummyNodes(_:_:)`: orders long-edge dummies by the node
    /// in their own layer they belong to (feedback edges).
    pub fn handle_helper_dummy_nodes(&mut self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> i64 {
        let layer_id = |n: LNodeId| lg[n].layer.map(|l| lg[l].id);
        let (t1, t2) = (lg[n1].node_type, lg[n2].node_type);
        if t1 == NodeType::LONG_EDGE && t2 == NodeType::NORMAL {
            let Some(dummy_node_source_port) = Self::get_first_incoming_source_port_of_node(lg, n1) else { return 0 };
            let Some(dummy_node_source_node) = lg[dummy_node_source_port].owner else { return 0 };
            let Some(dummy_node_target_port) = Self::get_first_outgoing_target_port_of_node(lg, n1) else { return 0 };
            let Some(dummy_node_target_node) = lg[dummy_node_target_port].owner else { return 0 };
            let Some(dummy_layer_id) = layer_id(n1) else { return 0 };

            let source_in_layer = layer_id(dummy_node_source_node) == Some(dummy_layer_id);
            let target_in_layer = layer_id(dummy_node_target_node) == Some(dummy_layer_id);
            if !source_in_layer && !target_in_layer {
                return 0;
            }
            if dummy_node_source_node == n2 {
                self.update_bigger_and_smaller_associations(n1, n2);
                return 1;
            }
            if dummy_node_target_node == n2 {
                self.update_bigger_and_smaller_associations(n1, n2);
                return 1;
            }
            return self.compare(lg, dummy_node_source_node, n2);
        } else if t1 == NodeType::NORMAL && t2 == NodeType::LONG_EDGE {
            let Some(dummy_node_source_port) = Self::get_first_incoming_source_port_of_node(lg, n2) else { return 0 };
            let Some(dummy_node_source_node) = lg[dummy_node_source_port].owner else { return 0 };
            let Some(dummy_node_target_port) = Self::get_first_outgoing_target_port_of_node(lg, n2) else { return 0 };
            let Some(dummy_node_target_node) = lg[dummy_node_target_port].owner else { return 0 };
            let Some(dummy_layer_id) = layer_id(n1) else { return 0 };

            let source_in_layer = layer_id(dummy_node_source_node) == Some(dummy_layer_id);
            let target_in_layer = layer_id(dummy_node_target_node) == Some(dummy_layer_id);
            if !source_in_layer && !target_in_layer {
                return 0;
            }
            if dummy_node_source_node == n1 {
                self.update_bigger_and_smaller_associations(n2, n1);
                return -1;
            }
            if dummy_node_target_node == n1 {
                self.update_bigger_and_smaller_associations(n2, n1);
                return -1;
            }
            return self.compare(lg, n1, dummy_node_source_node);
        } else if t1 == NodeType::LONG_EDGE && t2 == NodeType::LONG_EDGE {
            let Some(n1_dummy_node_source_port) = Self::get_first_incoming_source_port_of_node(lg, n1) else { return 0 };
            let Some(n1_dummy_node_target_port) = Self::get_first_outgoing_target_port_of_node(lg, n1) else { return 0 };
            let Some(n1_dummy_source_node) = lg[n1_dummy_node_source_port].owner else { return 0 };
            let Some(n1_dummy_target_node) = lg[n1_dummy_node_target_port].owner else { return 0 };
            let Some(n1_layer_id) = layer_id(n1) else { return 0 };
            let Some(n2_dummy_node_source_port) = Self::get_first_incoming_source_port_of_node(lg, n2) else { return 0 };
            let Some(n2_dummy_node_target_port) = Self::get_first_outgoing_target_port_of_node(lg, n2) else { return 0 };
            let Some(n2_dummy_source_node) = lg[n2_dummy_node_source_port].owner else { return 0 };
            let Some(n2_dummy_target_node) = lg[n2_dummy_node_target_port].owner else { return 0 };
            let Some(n2_layer_id) = layer_id(n2) else { return 0 };

            let mut n1_source_feedback_node = false;
            let mut n1_target_feedback_node = false;
            let mut n2_source_feedback_node = false;
            let mut n2_target_feedback_node = false;

            let mut n1_reference_node = n1;
            let mut n2_reference_node = n2;

            if layer_id(n1_dummy_source_node) == Some(n1_layer_id) {
                n1_source_feedback_node = true;
                n1_reference_node = n1_dummy_source_node;
            } else if layer_id(n1_dummy_target_node) == Some(n1_layer_id) {
                n1_target_feedback_node = true;
                n1_reference_node = n1_dummy_target_node;
            }

            if layer_id(n2_dummy_source_node) == Some(n2_layer_id) {
                n2_source_feedback_node = true;
                n2_reference_node = n2_dummy_source_node;
            } else if layer_id(n2_dummy_target_node) == Some(n2_layer_id) {
                n2_target_feedback_node = true;
                n2_reference_node = n2_dummy_target_node;
            }

            if n1_reference_node == n2_reference_node {
                if self.before_ports {
                    if n1_source_feedback_node && n2_source_feedback_node {
                        let return_value = ModelOrderPortComparator::new(
                            self.graph,
                            self.previous_layer.clone(),
                            self.ordering_strategy,
                            None,
                            n2_target_feedback_node,
                        )
                        .compare(lg, n1_dummy_node_source_port, n2_dummy_node_source_port);
                        if return_value > 0 {
                            self.update_bigger_and_smaller_associations(n2, n1);
                            return 1;
                        }
                        self.update_bigger_and_smaller_associations(n1, n2);
                        return -1;
                    } else if n1_source_feedback_node && n2_target_feedback_node {
                        self.update_bigger_and_smaller_associations(n2, n1);
                        return 1;
                    } else if n1_target_feedback_node && n2_source_feedback_node {
                        self.update_bigger_and_smaller_associations(n1, n2);
                        return -1;
                    } else if n1_target_feedback_node && n2_target_feedback_node {
                        return 0;
                    }
                } else {
                    for &port in &lg[n1_reference_node].ports {
                        if n1_dummy_node_source_port == port {
                            self.update_bigger_and_smaller_associations(n2, n1);
                            return -1;
                        } else if n2_dummy_node_source_port == port {
                            self.update_bigger_and_smaller_associations(n1, n2);
                            return 1;
                        }
                    }
                }
            }

            return self.compare(lg, n1_reference_node, n2_reference_node);
        }
        0
    }

    /// `getFirstIncomingPortOfNode(_:)`.
    pub fn get_first_incoming_port_of_node(lg: &LGraphArena, node: LNodeId) -> Option<LPortId> {
        lg[node].ports.iter().copied().find(|&p| !lg[p].incoming_edges.is_empty())
    }

    /// `getFirstIncomingSourcePortOfNode(_:)`.
    pub fn get_first_incoming_source_port_of_node(lg: &LGraphArena, node: LNodeId) -> Option<LPortId> {
        let incoming_port = Self::get_first_incoming_port_of_node(lg, node)?;
        lg[incoming_port].incoming_edges.first().and_then(|&e| lg[e].source)
    }

    /// `getFirstOutgoingPortOfNode(_:)`.
    pub fn get_first_outgoing_port_of_node(lg: &LGraphArena, node: LNodeId) -> Option<LPortId> {
        lg[node].ports.iter().copied().find(|&p| !lg[p].outgoing_edges.is_empty())
    }

    /// `getFirstOutgoingTargetPortOfNode(_:)`.
    pub fn get_first_outgoing_target_port_of_node(lg: &LGraphArena, node: LNodeId) -> Option<LPortId> {
        let outgoing_port = Self::get_first_outgoing_port_of_node(lg, node)?;
        lg[outgoing_port].outgoing_edges.first().and_then(|&e| lg[e].target)
    }

    pub fn clear_transitive_ordering(&mut self) {
        self.bigger_than = IdMap::default();
        self.smaller_than = IdMap::default();
    }

    pub fn ensure_node_in_maps(&mut self, node: LNodeId) {
        self.bigger_than.entry(node).or_default();
        self.smaller_than.entry(node).or_default();
    }
}
