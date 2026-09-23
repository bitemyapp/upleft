//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_ForsterConstraintResolver.swift`
//! (also defines `BarycenterState`, as that file does).
//!
//! Swift reference semantics modelled here:
//! * `BarycenterState` is a class. The resolver creates the states and keeps
//!   them in `barycenterStates`; `BarycenterHeuristic` takes a *copy* of that
//!   table (`getBarycenterStates()` returns a Swift array) holding the same
//!   objects, and may add states of its own that the resolver never sees.
//!   The states therefore live in an arena ([`ForsterConstraintResolver::states`])
//!   and both tables hold arena indices.
//! * `ConstraintGroup` is a class compared by identity; groups live in
//!   [`ForsterConstraintResolver::groups`] and are referred to by index. A
//!   group's `outgoingConstraints`/`incomingConstraints` keep Swift's
//!   nil-versus-empty distinction (it is observable when groups merge).

use std::collections::{HashMap, VecDeque};

use super::counting::i_initializable::IInitializable;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;

/// `BarycenterState` (a class; see the module docs).
#[derive(Clone, Debug)]
pub struct BarycenterState {
    pub node: LNodeId,
    pub barycenter: Option<f64>,
    pub summed_weight: f64,
    pub degree: i64,
    pub visited: bool,
}

impl BarycenterState {
    pub fn new(node: LNodeId) -> BarycenterState {
        BarycenterState { node, barycenter: None, summed_weight: 0.0, degree: 0, visited: false }
    }
}

/// Index of a `BarycenterState` in its arena.
pub type BarycenterStateId = usize;
/// Index of a `ConstraintGroup` in [`ForsterConstraintResolver::groups`].
pub type ConstraintGroupId = usize;

/// `ForsterConstraintResolver.ConstraintGroup`.
#[derive(Clone, Debug)]
pub struct ConstraintGroup {
    pub summed_weight: f64,
    pub degree: i64,
    pub nodes: Vec<LNodeId>,
    pub outgoing_constraints: Option<Vec<ConstraintGroupId>>,
    pub incoming_constraints: Option<Vec<ConstraintGroupId>>,
    pub incoming_constraints_count: i64,
}

#[derive(Clone, Debug)]
pub struct ForsterConstraintResolver {
    pub constraints_between_non_dummies: bool,
    /// `layoutUnits` (only looked up, never iterated).
    pub layout_units: HashMap<LNodeId, Vec<LNodeId>>,
    pub barycenter_states: Vec<Vec<Option<BarycenterStateId>>>,
    pub constraint_groups: Vec<Vec<Option<ConstraintGroupId>>>,
    /// Every `BarycenterState` object of this resolver and of the heuristic
    /// that uses it.
    pub states: Vec<BarycenterState>,
    /// Every `ConstraintGroup` object created by this resolver.
    pub groups: Vec<ConstraintGroup>,
}

impl IInitializable for ForsterConstraintResolver {}

/// `BARYCENTER_EQUALITY_DELTA` (only used by release-disabled asserts).
pub const BARYCENTER_EQUALITY_DELTA: f32 = 0.0001;

const IN_LAYER_SUCCESSOR_CONSTRAINTS_BETWEEN_NON_DUMMIES: &str = "org.eclipse.elk.layered.inLayerSuccessorConstraintsBetweenNonDummies";

impl ForsterConstraintResolver {
    pub fn new(lg: &LGraphArena, current_node_order: &[Vec<LNodeId>]) -> ForsterConstraintResolver {
        let mut constraints_between_non_dummies = false;
        if let Some(&first) = current_node_order.first().and_then(|l| l.first()) {
            if let Some(graph) = lg.node_graph(first) {
                constraints_between_non_dummies = lg[graph]
                    .props
                    .get_by_id(IN_LAYER_SUCCESSOR_CONSTRAINTS_BETWEEN_NON_DUMMIES)
                    .and_then(|v| v.cast::<bool>())
                    .unwrap_or(false);
            }
        }
        ForsterConstraintResolver {
            constraints_between_non_dummies,
            layout_units: HashMap::new(),
            barycenter_states: vec![Vec::new(); current_node_order.len()],
            constraint_groups: vec![Vec::new(); current_node_order.len()],
            states: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// `initAtLayerLevel(_:_:)`.
    pub fn init_at_layer_level(&mut self, l: usize, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() {
            return;
        }
        self.barycenter_states[l] = vec![None; node_order[l].len()];
        self.constraint_groups[l] = vec![None; node_order[l].len()];
    }

    /// `initAtNodeLevel(_:_:_:)`.
    pub fn init_at_node_level(&mut self, lg: &LGraphArena, l: usize, n: usize, node_order: &[Vec<LNodeId>]) {
        if l >= node_order.len() || n >= node_order[l].len() {
            return;
        }
        self.init_at_node_level_for(lg, node_order[l][n], true);
    }

    /// `getBarycenterStates()`: a copy of the table (same state objects).
    pub fn get_barycenter_states(&self) -> Vec<Vec<Option<BarycenterStateId>>> {
        self.barycenter_states.clone()
    }

    /// `processConstraints(_:)`.
    pub fn process_constraints(&mut self, lg: &LGraphArena, nodes: &mut Vec<LNodeId>) {
        if self.constraints_between_non_dummies {
            self.process_constraints_between(lg, nodes, true);
            for &node in nodes.iter() {
                self.init_at_node_level_for(lg, node, false);
            }
        }

        self.process_constraints_between(lg, nodes, false);
    }

    /// `initAtNodeLevel(_ node:, _ fullInit:)`.
    pub fn init_at_node_level_for(&mut self, lg: &LGraphArena, node: LNodeId, full_init: bool) {
        let Some(layer) = lg[node].layer else { return };
        let layer_index = lg[layer].id;
        let node_index = lg[node].id;
        if layer_index < 0
            || layer_index as usize >= self.constraint_groups.len()
            || node_index < 0
            || node_index as usize >= self.constraint_groups[layer_index as usize].len()
        {
            return;
        }
        let (li, ni) = (layer_index as usize, node_index as usize);

        let group = self.new_single_group(lg, node);
        self.constraint_groups[li][ni] = Some(group);

        if full_init {
            let state = self.states.len();
            self.states.push(BarycenterState::new(node));
            // Traps like the Swift if the state table is shorter than the group table.
            self.barycenter_states[li][ni] = Some(state);

            if let Some(layout_unit) = lg[node].props.get_as::<LNodeId>(&InternalProperties::IN_LAYER_LAYOUT_UNIT) {
                self.layout_units.entry(layout_unit).or_default().push(node);
            }
        }
    }

    /// `processConstraints(_:_:)`.
    pub fn process_constraints_between(&mut self, lg: &LGraphArena, nodes: &mut Vec<LNodeId>, only_between_normal_nodes: bool) {
        let mut groups: Vec<ConstraintGroupId> = nodes.iter().filter_map(|&n| self.group_of(lg, n)).collect();

        self.build_constraints_graph(lg, &groups, only_between_normal_nodes);

        while let Some((first, second)) = self.find_violated_constraint(lg, &groups) {
            self.handle_violated_constraint(lg, first, second, &mut groups);
        }

        let mut resolved: Vec<LNodeId> = Vec::with_capacity(nodes.len());
        for &group in &groups {
            for i in 0..self.groups[group].nodes.len() {
                let node = self.groups[group].nodes[i];
                resolved.push(node);
                if let Some(state) = self.state_of(lg, node) {
                    let barycenter = self.group_barycenter(lg, group);
                    self.states[state].barycenter = barycenter;
                }
            }
        }
        *nodes = resolved;
    }

    /// `buildConstraintsGraph(_:_:)`.
    pub fn build_constraints_graph(&mut self, lg: &LGraphArena, groups: &[ConstraintGroupId], only_between_normal_nodes: bool) {
        for &group in groups {
            self.groups[group].outgoing_constraints = None;
            self.groups[group].incoming_constraints_count = 0;
        }

        let mut last_non_dummy_node: Option<LNodeId> = None;

        for &group in groups {
            // `group.getNode()`: only single-node groups.
            let node = {
                let g = &self.groups[group];
                if g.nodes.len() != 1 {
                    continue;
                }
                g.nodes[0]
            };

            if only_between_normal_nodes && lg[node].node_type != NodeType::NORMAL {
                continue;
            }

            let successors = lg[node].props.get_as::<std::rc::Rc<Vec<LNodeId>>>(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS);
            if let Some(successors) = successors {
                for &successor in successors.iter() {
                    if !only_between_normal_nodes || lg[successor].node_type == NodeType::NORMAL {
                        let Some(successor_group) = self.group_of(lg, successor) else { continue };
                        self.add_outgoing_constraint(group, successor_group);
                        self.groups[successor_group].incoming_constraints_count += 1;
                    }
                }
            }

            if !only_between_normal_nodes && lg[node].node_type == NodeType::NORMAL {
                if let Some(last) = last_non_dummy_node {
                    let last_unit = self.layout_unit_nodes(last);
                    let current_unit = self.layout_unit_nodes(node);
                    for &last_unit_node in &last_unit {
                        for &current_unit_node in &current_unit {
                            let (Some(last_group), Some(current_group)) = (self.group_of(lg, last_unit_node), self.group_of(lg, current_unit_node)) else {
                                continue;
                            };
                            self.add_outgoing_constraint(last_group, current_group);
                            self.groups[current_group].incoming_constraints_count += 1;
                        }
                    }
                }

                last_non_dummy_node = Some(node);
            }
        }
    }

    /// `findViolatedConstraint(_:)`: `(predecessor, group)` of the first
    /// violated constraint in topological order.
    pub fn find_violated_constraint(&mut self, lg: &LGraphArena, groups: &[ConstraintGroupId]) -> Option<(ConstraintGroupId, ConstraintGroupId)> {
        let mut active_groups: VecDeque<ConstraintGroupId> = VecDeque::new();

        // (The Swift asserts ascending barycenters here; a no-op in release.)
        for &group in groups {
            self.groups[group].incoming_constraints = None;

            if self.has_outgoing_constraints(group) && self.groups[group].incoming_constraints_count == 0 {
                active_groups.push_back(group);
            }
        }

        while let Some(group) = active_groups.pop_front() {
            if self.has_incoming_constraints(group) {
                let incoming = self.get_incoming_constraints(group);
                for predecessor in incoming {
                    let predecessor_barycenter = self.group_barycenter(lg, predecessor);
                    let group_barycenter = self.group_barycenter(lg, group);

                    if (predecessor_barycenter.unwrap_or(f64::NAN) as f32) == (group_barycenter.unwrap_or(f64::NAN) as f32) {
                        if Self::index_of(groups, predecessor) > Self::index_of(groups, group) {
                            return Some((predecessor, group));
                        }
                    } else if predecessor_barycenter.unwrap_or(f64::NEG_INFINITY) > group_barycenter.unwrap_or(f64::INFINITY) {
                        return Some((predecessor, group));
                    }
                }
            }

            let outgoing = self.get_outgoing_constraints(group);
            for successor in outgoing {
                self.prepend_incoming_constraint(successor, group);
                let incoming_len = self.get_incoming_constraints_len(successor);
                if self.groups[successor].incoming_constraints_count == incoming_len {
                    active_groups.push_back(successor);
                }
            }
        }

        None
    }

    /// `handleViolatedConstraint(_:_:_:)`.
    pub fn handle_violated_constraint(
        &mut self,
        lg: &LGraphArena,
        first_node_group: ConstraintGroupId,
        second_node_group: ConstraintGroupId,
        node_groups: &mut Vec<ConstraintGroupId>,
    ) {
        let new_node_group = self.new_merged_group(lg, first_node_group, second_node_group);

        // (Two release-disabled asserts on the new barycenter.)

        let mut i = 0;
        let mut already_inserted = false;
        while i < node_groups.len() {
            let node_group = node_groups[i];

            if node_group == first_node_group || node_group == second_node_group {
                node_groups.remove(i);
                continue;
            }

            if !already_inserted {
                if let (Some(current_barycenter), Some(new_barycenter)) = (self.group_barycenter(lg, node_group), self.group_barycenter(lg, new_node_group)) {
                    if current_barycenter > new_barycenter {
                        node_groups.insert(i, new_node_group);
                        already_inserted = true;
                        i += 1;
                        continue;
                    }
                }
            }

            if self.has_outgoing_constraints(node_group) {
                let first_removed = self.remove_outgoing_constraint(node_group, first_node_group);
                let second_removed = self.remove_outgoing_constraint(node_group, second_node_group);

                if first_removed || second_removed {
                    self.add_outgoing_constraint(node_group, new_node_group);
                    self.groups[new_node_group].incoming_constraints_count += 1;
                }
            }

            i += 1;
        }

        if !already_inserted {
            node_groups.push(new_node_group);
        }
    }

    /// `layoutUnitNodes(for:)`.
    pub fn layout_unit_nodes(&self, node: LNodeId) -> Vec<LNodeId> {
        if let Some(own) = self.layout_units.get(&node) {
            if !own.is_empty() {
                return own.clone();
            }
        }
        vec![node]
    }

    /// `indexOf(_:_:)`.
    pub fn index_of(groups: &[ConstraintGroupId], target: ConstraintGroupId) -> i64 {
        groups.iter().position(|&g| g == target).map_or(-1, |i| i as i64)
    }

    /// `groupOf(_:)`.
    pub fn group_of(&self, lg: &LGraphArena, node: LNodeId) -> Option<ConstraintGroupId> {
        let layer_index = lg[lg[node].layer?].id;
        let node_id = lg[node].id;
        if layer_index < 0 || layer_index as usize >= self.constraint_groups.len() || node_id < 0 || node_id as usize >= self.constraint_groups[layer_index as usize].len() {
            return None;
        }
        self.constraint_groups[layer_index as usize][node_id as usize]
    }

    /// `stateOf(_:)`.
    pub fn state_of(&self, lg: &LGraphArena, node: LNodeId) -> Option<BarycenterStateId> {
        let layer_index = lg[lg[node].layer?].id;
        let node_id = lg[node].id;
        if layer_index < 0 || layer_index as usize >= self.barycenter_states.len() || node_id < 0 || node_id as usize >= self.barycenter_states[layer_index as usize].len() {
            return None;
        }
        self.barycenter_states[layer_index as usize][node_id as usize]
    }

    // MARK: - ConstraintGroup

    /// `ConstraintGroup(node, resolver:)`.
    fn new_single_group(&mut self, lg: &LGraphArena, node: LNodeId) -> ConstraintGroupId {
        let mut group = ConstraintGroup {
            summed_weight: 0.0,
            degree: 0,
            nodes: vec![node],
            outgoing_constraints: None,
            incoming_constraints: None,
            incoming_constraints_count: 0,
        };
        if let Some(state) = self.state_of(lg, node) {
            group.summed_weight = self.states[state].summed_weight;
            group.degree = self.states[state].degree;
        }
        let id = self.groups.len();
        self.groups.push(group);
        id
    }

    /// `ConstraintGroup(nodeGroup1, nodeGroup2, resolver:)`.
    fn new_merged_group(&mut self, lg: &LGraphArena, node_group1: ConstraintGroupId, node_group2: ConstraintGroupId) -> ConstraintGroupId {
        let mut nodes = self.groups[node_group1].nodes.clone();
        nodes.extend_from_slice(&self.groups[node_group2].nodes);

        let mut outgoing_constraints: Option<Vec<ConstraintGroupId>> = None;
        if let Some(out1) = self.groups[node_group1].outgoing_constraints.clone() {
            let mut merged: Vec<ConstraintGroupId> = out1.into_iter().filter(|&g| g != node_group2).collect();
            if let Some(out2) = self.groups[node_group2].outgoing_constraints.clone() {
                for candidate in out2 {
                    if candidate == node_group1 {
                        continue;
                    }
                    if merged.contains(&candidate) {
                        self.groups[candidate].incoming_constraints_count -= 1;
                    } else {
                        merged.push(candidate);
                    }
                }
            }
            outgoing_constraints = Some(merged);
        } else if let Some(out2) = &self.groups[node_group2].outgoing_constraints {
            outgoing_constraints = Some(out2.iter().copied().filter(|&g| g != node_group1).collect());
        }

        let summed_weight = self.groups[node_group1].summed_weight + self.groups[node_group2].summed_weight;
        let degree = self.groups[node_group1].degree + self.groups[node_group2].degree;

        let id = self.groups.len();
        self.groups.push(ConstraintGroup { summed_weight, degree, nodes, outgoing_constraints, incoming_constraints: None, incoming_constraints_count: 0 });

        if degree > 0 {
            self.group_set_barycenter(lg, id, Some(summed_weight / degree as f64));
        } else if let (Some(b1), Some(b2)) = (self.group_barycenter(lg, node_group1), self.group_barycenter(lg, node_group2)) {
            self.group_set_barycenter(lg, id, Some((b1 + b2) / 2.0));
        } else if let Some(b1) = self.group_barycenter(lg, node_group1) {
            self.group_set_barycenter(lg, id, Some(b1));
        } else if let Some(b2) = self.group_barycenter(lg, node_group2) {
            self.group_set_barycenter(lg, id, Some(b2));
        }
        id
    }

    /// `ConstraintGroup.setBarycenter(_:)`: writes every node's state.
    pub fn group_set_barycenter(&mut self, lg: &LGraphArena, group: ConstraintGroupId, barycenter: Option<f64>) {
        for i in 0..self.groups[group].nodes.len() {
            let node = self.groups[group].nodes[i];
            if let Some(state) = self.state_of(lg, node) {
                self.states[state].barycenter = barycenter;
            }
        }
    }

    /// `ConstraintGroup.getBarycenter()`: the first node's barycenter.
    pub fn group_barycenter(&self, lg: &LGraphArena, group: ConstraintGroupId) -> Option<f64> {
        let &first = self.groups[group].nodes.first()?;
        self.state_of(lg, first).and_then(|s| self.states[s].barycenter)
    }

    /// `getOutgoingConstraints()` (turns nil into an empty list).
    fn get_outgoing_constraints(&mut self, group: ConstraintGroupId) -> Vec<ConstraintGroupId> {
        self.groups[group].outgoing_constraints.get_or_insert_with(Vec::new).clone()
    }

    fn add_outgoing_constraint(&mut self, group: ConstraintGroupId, constraint: ConstraintGroupId) {
        self.groups[group].outgoing_constraints.get_or_insert_with(Vec::new).push(constraint);
    }

    fn has_outgoing_constraints(&self, group: ConstraintGroupId) -> bool {
        self.groups[group].outgoing_constraints.as_ref().map_or(false, |v| !v.is_empty())
    }

    /// `getIncomingConstraints()` (turns nil into an empty list).
    fn get_incoming_constraints(&mut self, group: ConstraintGroupId) -> Vec<ConstraintGroupId> {
        self.groups[group].incoming_constraints.get_or_insert_with(Vec::new).clone()
    }

    fn get_incoming_constraints_len(&mut self, group: ConstraintGroupId) -> i64 {
        self.groups[group].incoming_constraints.get_or_insert_with(Vec::new).len() as i64
    }

    fn prepend_incoming_constraint(&mut self, group: ConstraintGroupId, constraint: ConstraintGroupId) {
        self.groups[group].incoming_constraints.get_or_insert_with(Vec::new).insert(0, constraint);
    }

    fn has_incoming_constraints(&self, group: ConstraintGroupId) -> bool {
        self.groups[group].incoming_constraints.as_ref().map_or(false, |v| !v.is_empty())
    }

    /// `removeOutgoingConstraint(_:)`.
    fn remove_outgoing_constraint(&mut self, group: ConstraintGroupId, target: ConstraintGroupId) -> bool {
        let Some(outgoing) = self.groups[group].outgoing_constraints.as_mut() else { return false };
        let before = outgoing.len();
        outgoing.retain(|&g| g != target);
        outgoing.len() != before
    }
}
