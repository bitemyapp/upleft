//! Port of `alg/layered/p4nodes/bk/ThresholdStrategy.swift`
//! (`ThresholdStrategy`, `NullThresholdStrategy`, `SimpleThresholdStrategy`,
//! `Postprocessable`).
//!
//! The Swift strategies keep references to the layout and the neighbourhood
//! information; here both are passed to every call.

use std::collections::VecDeque;

use super::bk_aligned_layout::{BKAlignedLayout, HDirection, VDirection};
use super::neighborhood_information::NeighborhoodInformation;
use crate::prelude::*;

pub const THRESHOLD: f64 = f64::MAX;
pub const EPSILON: f64 = 0.0001;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThresholdStrategyKind {
    NullThresholdStrategy,
    SimpleThresholdStrategy,
}

/// `Postprocessable`. Swift passes it by reference, but `pickEdge` returns the
/// object it was given and callers only ever use the returned one, so a value
/// type behaves the same.
#[derive(Clone, Copy, Debug)]
pub struct Postprocessable {
    pub free: LNodeId,
    pub is_root: bool,
    pub has_edges: bool,
    pub edge: Option<LEdgeId>,
}

impl Postprocessable {
    pub fn new(free: LNodeId, is_root: bool) -> Postprocessable {
        Postprocessable { free, is_root, has_edges: false, edge: None }
    }
}

pub struct ThresholdStrategy {
    pub kind: ThresholdStrategyKind,
    /// `blockFinished`, a membership table indexed by node `id`.
    pub block_finished: Vec<bool>,
    pub post_processables_queue: VecDeque<Postprocessable>,
    pub post_processables_stack: Vec<Postprocessable>,
}

#[inline]
fn nid(lg: &LGraphArena, n: LNodeId) -> usize {
    lg[n].id as usize
}

impl ThresholdStrategy {
    pub fn null_threshold_strategy(node_count: usize) -> ThresholdStrategy {
        ThresholdStrategy::with_kind(ThresholdStrategyKind::NullThresholdStrategy, node_count)
    }

    pub fn simple_threshold_strategy(node_count: usize) -> ThresholdStrategy {
        ThresholdStrategy::with_kind(ThresholdStrategyKind::SimpleThresholdStrategy, node_count)
    }

    fn with_kind(kind: ThresholdStrategyKind, node_count: usize) -> ThresholdStrategy {
        ThresholdStrategy { kind, block_finished: vec![false; node_count], post_processables_queue: VecDeque::new(), post_processables_stack: Vec::new() }
    }

    /// `finishBlock(_:)`.
    pub fn finish_block(&mut self, lg: &LGraphArena, n: LNodeId) {
        let i = nid(lg, n);
        if i >= self.block_finished.len() {
            self.block_finished.resize(i + 1, false);
        }
        self.block_finished[i] = true;
    }

    fn is_block_finished(&self, lg: &LGraphArena, n: LNodeId) -> bool {
        self.block_finished.get(nid(lg, n)).copied().unwrap_or(false)
    }

    /// `calculateThreshold(_:_:_:)`.
    pub fn calculate_threshold(&mut self, lg: &LGraphArena, bal: &mut BKAlignedLayout, old_thresh: f64, block_root: LNodeId, current_node: LNodeId) -> f64 {
        match self.kind {
            ThresholdStrategyKind::NullThresholdStrategy => {
                if bal.vdir == VDirection::UP {
                    f64::INFINITY
                } else {
                    -f64::INFINITY
                }
            }
            ThresholdStrategyKind::SimpleThresholdStrategy => {
                let is_root = block_root == current_node;
                let is_last = bal.align[nid(lg, current_node)] == Some(block_root);

                if !(is_root || is_last) {
                    return old_thresh;
                }

                let mut threshold = old_thresh;
                if is_root {
                    threshold = self.get_bound(lg, bal, block_root, true);
                }
                if threshold.is_infinite() && is_last {
                    threshold = self.get_bound(lg, bal, current_node, false);
                }
                threshold
            }
        }
    }

    /// `postProcess()`.
    pub fn post_process(&mut self, lg: &LGraphArena, bal: &mut BKAlignedLayout, ni: &NeighborhoodInformation) {
        if self.kind == ThresholdStrategyKind::NullThresholdStrategy {
            return;
        }
        while let Some(pp) = self.post_processables_queue.pop_front() {
            let pick = self.pick_edge(lg, bal, pp);
            let Some(edge) = pick.edge else { continue };

            let free_root = bal.root[nid(lg, pick.free)].unwrap_or(pick.free);
            let only_dummies = bal.od[nid(lg, free_root)];
            if !only_dummies && lg.edge_is_in_layer_edge(edge) {
                continue;
            }

            let moved = Self::process(lg, bal, ni, pick);
            if !moved {
                self.post_processables_stack.push(pick);
            }
        }

        while let Some(pp) = self.post_processables_stack.pop() {
            let _ = Self::process(lg, bal, ni, pp);
        }
    }

    /// `getOther(_:_:)`.
    pub fn get_other(lg: &LGraphArena, edge: LEdgeId, n: LNodeId) -> LNodeId {
        if lg.edge_source_node(edge) == Some(n) {
            if let Some(other) = lg.edge_target_node(edge) {
                return other;
            }
        } else if lg.edge_target_node(edge) == Some(n) {
            if let Some(other) = lg.edge_source_node(edge) {
                return other;
            }
        }
        // assertionFailure (a no-op in release builds)
        n
    }

    /// `SimpleThresholdStrategy.pickEdge(_:)`.
    pub fn pick_edge(&self, lg: &LGraphArena, bal: &BKAlignedLayout, mut pp: Postprocessable) -> Postprocessable {
        let edges: Vec<LEdgeId> = if pp.is_root {
            if bal.hdir == HDirection::RIGHT {
                lg.node_incoming_edges(pp.free)
            } else {
                lg.node_outgoing_edges(pp.free)
            }
        } else if bal.hdir == HDirection::LEFT {
            lg.node_incoming_edges(pp.free)
        } else {
            lg.node_outgoing_edges(pp.free)
        };

        let mut has_edges = false;
        for edge in edges {
            let free_root = bal.root[nid(lg, pp.free)].unwrap_or(pp.free);
            let only_dummies = bal.od[nid(lg, free_root)];
            if !only_dummies && lg.edge_is_in_layer_edge(edge) {
                continue;
            }

            // (sic: the same flag twice)
            if bal.su[nid(lg, free_root)] || bal.su[nid(lg, free_root)] {
                continue;
            }

            has_edges = true;
            let other = Self::get_other(lg, edge, pp.free);
            let other_root = bal.root[nid(lg, other)].unwrap_or(other);
            if self.is_block_finished(lg, other_root) {
                pp.has_edges = true;
                pp.edge = Some(edge);
                return pp;
            }
        }

        pp.has_edges = has_edges;
        pp.edge = None;
        pp
    }

    /// `SimpleThresholdStrategy.getBound(_:_:)`.
    pub fn get_bound(&mut self, lg: &LGraphArena, bal: &mut BKAlignedLayout, block_node: LNodeId, is_root: bool) -> f64 {
        let invalid = if bal.vdir == VDirection::UP { f64::INFINITY } else { -f64::INFINITY };
        let pick = self.pick_edge(lg, bal, Postprocessable::new(block_node, is_root));

        if pick.edge.is_none() && pick.has_edges {
            self.post_processables_queue.push_back(pick);
            return invalid;
        } else if let Some(edge) = pick.edge {
            let (Some(left), Some(right)) = (lg[edge].source, lg[edge].target) else { return invalid };

            let threshold;
            if is_root {
                let root_port = if bal.hdir == HDirection::RIGHT { right } else { left };
                let other_port = if bal.hdir == HDirection::RIGHT { left } else { right };
                let (Some(root_node), Some(other_node)) = (lg[root_port].owner, lg[other_port].owner) else { return invalid };
                let other_root = bal.root[nid(lg, other_node)].unwrap_or(other_node);
                threshold = bal.y[nid(lg, other_root)] + bal.inner_shift[nid(lg, other_node)] + lg[other_port].position.y + lg[other_port].anchor.y
                    - bal.inner_shift[nid(lg, root_node)]
                    - lg[root_port].position.y
                    - lg[root_port].anchor.y;
            } else {
                let root_port = if bal.hdir == HDirection::LEFT { right } else { left };
                let other_port = if bal.hdir == HDirection::LEFT { left } else { right };
                let (Some(root_node), Some(other_node)) = (lg[root_port].owner, lg[other_port].owner) else { return invalid };
                let other_root = bal.root[nid(lg, other_node)].unwrap_or(other_node);
                threshold = bal.y[nid(lg, other_root)] + bal.inner_shift[nid(lg, other_node)] + lg[other_port].position.y + lg[other_port].anchor.y
                    - bal.inner_shift[nid(lg, root_node)]
                    - lg[root_port].position.y
                    - lg[root_port].anchor.y;
            }

            if let Some(left_node) = lg[left].owner {
                let left_root = bal.root[nid(lg, left_node)].unwrap_or(left_node);
                bal.su[nid(lg, left_root)] = true;
            }
            if let Some(right_node) = lg[right].owner {
                let right_root = bal.root[nid(lg, right_node)].unwrap_or(right_node);
                bal.su[nid(lg, right_root)] = true;
            }
            return threshold;
        }
        invalid
    }

    /// `SimpleThresholdStrategy.process(_:)`.
    pub fn process(lg: &LGraphArena, bal: &mut BKAlignedLayout, ni: &NeighborhoodInformation, pp: Postprocessable) -> bool {
        let Some(edge) = pp.edge else { return false };

        let (Some(source), Some(target)) = (lg[edge].source, lg[edge].target) else { return false };
        let (fix, block) = if lg[source].owner == Some(pp.free) { (target, source) } else { (source, target) };

        let delta = bal.calculate_delta(lg, fix, block);
        let Some(block_node) = lg[block].owner else { return false };

        if delta > 0.0 && delta < THRESHOLD {
            let available_space = bal.check_space_above(lg, block_node, delta, ni);
            bal.shift_block(lg, block_node, -available_space);
            return available_space > 0.0;
        } else if delta < 0.0 && -delta < THRESHOLD {
            let available_space = bal.check_space_below(lg, block_node, -delta, ni);
            bal.shift_block(lg, block_node, available_space);
            return available_space > 0.0;
        }

        false
    }
}
