//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_BarycenterHeuristic.swift`.
//!
//! Orders a free layer by the barycenters of its nodes' neighbours in the
//! fixed layer, then lets the constraint resolver repair in-layer
//! constraints.
//!
//! Swift class semantics modelled here:
//! * `ModelOrderBarycenterHeuristic` subclasses this class and overrides the
//!   layer-level `minimizeCrossings(_:_:_:_:)` (and `isDeterministic`).
//!   Swift dispatches that call dynamically from the order-level methods, so
//!   those are written once, generically over [`BarycenterLayerSweep`].
//! * The `BarycenterState` objects are shared with the constraint resolver
//!   (see `forster_constraint_resolver`): this heuristic's `barycenterState`
//!   table is a copy of the resolver's table taken in `initAfterTraversal`,
//!   holding indices into the resolver's state arena. States this heuristic
//!   creates itself go into the same arena (or into its own when it has no
//!   resolver) and only into its own table.
//! * The port distributor is shared with the `GraphInfoHolder`; port ranks
//!   are read from it on every use, as the Swift does.

use std::cell::RefCell;
use std::rc::Rc;

use super::abstract_barycenter_port_distributor::AbstractBarycenterPortDistributor;
use super::counting::i_initializable::IInitializable;
use super::forster_constraint_resolver::{BarycenterState, BarycenterStateId, ForsterConstraintResolver};
use super::graph_info_holder::GraphDataView;
use super::i_crossing_minimization_heuristic::ICrossingMinimizationHeuristic;
use super::layer_sweep_crossing_minimizer::RandomRef;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
use crate::swift;

/// `FORCE_MODEL_ORDER_KEY`.
pub const FORCE_MODEL_ORDER_KEY: &str = "org.eclipse.elk.layered.crossingMinimization.forceNodeModelOrder";
/// `RANDOM_AMOUNT`.
pub const RANDOM_AMOUNT: f32 = 0.07;

#[derive(Clone, Default)]
pub struct BarycenterHeuristic {
    pub port_ranks: Vec<f32>,
    pub random: Option<RandomRef>,
    pub constraint_resolver: Option<ForsterConstraintResolver>,
    pub barycenter_state: Vec<Vec<Option<BarycenterStateId>>>,
    pub port_distributor: Option<Rc<RefCell<AbstractBarycenterPortDistributor>>>,
    /// The state arena used while there is no constraint resolver.
    own_states: Vec<BarycenterState>,
}

impl IInitializable for BarycenterHeuristic {}

/// The dynamically dispatched part of `BarycenterHeuristic`: the layer-level
/// `minimizeCrossings(_:_:_:_:)` and `isDeterministic()`.
pub trait BarycenterLayerSweep {
    fn base(&mut self) -> &mut BarycenterHeuristic;
    fn minimize_crossings_in_layer(&mut self, lg: &LGraphArena, layer: &mut Vec<LNodeId>, pre_ordered: bool, randomize: bool, forward: bool);
}

impl BarycenterHeuristic {
    /// `BarycenterHeuristic()`.
    pub fn new_empty() -> BarycenterHeuristic {
        BarycenterHeuristic::default()
    }

    /// `BarycenterHeuristic(_:_:_:_:)`.
    pub fn new(
        constraint_resolver: ForsterConstraintResolver,
        random: Option<RandomRef>,
        port_distributor: Rc<RefCell<AbstractBarycenterPortDistributor>>,
        _graph: &[Vec<LNodeId>],
    ) -> BarycenterHeuristic {
        BarycenterHeuristic {
            port_ranks: Vec::new(),
            random,
            constraint_resolver: Some(constraint_resolver),
            barycenter_state: Vec::new(),
            port_distributor: Some(port_distributor),
            own_states: Vec::new(),
        }
    }

    /// The `BarycenterState` objects this heuristic's table points into.
    pub fn states(&self) -> &Vec<BarycenterState> {
        match &self.constraint_resolver {
            Some(r) => &r.states,
            None => &self.own_states,
        }
    }

    pub fn states_mut(&mut self) -> &mut Vec<BarycenterState> {
        match &mut self.constraint_resolver {
            Some(r) => &mut r.states,
            None => &mut self.own_states,
        }
    }

    /// The state of `node` (see [`Self::state_of`]).
    pub fn state(&mut self, lg: &LGraphArena, node: LNodeId) -> &mut BarycenterState {
        let id = self.state_of(lg, node);
        &mut self.states_mut()[id]
    }

    /// The base class's layer-level `minimizeCrossings(_:_:_:_:)`.
    pub fn minimize_crossings_layer(&mut self, lg: &LGraphArena, layer: &mut Vec<LNodeId>, pre_ordered: bool, randomize: bool, forward: bool) {
        if randomize {
            self.randomize_barycenters(lg, layer);
        } else {
            self.calculate_barycenters(lg, layer, forward);
            self.fill_in_unknown_barycenters(lg, layer, pre_ordered);
        }

        if layer.len() > 1 {
            // The Swift reads `FORCE_MODEL_ORDER_KEY` here and sorts the same
            // way in both branches; model-order sorting is in
            // `ModelOrderBarycenterHeuristic`.
            swift::sort_by(layer, |&a, &b| self.compare_by_barycenter(lg, a, b) < 0);

            if let Some(resolver) = self.constraint_resolver.as_mut() {
                resolver.process_constraints(lg, layer);
            }
        }
    }

    /// `randomizeBarycenters(_:)`.
    pub fn randomize_barycenters(&mut self, lg: &LGraphArena, nodes: &[LNodeId]) {
        for &node in nodes {
            let value = self.next_double();
            let state = self.state(lg, node);
            state.barycenter = Some(value);
            state.summed_weight = state.barycenter.unwrap_or(0.0);
            state.degree = 1;
        }
    }

    /// `fillInUnknownBarycenters(_:_:)`.
    pub fn fill_in_unknown_barycenters(&mut self, lg: &LGraphArena, nodes: &[LNodeId], pre_ordered: bool) {
        if pre_ordered {
            let mut last_value: f64 = -1.0;
            let mut i = 0;
            while i < nodes.len() {
                let node = nodes[i];
                let state = self.state_of(lg, node);
                let mut value = self.states()[state].barycenter;
                if value.is_none() {
                    let mut next_value = last_value + 1.0;
                    let mut j = i + 1;
                    while j < nodes.len() {
                        if let Some(x) = self.state(lg, nodes[j]).barycenter {
                            next_value = x;
                            break;
                        }
                        j += 1;
                    }

                    value = Some((last_value + next_value) / 2.0);
                    let s = &mut self.states_mut()[state];
                    s.barycenter = value;
                    s.summed_weight = value.unwrap_or(0.0);
                    s.degree = 1;
                }

                last_value = value.unwrap_or(last_value);
                i += 1;
            }
        } else {
            let mut max_bary: f64 = 0.0;
            for &node in nodes {
                if let Some(bary) = self.state(lg, node).barycenter {
                    max_bary = swift::max(max_bary, bary);
                }
            }

            max_bary += 2.0;
            for &node in nodes {
                let state = self.state_of(lg, node);
                if self.states()[state].barycenter.is_none() {
                    let value = self.next_float() as f64 * max_bary - 1.0;
                    let s = &mut self.states_mut()[state];
                    s.barycenter = Some(value);
                    s.summed_weight = value;
                    s.degree = 1;
                }
            }
        }
    }

    /// `calculateBarycenters(_:_:)`.
    pub fn calculate_barycenters(&mut self, lg: &LGraphArena, nodes: &[LNodeId], forward: bool) {
        for &node in nodes {
            self.state(lg, node).visited = false;
        }
        // The distributor's port ranks, read once per layer (nothing writes
        // them while barycenters are computed).
        let distributor = self.port_distributor.clone();
        let distributor = distributor.as_ref().map(|d| d.borrow());
        let ranks: Option<&[f32]> = distributor.as_ref().map(|d| d.get_port_ranks());
        for &node in nodes {
            self.calculate_barycenter(lg, node, forward, ranks);
        }
    }

    /// `calculateBarycenter(_:_:)`.
    pub fn calculate_barycenter(&mut self, lg: &LGraphArena, node: LNodeId, forward: bool, distributor_ranks: Option<&[f32]>) {
        let node_state = self.state_of(lg, node);
        if self.states()[node_state].visited {
            return;
        }
        {
            let s = &mut self.states_mut()[node_state];
            s.visited = true;
            s.degree = 0;
            s.summed_weight = 0.0;
            s.barycenter = None;
        }

        let node_layer = lg[node].layer;
        for &free_port in &lg[node].ports {
            // `getPredecessorPorts()` / `getSuccessorPorts()`.
            let edges = if forward { &lg[free_port].incoming_edges } else { &lg[free_port].outgoing_edges };
            for &edge in edges {
                let fixed_port = if forward { lg[edge].source } else { lg[edge].target };
                let Some(fixed_port) = fixed_port else { continue };
                let Some(fixed_node) = lg[fixed_port].owner else { continue };
                if lg[fixed_node].layer == node_layer {
                    if fixed_node != node {
                        self.calculate_barycenter(lg, fixed_node, forward, distributor_ranks);
                        let fixed_state = self.state_of(lg, fixed_node);
                        let (degree, summed_weight) = {
                            let f = &self.states()[fixed_state];
                            (f.degree, f.summed_weight)
                        };
                        let s = &mut self.states_mut()[node_state];
                        s.degree += degree;
                        s.summed_weight += summed_weight;
                    }
                } else {
                    let rank = self.rank_of_port_with(lg, fixed_port, distributor_ranks);
                    let s = &mut self.states_mut()[node_state];
                    s.summed_weight += rank;
                    s.degree += 1;
                }
            }
        }

        let barycenter_associates = lg[node].props.get_typed::<Rc<Vec<LNodeId>>>(&InternalProperties::BARYCENTER_ASSOCIATES);
        if let Some(barycenter_associates) = barycenter_associates {
            for &associate in barycenter_associates.iter() {
                if node_layer != lg[associate].layer {
                    continue;
                }
                self.calculate_barycenter(lg, associate, forward, distributor_ranks);
                let associate_state = self.state_of(lg, associate);
                let (degree, summed_weight) = {
                    let a = &self.states()[associate_state];
                    (a.degree, a.summed_weight)
                };
                let s = &mut self.states_mut()[node_state];
                s.degree += degree;
                s.summed_weight += summed_weight;
            }
        }

        if self.states()[node_state].degree > 0 {
            let rf = self.next_float();
            let s = &mut self.states_mut()[node_state];
            s.summed_weight += rf as f64 * RANDOM_AMOUNT as f64 - (RANDOM_AMOUNT / 2.0) as f64;
            s.barycenter = Some(s.summed_weight / s.degree as f64);
        }
    }

    /// `stateOf(_:)`: the state in this heuristic's table, created there if
    /// missing (negative ids count as 0).
    pub fn state_of(&mut self, lg: &LGraphArena, node: LNodeId) -> BarycenterStateId {
        let layer_id = swift::max(0, lg[node].layer.map_or(0, |l| lg[l].id) as i64) as usize;
        let node_id = swift::max(0, lg[node].id as i64) as usize;

        self.ensure_barycenter_state_capacity(layer_id, node_id);
        if let Some(existing) = self.barycenter_state[layer_id][node_id] {
            return existing;
        }
        let states = self.states_mut();
        let created = states.len();
        states.push(BarycenterState::new(node));
        self.barycenter_state[layer_id][node_id] = Some(created);
        created
    }

    /// `compareByBarycenter(_:_:)`.
    pub fn compare_by_barycenter(&mut self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> i64 {
        let s1 = self.state_of(lg, n1);
        let s2 = self.state_of(lg, n2);
        let b1 = self.states()[s1].barycenter;
        let b2 = self.states()[s2].barycenter;
        match (b1, b2) {
            (Some(b1), Some(b2)) => {
                if b1 < b2 {
                    -1
                } else if b1 > b2 {
                    1
                } else {
                    0
                }
            }
            (Some(_), None) => -1,
            (None, Some(_)) => 1,
            (None, None) => 0,
        }
    }

    /// The order-level `minimizeCrossings(_:_:_:_:)`, dispatching the layer
    /// sort to `this`.
    pub fn minimize_crossings_in_order<H: BarycenterLayerSweep + ?Sized>(
        this: &mut H,
        lg: &LGraphArena,
        order: &mut Vec<Vec<LNodeId>>,
        free_layer_index: i64,
        forward_sweep: bool,
        is_first_sweep: bool,
    ) -> bool {
        if free_layer_index < 0 || free_layer_index as usize >= order.len() || order[free_layer_index as usize].is_empty() {
            return false;
        }
        let free = free_layer_index as usize;

        let base = this.base();
        if !base.is_first_layer(order, free_layer_index, forward_sweep) {
            let fixed_layer = &order[(free_layer_index - Self::change_index(forward_sweep)) as usize];
            if let Some(distributor) = &base.port_distributor {
                distributor.borrow_mut().calculate_port_ranks(lg, fixed_layer, Self::port_type_for(forward_sweep));
            }
        }

        let first_node_in_layer = order[free][0];
        let pre_ordered = !is_first_sweep || Self::is_external_port_dummy(lg, first_node_in_layer);

        let mut nodes = std::mem::take(&mut order[free]);
        this.minimize_crossings_in_layer(lg, &mut nodes, pre_ordered, false, forward_sweep);
        order[free] = nodes;
        false
    }

    /// The order-level `setFirstLayerOrder(_:_:)`, dispatching the layer
    /// sort to `this`.
    pub fn set_first_layer_order_in_order<H: BarycenterLayerSweep + ?Sized>(this: &mut H, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, is_forward_sweep: bool) -> bool {
        if order.is_empty() {
            return false;
        }
        let start_index = Self::start_index(is_forward_sweep, order.len());
        if start_index < 0 || start_index as usize >= order.len() {
            return false;
        }

        let mut nodes = std::mem::take(&mut order[start_index as usize]);
        this.minimize_crossings_in_layer(lg, &mut nodes, false, true, is_forward_sweep);
        order[start_index as usize] = nodes;
        false
    }

    /// `isExternalPortDummy(_:)`.
    pub fn is_external_port_dummy(lg: &LGraphArena, first_node: LNodeId) -> bool {
        lg[first_node].node_type == NodeType::EXTERNAL_PORT
    }

    /// `changeIndex(_:)`.
    pub fn change_index(dir: bool) -> i64 {
        if dir { 1 } else { -1 }
    }

    /// `portTypeFor(_:)`.
    pub fn port_type_for(direction: bool) -> PortType {
        if direction { PortType::OUTPUT } else { PortType::INPUT }
    }

    /// `startIndex(_:_:)`.
    pub fn start_index(dir: bool, length: usize) -> i64 {
        if dir { 0 } else { swift::max(0, length as i64 - 1) }
    }

    /// `isFirstLayer(_:_:_:)`.
    pub fn is_first_layer(&self, node_order: &[Vec<LNodeId>], current_index: i64, forward_sweep: bool) -> bool {
        current_index == Self::start_index(forward_sweep, node_order.len())
    }

    /// `initAfterTraversal()`.
    pub fn init_after_traversal(&mut self) {
        self.barycenter_state = self.constraint_resolver.as_ref().map(|r| r.get_barycenter_states()).unwrap_or_default();
        self.port_ranks = match &self.port_distributor {
            Some(distributor) => distributor.borrow().get_port_ranks().to_vec(),
            None => Vec::new(),
        };
    }

    /// `initAtLayerLevel(_:_:)`.
    pub fn init_at_layer_level(&mut self, lg: &mut LGraphArena, l: usize, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() {
            return;
        }
        let Some(&first) = node_order[l].first() else { return };
        let Some(layer) = lg[first].layer else { return };
        lg[layer].id = l as i32;
        let max_node_id = swift::seq_max(node_order[l].iter().map(|&n| lg[n].id as i64)).unwrap_or(-1);
        if max_node_id >= 0 {
            self.ensure_barycenter_state_capacity(l, max_node_id as usize);
        }
    }

    /// `ensureBarycenterStateCapacity(layerId:nodeId:)`.
    pub fn ensure_barycenter_state_capacity(&mut self, layer_id: usize, node_id: usize) {
        if self.barycenter_state.len() <= layer_id {
            self.barycenter_state.resize(layer_id + 1, Vec::new());
        }
        let row = &mut self.barycenter_state[layer_id];
        if row.len() <= node_id {
            row.resize(node_id + 1, None);
        }
    }

    /// `rankOfPort(_:)`: reads the distributor's current ranks (in Java its
    /// array is shared; Swift reads it from the distributor every time).
    pub fn rank_of_port(&self, lg: &LGraphArena, port: crate::org::eclipse::elk::alg::layered::graph::l_graph::LPortId) -> f64 {
        match &self.port_distributor {
            Some(distributor) => self.rank_of_port_with(lg, port, Some(distributor.borrow().get_port_ranks())),
            None => self.rank_of_port_with(lg, port, None),
        }
    }

    fn rank_of_port_with(&self, lg: &LGraphArena, port: crate::org::eclipse::elk::alg::layered::graph::l_graph::LPortId, distributor_ranks: Option<&[f32]>) -> f64 {
        let id = lg[port].id;
        let ranks = distributor_ranks.unwrap_or(&self.port_ranks);
        if id < 0 || id as usize >= ranks.len() {
            return 0.0;
        }
        ranks[id as usize] as f64
    }

    /// `nextDouble()`.
    pub fn next_double(&self) -> f64 {
        match &self.random {
            Some(r) => r.borrow_mut().next_double(),
            None => 0.5,
        }
    }

    /// `nextFloat()`.
    pub fn next_float(&self) -> f32 {
        match &self.random {
            Some(r) => r.borrow_mut().next_float(),
            None => 0.5,
        }
    }
}

impl BarycenterLayerSweep for BarycenterHeuristic {
    fn base(&mut self) -> &mut BarycenterHeuristic {
        self
    }

    fn minimize_crossings_in_layer(&mut self, lg: &LGraphArena, layer: &mut Vec<LNodeId>, pre_ordered: bool, randomize: bool, forward: bool) {
        self.minimize_crossings_layer(lg, layer, pre_ordered, randomize, forward);
    }
}

impl ICrossingMinimizationHeuristic for BarycenterHeuristic {
    fn always_improves(&self) -> bool {
        false
    }

    fn set_first_layer_order(&mut self, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, forward_sweep: bool, _graph_data: &GraphDataView) -> bool {
        BarycenterHeuristic::set_first_layer_order_in_order(self, lg, order, forward_sweep)
    }

    fn minimize_crossings(&mut self, lg: &LGraphArena, order: &mut Vec<Vec<LNodeId>>, free_layer_index: i64, forward_sweep: bool, is_first_sweep: bool, _graph_data: &GraphDataView) -> bool {
        BarycenterHeuristic::minimize_crossings_in_order(self, lg, order, free_layer_index, forward_sweep, is_first_sweep)
    }

    fn is_deterministic(&self) -> bool {
        false
    }
}
