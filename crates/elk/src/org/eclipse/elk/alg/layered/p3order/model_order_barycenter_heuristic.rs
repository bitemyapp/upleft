//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_ModelOrderBarycenterHeuristic.swift`.
//!
//! A `BarycenterHeuristic` subclass that keeps real nodes in model order and
//! only sorts dummy nodes between them. The subclass is modelled as a struct
//! wrapping its base; the overridden layer-level `minimizeCrossings` is
//! reached from the base's order-level methods through
//! [`BarycenterLayerSweep`].

use std::collections::{HashMap, HashSet};

use super::barycenter_heuristic::{BarycenterHeuristic, BarycenterLayerSweep, FORCE_MODEL_ORDER_KEY};
use super::counting::i_initializable::IInitializable;
use super::graph_info_holder::GraphDataView;
use super::i_crossing_minimization_heuristic::ICrossingMinimizationHeuristic;
use super::layer_sweep_crossing_minimizer::RandomRef;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};
use crate::org::eclipse::elk::alg::layered::options::group_order_strategy::GroupOrderStrategy;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::graph::properties::property::PropValue;
use crate::swift;

use super::abstract_barycenter_port_distributor::AbstractBarycenterPortDistributor;
use super::forster_constraint_resolver::ForsterConstraintResolver;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Default)]
pub struct ModelOrderBarycenterHeuristic {
    pub base: BarycenterHeuristic,
    /// Each node has an entry of nodes for which it is bigger.
    pub bigger_than: HashMap<LNodeId, HashSet<LNodeId>>,
    /// Each node has an entry of nodes for which it is smaller.
    pub smaller_than: HashMap<LNodeId, HashSet<LNodeId>>,
}

impl IInitializable for ModelOrderBarycenterHeuristic {}

impl ModelOrderBarycenterHeuristic {
    pub fn new(
        constraint_resolver: ForsterConstraintResolver,
        random: Option<RandomRef>,
        port_distributor: Rc<RefCell<AbstractBarycenterPortDistributor>>,
        graph: &[Vec<LNodeId>],
    ) -> ModelOrderBarycenterHeuristic {
        ModelOrderBarycenterHeuristic {
            base: BarycenterHeuristic::new(constraint_resolver, random, port_distributor, graph),
            bigger_than: HashMap::new(),
            smaller_than: HashMap::new(),
        }
    }

    /// The overridden layer-level `minimizeCrossings(_:_:_:_:)`.
    pub fn minimize_crossings_layer(&mut self, lg: &LGraphArena, layer: &mut Vec<LNodeId>, pre_ordered: bool, randomize: bool, forward: bool) {
        if layer.is_empty() {
            return;
        }

        // Use inherited (parent) barycenter calculation — port-position-based
        if randomize {
            self.base.randomize_barycenters(lg, layer);
        } else {
            self.base.calculate_barycenters(lg, layer, forward);
            self.base.fill_in_unknown_barycenters(lg, layer, pre_ordered);
        }

        if layer.len() > 1 {
            let force_model_order = lg
                .node_graph(layer[0])
                .and_then(|g| lg[g].props.get_by_id(FORCE_MODEL_ORDER_KEY))
                .and_then(|v| v.cast::<bool>())
                .unwrap_or(false);

            if force_model_order {
                Self::insertion_sort(lg, layer, self);
            } else {
                // Java: Collections.sort(layer, barycenterStateComparator)
                let enumerated: Vec<(usize, LNodeId)> = layer.iter().copied().enumerate().collect();
                let sorted = swift::sorted_by(enumerated, |lhs, rhs| {
                    let value = self.compare_nodes(lg, lhs.1, rhs.1);
                    if value == 0 {
                        return lhs.0 < rhs.0;
                    }
                    value < 0
                });
                *layer = sorted.into_iter().map(|(_, n)| n).collect();
            }

            // Resolve ordering constraints (matching Java: only when NOT forceModelOrder)
            if !force_model_order {
                if let Some(resolver) = self.base.constraint_resolver.as_mut() {
                    resolver.process_constraints(lg, layer);
                }
            }
        }
    }

    /// `compareNodes(_:_:)` (Java's `barycenterStateComparator`).
    pub fn compare_nodes(&mut self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> i64 {
        // Skip FIRST_SEPARATE / LAST_SEPARATE constraint nodes
        if Self::is_separate_constraint_node(lg, n1) || Self::is_separate_constraint_node(lg, n2) {
            return 0;
        }

        // Check transitive dependencies first
        let transitive = self.compare_based_on_transitive_dependencies(n1, n2);
        if transitive != 0 {
            return transitive;
        }

        // Compare by model order for real nodes (both must have MODEL_ORDER)
        let m1 = lg[n1].props.get_as::<i64>(&InternalProperties::MODEL_ORDER);
        let m2 = lg[n2].props.get_as::<i64>(&InternalProperties::MODEL_ORDER);

        if let (Some(m1), Some(m2)) = (m1, m2) {
            let mut value = if m1 == m2 {
                0
            } else if m1 < m2 {
                -1
            } else {
                1
            };

            // Check group order strategy
            let strategy = lg
                .node_graph(n1)
                .and_then(|g| lg[g].props.get_as::<GroupOrderStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CM_GROUP_ORDER_STRATEGY));

            if strategy == Some(GroupOrderStrategy::ONLY_WITHIN_GROUP) {
                let group1 = lg[n1].props.get(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CROSSING_MINIMIZATION_ID);
                let group2 = lg[n2].props.get(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CROSSING_MINIMIZATION_ID);
                if !any_hashable_eq(group1.as_ref(), group2.as_ref()) {
                    value = 0;
                }
            }

            if value < 0 {
                self.update_bigger_and_smaller_associations(n1, n2);
                return value;
            } else if value > 0 {
                self.update_bigger_and_smaller_associations(n2, n1);
                return value;
            }
        }

        // Fall back to barycenter comparison (uses port-position-based barycenters from parent)
        self.compare_based_on_barycenter(lg, n1, n2)
    }

    fn compare_based_on_barycenter(&mut self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> i64 {
        let s1 = self.base.state_of(lg, n1);
        let s2 = self.base.state_of(lg, n2);
        let b1 = self.base.states()[s1].barycenter;
        let b2 = self.base.states()[s2].barycenter;
        match (b1, b2) {
            (Some(b1), Some(b2)) => {
                if b1 < b2 {
                    self.update_bigger_and_smaller_associations(n1, n2);
                    return -1;
                }
                if b1 > b2 {
                    self.update_bigger_and_smaller_associations(n2, n1);
                    return 1;
                }
                0
            }
            (Some(_), None) => {
                self.update_bigger_and_smaller_associations(n1, n2);
                -1
            }
            (None, Some(_)) => {
                self.update_bigger_and_smaller_associations(n2, n1);
                1
            }
            (None, None) => 0,
        }
    }

    fn compare_based_on_transitive_dependencies(&mut self, n1: LNodeId, n2: LNodeId) -> i64 {
        match self.bigger_than.get(&n1) {
            Some(set) if set.contains(&n2) => return 1,
            Some(_) => {}
            None => {
                self.bigger_than.insert(n1, HashSet::new());
            }
        }
        match self.bigger_than.get(&n2) {
            Some(set) if set.contains(&n1) => return -1,
            Some(_) => {}
            None => {
                self.bigger_than.insert(n2, HashSet::new());
            }
        }
        match self.smaller_than.get(&n1) {
            Some(set) if set.contains(&n2) => return -1,
            Some(_) => {}
            None => {
                self.smaller_than.insert(n1, HashSet::new());
            }
        }
        match self.smaller_than.get(&n2) {
            Some(set) if set.contains(&n1) => return 1,
            Some(_) => {}
            None => {
                self.smaller_than.insert(n2, HashSet::new());
            }
        }
        0
    }

    /// `updateBiggerAndSmallerAssociations(_:_:)`. The Swift iterates two
    /// sets here, but only to insert into and union other sets, so the
    /// (hash) iteration order cannot change the result.
    pub fn update_bigger_and_smaller_associations(&mut self, bigger: LNodeId, smaller: LNodeId) {
        let smaller_node_bigger_than = self.bigger_than.get(&smaller).cloned().unwrap_or_default();
        let bigger_node_smaller_than = self.smaller_than.get(&bigger).cloned().unwrap_or_default();

        self.bigger_than.entry(bigger).or_default().insert(smaller);
        self.smaller_than.entry(smaller).or_default().insert(bigger);

        for &very_small in &smaller_node_bigger_than {
            self.bigger_than.entry(bigger).or_default().insert(very_small);
            self.smaller_than.entry(very_small).or_default().insert(bigger);
            self.smaller_than.entry(very_small).or_default().extend(bigger_node_smaller_than.iter().copied());
        }

        for &very_big in &bigger_node_smaller_than {
            self.smaller_than.entry(smaller).or_default().insert(very_big);
            self.bigger_than.entry(very_big).or_default().insert(smaller);
            self.bigger_than.entry(very_big).or_default().extend(smaller_node_bigger_than.iter().copied());
        }
    }

    /// `insertionSort(_:_:_:)` with `compareNodes` as the comparator.
    pub fn insertion_sort(lg: &LGraphArena, layer: &mut Vec<LNodeId>, barycenter_heuristic: &mut ModelOrderBarycenterHeuristic) {
        if layer.len() <= 1 {
            barycenter_heuristic.clear_transitive_ordering();
            return;
        }

        let mut i = 1;
        while i < layer.len() {
            let temp = layer[i];
            let mut j = i;
            while j > 0 && barycenter_heuristic.compare_nodes(lg, layer[j - 1], temp) > 0 {
                layer[j] = layer[j - 1];
                j -= 1;
            }
            layer[j] = temp;
            i += 1;
        }

        barycenter_heuristic.clear_transitive_ordering();
    }

    /// `clearTransitiveOrdering()`.
    pub fn clear_transitive_ordering(&mut self) {
        self.bigger_than = HashMap::new();
        self.smaller_than = HashMap::new();
    }

    fn is_separate_constraint_node(lg: &LGraphArena, node: LNodeId) -> bool {
        match lg[node].props.get_as::<LayerConstraint>(&LayeredOptions::LAYERING_LAYER_CONSTRAINT) {
            Some(constraint) => constraint == LayerConstraint::FIRST_SEPARATE || constraint == LayerConstraint::LAST_SEPARATE,
            None => false,
        }
    }
}

/// `(a as? AnyHashable) == (b as? AnyHashable)`. `AnyHashable` compares
/// numbers by value across types (`AnyHashable(1) == AnyHashable(1.0)`); a
/// value that is not `Hashable` casts to nil.
fn any_hashable_eq(a: Option<&PropValue>, b: Option<&PropValue>) -> bool {
    fn hashable(v: Option<&PropValue>) -> Option<&PropValue> {
        match v {
            Some(PropValue::Object(_) | PropValue::KVector(_) | PropValue::KVectorChain(_) | PropValue::ElkPadding(_) | PropValue::ElkMargin(_) | PropValue::Random(_)) => None,
            other => other,
        }
    }
    fn int_of_double(d: f64) -> Option<i64> {
        if d.is_finite() && d.trunc() == d && d >= -9223372036854775808.0 && d < 9223372036854775808.0 {
            Some(d as i64)
        } else {
            None
        }
    }
    match (hashable(a), hashable(b)) {
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
        (Some(PropValue::Int(x)), Some(PropValue::Int(y))) => x == y,
        (Some(PropValue::Double(x)), Some(PropValue::Double(y))) => x == y,
        (Some(PropValue::Int(x)), Some(PropValue::Double(y))) | (Some(PropValue::Double(y)), Some(PropValue::Int(x))) => int_of_double(*y) == Some(*x),
        (Some(PropValue::Bool(x)), Some(PropValue::Bool(y))) => x == y,
        (Some(PropValue::Str(x)), Some(PropValue::Str(y))) => x == y,
        // Other hashable values (enums, element references, lists) compare
        // by type and value.
        (Some(x), Some(y)) => format!("{x:?}") == format!("{y:?}"),
    }
}

impl BarycenterLayerSweep for ModelOrderBarycenterHeuristic {
    fn base(&mut self) -> &mut BarycenterHeuristic {
        &mut self.base
    }

    fn minimize_crossings_in_layer(&mut self, lg: &LGraphArena, layer: &mut Vec<LNodeId>, pre_ordered: bool, randomize: bool, forward: bool) {
        self.minimize_crossings_layer(lg, layer, pre_ordered, randomize, forward);
    }
}

impl ICrossingMinimizationHeuristic for ModelOrderBarycenterHeuristic {
    fn always_improves(&self) -> bool {
        false
    }

    fn set_first_layer_order(&mut self, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, forward_sweep: bool, _graph_data: &GraphDataView) -> bool {
        BarycenterHeuristic::set_first_layer_order_in_order(self, lg, order, forward_sweep)
    }

    fn minimize_crossings(&mut self, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, free_layer_index: i64, forward_sweep: bool, is_first_sweep: bool, _graph_data: &GraphDataView) -> bool {
        BarycenterHeuristic::minimize_crossings_in_order(self, lg, order, free_layer_index, forward_sweep, is_first_sweep)
    }

    /// Overridden: `true`.
    fn is_deterministic(&self) -> bool {
        true
    }
}
