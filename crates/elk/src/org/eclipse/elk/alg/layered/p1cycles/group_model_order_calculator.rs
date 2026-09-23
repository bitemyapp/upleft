//! Port of `alg/layered/p1cycles/GroupModelOrderCalculator.swift` (with its
//! `ModelOrderPropertyScaffolding`).
//!
//! Only the model-order cycle breakers use this, and none of them can be
//! selected through the JSON bridge. elk-swift's scaffolding maps have no
//! writers, so every node's layer constraint reads as `NONE`, its group id as
//! 0, and its model order as its `id`.

use crate::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
use crate::prelude::*;

/// `ModelOrderPropertyScaffolding`: lookups into maps that are always empty.
pub mod ModelOrderPropertyScaffolding {
    use crate::org::eclipse::elk::alg::layered::options::group_order_strategy::GroupOrderStrategy;
    use crate::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
    use crate::prelude::*;

    pub fn layer_constraint(_node: LNodeId) -> Option<LayerConstraint> {
        None
    }

    pub fn model_order(_node: LNodeId) -> Option<i64> {
        None
    }

    pub fn cycle_breaking_group_id(_node: LNodeId) -> Option<i64> {
        None
    }

    pub fn group_order_strategy(_graph: Option<LGraphId>) -> Option<GroupOrderStrategy> {
        None
    }

    pub fn max_model_order_nodes(_graph: Option<LGraphId>) -> Option<i64> {
        None
    }

    pub fn cb_num_model_order_groups(_graph: Option<LGraphId>) -> Option<i64> {
        None
    }
}

#[derive(Default)]
pub struct GroupModelOrderCalculator {
    pub first_separate_nodes: i64,
    pub last_separate_nodes: i64,
}

impl GroupModelOrderCalculator {
    pub fn new() -> GroupModelOrderCalculator {
        GroupModelOrderCalculator::default()
    }

    fn constraint_offset(&mut self, node: LNodeId, offset: i64) -> i64 {
        match self.layer_constraint(node) {
            LayerConstraint::FIRST_SEPARATE => {
                let model_order = (2 * -offset) + self.first_separate_nodes;
                self.first_separate_nodes += 1;
                model_order
            }
            LayerConstraint::FIRST => -offset,
            LayerConstraint::LAST => offset,
            LayerConstraint::LAST_SEPARATE => {
                let model_order = (2 * offset) + self.last_separate_nodes;
                self.last_separate_nodes += 1;
                model_order
            }
            LayerConstraint::NONE => 0,
        }
    }

    /// `computeConstraintModelOrder(_:_:)`.
    pub fn compute_constraint_model_order(&mut self, lg: &LGraphArena, node: LNodeId, offset: i64) -> i64 {
        let mut model_order = self.constraint_offset(node, offset);
        if let Some(node_model_order) = self.model_order_property(lg, node) {
            model_order += node_model_order;
        }
        model_order
    }

    /// `computeConstraintGroupModelOrder(_:_:_:)`.
    pub fn compute_constraint_group_model_order(&mut self, lg: &LGraphArena, node: LNodeId, offset: i64, small_offset: i64) -> i64 {
        let mut model_order = self.constraint_offset(node, offset);
        if let Some((group_id, node_model_order)) = self.group_model_order_components(lg, node) {
            model_order += (group_id * small_offset) + node_model_order;
        }
        model_order
    }

    pub fn reset_internal_counters(&mut self) {
        self.first_separate_nodes = 0;
        self.last_separate_nodes = 0;
    }

    pub fn layer_constraint(&self, node: LNodeId) -> LayerConstraint {
        ModelOrderPropertyScaffolding::layer_constraint(node).unwrap_or(LayerConstraint::NONE)
    }

    /// `modelOrderProperty(for:)`: the scaffolding value, else the node's `id`.
    pub fn model_order_property(&self, lg: &LGraphArena, node: LNodeId) -> Option<i64> {
        Some(ModelOrderPropertyScaffolding::model_order(node).unwrap_or(lg[node].id as i64))
    }

    /// `groupModelOrderComponents(for:)`.
    pub fn group_model_order_components(&self, lg: &LGraphArena, node: LNodeId) -> Option<(i64, i64)> {
        let model_order = self.model_order_property(lg, node)?;
        let group_id = ModelOrderPropertyScaffolding::cycle_breaking_group_id(node).unwrap_or(0);
        Some((group_id, model_order))
    }
}
