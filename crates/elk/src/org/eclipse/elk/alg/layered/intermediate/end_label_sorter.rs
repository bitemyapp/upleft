//! Port of `alg/layered/intermediate/EndLabelSorter.swift`.
//!
//! Sorts the end labels in each port's label cell (created by
//! `EndLabelPreprocessor`) so that labels of the same edge stay together and
//! the edges' label groups follow the edge order.

use super::end_label_preprocessor::EndLabelCells;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::label_cell::LabelCellRef;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LLabelAdapter;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use crate::prelude::*;

/// A group of labels that have a certain order and belong to a single edge.
#[derive(Clone, Debug)]
pub struct LabelGroup {
    /// The edge the labels belong to.
    pub edge: LEdgeId,
    /// List of labels that belong to this group.
    pub labels: Vec<LLabelAdapter>,
}

impl LabelGroup {
    pub fn new(edge: LEdgeId) -> LabelGroup {
        LabelGroup { edge, labels: Vec::new() }
    }
}

#[derive(Default)]
pub struct EndLabelSorter;

impl EndLabelSorter {
    pub fn new() -> EndLabelSorter {
        EndLabelSorter
    }

    // MARK: - Node Processing and Initialization

    pub fn process_node(&self, lg: &mut LGraphArena, node: LNodeId) {
        let mut initialize_method_called = false;

        let Some(label_cell_map) = lg[node].props.get_object::<EndLabelCells>(&InternalProperties::END_LABELS) else { return };

        for port in lg[node].ports.clone() {
            if self.needs_sorting(lg, port) {
                if !initialize_method_called {
                    if let Some(graph) = lg[node].graph {
                        self.initialize(lg, graph);
                    }
                    initialize_method_called = true;
                }

                if let Some(port_label_cell) = label_cell_map.get(port) {
                    self.sort(lg, port, port_label_cell);
                }
            }
        }
    }

    /// A port requires its end labels to be sorted if there are end labels of
    /// at least two edges there.
    pub fn needs_sorting(&self, lg: &LGraphArena, port: LPortId) -> bool {
        let mut edges_with_end_labels = 0;

        let has_labels_placed = |edge: LEdgeId, placement: EdgeLabelPlacement| {
            lg[edge]
                .labels
                .iter()
                .any(|&label| lg[label].props.get_as::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT) == Some(placement))
        };

        for &in_edge in &lg[port].incoming_edges {
            if has_labels_placed(in_edge, EdgeLabelPlacement::HEAD) {
                edges_with_end_labels += 1;
            }
        }

        for &out_edge in &lg[port].outgoing_edges {
            if has_labels_placed(out_edge, EdgeLabelPlacement::TAIL) {
                edges_with_end_labels += 1;
            }
        }

        edges_with_end_labels >= 2
    }

    /// Called once we find the first instance of labels that have to be
    /// sorted: assigns ids to all nodes and ports of the graph.
    pub fn initialize(&self, lg: &mut LGraphArena, l_graph: LGraphId) {
        let mut next_element_id = 0;
        for layer in lg[l_graph].layers.clone() {
            for node in lg[layer].nodes.clone() {
                lg[node].id = next_element_id;
                next_element_id += 1;

                for port in lg[node].ports.clone() {
                    lg[port].id = next_element_id;
                    next_element_id += 1;
                }
            }
        }
    }

    // MARK: - Sorting

    /// Sorts the labels of the given port which are contained in the given
    /// label cell. Labels without an `END_LABEL_EDGE` are dropped, as in Swift.
    pub fn sort(&self, lg: &LGraphArena, _port: LPortId, port_label_cell: &LabelCellRef) {
        let label_groups = self.create_label_groups(lg, &port_label_cell.borrow().labels);
        let sorted_groups = swift::sorted_by(label_groups, |g1, g2| Self::label_group_comparator(lg, g1, g2));

        // Re-add the label cell's labels in the proper order
        let mut port_label_cell_labels: Vec<LLabelAdapter> = Vec::new();
        for group in sorted_groups {
            port_label_cell_labels.extend_from_slice(&group.labels);
        }
        port_label_cell.borrow_mut().labels = port_label_cell_labels;
    }

    /// Creates a list of `LabelGroup`s that group labels from the same edge.
    pub fn create_label_groups(&self, lg: &LGraphArena, labels: &[LLabelAdapter]) -> Vec<LabelGroup> {
        // NONDETERMINISTIC IN SWIFT: the groups live in an `[LEdge: LabelGroup]`
        // dictionary and are returned as `Array(edgeToGroupMap.values)` in hash
        // (heap-address) order; the stable sort that follows only fixes the
        // order of groups the comparator distinguishes (it cannot tell apart
        // two edges between the same source port and target port). The port
        // keeps the groups in first-seen order (insertion order).
        let mut groups: Vec<LabelGroup> = Vec::new();

        // Make sure every label is contained in a label group
        for &label in labels {
            let opt_edge: Option<LEdgeId> = label.get_property(lg, &InternalProperties::END_LABEL_EDGE);
            let Some(edge) = opt_edge else { continue };

            match groups.iter_mut().find(|g| g.edge == edge) {
                Some(group) => group.labels.push(label),
                None => {
                    let mut group = LabelGroup::new(edge);
                    group.labels.push(label);
                    groups.push(group);
                }
            }
        }

        groups
    }

    /// `LABEL_GROUP_COMPARATOR` (`group1 < group2`).
    pub fn label_group_comparator(lg: &LGraphArena, group1: &LabelGroup, group2: &LabelGroup) -> bool {
        let id_of_port = |p: Option<LPortId>| p.map(|p| lg[p].id as i64).unwrap_or(0);
        let id_of_node = |n: Option<LNodeId>| n.map(|n| lg[n].id as i64).unwrap_or(0);

        let source1 = lg[group1.edge].source;
        let source2 = lg[group2.edge].source;
        let source_port_diff = id_of_port(source1) - id_of_port(source2);
        if source_port_diff != 0 {
            return source_port_diff < 0;
        }

        let target1_node = lg[group1.edge].target.and_then(|p| lg[p].owner);
        let target2_node = lg[group2.edge].target.and_then(|p| lg[p].owner);
        let target_node_diff = id_of_node(target1_node) - id_of_node(target2_node);
        if target_node_diff != 0 {
            return target_node_diff < 0;
        }

        // (Swapped in elk-swift: group2's target first.)
        let target1_id = id_of_port(lg[group2.edge].target);
        let target2_id = id_of_port(lg[group1.edge].target);
        (target1_id - target2_id) < 0
    }
}

impl ILayoutProcessor for EndLabelSorter {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Sort end labels", 1.0);

        let normal_nodes: Vec<LNodeId> = lg[layered_graph]
            .layers
            .iter()
            .flat_map(|&l| lg[l].nodes.iter().copied())
            .filter(|&n| lg[n].node_type == NodeType::NORMAL)
            .collect();
        for node in normal_nodes {
            self.process_node(lg, node);
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "EndLabelSorter"
    }
}
