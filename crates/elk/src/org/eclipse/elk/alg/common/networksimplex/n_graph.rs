//! Port of `alg/common/networksimplex/NGraph.swift`.
//!
//! The graph the network simplex algorithm works on. Besides the Swift
//! `nodes` list it owns the stores every `NNode` and `NEdge` of the graph live
//! in (Swift keeps them alive by reference instead); a node removed from
//! `nodes` stays in the store, as a removed Swift object stays alive while
//! referenced.

use std::collections::VecDeque;
use std::ops::{Index, IndexMut};

use super::n_edge::{NEdge, NEdgeId};
use super::n_node::{NNode, NNodeId};
use crate::swift;

#[derive(Clone, Debug, Default)]
pub struct NGraph {
    /// The nodes of the network simplex graph.
    pub nodes: Vec<NNodeId>,
    /// Every node ever created for this graph.
    pub node_store: Vec<NNode>,
    /// Every edge ever created for this graph.
    pub edge_store: Vec<NEdge>,
}

impl Index<NNodeId> for NGraph {
    type Output = NNode;
    #[inline]
    fn index(&self, id: NNodeId) -> &NNode {
        &self.node_store[id.0 as usize]
    }
}

impl IndexMut<NNodeId> for NGraph {
    #[inline]
    fn index_mut(&mut self, id: NNodeId) -> &mut NNode {
        &mut self.node_store[id.0 as usize]
    }
}

impl Index<NEdgeId> for NGraph {
    type Output = NEdge;
    #[inline]
    fn index(&self, id: NEdgeId) -> &NEdge {
        &self.edge_store[id.0 as usize]
    }
}

impl IndexMut<NEdgeId> for NGraph {
    #[inline]
    fn index_mut(&mut self, id: NEdgeId) -> &mut NEdge {
        &mut self.edge_store[id.0 as usize]
    }
}

impl NGraph {
    pub fn new() -> NGraph {
        NGraph::default()
    }

    /// `writeDebugGraph(filePath:)`: not supported in elk-swift either.
    pub fn write_debug_graph(&self, _file_path: &str) {}

    /// `makeConnected()`: connects one representative per connected component
    /// to a new artificial root, if there is more than one component.
    pub fn make_connected(&mut self) -> Option<NNodeId> {
        let mut id = 0;
        for i in 0..self.nodes.len() {
            let n = self.nodes[i];
            self[n].internal_id = id;
            id += 1;
        }
        let cc_rep = self.find_con_comp_representatives();
        let mut root = None;
        if cc_rep.len() > 1 {
            root = Some(self.create_artificial_root_and_connect(&cc_rep));
        }
        root
    }

    /// `createArtificialRootAndConnect(nodesToConnect:)`.
    pub fn create_artificial_root_and_connect(&mut self, nodes_to_connect: &[NNodeId]) -> NNodeId {
        let root = NNode::of().create(self);
        for &src in nodes_to_connect {
            NEdge::of().delta(0).weight(0.0).source(root).target(src).create(self);
        }
        root
    }

    /// `findConCompRepresentatives()`.
    pub fn find_con_comp_representatives(&self) -> Vec<NNodeId> {
        let mut cc_rep = Vec::new();
        let mut mark = vec![false; self.nodes.len()];
        for &node in &self.nodes {
            if !mark[self[node].internal_id] {
                cc_rep.push(node);
                self.dfs(node, &mut mark);
            }
        }
        cc_rep
    }

    /// `dfs(node:mark:)`, with an explicit stack (same visiting order as the
    /// Swift recursion: each node's connected edges in order, a neighbour's
    /// mark checked when its edge is reached).
    pub fn dfs(&self, node: NNodeId, mark: &mut [bool]) {
        if mark[self[node].internal_id] {
            return;
        }
        mark[self[node].internal_id] = true;
        let mut stack: Vec<(NNodeId, usize)> = vec![(node, 0)];
        while let Some(top) = stack.last_mut() {
            let (n, i) = *top;
            if i >= self[n].connected_edge_count() {
                stack.pop();
                continue;
            }
            top.1 += 1;
            let edge = self[n].connected_edge(i);
            let other = self[edge].get_other(n);
            if !mark[self[other].internal_id] {
                mark[self[other].internal_id] = true;
                stack.push((other, 0));
            }
        }
    }

    /// `isAcyclic()`: a topological numbering, then a check for backward edges.
    pub fn is_acyclic(&mut self) -> bool {
        let mut id = 0;
        for i in 0..self.nodes.len() {
            let n = self.nodes[i];
            self[n].internal_id = id;
            id += 1;
        }

        let mut incident = vec![0i64; self.nodes.len()];
        let mut layer = vec![0i64; self.nodes.len()];
        for &node in &self.nodes {
            incident[self[node].internal_id] += self[node].incoming_edges.len() as i64;
        }

        let mut roots: VecDeque<NNodeId> = VecDeque::new();
        for &node in &self.nodes {
            if self[node].incoming_edges.is_empty() {
                roots.push_back(node);
            }
        }
        if roots.is_empty() && !self.nodes.is_empty() {
            return false;
        }
        while let Some(node) = roots.pop_front() {
            for &edge in &self[node].outgoing_edges {
                let Some(tgt) = self[edge].target else { continue };
                let t = self[tgt].internal_id;
                layer[t] = swift::max(layer[t], layer[self[node].internal_id] + 1);
                incident[t] -= 1;
                if incident[t] == 0 {
                    roots.push_back(tgt);
                }
            }
        }

        for &node in &self.nodes {
            for &edge in &self[node].outgoing_edges {
                let (Some(tgt), Some(src)) = (self[edge].target, self[edge].source) else { continue };
                if layer[self[tgt].internal_id] <= layer[self[src].internal_id] {
                    return false;
                }
            }
        }
        true
    }
}
