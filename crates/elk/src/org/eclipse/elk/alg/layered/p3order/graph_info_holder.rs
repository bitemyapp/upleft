//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_GraphInfoHolder.swift`.
//!
//! Per-graph state of the layer sweep: the current node order, the best
//! orders found, the crossing counter, the heuristic and the port
//! distributor.
//!
//! Swift reference semantics modelled here:
//! * `_parentGraphData` (weak) is the index of the parent's holder in
//!   `LayerSweepCrossingMinimizer.graphInfoHolders` (the Swift looks it up in
//!   a snapshot of that array, whose elements are the same objects).
//! * `_parent` (weak) is the parent node; `parent()` returns a fresh empty
//!   `LNode` when there is none, which the port represents as `None`.
//! * `_layerSweepTypeDecider` holds a back reference and is only used while
//!   the holder is built, so it is a local of the constructor.
//! * `GreedySwitchHeuristic` holds a back reference to this holder; it gets a
//!   [`GraphDataView`] per call instead.
//! * A barycenter port distributor is shared with the barycenter heuristic
//!   (`Rc<RefCell<_>>`); `_portPositions` is a shared `SharedIntArray`.

use std::cell::RefCell;
use std::rc::Rc;

use super::abstract_barycenter_port_distributor::AbstractBarycenterPortDistributor;
use super::barycenter_heuristic::BarycenterHeuristic;
use super::counting::all_crossings_counter::AllCrossingsCounter;
use super::counting::i_initializable::IInitializable;
use super::counting::shared_int_array::SharedIntArray;
use super::forster_constraint_resolver::ForsterConstraintResolver;
use super::greedy_port_distributor::GreedyPortDistributor;
use super::i_crossing_minimization_heuristic::ICrossingMinimizationHeuristic;
use super::i_sweep_port_distributor::SweepPortDistributor;
use super::layer_sweep_crossing_minimizer::{random_of, CrossMinType};
use super::layer_sweep_type_decider::LayerSweepTypeDecider;
use super::layer_total_port_distributor::LayerTotalPortDistributor;
use super::model_order_barycenter_heuristic::ModelOrderBarycenterHeuristic;
use super::node_relative_port_distributor::NodeRelativePortDistributor;
use super::sweep_copy::SweepCopy;
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LNodeId};
use crate::org::eclipse::elk::alg::layered::intermediate::greedyswitch::greedy_switch_heuristic::GreedySwitchHeuristic;
use crate::org::eclipse::elk::alg::layered::options::graph_properties::GraphProperties;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;

const FORCE_NODE_MODEL_ORDER: &str = "org.eclipse.elk.layered.crossingMinimization.forceNodeModelOrder";

/// `LayerSweepCrossingMinimizer_CrossMinType` (declared in the Swift file of
/// `GraphInfoHolder`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerSweepCrossingMinimizerCrossMinType {
    BARYCENTER,
    MEDIAN,
    GREEDY_SWITCH,
    INTERACTIVE,
    NONE,
}

/// What a heuristic may read of its `GraphInfoHolder` (and that holder's
/// parent holder) while the holder is busy running it.
#[derive(Clone, Copy)]
pub struct GraphDataView<'a> {
    pub has_parent: bool,
    /// `dontSweepInto()`.
    pub dont_sweep_into: bool,
    /// `parent()` (`None`: Swift's fresh empty node).
    pub parent: Option<LNodeId>,
    /// `parentGraphData()`.
    pub parent_graph_data: Option<&'a GraphInfoHolder>,
}

impl GraphDataView<'_> {
    /// A holder with no parent that is not swept into (for heuristics used
    /// outside a sweep).
    pub fn detached() -> GraphDataView<'static> {
        GraphDataView { has_parent: false, dont_sweep_into: false, parent: None, parent_graph_data: None }
    }
}

/// The unported `MedianHeuristic` (only selectable through `MEDIAN` layer
/// sweeps, which the JSON bridge cannot choose; also the placeholder the
/// Swift installs before choosing the real heuristic).
#[derive(Clone, Debug, Default)]
pub struct MedianHeuristicPlaceholder;

impl IInitializable for MedianHeuristicPlaceholder {}

impl ICrossingMinimizationHeuristic for MedianHeuristicPlaceholder {
    fn always_improves(&self) -> bool {
        false
    }

    fn set_first_layer_order(&mut self, _lg: &LGraphArena, _order: &mut Vec<Vec<LNodeId>>, _forward_sweep: bool, _graph_data: &GraphDataView) -> bool {
        unimplemented!("MedianHeuristic is not ported (not reachable through the JSON bridge)")
    }

    fn minimize_crossings(&mut self, _lg: &LGraphArena, _order: &mut Vec<Vec<LNodeId>>, _i: i64, _f: bool, _s: bool, _graph_data: &GraphDataView) -> bool {
        unimplemented!("MedianHeuristic is not ported (not reachable through the JSON bridge)")
    }

    fn is_deterministic(&self) -> bool {
        true
    }
}

/// `any ICrossingMinimizationHeuristic` as `GraphInfoHolder` holds it.
#[derive(Clone)]
pub enum CrossMinimizer {
    Barycenter(BarycenterHeuristic),
    ModelOrderBarycenter(ModelOrderBarycenterHeuristic),
    GreedySwitch(GreedySwitchHeuristic),
    Median(MedianHeuristicPlaceholder),
}

impl CrossMinimizer {
    /// `as? BarycenterHeuristic` (true for the model-order subclass too).
    pub fn as_barycenter_mut(&mut self) -> Option<&mut BarycenterHeuristic> {
        match self {
            CrossMinimizer::Barycenter(h) => Some(h),
            CrossMinimizer::ModelOrderBarycenter(h) => Some(&mut h.base),
            _ => None,
        }
    }

    fn heuristic(&self) -> &dyn ICrossingMinimizationHeuristic {
        match self {
            CrossMinimizer::Barycenter(h) => h,
            CrossMinimizer::ModelOrderBarycenter(h) => h,
            CrossMinimizer::GreedySwitch(h) => h,
            CrossMinimizer::Median(h) => h,
        }
    }

    fn heuristic_mut(&mut self) -> &mut dyn ICrossingMinimizationHeuristic {
        match self {
            CrossMinimizer::Barycenter(h) => h,
            CrossMinimizer::ModelOrderBarycenter(h) => h,
            CrossMinimizer::GreedySwitch(h) => h,
            CrossMinimizer::Median(h) => h,
        }
    }
}

impl IInitializable for CrossMinimizer {}

impl ICrossingMinimizationHeuristic for CrossMinimizer {
    fn always_improves(&self) -> bool {
        self.heuristic().always_improves()
    }

    fn set_first_layer_order(&mut self, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, forward_sweep: bool, graph_data: &GraphDataView) -> bool {
        self.heuristic_mut().set_first_layer_order(lg, order, forward_sweep, graph_data)
    }

    fn minimize_crossings(&mut self, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, free_layer_index: i64, forward_sweep: bool, is_first_sweep: bool, graph_data: &GraphDataView) -> bool {
        self.heuristic_mut().minimize_crossings(lg, order, free_layer_index, forward_sweep, is_first_sweep, graph_data)
    }

    fn is_deterministic(&self) -> bool {
        self.heuristic().is_deterministic()
    }
}

#[derive(Clone)]
pub struct GraphInfoHolder {
    pub l_graph: LGraphId,

    pub current_node_order: Vec<Vec<LNodeId>>,
    pub currently_best_node_and_port_order: Option<SweepCopy>,
    pub best_node_and_port_order: Option<SweepCopy>,
    pub port_positions: SharedIntArray,

    pub use_bottom_up: bool,

    pub child_graphs: Vec<LGraphId>,
    pub has_external_ports: bool,
    pub has_parent: bool,
    /// `_parentGraphData`: index into the minimizer's holders.
    pub parent_graph_data: Option<usize>,
    pub parent: Option<LNodeId>,

    pub cross_minimizer: CrossMinimizer,
    pub port_distributor: SweepPortDistributor,
    pub crossings_counter: AllCrossingsCounter,
    pub n_ports: i64,
    pub original_cross_min_type: CrossMinType,
}

impl IInitializable for GraphInfoHolder {}

impl GraphInfoHolder {
    /// `GraphInfoHolder(_:_:_:_:)`. `graphs` is the minimizer's holder list
    /// so far (only used to find the parent's holder by graph id).
    pub fn new(
        lg: &mut LGraphArena,
        graph: LGraphId,
        cross_min_type: LayerSweepCrossingMinimizerCrossMinType,
        graphs: &[GraphInfoHolder],
        original_cross_min_type: CrossMinType,
    ) -> GraphInfoHolder {
        let current_node_order = lg.graph_to_node_array(graph);

        let parent = lg[graph].parent_node;
        let has_parent = parent.is_some();
        let mut parent_graph_data = None;
        if let Some(parent_graph) = parent.and_then(|p| lg.node_graph(p)) {
            let parent_id = lg[parent_graph].id;
            if parent_id >= 0 && (parent_id as usize) < graphs.len() {
                parent_graph_data = Some(parent_id as usize);
            }
        }

        let graph_properties = lg[graph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
        let has_external_ports = graph_properties.contains(GraphProperties::EXTERNAL_PORTS);

        let crossings_counter = AllCrossingsCounter::new(&current_node_order);

        let random = random_of(&lg[graph].props);
        let force_model_order = lg[graph].props.get_by_id(FORCE_NODE_MODEL_ORDER).and_then(|v| v.cast::<bool>()).unwrap_or(false);

        // Java: portDistributor = ISweepPortDistributor.create(crossMinType, random, currentNodeOrder)
        // For GREEDY_SWITCH, Java uses GreedyPortDistributor (greedy port swapping via CrossingsCounter).
        // For BARYCENTER/MEDIAN, random.nextBoolean() chooses between NodeRelative and LayerTotal.
        let shared_port_distributor: Option<Rc<RefCell<AbstractBarycenterPortDistributor>>>;
        let port_distributor: SweepPortDistributor;
        if cross_min_type == LayerSweepCrossingMinimizerCrossMinType::GREEDY_SWITCH {
            shared_port_distributor = None;
            port_distributor = SweepPortDistributor::Greedy(GreedyPortDistributor::new());
        } else if random.as_ref().map_or(true, |r| r.borrow_mut().next_boolean()) {
            let pd = Rc::new(RefCell::new(NodeRelativePortDistributor::new(current_node_order.len() as i64)));
            shared_port_distributor = Some(pd.clone());
            port_distributor = SweepPortDistributor::Barycenter(pd);
        } else {
            let pd = Rc::new(RefCell::new(LayerTotalPortDistributor::new(current_node_order.len() as i64)));
            shared_port_distributor = Some(pd.clone());
            port_distributor = SweepPortDistributor::Barycenter(pd);
        }

        // The Swift first installs a `MedianHeuristic` placeholder.
        let mut cross_minimizer = CrossMinimizer::Median(MedianHeuristicPlaceholder);

        match cross_min_type {
            LayerSweepCrossingMinimizerCrossMinType::BARYCENTER => {
                let constraint_resolver = ForsterConstraintResolver::new(lg, &current_node_order);
                if let Some(dist) = shared_port_distributor {
                    if force_model_order {
                        cross_minimizer = CrossMinimizer::ModelOrderBarycenter(ModelOrderBarycenterHeuristic::new(
                            constraint_resolver,
                            random.clone(),
                            dist,
                            &current_node_order,
                        ));
                    } else {
                        cross_minimizer =
                            CrossMinimizer::Barycenter(BarycenterHeuristic::new(constraint_resolver, random.clone(), dist, &current_node_order));
                    }
                }
            }
            LayerSweepCrossingMinimizerCrossMinType::MEDIAN => {}
            LayerSweepCrossingMinimizerCrossMinType::GREEDY_SWITCH => {
                cross_minimizer = CrossMinimizer::GreedySwitch(GreedySwitchHeuristic::new(original_cross_min_type));
            }
            LayerSweepCrossingMinimizerCrossMinType::INTERACTIVE | LayerSweepCrossingMinimizerCrossMinType::NONE => {}
        }

        let mut holder = GraphInfoHolder {
            l_graph: graph,
            current_node_order,
            currently_best_node_and_port_order: None,
            best_node_and_port_order: None,
            port_positions: SharedIntArray::new(),
            use_bottom_up: false,
            child_graphs: Vec::new(),
            has_external_ports,
            has_parent,
            parent_graph_data,
            parent,
            cross_minimizer,
            port_distributor,
            crossings_counter,
            n_ports: 0,
            original_cross_min_type,
        };

        // `_layerSweepTypeDecider` (lazy; creating it has no side effects).
        let mut layer_sweep_type_decider = LayerSweepTypeDecider::new(&holder);
        holder.initialize_by_traversal(lg, &mut layer_sweep_type_decider);
        holder.use_bottom_up = layer_sweep_type_decider.use_bottom_up(lg, &holder);
        holder
    }

    /// `dontSweepInto()`.
    pub fn dont_sweep_into(&self) -> bool {
        self.use_bottom_up
    }

    /// `lGraph()`.
    pub fn l_graph(&self) -> LGraphId {
        self.l_graph
    }

    /// `parent()`: `None` stands for the fresh `LNode(LGraph())` the Swift
    /// returns when the graph has no parent node.
    pub fn parent(&self) -> Option<LNodeId> {
        self.parent
    }

    /// `getBestSweep()`.
    pub fn get_best_sweep(&self) -> Option<&SweepCopy> {
        if self.cross_min_deterministic() {
            self.currently_best_node_and_port_order.as_ref()
        } else {
            self.best_node_and_port_order.as_ref()
        }
    }

    /// `crossMinDeterministic()`.
    pub fn cross_min_deterministic(&self) -> bool {
        self.cross_minimizer.is_deterministic()
    }

    /// `crossMinAlwaysImproves()`.
    pub fn cross_min_always_improves(&self) -> bool {
        self.cross_minimizer.always_improves()
    }

    /// `portPositions()`: the shared array itself.
    pub fn port_positions(&self) -> SharedIntArray {
        self.port_positions.clone()
    }

    /// A view of this holder for its own heuristic.
    pub fn view<'a>(&self, parent_graph_data: Option<&'a GraphInfoHolder>) -> GraphDataView<'a> {
        GraphDataView { has_parent: self.has_parent, dont_sweep_into: self.use_bottom_up, parent: self.parent, parent_graph_data }
    }

    /// `initAtNodeLevel(_:_:_:)`: collects nested graphs.
    pub fn init_at_node_level(&mut self, lg: &LGraphArena, l: usize, n: usize, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() || n >= node_order[l].len() {
            return;
        }
        if let Some(nested_graph) = lg[node_order[l][n]].nested_graph {
            self.child_graphs.push(nested_graph);
        }
    }

    /// `initAtPortLevel(_:_:_:_:)`.
    pub fn init_at_port_level(&mut self, _l: usize, _n: usize, _p: usize, _node_order: &[Vec<LNodeId>]) {
        self.n_ports += 1;
    }

    /// `initAfterTraversal()`.
    pub fn init_after_traversal(&mut self) {
        self.port_positions = SharedIntArray::repeating(0, self.n_ports as usize);
    }

    /// `initializeByTraversal()`: Java's `IInitializable.init` traversal,
    /// calling the components in the order the Swift does.
    pub fn initialize_by_traversal(&mut self, lg: &mut LGraphArena, layer_sweep_type_decider: &mut LayerSweepTypeDecider) {
        // The components get the holder's order (a Swift copy); nothing here
        // changes it.
        let order = std::mem::take(&mut self.current_node_order);

        for (layer_index, layer) in order.iter().enumerate() {
            layer_sweep_type_decider.init_at_layer_level(lg, layer_index, &order);
            if let Some(bh) = self.cross_minimizer.as_barycenter_mut() {
                if let Some(cr) = bh.constraint_resolver.as_mut() {
                    cr.init_at_layer_level(layer_index, &order);
                }
                bh.init_at_layer_level(lg, layer_index, &order);
            }
            if let CrossMinimizer::GreedySwitch(gs) = &mut self.cross_minimizer {
                gs.init_at_layer_level(lg, layer_index, &order);
            }

            for (node_index, &node) in layer.iter().enumerate() {
                self.init_at_node_level(lg, layer_index, node_index, &order);
                self.crossings_counter.init_at_node_level(lg, layer_index, node_index, &order);
                layer_sweep_type_decider.init_at_node_level(lg, layer_index, node_index, &order);
                match &mut self.port_distributor {
                    SweepPortDistributor::Barycenter(pd) => pd.borrow_mut().init_at_node_level(lg, layer_index, node_index, &order),
                    SweepPortDistributor::Greedy(pd) => pd.init_at_node_level(lg, layer_index, node_index, &order),
                }
                if let Some(cr) = self.cross_minimizer.as_barycenter_mut().and_then(|bh| bh.constraint_resolver.as_mut()) {
                    cr.init_at_node_level(lg, layer_index, node_index, &order);
                }

                let port_count = lg[node].ports.len();
                for port_index in 0..port_count {
                    self.init_at_port_level(layer_index, node_index, port_index, &order);
                    self.crossings_counter.init_at_port_level(lg, layer_index, node_index, port_index, &order);
                    if let SweepPortDistributor::Barycenter(pd) = &self.port_distributor {
                        pd.borrow_mut().init_at_port_level(lg, layer_index, node_index, port_index, &order);
                    }
                    if let CrossMinimizer::GreedySwitch(gs) = &mut self.cross_minimizer {
                        gs.init_at_port_level(layer_index, node_index, port_index, &order);
                    }

                    let port = lg[node].ports[port_index];
                    let connected_edges = lg.port_connected_edges(port);
                    for (edge_index, &edge) in connected_edges.iter().enumerate() {
                        self.crossings_counter.init_at_edge_level(lg, layer_index, node_index, port_index, edge_index, edge, &order);
                    }
                }
            }
        }

        self.init_after_traversal();
        self.crossings_counter.init_after_traversal();
        match &mut self.port_distributor {
            SweepPortDistributor::Barycenter(pd) => pd.borrow_mut().init_after_traversal(),
            SweepPortDistributor::Greedy(pd) => pd.init_after_traversal(),
        }
        if let Some(bh) = self.cross_minimizer.as_barycenter_mut() {
            bh.init_after_traversal();
        }
        if let CrossMinimizer::GreedySwitch(gs) = &mut self.cross_minimizer {
            gs.init_after_traversal();
        }

        self.current_node_order = order;
    }

    /// `graphPropertiesKey()`.
    pub fn graph_properties_key(&self) -> &'static str {
        "graphProperties"
    }

    /// `randomKey()`.
    pub fn random_key(&self) -> &'static str {
        "random"
    }

    /// `crossingMinForceNodeModelOrderKey()`.
    pub fn crossing_min_force_node_model_order_key(&self) -> &'static str {
        FORCE_NODE_MODEL_ORDER
    }
}
