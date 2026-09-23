//! Port of `alg/common/networksimplex/NetworkSimplex.swift`.
//!
//! The network simplex layering of Gansner, Koutsofios, North and Vo, "A
//! technique for drawing directed graphs" (IEEE TSE 19(3), 1993): an optimal
//! layering of an acyclic graph with respect to the weighted edge lengths.
//!
//! The Swift recursions (`tightTreeDFS`, `postorderTraversal`) are explicit
//! stacks here with the same visiting order, so deep graphs cannot overflow
//! the thread stack.

use std::collections::VecDeque;

use super::n_edge::{remove_first, NEdgeId};
use super::n_graph::NGraph;
use super::n_node::NNodeId;
use crate::org::eclipse::elk::core::util::basic_progress_monitor::BasicProgressMonitor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::swift;

/// `treeEdges`, a Swift `Set<NEdge>` hashed by object address.
///
/// NONDETERMINISTIC IN SWIFT: `leaveEdge()` returns the *first* tree edge
/// with a negative cut value in the set's iteration order, which for a Swift
/// `Set` of address-hashed objects differs from run to run. ELK's Java uses a
/// `LinkedHashSet` here; the port iterates in insertion order (an insert of a
/// present edge keeps its place, a removed and re-inserted edge goes to the
/// end), which is what the instrumented lab reproduces and the most frequent
/// Swift outcome.
#[derive(Clone, Debug, Default)]
pub struct TreeEdgeSet {
    items: Vec<NEdgeId>,
    /// Membership, indexed by the edge's store index.
    member: Vec<bool>,
}

impl TreeEdgeSet {
    fn with_capacity(edge_store_len: usize) -> TreeEdgeSet {
        TreeEdgeSet { items: Vec::new(), member: vec![false; edge_store_len] }
    }

    pub fn insert(&mut self, e: NEdgeId) {
        if !self.member[e.index()] {
            self.member[e.index()] = true;
            self.items.push(e);
        }
    }

    pub fn remove(&mut self, e: NEdgeId) {
        if self.member[e.index()] {
            self.member[e.index()] = false;
            remove_first(&mut self.items, e);
        }
    }

    pub fn contains(&self, e: NEdgeId) -> bool {
        self.member.get(e.index()).copied().unwrap_or(false)
    }

    pub fn iter(&self) -> impl Iterator<Item = NEdgeId> + '_ {
        self.items.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

pub struct NetworkSimplex<'a> {
    /// Node counts per layer of a previous layering, considered by
    /// `normalize()` and `balance(_:)`.
    pub previous_layering_node_counts: Option<Vec<i64>>,
    /// Whether to apply `balance(_:)`.
    pub balance: bool,
    /// A limit on the number of iterations.
    pub iteration_limit: i64,

    /// The graph all methods operate on.
    pub graph: &'a mut NGraph,
    /// All edges of the graph.
    pub edges: Option<Vec<NEdgeId>>,
    /// The edges of the spanning tree.
    pub tree_edges: Option<TreeEdgeSet>,
    /// All nodes without incoming edges.
    pub sources: Option<Vec<NNodeId>>,
    /// Whether an edge was visited during a DFS traversal.
    pub edge_visited: Option<Vec<bool>>,
    /// The current postorder traversal number.
    pub post_order: i64,
    /// The postorder traversal id of each node.
    pub po_id: Option<Vec<i64>>,
    /// The lowest postorder id reachable below each node.
    pub lowest_po_id: Option<Vec<i64>>,
    /// The cut value of every edge.
    pub cutvalue: Option<Vec<f64>>,
    /// Nodes (with their single edge) removed as parts of subtrees.
    pub subtree_nodes_stack: Option<Vec<(NNodeId, NEdgeId)>>,
}

impl<'a> NetworkSimplex<'a> {
    /// Empirically determined threshold when removing subtrees pays off.
    pub const REMOVE_SUBTREES_THRESH: usize = 40;

    /// Small value below zero, used for cut value comparisons.
    pub const FUZZY_ST_ZERO: f64 = -1e-10;

    /// `NetworkSimplex.forGraph(_:)`.
    pub fn for_graph(graph: &'a mut NGraph) -> NetworkSimplex<'a> {
        NetworkSimplex {
            previous_layering_node_counts: None,
            balance: false,
            iteration_limit: i64::MAX,
            graph,
            edges: None,
            tree_edges: None,
            sources: None,
            edge_visited: None,
            post_order: 0,
            po_id: None,
            lowest_po_id: None,
            cutvalue: None,
            subtree_nodes_stack: None,
        }
    }

    /// `withBalancing(_:)`.
    pub fn with_balancing(mut self, do_balance: bool) -> Self {
        self.balance = do_balance;
        self
    }

    /// `withPreviousLayering(_:)`.
    pub fn with_previous_layering(mut self, consider_previous_layering: Option<Vec<i64>>) -> Self {
        self.previous_layering_node_counts = consider_previous_layering;
        self
    }

    /// `withIterationLimit(_:)`.
    pub fn with_iteration_limit(mut self, limit: i64) -> Self {
        self.iteration_limit = limit;
        self
    }

    /// `initialize()`: indexes nodes and edges, collects the sources.
    pub fn initialize(&mut self) {
        let graph = &mut *self.graph;
        let num_nodes = graph.nodes.len();
        for i in 0..num_nodes {
            let n = graph.nodes[i];
            graph[n].tree_node = false;
        }
        self.po_id = Some(vec![0; num_nodes]);
        self.lowest_po_id = Some(vec![0; num_nodes]);
        let mut sources = Vec::new();

        let mut index = 0;
        let mut the_edges: Vec<NEdgeId> = Vec::new();
        for i in 0..num_nodes {
            let node = graph.nodes[i];
            graph[node].internal_id = index;
            index += 1;
            if graph[node].incoming_edges.is_empty() {
                sources.push(node);
            }
            the_edges.extend_from_slice(&graph[node].outgoing_edges);
        }
        let mut counter = 0;
        for &edge in &the_edges {
            graph[edge].internal_id = counter;
            graph[edge].tree_edge = false;
            counter += 1;
        }
        let num_edges = the_edges.len();
        if self.cutvalue.as_ref().is_some_and(|cv| cv.len() >= num_edges) {
            self.edge_visited = Some(vec![false; num_edges]);
        } else {
            self.cutvalue = Some(vec![0.0; num_edges]);
            self.edge_visited = Some(vec![false; num_edges]);
        }
        self.edges = Some(the_edges);
        self.tree_edges = Some(TreeEdgeSet::with_capacity(graph.edge_store.len()));
        self.sources = Some(sources);
        self.post_order = 1;
    }

    /// `dispose()`.
    pub fn dispose(&mut self) {
        self.cutvalue = None;
        self.edges = None;
        self.tree_edges = None;
        self.edge_visited = None;
        self.lowest_po_id = None;
        self.po_id = None;
        self.sources = None;
        self.subtree_nodes_stack = None;
    }

    /// `execute()` (with a fresh `BasicProgressMonitor`).
    pub fn execute_default(&mut self) {
        let mut monitor = BasicProgressMonitor::new();
        self.execute(&mut monitor);
    }

    /// `execute(_:)`: determines the optimal layering (in `NNode.layer`).
    pub fn execute(&mut self, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Network simplex", 1.0);

        if self.graph.nodes.len() < 1 {
            monitor.done();
            return;
        }

        for i in 0..self.graph.nodes.len() {
            let node = self.graph.nodes[i];
            self.graph[node].layer = 0;
        }

        let should_remove_subtrees = self.graph.nodes.len() >= Self::REMOVE_SUBTREES_THRESH;
        if should_remove_subtrees {
            self.remove_subtrees();
        }

        self.initialize();
        self.feasible_tree();
        let mut e = self.leave_edge();
        let mut iter: i64 = 0;
        while let Some(current_edge) = e {
            if !(iter < self.iteration_limit) {
                break;
            }
            let enter = self.enter_edge(current_edge);
            self.exchange(current_edge, enter);
            e = self.leave_edge();
            iter += 1;
        }

        if should_remove_subtrees {
            self.reattach_subtrees();
        }

        if self.balance {
            let filling = self.normalize();
            self.balance(filling);
        } else {
            self.normalize();
        }

        self.dispose();
        monitor.done();
    }

    /// `removeSubtrees()`: removes leaves until none are left.
    pub fn remove_subtrees(&mut self) {
        let graph = &mut *self.graph;
        let mut stack: Vec<(NNodeId, NEdgeId)> = Vec::new();

        let mut leafs: VecDeque<NNodeId> = VecDeque::new();
        for &node in &graph.nodes {
            if graph[node].connected_edge_count() == 1 {
                leafs.push_back(node);
            }
        }

        while let Some(node) = leafs.pop_front() {
            // was the edge already removed?
            if graph[node].connected_edge_count() == 0 {
                continue;
            }
            let edge = graph[node].connected_edge(0);
            let is_out_edge = !graph[node].outgoing_edges.is_empty();

            let other = graph[edge].get_other(node);
            if is_out_edge {
                remove_first(&mut graph[other].incoming_edges, edge);
            } else {
                remove_first(&mut graph[other].outgoing_edges, edge);
            }

            if graph[other].connected_edge_count() == 1 {
                leafs.push_back(other);
            }

            stack.push((node, edge));
            if let Some(index) = graph.nodes.iter().position(|&n| n == node) {
                graph.nodes.remove(index);
            }
        }
        self.subtree_nodes_stack = Some(stack);
    }

    /// `reattachSubtrees()`: in the opposite order of removal.
    pub fn reattach_subtrees(&mut self) {
        let Some(mut stack) = self.subtree_nodes_stack.take() else { return };
        let graph = &mut *self.graph;
        while let Some((node, edge)) = stack.pop() {
            let placed = graph[edge].get_other(node);
            let delta = graph[edge].delta;
            if graph[edge].target == Some(node) {
                graph[placed].outgoing_edges.push(edge);
                graph[node].layer = graph[placed].layer + delta;
            } else {
                graph[placed].incoming_edges.push(edge);
                graph[node].layer = graph[placed].layer - delta;
            }
            graph.nodes.push(node);
        }
        self.subtree_nodes_stack = Some(stack);
    }

    /// `feasibleTree()`: an initial tight spanning tree and its cut values.
    pub fn feasible_tree(&mut self) {
        let (Some(sources), Some(edge_visited_len), Some(edge_count)) =
            (self.sources.clone(), self.edge_visited.as_ref().map(|v| v.len()), self.edges.as_ref().map(|e| e.len()))
        else {
            return;
        };
        self.layering_topological_numbering(&sources);

        if edge_count > 0 {
            self.edge_visited = Some(vec![false; edge_visited_len]);
            while self.tight_tree_dfs(self.graph.nodes[0]) < self.graph.nodes.len() {
                // some nodes are still not part of the tree
                let Some(e) = self.minimal_slack() else { break };
                let (Some(e_tgt), Some(e_src)) = (self.graph[e].target, self.graph[e].source) else { break };
                let slack = self.graph[e_tgt].layer - self.graph[e_src].layer - self.graph[e].delta;
                let actual_slack = if self.graph[e_tgt].tree_node { -slack } else { slack };

                for i in 0..self.graph.nodes.len() {
                    let node = self.graph.nodes[i];
                    if self.graph[node].tree_node {
                        self.graph[node].layer += actual_slack;
                    }
                }
                self.reset_edge_visited();
            }
            self.reset_edge_visited();
            let first = self.graph.nodes[0];
            self.postorder_traversal(first);
            self.cutvalues();
        }
    }

    fn reset_edge_visited(&mut self) {
        let len = self.edge_visited.as_ref().map_or(0, |v| v.len());
        self.edge_visited = Some(vec![false; len]);
    }

    /// `layeringTopologicalNumbering(_:)`: a minimal topological numbering
    /// along outgoing edges from the given roots.
    pub fn layering_topological_numbering(&mut self, initial_root_nodes: &[NNodeId]) {
        let graph = &mut *self.graph;
        let mut incident = vec![0i64; graph.nodes.len()];
        for &node in &graph.nodes {
            incident[graph[node].internal_id] += graph[node].incoming_edges.len() as i64;
        }

        let mut roots: VecDeque<NNodeId> = initial_root_nodes.iter().copied().collect();
        while let Some(node) = roots.pop_front() {
            for i in 0..graph[node].outgoing_edges.len() {
                let edge = graph[node].outgoing_edges[i];
                let Some(tgt) = graph[edge].target else { continue };
                graph[tgt].layer = swift::max(graph[tgt].layer, graph[node].layer + graph[edge].delta);
                let t = graph[tgt].internal_id;
                incident[t] -= 1;
                if incident[t] == 0 {
                    roots.push_back(tgt);
                }
            }
        }
    }

    /// `minimalSpan(_:)`: the lengths of the shortest incoming and outgoing
    /// edges (`-1` if none). As in Swift, an incoming edge not shorter than
    /// the current incoming minimum is compared against the outgoing minimum.
    pub fn minimal_span(&self, node: NNodeId) -> (i64, i64) {
        let mut min_span_out = i64::MAX;
        let mut min_span_in = i64::MAX;
        let graph = &*self.graph;
        for i in 0..graph[node].connected_edge_count() {
            let edge = graph[node].connected_edge(i);
            let (Some(tgt), Some(src)) = (graph[edge].target, graph[edge].source) else { continue };
            let current_span = graph[tgt].layer - graph[src].layer;
            if tgt == node && current_span < min_span_in {
                min_span_in = current_span;
            } else if current_span < min_span_out {
                min_span_out = current_span;
            }
        }
        if min_span_in == i64::MAX {
            min_span_in = -1;
        }
        if min_span_out == i64::MAX {
            min_span_out = -1;
        }
        (min_span_in, min_span_out)
    }

    /// `tightTreeDFS(_:)`: grows the tight tree from `root` along tree and
    /// tight edges; returns the number of nodes reached.
    pub fn tight_tree_dfs(&mut self, root: NNodeId) -> usize {
        let graph = &mut *self.graph;
        let edge_visited = self.edge_visited.as_mut().expect("edgeVisited");
        let mut tree_edges = self.tree_edges.as_mut();
        // (node, next connected-edge index, node count so far)
        let mut stack: Vec<(NNodeId, usize, usize)> = vec![(root, 0, 1)];
        graph[root].tree_node = true;
        loop {
            let top = stack.last_mut().unwrap();
            let (node, i) = (top.0, top.1);
            if i >= graph[node].connected_edge_count() {
                let (_, _, count) = stack.pop().unwrap();
                match stack.last_mut() {
                    Some(parent) => parent.2 += count,
                    None => return count,
                }
                continue;
            }
            top.1 += 1;
            let edge = graph[node].connected_edge(i);
            let ei = graph[edge].internal_id;
            if edge_visited[ei] {
                continue;
            }
            edge_visited[ei] = true;
            let opposite = graph[edge].get_other(node);
            if graph[edge].tree_edge {
                graph[opposite].tree_node = true;
                stack.push((opposite, 0, 1));
            } else if let (Some(tgt), Some(src)) = (graph[edge].target, graph[edge].source) {
                if !graph[opposite].tree_node && graph[edge].delta == graph[tgt].layer - graph[src].layer {
                    graph[edge].tree_edge = true;
                    if let Some(te) = tree_edges.as_mut() {
                        te.insert(edge);
                    }
                    graph[opposite].tree_node = true;
                    stack.push((opposite, 0, 1));
                }
            }
        }
    }

    /// `minimalSlack()`: the non-tree edge between tree and non-tree nodes
    /// with the least slack (the first one on ties).
    pub fn minimal_slack(&self) -> Option<NEdgeId> {
        let edges = self.edges.as_ref()?;
        let graph = &*self.graph;
        let mut min_slack = i64::MAX;
        let mut min_slack_edge = None;
        for &edge in edges {
            let (Some(src), Some(tgt)) = (graph[edge].source, graph[edge].target) else { continue };
            if graph[src].tree_node != graph[tgt].tree_node {
                let cur_slack = graph[tgt].layer - graph[src].layer - graph[edge].delta;
                if cur_slack < min_slack {
                    min_slack = cur_slack;
                    min_slack_edge = Some(edge);
                }
            }
        }
        min_slack_edge
    }

    /// `postorderTraversal(_:)`: assigns postorder ids along tree edges;
    /// returns the lowest id below `root`.
    pub fn postorder_traversal(&mut self, root: NNodeId) -> i64 {
        let graph = &*self.graph;
        let edge_visited = self.edge_visited.as_mut().expect("edgeVisited");
        // (node, next connected-edge index, lowest so far)
        let mut stack: Vec<(NNodeId, usize, i64)> = vec![(root, 0, i64::MAX)];
        loop {
            let top = stack.last_mut().unwrap();
            let (node, i) = (top.0, top.1);
            if i >= graph[node].connected_edge_count() {
                let (_, _, lowest) = stack.pop().unwrap();
                let id = graph[node].internal_id;
                if let Some(po) = self.po_id.as_mut() {
                    po[id] = self.post_order;
                }
                let lpid = swift::min(lowest, self.post_order);
                if let Some(lpo) = self.lowest_po_id.as_mut() {
                    lpo[id] = lpid;
                }
                self.post_order += 1;
                match stack.last_mut() {
                    Some(parent) => parent.2 = swift::min(parent.2, lpid),
                    None => return lpid,
                }
                continue;
            }
            top.1 += 1;
            let edge = graph[node].connected_edge(i);
            let ei = graph[edge].internal_id;
            if !(graph[edge].tree_edge && !edge_visited[ei]) {
                continue;
            }
            edge_visited[ei] = true;
            stack.push((graph[edge].get_other(node), 0, i64::MAX));
        }
    }

    /// `isInHead(_:_:)`: whether `node` is in the head component of the
    /// tree edge `edge`.
    pub fn is_in_head(&self, node: NNodeId, edge: NEdgeId) -> bool {
        let graph = &*self.graph;
        let (Some(source), Some(target), Some(po_id), Some(lowest_po_id)) =
            (graph[edge].source, graph[edge].target, self.po_id.as_ref(), self.lowest_po_id.as_ref())
        else {
            return false;
        };
        let s = graph[source].internal_id;
        let t = graph[target].internal_id;
        let n = graph[node].internal_id;

        if lowest_po_id[s] <= po_id[n] && po_id[n] <= po_id[s] && lowest_po_id[t] <= po_id[n] && po_id[n] <= po_id[t] {
            if po_id[s] < po_id[t] {
                return false;
            }
            return true;
        }
        if po_id[s] < po_id[t] {
            return true;
        }
        false
    }

    /// `cutvalues()`: the cut value of every tree edge, from the leaves in.
    pub fn cutvalues(&mut self) {
        let graph = &mut *self.graph;
        let mut leafs: Vec<NNodeId> = Vec::new();
        for i in 0..graph.nodes.len() {
            let node = graph.nodes[i];
            let mut tree_edge_count = 0;
            let mut unknown = std::mem::take(&mut graph[node].unknown_cutvalues);
            unknown.clear();
            for j in 0..graph[node].connected_edge_count() {
                let edge = graph[node].connected_edge(j);
                if graph[edge].tree_edge {
                    unknown.push(edge);
                    tree_edge_count += 1;
                }
            }
            graph[node].unknown_cutvalues = unknown;
            if tree_edge_count == 1 {
                leafs.push(node);
            }
        }

        let Some(mut cv) = self.cutvalue.take() else { return };
        for &leaf_node in &leafs {
            let mut current_node = leaf_node;
            while graph[current_node].unknown_cutvalues.len() == 1 {
                let to_determine = graph[current_node].unknown_cutvalues[0];
                let td = graph[to_determine].internal_id;
                cv[td] = graph[to_determine].weight;
                let (Some(src), Some(tgt)) = (graph[to_determine].source, graph[to_determine].target) else { break };
                for j in 0..graph[current_node].connected_edge_count() {
                    let edge = graph[current_node].connected_edge(j);
                    if edge != to_determine {
                        let e = &graph[edge];
                        if e.tree_edge {
                            if Some(src) == e.source || Some(tgt) == e.target {
                                cv[td] -= cv[e.internal_id] - e.weight;
                            } else {
                                cv[td] += cv[e.internal_id] - e.weight;
                            }
                        } else if current_node == src {
                            if e.source == Some(current_node) {
                                cv[td] += e.weight;
                            } else {
                                cv[td] -= e.weight;
                            }
                        } else if e.source == Some(current_node) {
                            cv[td] -= e.weight;
                        } else {
                            cv[td] += e.weight;
                        }
                    }
                }

                if let Some(index) = graph[src].unknown_cutvalues.iter().position(|&e| e == to_determine) {
                    graph[src].unknown_cutvalues.remove(index);
                }
                if let Some(index) = graph[tgt].unknown_cutvalues.iter().position(|&e| e == to_determine) {
                    graph[tgt].unknown_cutvalues.remove(index);
                }

                if src == current_node {
                    let Some(next_node) = graph[to_determine].target else { break };
                    current_node = next_node;
                } else {
                    let Some(next_node) = graph[to_determine].source else { break };
                    current_node = next_node;
                }
            }
        }
        self.cutvalue = Some(cv);
    }

    /// `leaveEdge()`: a tree edge with a negative cut value, or `None` if the
    /// layering is optimal. See [`TreeEdgeSet`] for the iteration order.
    pub fn leave_edge(&self) -> Option<NEdgeId> {
        let (Some(tree_edges), Some(cutvalue)) = (self.tree_edges.as_ref(), self.cutvalue.as_ref()) else { return None };
        for edge in tree_edges.iter() {
            let e = &self.graph[edge];
            if e.tree_edge && cutvalue[e.internal_id] < Self::FUZZY_ST_ZERO {
                return Some(edge);
            }
        }
        None
    }

    /// `enterEdge(_:)`: the non-tree edge from the head to the tail component
    /// of `leave` with the least slack (`leave` itself if none, where Swift
    /// asserts).
    pub fn enter_edge(&self, leave: NEdgeId) -> NEdgeId {
        let Some(edges) = self.edges.as_ref() else { return leave };
        let graph = &*self.graph;
        let mut replace = None;
        let mut rep_slack = i64::MAX;
        for &edge in edges {
            let (Some(src), Some(tgt)) = (graph[edge].source, graph[edge].target) else { continue };
            if self.is_in_head(src, leave) && !self.is_in_head(tgt, leave) {
                let slack = graph[tgt].layer - graph[src].layer - graph[edge].delta;
                if slack < rep_slack {
                    rep_slack = slack;
                    replace = Some(edge);
                }
            }
        }
        replace.unwrap_or(leave)
    }

    /// `exchange(leave:enter:)`: replaces the tree edge `leave` by `enter`,
    /// shifts the tail component, and recomputes the tree values.
    pub fn exchange(&mut self, leave: NEdgeId, enter: NEdgeId) {
        if !self.graph[leave].tree_edge {
            return;
        }
        if self.graph[enter].tree_edge {
            return;
        }
        let (Some(enter_tgt), Some(enter_src)) = (self.graph[enter].target, self.graph[enter].source) else { return };

        self.graph[leave].tree_edge = false;
        if let Some(te) = self.tree_edges.as_mut() {
            te.remove(leave);
        }
        self.graph[enter].tree_edge = true;
        if let Some(te) = self.tree_edges.as_mut() {
            te.insert(enter);
        }
        let mut delta = self.graph[enter_tgt].layer - self.graph[enter_src].layer - self.graph[enter].delta;
        if !self.is_in_head(enter_tgt, leave) {
            delta = -delta;
        }
        for i in 0..self.graph.nodes.len() {
            let node = self.graph.nodes[i];
            if !self.is_in_head(node, leave) {
                self.graph[node].layer += delta;
            }
        }

        self.post_order = 1;
        self.reset_edge_visited();
        let first = self.graph.nodes[0];
        self.postorder_traversal(first);
        self.cutvalues();
    }

    /// `normalize()`: shifts the lowest layer to 0; returns the node count
    /// per layer (plus the previous layering's counts).
    pub fn normalize(&mut self) -> Vec<i64> {
        let graph = &mut *self.graph;
        let mut highest = i64::MIN;
        let mut lowest = i64::MAX;
        for &node in &graph.nodes {
            lowest = swift::min(lowest, graph[node].layer);
            highest = swift::max(highest, graph[node].layer);
        }
        let filling_size = highest - lowest + 1;
        let mut filling = vec![0i64; filling_size as usize];
        for i in 0..graph.nodes.len() {
            let node = graph.nodes[i];
            graph[node].layer -= lowest;
            filling[graph[node].layer as usize] += 1;
        }

        let mut layer_id = 0usize;
        if let Some(previous) = self.previous_layering_node_counts.as_ref() {
            for &node_cnt_in_layer in previous {
                if layer_id < filling.len() {
                    filling[layer_id] += node_cnt_in_layer;
                }
                layer_id += 1;
                if layer_id == filling.len() {
                    break;
                }
            }
        }
        filling
    }

    /// `balance(_:)`: moves nodes with as many incoming as outgoing edges to
    /// less filled layers within their feasible range.
    pub fn balance(&mut self, filling: Vec<i64>) {
        let mut mutable_filling = filling;
        for i in 0..self.graph.nodes.len() {
            let node = self.graph.nodes[i];
            if self.graph[node].incoming_edges.len() == self.graph[node].outgoing_edges.len() {
                let layer = self.graph[node].layer;
                let mut new_layer = layer;
                let range = self.minimal_span(node);
                let lo = layer - range.0 + 1;
                let hi = layer + range.1;
                if lo < hi {
                    for l in lo..hi {
                        // A negative index traps in Swift as well.
                        if l < mutable_filling.len() as i64 && mutable_filling[swift_index(l)] < mutable_filling[swift_index(new_layer)] {
                            new_layer = l;
                        }
                    }
                }
                if mutable_filling[swift_index(new_layer)] < mutable_filling[swift_index(layer)] {
                    mutable_filling[swift_index(layer)] -= 1;
                    mutable_filling[swift_index(new_layer)] += 1;
                    self.graph[node].layer = new_layer;
                }
            }
        }
    }
}

/// A Swift `Int` used as an array index: negative values trap.
#[inline]
fn swift_index(i: i64) -> usize {
    usize::try_from(i).expect("Index out of range")
}

#[cfg(test)]
mod tests {
    //! Port of `Tests/ElkSwiftTests/NetworkSimplexTests.swift`.

    use super::*;
    use crate::org::eclipse::elk::alg::common::networksimplex::n_edge::NEdge;
    use crate::org::eclipse::elk::alg::common::networksimplex::n_node::NNode;
    use crate::org::eclipse::elk::alg::layered::graph_configurator::Random;

    fn generate_random_graph(random: &mut Random, n: i64, e: i64) -> NGraph {
        let mut graph = NGraph::new();
        for i in 0..n {
            NNode::of().id(i).create(&mut graph);
        }
        for _ in 0..e {
            let src = random.next_int_bounded(n);
            let mut tgt = random.next_int_bounded(n);
            while src == tgt {
                tgt = random.next_int_bounded(n);
            }
            let delta = random.next_int_bounded(50);
            let weight = random.next_double() * 50.0;
            let (s, t) = (graph.nodes[src as usize], graph.nodes[tgt as usize]);
            NEdge::of().delta(delta).weight(weight).source(s).target(t).create(&mut graph);
        }
        for i in 0..(n - 1) as usize {
            let delta = random.next_int_bounded(50);
            let weight = random.next_double() * 50.0;
            let (s, t) = (graph.nodes[i], graph.nodes[i + 1]);
            NEdge::of().delta(delta).weight(weight).source(s).target(t).create(&mut graph);
        }
        for k in 0..graph.nodes.len() {
            let node = graph.nodes[k];
            for edge in graph[node].outgoing_edges.clone() {
                if let (Some(s), Some(t)) = (graph[edge].source, graph[edge].target) {
                    if graph[s].id > graph[t].id {
                        graph.edge_reverse(edge);
                    }
                }
            }
        }
        graph
    }

    #[test]
    fn test_deltas() {
        let mut random = Random::with_seed(1);
        for _ in 0..5 {
            for _ in 0..5 {
                let mut graph = generate_random_graph(&mut random, 4000, 8000);
                assert!(graph.is_acyclic(), "Graph should be acyclic");
                NetworkSimplex::for_graph(&mut graph).execute(&mut BasicProgressMonitor::new());
                for &node in &graph.nodes {
                    for &edge in &graph[node].outgoing_edges {
                        let (t, s) = (graph[edge].target.unwrap(), graph[edge].source.unwrap());
                        assert!(graph[t].layer - graph[s].layer >= graph[edge].delta);
                    }
                }
            }
        }
    }

    /// The lab's `ns` experiment (see `tests/data/network_simplex_golden.txt`,
    /// produced by an instrumented elk-swift with insertion-ordered
    /// `treeEdges`): a random graph with small deltas and integer weights, so
    /// that ties (and therefore the `leaveEdge` order) matter.
    fn lab_ns(seed: i64, n: i64, e: i64, balance: bool, limit: i64) -> String {
        let mut random = Random::with_seed(seed);
        let mut graph = NGraph::new();
        for i in 0..n {
            NNode::of().id(i).create(&mut graph);
        }
        for _ in 0..e {
            let src = random.next_int_bounded(n);
            let mut tgt = random.next_int_bounded(n);
            while src == tgt {
                tgt = random.next_int_bounded(n);
            }
            let delta = random.next_int_bounded(3);
            let weight = random.next_int_bounded(4) as f64;
            let (s, t) = (graph.nodes[src as usize], graph.nodes[tgt as usize]);
            NEdge::of().delta(delta).weight(weight).source(s).target(t).create(&mut graph);
        }
        for i in 0..(n / 2) as usize {
            let delta = random.next_int_bounded(3);
            let weight = random.next_int_bounded(4) as f64;
            let (s, t) = (graph.nodes[i], graph.nodes[i + 1]);
            NEdge::of().delta(delta).weight(weight).source(s).target(t).create(&mut graph);
        }
        for k in 0..graph.nodes.len() {
            let node = graph.nodes[k];
            for edge in graph[node].outgoing_edges.clone() {
                if let (Some(s), Some(t)) = (graph[edge].source, graph[edge].target) {
                    if graph[s].id > graph[t].id {
                        graph.edge_reverse(edge);
                    }
                }
            }
        }
        NetworkSimplex::for_graph(&mut graph).with_iteration_limit(limit).with_balancing(balance).execute_default();
        graph.nodes.iter().map(|&n| format!("{}:{}", graph[n].id, graph[n].layer)).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn matches_swift_lab() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/network_simplex_golden.txt");
        let data = std::fs::read_to_string(path).unwrap();
        let mut count = 0;
        for line in data.lines() {
            let (params, expected) = line.split_once('|').unwrap();
            let p: Vec<i64> = params.split(' ').map(|x| x.parse().unwrap()).collect();
            let actual = lab_ns(p[0], p[1], p[2], p[3] == 1, p[4]);
            assert_eq!(actual, expected, "params {params}");
            count += 1;
        }
        assert_eq!(count, 96);
    }
}
