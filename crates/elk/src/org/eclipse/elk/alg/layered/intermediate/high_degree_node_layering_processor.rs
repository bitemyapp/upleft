//! Port of `alg/layered/intermediate/HighDegreeNodeLayeringProcessor.swift`.
//!
//! Moves the small trees hanging off high-degree nodes into new layers before
//! and after the high-degree node's layer, so that they don't widen it.
//!
//! The threshold is read `as? Int ?? 16`: the JSON importer stores
//! `"highDegreeNodes.threshold": "8"` as a `Double`, so Mermaid graphs get 16.

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Clone, Copy)]
enum EdgeSel {
    Incoming,
    Outgoing,
}

fn edges_of(lg: &LGraphArena, node: LNodeId, sel: EdgeSel) -> Vec<LEdgeId> {
    match sel {
        EdgeSel::Incoming => lg.node_incoming_edges(node),
        EdgeSel::Outgoing => lg.node_outgoing_edges(node),
    }
}

/// `HighDegreeNodeInformation`.
#[derive(Default)]
struct HighDegreeNodeInformation {
    inc_trees_max_height: i64,
    inc_tree_roots: Option<Vec<LNodeId>>,
    out_trees_max_height: i64,
    out_tree_roots: Option<Vec<LNodeId>>,
}

impl HighDegreeNodeInformation {
    fn new() -> Self {
        HighDegreeNodeInformation { inc_trees_max_height: -1, inc_tree_roots: None, out_trees_max_height: -1, out_tree_roots: None }
    }
}

#[derive(Default)]
pub struct HighDegreeNodeLayeringProcessor {
    degree_threshold: i64,
    tree_height_threshold: i64,
}

impl HighDegreeNodeLayeringProcessor {
    pub fn new() -> HighDegreeNodeLayeringProcessor {
        HighDegreeNodeLayeringProcessor::default()
    }

    fn is_high_degree_node(&self, lg: &LGraphArena, node: LNodeId) -> bool {
        Self::degree(lg, node) >= self.degree_threshold
    }

    fn calculate_information(&self, lg: &LGraphArena, hdn: LNodeId) -> HighDegreeNodeInformation {
        let mut hdni = HighDegreeNodeInformation::new();

        // check for incoming trees
        for inc_edge in lg.node_incoming_edges(hdn) {
            if lg.edge_is_self_loop(inc_edge) {
                continue;
            }
            let Some(src) = lg.edge_source_node(inc_edge) else { continue };
            if Self::has_single_connection(lg, src, EdgeSel::Outgoing) {
                let tree_height = self.is_tree_root(lg, src, EdgeSel::Outgoing, EdgeSel::Incoming);
                if tree_height == -1 {
                    continue;
                }
                hdni.inc_trees_max_height = swift::max(hdni.inc_trees_max_height, tree_height);
                hdni.inc_tree_roots.get_or_insert_with(Vec::new).push(src);
            }
        }

        // outgoing trees
        for out_edge in lg.node_outgoing_edges(hdn) {
            if lg.edge_is_self_loop(out_edge) {
                continue;
            }
            let Some(tgt) = lg.edge_target_node(out_edge) else { continue };
            if Self::has_single_connection(lg, tgt, EdgeSel::Incoming) {
                let tree_height = self.is_tree_root(lg, tgt, EdgeSel::Incoming, EdgeSel::Outgoing);
                if tree_height == -1 {
                    continue;
                }
                hdni.out_trees_max_height = swift::max(hdni.out_trees_max_height, tree_height);
                hdni.out_tree_roots.get_or_insert_with(Vec::new).push(tgt);
            }
        }

        hdni
    }

    fn move_tree(lg: &mut LGraphArena, root: LNodeId, edges_fun: EdgeSel, layers: &[LayerId]) {
        if layers.is_empty() {
            return;
        }
        lg.node_set_layer(root, Some(layers[0]));
        let sub_list = &layers[1..];
        for e in edges_of(lg, root, edges_fun) {
            let Some(other_node) = Self::other(lg, e, root) else { continue };
            Self::move_tree(lg, other_node, edges_fun, sub_list);
        }
    }

    fn degree(lg: &LGraphArena, node: LNodeId) -> i64 {
        lg[node].ports.iter().map(|&p| lg[p].incoming_edges.len() + lg[p].outgoing_edges.len()).sum::<usize>() as i64
    }

    fn has_single_connection(lg: &LGraphArena, node: LNodeId, edge_selector: EdgeSel) -> bool {
        let mut connection: Option<LNodeId> = None;
        for e in edges_of(lg, node, edge_selector) {
            let Some(other_node) = Self::other(lg, e, node) else { continue };
            match connection {
                None => connection = Some(other_node),
                Some(c) => {
                    if other_node != c {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn other(lg: &LGraphArena, edge: LEdgeId, node: LNodeId) -> Option<LNodeId> {
        if lg.edge_source_node(edge) == Some(node) {
            lg.edge_target_node(edge)
        } else {
            lg.edge_source_node(edge)
        }
    }

    /// `isTreeRoot(_:ancestorEdges:descendantEdges:)`: the tree's height, or
    /// -1 if `root` does not root a small enough tree.
    fn is_tree_root(&self, lg: &LGraphArena, root: LNodeId, ancestor_edges: EdgeSel, descendant_edges: EdgeSel) -> i64 {
        // exclude high degree nodes themselves
        if self.is_high_degree_node(lg, root) {
            return -1;
        }
        // does the node have exactly one parent?
        if !Self::has_single_connection(lg, root, ancestor_edges) {
            return -1;
        }
        let descendants = edges_of(lg, root, descendant_edges);
        // is it a leaf?
        if descendants.is_empty() {
            return 1;
        }
        // recursively check subtrees
        let mut current_height = 0;
        for e in descendants {
            let Some(other_node) = Self::other(lg, e, root) else { return -1 };
            let height = self.is_tree_root(lg, other_node, ancestor_edges, descendant_edges);
            if height == -1 {
                return -1;
            }
            current_height = swift::max(current_height, height);
            if current_height > self.tree_height_threshold - 1 {
                return -1;
            }
        }
        current_height + 1
    }
}

impl ILayoutProcessor for HighDegreeNodeLayeringProcessor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {
        self.degree_threshold = lg[graph].props.get_as::<i64>(&LayeredOptions::HIGH_DEGREE_NODES_THRESHOLD).unwrap_or(16);
        self.tree_height_threshold = lg[graph].props.get_as::<i64>(&LayeredOptions::HIGH_DEGREE_NODES_TREE_HEIGHT).unwrap_or(0);
        if self.tree_height_threshold == 0 {
            self.tree_height_threshold = i64::MAX;
        }

        // Iterate through all layers using index-based iteration since we insert layers
        let mut layer_index = 0usize;
        while layer_index < lg[graph].layers.len() {
            let lay = lg[graph].layers[layer_index];

            // #1 find high degree nodes and their incoming/outgoing trees
            let mut high_degree_nodes: Vec<(LNodeId, HighDegreeNodeInformation)> = Vec::new();
            let mut inc_max: i64 = -1;
            let mut out_max: i64 = -1;
            for &n in &lg[lay].nodes {
                if self.is_high_degree_node(lg, n) {
                    let hdni = self.calculate_information(lg, n);
                    inc_max = swift::max(inc_max, hdni.inc_trees_max_height);
                    out_max = swift::max(out_max, hdni.out_trees_max_height);
                    high_degree_nodes.push((n, hdni));
                }
            }

            // #2 insert layers before the current layer and move the trees
            let mut pre_layers: Vec<LayerId> = Vec::new();
            for _ in 0..swift::max(0, inc_max) {
                let l = lg.new_layer(graph);
                lg[graph].layers.insert(layer_index, l);
                pre_layers.insert(0, l);
                layer_index += 1; // current layer shifted right
            }
            for (_, hdni) in &high_degree_nodes {
                let Some(inc_roots) = &hdni.inc_tree_roots else { continue };
                for &inc_root in inc_roots {
                    Self::move_tree(lg, inc_root, EdgeSel::Incoming, &pre_layers);
                }
            }

            // #3 insert layers after the current layer and move the trees
            let mut after_layers: Vec<LayerId> = Vec::new();
            for _ in 0..swift::max(0, out_max) {
                let l = lg.new_layer(graph);
                let insert_index = layer_index + 1 + after_layers.len();
                lg[graph].layers.insert(insert_index, l);
                after_layers.push(l);
            }
            for (_, hdni) in &high_degree_nodes {
                let Some(out_roots) = &hdni.out_tree_roots else { continue };
                for &out_root in out_roots {
                    Self::move_tree(lg, out_root, EdgeSel::Outgoing, &after_layers);
                }
            }

            layer_index += 1 + after_layers.len();
        }

        // Remove empty layers
        let layers = std::mem::take(&mut lg[graph].layers);
        let kept: Vec<LayerId> = layers.into_iter().filter(|&l| !lg[l].nodes.is_empty()).collect();
        lg[graph].layers = kept;
    }

    fn name(&self) -> &'static str {
        "HighDegreeNodeLayeringProcessor"
    }
}
