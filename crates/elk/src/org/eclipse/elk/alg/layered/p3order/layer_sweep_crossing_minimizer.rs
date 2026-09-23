//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_LayerSweepCrossingMinimizer.swift`.
//!
//! The layer-sweep crossing minimisation phase (P3), also used as the
//! one- and two-sided greedy-switch intermediate processors. It is the only
//! hierarchy-aware processor: with `INCLUDE_CHILDREN` it runs once on the
//! root graph and sweeps into nested graphs.
//!
//! Swift reference semantics modelled here:
//! * `GraphInfoHolder`s are referred to by their index in
//!   `graph_info_holders` (a holder's index is also its graph's `id`, which
//!   `initialize` sets); `graphsWhoseNodeOrderChanged` is a flag per holder.
//! * The minimizer's `random` is the root graph's `Random` object (shared
//!   with the graph and, for the root, with its holder's heuristic).
//! * `chooseMinimizingMethod` returns a closure in Swift, an enum here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::counting::cross_min_util::port_side_view;
use super::graph_info_holder::{GraphInfoHolder, LayerSweepCrossingMinimizerCrossMinType};
use super::i_crossing_minimization_heuristic::ICrossingMinimizationHeuristic;
use super::i_sweep_port_distributor::ISweepPortDistributor;
use super::sweep_copy::SweepCopy;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::graph_configurator::Random;
use crate::org::eclipse::elk::alg::layered::intermediate::intermediate_processor_strategy::IntermediateProcessorStrategy;
use crate::org::eclipse::elk::alg::layered::layered_phases::LayeredPhases;
use crate::org::eclipse::elk::alg::layered::options::group_order_strategy::GroupOrderStrategy;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::long_edge_ordering_strategy::LongEdgeOrderingStrategy;
use crate::org::eclipse::elk::alg::layered::options::ordering_strategy::OrderingStrategy;
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::alg::layout_processor_configuration::LayoutProcessorConfiguration;
use crate::org::eclipse::elk::core::options::hierarchy_handling::HierarchyHandling;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;
use crate::org::eclipse::elk::graph::properties::property::PropValue;
use crate::swift;

// MARK: - Random

/// The draws the crossing minimisation makes on Swift's `Random` class
/// (`nextBoolean`, `nextFloat`, `nextDouble`, `nextLong`, `setSeed`), so a
/// subclass (the tests' `MockRandom`) can stand in for it, as in Swift.
pub trait RandomDraws {
    fn next_boolean(&mut self) -> bool;
    fn next_float(&mut self) -> f32;
    fn next_double(&mut self) -> f64;
    fn next_long(&mut self) -> i64;
    fn set_seed(&mut self, seed: i64);
}

impl RandomDraws for Random {
    fn next_boolean(&mut self) -> bool {
        Random::next_boolean(self)
    }
    fn next_float(&mut self) -> f32 {
        Random::next_float(self)
    }
    fn next_double(&mut self) -> f64 {
        Random::next_double(self)
    }
    fn next_long(&mut self) -> i64 {
        Random::next_long(self)
    }
    fn set_seed(&mut self, seed: i64) {
        Random::set_seed(self, seed)
    }
}

/// A reference to a `Random` object (or a subclass instance).
pub type RandomRef = Rc<RefCell<dyn RandomDraws>>;

/// A `Random` subclass instance stored in a `RANDOM` property
/// (`PropValue::object(Rc::new(RandomOverride(...)))`), which
/// `getProperty(RANDOM) as? Random` also accepts.
pub struct RandomOverride(pub RandomRef);

/// `holder.getProperty(InternalProperties.RANDOM) as? Random`.
pub fn random_of(props: &PropertyMap) -> Option<RandomRef> {
    match props.get(&InternalProperties::RANDOM)? {
        PropValue::Random(r) => Some(r as RandomRef),
        other => other.downcast::<RandomOverride>().map(|o| o.0.clone()),
    }
}

// MARK: - Keys

const HIERARCHY_HANDLING: &str = "org.eclipse.elk.hierarchyHandling";
const PORT_CONSTRAINTS: &str = "org.eclipse.elk.portConstraints";
const FIRST_TRY_WITH_INITIAL_ORDER: &str = "org.eclipse.elk.layered.firstTryWithInitialOrder";
const SECOND_TRY_WITH_INITIAL_ORDER: &str = "org.eclipse.elk.layered.secondTryWithInitialOrder";
const CONSIDER_MODEL_ORDER_STRATEGY: &str = "org.eclipse.elk.layered.considerModelOrder.strategy";
const CONSIDER_MODEL_ORDER_PORT_MODEL_ORDER: &str = "org.eclipse.elk.layered.considerModelOrder.portModelOrder";
const THOROUGHNESS: &str = "org.eclipse.elk.layered.thoroughness";
const MODEL_ORDER_COUNTER_NODE_INFLUENCE: &str = "org.eclipse.elk.layered.considerModelOrder.crossingCounterNodeInfluence";
const MODEL_ORDER_COUNTER_PORT_INFLUENCE: &str = "org.eclipse.elk.layered.considerModelOrder.crossingCounterPortInfluence";
/// Not the id `InternalProperties.ORIGIN` uses (`"origin"`), so this lookup
/// finds nothing on graphs built by the pipeline.
const ORIGIN: &str = "org.eclipse.elk.layered.origin";

/// `CrossMinType` (the phase-level one declared in this file).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrossMinType {
    BARYCENTER,
    ONE_SIDED_GREEDY_SWITCH,
    TWO_SIDED_GREEDY_SWITCH,
    MEDIAN,
}

/// The closure `chooseMinimizingMethod(_:)` returns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MinimizingMethod {
    /// `{ _ in }` (no graph to sweep on).
    Nothing,
    CompareDifferentRandomizedLayouts,
    MinimizeCrossingsNoCounter,
    MinimizeCrossingsWithCounter,
}

pub struct LayerSweepCrossingMinimizer {
    pub graph_info_holders: Vec<GraphInfoHolder>,
    /// `graphsWhoseNodeOrderChanged`, per holder index.
    pub graphs_whose_node_order_changed: Vec<bool>,
    pub random: Option<RandomRef>,
    pub random_seed: i64,
    pub cross_min_type: CrossMinType,
}

/// `Int(d)` for a `Double`: truncates, traps when not representable.
fn swift_int(d: f64) -> i64 {
    if d.is_nan() || d < -9223372036854775808.0 || d >= 9223372036854775808.0 {
        panic!("Double value cannot be converted to Int because it is either infinite, NaN or outside the representable range");
    }
    d as i64
}

fn string_key_bool(lg: &LGraphArena, graph: LGraphId, key: &str) -> Option<bool> {
    lg[graph].props.get_by_id(key).and_then(|v| v.cast::<bool>())
}

fn string_key_double(lg: &LGraphArena, graph: LGraphId, key: &str) -> Option<f64> {
    lg[graph].props.get_by_id(key).and_then(|v| v.cast::<f64>())
}

fn string_key_ordering_strategy(lg: &LGraphArena, graph: LGraphId, key: &str) -> Option<OrderingStrategy> {
    lg[graph].props.get_by_id(key).and_then(|v| v.cast::<OrderingStrategy>())
}

fn set_string_key(lg: &mut LGraphArena, graph: LGraphId, key: &str, value: bool) {
    lg[graph].props.set_by_id(key, Some(PropValue::Bool(value)));
}

impl LayerSweepCrossingMinimizer {
    /// `init()`.
    pub fn new_default() -> LayerSweepCrossingMinimizer {
        LayerSweepCrossingMinimizer::new(CrossMinType::BARYCENTER)
    }

    /// `init(_ cT:)`.
    pub fn new(cross_min_type: CrossMinType) -> LayerSweepCrossingMinimizer {
        LayerSweepCrossingMinimizer { graph_info_holders: Vec::new(), graphs_whose_node_order_changed: Vec::new(), random: None, random_seed: 0, cross_min_type }
    }

    /// `INTERMEDIATE_PROCESSING_CONFIGURATION`.
    pub fn intermediate_processing_configuration() -> LayoutProcessorConfiguration {
        let mut configuration = LayoutProcessorConfiguration::create();
        configuration
            .add_before(LayeredPhases::P3_NODE_ORDERING, IntermediateProcessorStrategy::LONG_EDGE_SPLITTER)
            .add_before(LayeredPhases::P4_NODE_PLACEMENT, IntermediateProcessorStrategy::IN_LAYER_CONSTRAINT_PROCESSOR)
            .add_after(LayeredPhases::P5_EDGE_ROUTING, IntermediateProcessorStrategy::LONG_EDGE_JOINER);
        configuration
    }

    /// `chooseMinimizingMethod(_:)`.
    pub fn choose_minimizing_method(&self, graphs_to_sweep_on: &[usize]) -> MinimizingMethod {
        let Some(&parent) = graphs_to_sweep_on.first() else { return MinimizingMethod::Nothing };
        let parent = &self.graph_info_holders[parent];

        if !parent.cross_min_deterministic() {
            return MinimizingMethod::CompareDifferentRandomizedLayouts;
        }
        if parent.cross_min_always_improves() {
            return MinimizingMethod::MinimizeCrossingsNoCounter;
        }
        MinimizingMethod::MinimizeCrossingsWithCounter
    }

    fn run_minimizing_method(&mut self, lg: &mut LGraphArena, method: MinimizingMethod, g_data: usize) {
        match method {
            MinimizingMethod::Nothing => {}
            MinimizingMethod::CompareDifferentRandomizedLayouts => self.compare_different_randomized_layouts(lg, g_data),
            MinimizingMethod::MinimizeCrossingsNoCounter => self.minimize_crossings_no_counter(lg, g_data),
            MinimizingMethod::MinimizeCrossingsWithCounter => {
                self.minimize_crossings_with_counter(lg, g_data);
            }
        }
    }

    /// `minimizeCrossings(_:_:)`.
    pub fn minimize_crossings(&mut self, lg: &mut LGraphArena, graphs_to_sweep_on: &[usize], minimizing_method: MinimizingMethod) {
        for &g_data in graphs_to_sweep_on {
            if !self.graph_info_holders[g_data].current_node_order.is_empty() {
                self.run_minimizing_method(lg, minimizing_method, g_data);
                if self.graph_info_holders[g_data].has_parent {
                    self.set_port_order_on_parent_graph(lg, g_data);
                }
            }
        }
    }

    /// `setPortOrderOnParentGraph(_:)`.
    pub fn set_port_order_on_parent_graph(&mut self, lg: &mut LGraphArena, g_data: usize) {
        let holder = &self.graph_info_holders[g_data];
        if !holder.has_external_ports {
            return;
        }
        let Some(best_sweep) = holder.get_best_sweep() else { return };
        let Some(best_nodes) = best_sweep.nodes() else { return };
        let best_nodes = best_nodes.clone();
        let parent = holder.parent();

        self.sort_ports_by_dummy_positions_in_last_layer(lg, &best_nodes, parent, true);
        self.sort_ports_by_dummy_positions_in_last_layer(lg, &best_nodes, parent, false);
        if let Some(parent) = parent {
            lg[parent].props.set_by_id(PORT_CONSTRAINTS, Some(PortConstraints::FIXED_ORDER.into()));
        }
    }

    /// `minimizeCrossingsNoCounter(_:)`.
    pub fn minimize_crossings_no_counter(&mut self, lg: &mut LGraphArena, g_data: usize) {
        let mut is_forward_sweep = self.next_random_bool();
        let mut improved = true;

        while improved {
            improved = self.set_first_layer_order(lg, g_data, is_forward_sweep);
            improved = self.sweep_reducing_crossings(lg, g_data, is_forward_sweep, false) || improved;
            is_forward_sweep = !is_forward_sweep;
        }

        self.set_currently_best_node_orders(lg);
    }

    /// `compareDifferentRandomizedLayouts(_:)`.
    pub fn compare_different_randomized_layouts(&mut self, lg: &mut LGraphArena, g_data: usize) {
        // Reset the seed, otherwise copies of hierarchical graphs in different parent nodes
        // are laid out differently. Matches Java: random.setSeed(randomSeed)
        if let Some(random) = &self.random {
            random.borrow_mut().set_seed(self.random_seed);
        }
        self.graphs_whose_node_order_changed.iter_mut().for_each(|c| *c = false);

        let graph = self.graph_info_holders[g_data].l_graph;
        let node_influence = string_key_double(lg, graph, MODEL_ORDER_COUNTER_NODE_INFLUENCE).unwrap_or(0.0);
        let port_influence = string_key_double(lg, graph, MODEL_ORDER_COUNTER_PORT_INFLUENCE).unwrap_or(0.0);
        let strategy = string_key_ordering_strategy(lg, graph, CONSIDER_MODEL_ORDER_STRATEGY).unwrap_or(OrderingStrategy::NONE);

        if node_influence != 0.0 || port_influence != 0.0 {
            let mut best_crossings = f64::MAX;
            if strategy != OrderingStrategy::NONE {
                set_string_key(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER, true);
            }
            let thoroughness = Self::int_property(lg, graph, THOROUGHNESS).unwrap_or(7);
            for _ in 0..swift::max(1, thoroughness) {
                let crossings = self.minimize_crossings_node_port_order_with_counter(lg, g_data);
                if crossings < best_crossings {
                    best_crossings = crossings;
                    self.save_all_node_orders_of_changed_graphs(lg);
                    if best_crossings == 0.0 {
                        break;
                    }
                }
            }
        } else {
            let mut best_crossings = i64::MAX;
            if strategy != OrderingStrategy::NONE {
                set_string_key(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER, true);
            }
            let thoroughness = Self::int_property(lg, graph, THOROUGHNESS).unwrap_or(7);
            for _ in 0..swift::max(1, thoroughness) {
                let crossings = self.minimize_crossings_with_counter(lg, g_data);
                if crossings < best_crossings {
                    best_crossings = crossings;
                    self.save_all_node_orders_of_changed_graphs(lg);
                    if best_crossings == 0 {
                        break;
                    }
                }
            }
        }
    }

    /// `minimizeCrossingsWithCounter(_:)`.
    pub fn minimize_crossings_with_counter(&mut self, lg: &mut LGraphArena, g_data: usize) -> i64 {
        let graph = self.graph_info_holders[g_data].l_graph;
        let mut is_forward_sweep = self.next_random_bool();
        let initial_crossings = self.count_current_number_of_crossings(lg, g_data);
        let first_try = string_key_bool(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER).unwrap_or(false);
        let second_try = string_key_bool(lg, graph, SECOND_TRY_WITH_INITIAL_ORDER).unwrap_or(false);

        if initial_crossings == 0 && first_try {
            return 0;
        }

        let consider_model_order = string_key_ordering_strategy(lg, graph, CONSIDER_MODEL_ORDER_STRATEGY).unwrap_or(OrderingStrategy::NONE);

        if !(first_try || second_try) || consider_model_order == OrderingStrategy::NONE {
            self.set_first_layer_order(lg, g_data, is_forward_sweep);
        } else {
            is_forward_sweep = first_try;
        }

        self.sweep_reducing_crossings(lg, g_data, is_forward_sweep, true);

        if string_key_bool(lg, graph, SECOND_TRY_WITH_INITIAL_ORDER).unwrap_or(false) {
            set_string_key(lg, graph, SECOND_TRY_WITH_INITIAL_ORDER, false);
        }
        if string_key_bool(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER).unwrap_or(false) {
            set_string_key(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER, false);
            set_string_key(lg, graph, SECOND_TRY_WITH_INITIAL_ORDER, true);
        }

        let mut crossings_in_graph = self.count_current_number_of_crossings(lg, g_data);
        let mut old_crossings;
        loop {
            self.set_currently_best_node_orders(lg);
            if crossings_in_graph == 0 {
                return 0;
            }
            is_forward_sweep = !is_forward_sweep;
            old_crossings = crossings_in_graph;
            self.sweep_reducing_crossings(lg, g_data, is_forward_sweep, false);
            crossings_in_graph = self.count_current_number_of_crossings(lg, g_data);
            if !(old_crossings > crossings_in_graph) {
                break;
            }
        }

        old_crossings
    }

    /// `minimizeCrossingsNodePortOrderWithCounter(_:)`.
    pub fn minimize_crossings_node_port_order_with_counter(&mut self, lg: &mut LGraphArena, g_data: usize) -> f64 {
        let graph = self.graph_info_holders[g_data].l_graph;
        let mut is_forward_sweep = self.next_random_bool();
        let initial_crossings = self.count_current_number_of_crossings_node_port_order(lg, g_data);

        if initial_crossings == 0.0 && string_key_bool(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER).unwrap_or(false) {
            return 0.0;
        }

        let consider_model_order = string_key_ordering_strategy(lg, graph, CONSIDER_MODEL_ORDER_STRATEGY).unwrap_or(OrderingStrategy::NONE);
        if !(string_key_bool(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER).unwrap_or(false)
            || string_key_bool(lg, graph, SECOND_TRY_WITH_INITIAL_ORDER).unwrap_or(false))
            || consider_model_order == OrderingStrategy::NONE
        {
            self.set_first_layer_order(lg, g_data, is_forward_sweep);
        } else {
            is_forward_sweep = string_key_bool(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER).unwrap_or(false);
        }

        self.sweep_reducing_crossings(lg, g_data, is_forward_sweep, true);
        if string_key_bool(lg, graph, SECOND_TRY_WITH_INITIAL_ORDER).unwrap_or(false) {
            set_string_key(lg, graph, SECOND_TRY_WITH_INITIAL_ORDER, false);
        }
        if string_key_bool(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER).unwrap_or(false) {
            set_string_key(lg, graph, FIRST_TRY_WITH_INITIAL_ORDER, false);
            set_string_key(lg, graph, SECOND_TRY_WITH_INITIAL_ORDER, true);
        }

        let mut crossings_in_graph = self.count_current_number_of_crossings_node_port_order(lg, g_data);
        let mut old_crossings;
        loop {
            self.set_currently_best_node_orders(lg);
            if crossings_in_graph == 0.0 {
                return 0.0;
            }
            is_forward_sweep = !is_forward_sweep;
            old_crossings = crossings_in_graph;
            self.sweep_reducing_crossings(lg, g_data, is_forward_sweep, false);
            crossings_in_graph = self.count_current_number_of_crossings_node_port_order(lg, g_data);
            if !(old_crossings > crossings_in_graph) {
                break;
            }
        }

        old_crossings
    }

    /// `countModelOrderNodeChanges(_:_:_:_:)`.
    pub fn count_model_order_node_changes(
        &self,
        lg: &mut LGraphArena,
        graph: LGraphId,
        layers: &[Vec<LNodeId>],
        strategy: OrderingStrategy,
        cm_group_order_strategy: GroupOrderStrategy,
    ) -> i64 {
        let mut previous_layer_index: i64 = -1;
        let mut wrong_model_order = 0;

        for layer in layers {
            let previous_layer = if previous_layer_index == -1 { &layers[0] } else { &layers[previous_layer_index as usize] };
            let mut comp = _needs_group_a_model_order_node_comparator(lg, graph, previous_layer, strategy, LongEdgeOrderingStrategy::EQUAL, cm_group_order_strategy, false);
            if layer.len() > 1 {
                for i in 0..(layer.len() - 1) {
                    for j in (i + 1)..layer.len() {
                        if lg[layer[i]].props.has(&InternalProperties::MODEL_ORDER)
                            && lg[layer[j]].props.has(&InternalProperties::MODEL_ORDER)
                            && comp(lg, layer[i], layer[j]) > 0
                        {
                            wrong_model_order += 1;
                        }
                    }
                }
            }
            previous_layer_index += 1;
        }
        wrong_model_order
    }

    /// `countModelOrderPortChanges(_:_:_:)`.
    pub fn count_model_order_port_changes(&self, lg: &mut LGraphArena, graph: LGraphId, layers: &[Vec<LNodeId>], _group_order_strategy: GroupOrderStrategy) -> i64 {
        let mut previous_layer_index: i64 = -1;
        let mut wrong_model_order = 0;

        for layer in layers {
            let previous_layer = if previous_layer_index == -1 { &layers[0] } else { &layers[previous_layer_index as usize] };
            for &node in layer {
                let node_graph = lg.node_graph(node);
                let strategy = node_graph
                    .and_then(|g| string_key_ordering_strategy(lg, g, CONSIDER_MODEL_ORDER_STRATEGY))
                    .unwrap_or(OrderingStrategy::NODES_AND_EDGES);
                let target_node_model_order = _needs_group_a_long_edge_target_node_preprocessing(lg, node);
                let port_model_order = node_graph.and_then(|g| string_key_bool(lg, g, CONSIDER_MODEL_ORDER_PORT_MODEL_ORDER)).unwrap_or(false);
                let mut comp = _needs_group_a_model_order_port_comparator(lg, graph, previous_layer, strategy, Some(target_node_model_order), port_model_order);
                let ports = lg[node].ports.clone();
                if ports.len() > 1 {
                    for i in 0..(ports.len() - 1) {
                        for j in (i + 1)..ports.len() {
                            if comp(lg, ports[i], ports[j]) > 0 {
                                wrong_model_order += 1;
                            }
                        }
                    }
                }
            }
            previous_layer_index += 1;
        }
        wrong_model_order
    }

    /// `countCurrentNumberOfCrossings(_:)`.
    pub fn count_current_number_of_crossings(&mut self, lg: &LGraphArena, current_graph: usize) -> i64 {
        let own_crossings = {
            let h = &mut self.graph_info_holders[current_graph];
            h.crossings_counter.count_all_crossings(lg, &h.current_node_order)
        };
        let mut child_crossings = 0;
        for child_graph in self.graph_info_holders[current_graph].child_graphs.clone() {
            let Some(child) = self.graph_data_for(lg, child_graph) else { continue };
            if self.graph_info_holders[child].dont_sweep_into() {
                continue;
            }
            child_crossings += self.count_current_number_of_crossings(lg, child);
        }
        own_crossings + child_crossings
    }

    /// `countCurrentNumberOfCrossingsNodePortOrder(_:)`.
    pub fn count_current_number_of_crossings_node_port_order(&mut self, lg: &mut LGraphArena, current_graph: usize) -> f64 {
        let graph = self.graph_info_holders[current_graph].l_graph;
        let mut model_order_influence = 0.0;
        let model_order_strategy = string_key_ordering_strategy(lg, graph, CONSIDER_MODEL_ORDER_STRATEGY).unwrap_or(OrderingStrategy::NONE);
        let cm_group_order_strategy = lg[graph]
            .props
            .get_as::<GroupOrderStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CM_GROUP_ORDER_STRATEGY)
            .unwrap_or(GroupOrderStrategy::ONLY_WITHIN_GROUP);
        let node_influence = string_key_double(lg, graph, MODEL_ORDER_COUNTER_NODE_INFLUENCE).unwrap_or(0.0);
        let port_influence = string_key_double(lg, graph, MODEL_ORDER_COUNTER_PORT_INFLUENCE).unwrap_or(0.0);

        if model_order_strategy != OrderingStrategy::NONE {
            let order = self.graph_info_holders[current_graph].current_node_order.clone();
            model_order_influence += node_influence * self.count_model_order_node_changes(lg, graph, &order, model_order_strategy, cm_group_order_strategy) as f64;
            model_order_influence += port_influence * self.count_model_order_port_changes(lg, graph, &order, cm_group_order_strategy) as f64;
        }

        let own = {
            let h = &mut self.graph_info_holders[current_graph];
            h.crossings_counter.count_all_crossings(lg, &h.current_node_order)
        };
        let mut total_crossings = own as f64 + model_order_influence;
        for child_graph in self.graph_info_holders[current_graph].child_graphs.clone() {
            let Some(child) = self.graph_data_for(lg, child_graph) else { continue };
            if self.graph_info_holders[child].dont_sweep_into() {
                continue;
            }
            total_crossings += self.count_current_number_of_crossings(lg, child) as f64;
        }
        total_crossings
    }

    /// `sweepReducingCrossings(_:_:_:)`.
    pub fn sweep_reducing_crossings(&mut self, lg: &mut LGraphArena, graph: usize, forward: bool, first_sweep: bool) -> bool {
        let length = self.graph_info_holders[graph].current_node_order.len();
        if length == 0 {
            return false;
        }
        let first = Self::first_index(forward, length);

        let mut improved = {
            let h = &mut self.graph_info_holders[graph];
            h.port_distributor.distribute_ports_while_sweeping(lg, &h.current_node_order, first as usize, forward)
        };
        let first_layer = self.graph_info_holders[graph].current_node_order[first as usize].clone();
        improved = self.sweep_in_hierarchical_nodes(lg, &first_layer, forward, first_sweep) || improved;

        let l_graph = self.graph_info_holders[graph].l_graph;
        let mut i = Self::first_free(forward, length);
        while Self::is_not_end(length, i, forward) {
            let is_randomizing_sweep = first_sweep
                && !string_key_bool(lg, l_graph, FIRST_TRY_WITH_INITIAL_ORDER).unwrap_or(false)
                && !string_key_bool(lg, l_graph, SECOND_TRY_WITH_INITIAL_ORDER).unwrap_or(false);
            improved = self.cross_minimize(lg, graph, i, forward, is_randomizing_sweep) || improved;
            improved = {
                let h = &mut self.graph_info_holders[graph];
                h.port_distributor.distribute_ports_while_sweeping(lg, &h.current_node_order, i as usize, forward)
            } || improved;
            let current_nodes = &self.graph_info_holders[graph].current_node_order;
            if i >= 0 && (i as usize) < current_nodes.len() {
                let layer = current_nodes[i as usize].clone();
                improved = self.sweep_in_hierarchical_nodes(lg, &layer, forward, first_sweep) || improved;
            }
            i += Self::next(forward);
        }

        self.graphs_whose_node_order_changed[graph] = true;
        improved
    }

    /// `sweepInHierarchicalNodes(_:_:_:)`.
    pub fn sweep_in_hierarchical_nodes(&mut self, lg: &mut LGraphArena, layer: &[LNodeId], is_forward_sweep: bool, is_first_sweep: bool) -> bool {
        let mut improved = false;
        for &node in layer {
            let Some(nested) = lg[node].nested_graph else { continue };
            let Some(nested_data) = self.graph_data_for(lg, nested) else { continue };
            if self.graph_info_holders[nested_data].dont_sweep_into() {
                continue;
            }
            improved = self.sweep_in_hierarchical_node(lg, is_forward_sweep, node, is_first_sweep) || improved;
        }
        improved
    }

    /// `sweepInHierarchicalNode(_:_:_:)`.
    pub fn sweep_in_hierarchical_node(&mut self, lg: &mut LGraphArena, is_forward_sweep: bool, node: LNodeId, is_first_sweep: bool) -> bool {
        let Some(nested_l_graph) = lg[node].nested_graph else { return false };
        let Some(nested_graph) = self.graph_data_for(lg, nested_l_graph) else { return false };
        let count = self.graph_info_holders[nested_graph].current_node_order.len();
        if count == 0 {
            return false;
        }

        let start_index = Self::first_index(is_forward_sweep, count) as usize;
        let Some(&first_node) = self.graph_info_holders[nested_graph].current_node_order[start_index].first() else { return false };

        if Self::is_external_port_dummy(lg, first_node) {
            let layer = self.graph_info_holders[nested_graph].current_node_order[start_index].clone();
            let sorted = self.sort_port_dummies_by_port_positions(lg, node, &layer, Self::side_opposed_sweep_direction(is_forward_sweep));
            self.graph_info_holders[nested_graph].current_node_order[start_index] = sorted;
        } else {
            self.set_first_layer_order(lg, nested_graph, is_forward_sweep);
        }

        let improved = self.sweep_reducing_crossings(lg, nested_graph, is_forward_sweep, is_first_sweep);
        let order = self.graph_info_holders[nested_graph].current_node_order.clone();
        let parent = self.graph_info_holders[nested_graph].parent();
        self.sort_ports_by_dummy_positions_in_last_layer(lg, &order, parent, is_forward_sweep);

        improved
    }

    /// `sortPortsByDummyPositionsInLastLayer(_:_:_:)`. `parent == None` is
    /// Swift's fresh empty node (nothing to reorder).
    pub fn sort_ports_by_dummy_positions_in_last_layer(&self, lg: &mut LGraphArena, node_order: &[Vec<LNodeId>], parent: Option<LNodeId>, on_right_most_layer: bool) {
        if node_order.is_empty() {
            return;
        }
        let last_layer_index = Self::end_index(on_right_most_layer, node_order.len());
        let last_layer = &node_order[last_layer_index as usize];
        if last_layer.is_empty() {
            return;
        }

        let mut dummy_index = Self::first_index(on_right_most_layer, last_layer.len());
        if !Self::is_external_port_dummy(lg, last_layer[dummy_index as usize]) {
            return;
        }
        let Some(parent) = parent else { return };

        let mut reordered = lg[parent].ports.clone();
        for i in 0..reordered.len() {
            let port = reordered[i];
            if Self::is_on_end_of_sweep_side(lg, port, on_right_most_layer)
                && Self::is_hierarchical(lg, port)
                && dummy_index >= 0
                && (dummy_index as usize) < last_layer.len()
            {
                if let Some(origin) = Self::origin_port(lg, last_layer[dummy_index as usize]) {
                    reordered[i] = origin;
                    dummy_index += Self::next(on_right_most_layer);
                }
            }
        }
        lg[parent].ports = reordered;
    }

    /// `sortPortDummiesByPortPositions(_:_:_:)`.
    pub fn sort_port_dummies_by_port_positions(&self, lg: &LGraphArena, parent_node: LNodeId, layer_close_to_node_edge: &[LNodeId], side: PortSide) -> Vec<LNodeId> {
        let ports = Self::ordered_ports(lg, parent_node, side);
        let mut sorted_dummies: Vec<LNodeId> = Vec::with_capacity(layer_close_to_node_edge.len());

        for port in ports {
            if !Self::is_hierarchical(lg, port) {
                continue;
            }
            if let Some(dummy) = Self::dummy_node_for(lg, port) {
                sorted_dummies.push(dummy);
            }
        }

        sorted_dummies.truncate(layer_close_to_node_edge.len());
        sorted_dummies
    }

    /// `saveAllNodeOrdersOfChangedGraphs()`.
    pub fn save_all_node_orders_of_changed_graphs(&mut self, lg: &LGraphArena) {
        for (i, graph) in self.graph_info_holders.iter_mut().enumerate() {
            if !self.graphs_whose_node_order_changed[i] {
                continue;
            }
            let best = match &graph.currently_best_node_and_port_order {
                Some(sc) => SweepCopy::from_copy(sc),
                None => SweepCopy::from_copy(&SweepCopy::new(lg, Some(&graph.current_node_order))),
            };
            graph.best_node_and_port_order = Some(best);
        }
    }

    /// `setCurrentlyBestNodeOrders()`.
    pub fn set_currently_best_node_orders(&mut self, lg: &LGraphArena) {
        for (i, graph) in self.graph_info_holders.iter_mut().enumerate() {
            if !self.graphs_whose_node_order_changed[i] {
                continue;
            }
            graph.currently_best_node_and_port_order = Some(SweepCopy::new(lg, Some(&graph.current_node_order)));
        }
    }

    /// `firstIndex(_:_:)`.
    pub fn first_index(is_forward_sweep: bool, length: usize) -> i64 {
        if is_forward_sweep { 0 } else { length as i64 - 1 }
    }

    /// `endIndex(_:_:)`.
    pub fn end_index(is_forward_sweep: bool, length: usize) -> i64 {
        if is_forward_sweep { length as i64 - 1 } else { 0 }
    }

    /// `firstFree(_:_:)`.
    pub fn first_free(is_forward_sweep: bool, length: usize) -> i64 {
        if is_forward_sweep { 1 } else { length as i64 - 2 }
    }

    /// `next(_:)`.
    pub fn next(is_forward_sweep: bool) -> i64 {
        if is_forward_sweep { 1 } else { -1 }
    }

    /// `isNotEnd(_:_:_:)`.
    pub fn is_not_end(length: usize, free_layer_index: i64, is_forward_sweep: bool) -> bool {
        if is_forward_sweep {
            return free_layer_index < length as i64;
        }
        free_layer_index >= 0
    }

    /// `hasNestedGraph(_:)`.
    pub fn has_nested_graph(lg: &LGraphArena, node: LNodeId) -> bool {
        lg[node].nested_graph.is_some()
    }

    /// `sideOpposedSweepDirection(_:)`.
    pub fn side_opposed_sweep_direction(is_forward_sweep: bool) -> PortSide {
        if is_forward_sweep { PortSide::WEST } else { PortSide::EAST }
    }

    /// `isExternalPortDummy(_:)`.
    pub fn is_external_port_dummy(lg: &LGraphArena, first_node: LNodeId) -> bool {
        lg[first_node].node_type == NodeType::EXTERNAL_PORT
    }

    /// `originPort(_:)`: reads the string key `"org.eclipse.elk.layered.origin"`.
    pub fn origin_port(lg: &LGraphArena, node: LNodeId) -> Option<LPortId> {
        lg[node].props.get_by_id(ORIGIN).and_then(|v| v.cast::<LPortId>())
    }

    /// `isHierarchical(_:)`.
    pub fn is_hierarchical(lg: &LGraphArena, port: LPortId) -> bool {
        lg[port].props.get_as::<bool>(&InternalProperties::INSIDE_CONNECTIONS).unwrap_or(false)
    }

    /// `dummyNodeFor(_:)`.
    pub fn dummy_node_for(lg: &LGraphArena, port: LPortId) -> Option<LNodeId> {
        lg[port].props.get_as::<LNodeId>(&InternalProperties::PORT_DUMMY)
    }

    /// `isOnEndOfSweepSide(_:_:)`.
    pub fn is_on_end_of_sweep_side(lg: &LGraphArena, port: LPortId, is_forward_sweep: bool) -> bool {
        if is_forward_sweep {
            return lg[port].side == PortSide::EAST;
        }
        lg[port].side == PortSide::WEST
    }

    /// `initialize(_:)`: builds a `GraphInfoHolder` for the root and every
    /// nested graph (breadth first, numbering the graphs) and returns the
    /// graphs to sweep on, bottom-up ones first.
    pub fn initialize(&mut self, lg: &mut LGraphArena, root_graph: LGraphId) -> Vec<usize> {
        self.graph_info_holders = Vec::new();
        self.random = random_of(&lg[root_graph].props);
        self.random_seed = match &self.random {
            Some(r) => r.borrow_mut().next_long(),
            None => 0,
        };

        let mut graphs_to_sweep_on: Vec<usize> = Vec::new();
        let mut graphs: Vec<LGraphId> = vec![root_graph];

        let mut i = 0;
        while i < graphs.len() {
            let graph = graphs[i];
            lg[graph].id = i as i32;
            i += 1;

            let graph_data = GraphInfoHolder::new(lg, graph, self.map_cross_min_type(self.cross_min_type), &self.graph_info_holders, self.cross_min_type);
            graphs.extend_from_slice(&graph_data.child_graphs);
            let sweep = graph_data.dont_sweep_into();
            self.graph_info_holders.push(graph_data);
            if sweep {
                graphs_to_sweep_on.insert(0, self.graph_info_holders.len() - 1);
            }
        }

        self.graphs_whose_node_order_changed = vec![false; self.graph_info_holders.len()];
        graphs_to_sweep_on
    }

    /// `transferNodeAndPortOrdersToGraph()`.
    pub fn transfer_node_and_port_orders_to_graph(&self, lg: &mut LGraphArena) {
        for holder in &self.graph_info_holders {
            if let Some(best_sweep) = holder.get_best_sweep() {
                best_sweep.transfer_node_and_port_orders_to_graph(lg, holder.l_graph, true);
            }
        }
    }

    /// `getGraphData()`.
    pub fn get_graph_data(&self) -> &[GraphInfoHolder] {
        &self.graph_info_holders
    }

    /// `graphData(for:)`: the holder whose index is the graph's `id`.
    pub fn graph_data_for(&self, lg: &LGraphArena, graph: LGraphId) -> Option<usize> {
        let graph_id = lg[graph].id;
        if graph_id < 0 || graph_id as usize >= self.graph_info_holders.len() {
            return None;
        }
        Some(graph_id as usize)
    }

    /// `mapCrossMinType(_:)`.
    pub fn map_cross_min_type(&self, value: CrossMinType) -> LayerSweepCrossingMinimizerCrossMinType {
        match value {
            CrossMinType::BARYCENTER => LayerSweepCrossingMinimizerCrossMinType::BARYCENTER,
            CrossMinType::MEDIAN => LayerSweepCrossingMinimizerCrossMinType::MEDIAN,
            CrossMinType::ONE_SIDED_GREEDY_SWITCH | CrossMinType::TWO_SIDED_GREEDY_SWITCH => LayerSweepCrossingMinimizerCrossMinType::GREEDY_SWITCH,
        }
    }

    /// `setFirstLayerOrder(_:_:)`.
    pub fn set_first_layer_order(&mut self, lg: &LGraphArena, graph: usize, forward: bool) -> bool {
        let (before, rest) = self.graph_info_holders.split_at_mut(graph);
        let holder = &mut rest[0];
        let view = holder.view(holder.parent_graph_data.and_then(|p| before.get(p)));
        let mut order = std::mem::take(&mut holder.current_node_order);
        let improved = holder.cross_minimizer.set_first_layer_order(lg, &mut order, forward, &view);
        holder.current_node_order = order;
        improved
    }

    /// `crossMinimize(_:_:_:_:)`.
    pub fn cross_minimize(&mut self, lg: &LGraphArena, graph: usize, index: i64, forward: bool, randomize: bool) -> bool {
        // A parent holder always precedes its children.
        let (before, rest) = self.graph_info_holders.split_at_mut(graph);
        let holder = &mut rest[0];
        let view = holder.view(holder.parent_graph_data.and_then(|p| before.get(p)));
        let mut order = std::mem::take(&mut holder.current_node_order);
        let improved = holder.cross_minimizer.minimize_crossings(lg, &mut order, index, forward, randomize, &view);
        holder.current_node_order = order;
        improved
    }

    /// `orderedPorts(_:_:)`.
    pub fn ordered_ports(lg: &LGraphArena, node: LNodeId, side: PortSide) -> Vec<LPortId> {
        match side {
            PortSide::EAST | PortSide::NORTH => port_side_view(lg, node, side).to_vec(),
            PortSide::SOUTH | PortSide::WEST => port_side_view(lg, node, side).iter().rev().copied().collect(),
            PortSide::UNDEFINED => Vec::new(),
        }
    }

    /// `nextRandomBool()`.
    pub fn next_random_bool(&self) -> bool {
        match &self.random {
            Some(r) => r.borrow_mut().next_boolean(),
            None => true,
        }
    }

    /// `_intProperty(_:_:)`: an Int property that might be stored as Double
    /// (from JSON parsing); string-key read.
    fn int_property(lg: &LGraphArena, graph: LGraphId, key: &str) -> Option<i64> {
        let value = lg[graph].props.get_by_id(key);
        if let Some(i) = value.as_ref().and_then(|v| v.cast::<i64>()) {
            return Some(i);
        }
        if let Some(d) = value.as_ref().and_then(|v| v.cast::<f64>()) {
            return Some(swift_int(d));
        }
        None
    }
}

impl ILayoutProcessor for LayerSweepCrossingMinimizer {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, progress_monitor: &mut dyn IElkProgressMonitor) {
        progress_monitor.begin(&format!("Minimize Crossings {:?}", self.cross_min_type), 1.0);

        let layers = &lg[layered_graph].layers;
        let empty_graph = layers.is_empty() || layers.iter().all(|&l| lg[l].nodes.is_empty());
        let single_node = layers.len() == 1 && layers.first().map_or(0, |&l| lg[l].nodes.len()) == 1;
        let hierarchy = lg[layered_graph]
            .props
            .get_by_id(HIERARCHY_HANDLING)
            .and_then(|v| v.cast::<HierarchyHandling>())
            .unwrap_or(HierarchyHandling::INHERIT);
        let hierarchical_layout = hierarchy == HierarchyHandling::INCLUDE_CHILDREN;

        if empty_graph || (single_node && !hierarchical_layout) {
            progress_monitor.done();
            return;
        }

        let graphs_to_sweep_on = self.initialize(lg, layered_graph);
        let minimizing_method = self.choose_minimizing_method(&graphs_to_sweep_on);
        self.minimize_crossings(lg, &graphs_to_sweep_on, minimizing_method);
        self.transfer_node_and_port_orders_to_graph(lg);

        progress_monitor.done();
    }

    fn is_hierarchy_aware(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "LayerSweepCrossingMinimizer"
    }
}

impl ILayoutPhase for LayerSweepCrossingMinimizer {
    /// `getLayoutProcessorConfiguration(_:)`.
    fn get_layout_processor_configuration(&self, _lg: &LGraphArena, _graph: LGraphId) -> Option<LayoutProcessorConfiguration> {
        let mut configuration = LayoutProcessorConfiguration::create_from(&Self::intermediate_processing_configuration());
        configuration.add_before(LayeredPhases::P3_NODE_ORDERING, IntermediateProcessorStrategy::PORT_LIST_SORTER);
        Some(configuration)
    }
}

// MARK: - Group A dependencies (intermediate::preserveorder, SortByInputModelProcessor)

/// NEEDS GROUP A: `ModelOrderNodeComparator(graph, previousLayer, strategy,
/// longEdgeOrderingStrategy, groupOrderStrategy, beforePorts)` (the
/// `[LNode]` initializer) followed by `comp.compare(n1, n2)` calls. Assumed
/// Rust API in `intermediate::preserveorder::model_order_node_comparator`:
/// `ModelOrderNodeComparator::new_with_nodes(graph: LGraphId, previous_layer: Vec<LNodeId>,
/// ordering_strategy: OrderingStrategy, long_edge_ordering_strategy: LongEdgeOrderingStrategy,
/// group_order_strategy: GroupOrderStrategy, before_ports: bool)` and
/// `compare(&mut self, lg: &mut LGraphArena, n1: LNodeId, n2: LNodeId) -> i64`.
/// Only reached when a crossing-counter node/port influence is set.
#[allow(clippy::type_complexity)]
fn _needs_group_a_model_order_node_comparator(
    _lg: &mut LGraphArena,
    _graph: LGraphId,
    _previous_layer: &[LNodeId],
    _strategy: OrderingStrategy,
    _long_edge_ordering_strategy: LongEdgeOrderingStrategy,
    _group_order_strategy: GroupOrderStrategy,
    _before_ports: bool,
) -> Box<dyn FnMut(&mut LGraphArena, LNodeId, LNodeId) -> i64> {
    unimplemented!("ModelOrderNodeComparator (group A) is not wired yet")
}

/// NEEDS GROUP A: `ModelOrderPortComparator(graph, previousLayer, strategy,
/// targetNodeModelOrder, portModelOrder)` (the `[LNode]` initializer)
/// followed by `comp.compare(p1, p2)` calls. Assumed Rust API in
/// `intermediate::preserveorder::model_order_port_comparator`:
/// `ModelOrderPortComparator::new(graph: LGraphId, previous_layer: Vec<LNodeId>,
/// strategy: OrderingStrategy, target_node_model_order: Option<HashMap<LNodeId, i64>>,
/// port_model_order: bool)` and `compare(&mut self, lg: &mut LGraphArena, p1: LPortId, p2: LPortId) -> i64`.
#[allow(clippy::type_complexity)]
fn _needs_group_a_model_order_port_comparator(
    _lg: &mut LGraphArena,
    _graph: LGraphId,
    _previous_layer: &[LNodeId],
    _strategy: OrderingStrategy,
    _target_node_model_order: Option<HashMap<LNodeId, i64>>,
    _port_model_order: bool,
) -> Box<dyn FnMut(&mut LGraphArena, LPortId, LPortId) -> i64> {
    unimplemented!("ModelOrderPortComparator (group A) is not wired yet")
}

/// NEEDS GROUP A: `SortByInputModelProcessor.longEdgeTargetNodePreprocessing(node)`
/// (`[ObjectIdentifier: Int]` keyed by target node). Assumed Rust API in
/// `intermediate::sort_by_input_model_processor`:
/// `SortByInputModelProcessor::long_edge_target_node_preprocessing(lg: &mut LGraphArena, node: LNodeId) -> HashMap<LNodeId, i64>`.
fn _needs_group_a_long_edge_target_node_preprocessing(_lg: &mut LGraphArena, _node: LNodeId) -> HashMap<LNodeId, i64> {
    unimplemented!("SortByInputModelProcessor::long_edge_target_node_preprocessing (group A) is not wired yet")
}
