//! Port of `alg/layered/intermediate/LabelSideSelector.swift`.
//!
//! Decides on which side of its edge each edge label (and label dummy node)
//! is placed, according to `LayeredOptions.EDGE_LABELS_SIDE_SELECTION`, and
//! stores the decision in `InternalProperties.LABEL_SIDE`.

use super::end_label_preprocessor::EndLabelPreprocessor;
use crate::org::eclipse::elk::alg::layered::options::edge_label_side_selection::EdgeLabelSideSelection;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::label_side::LabelSide;
use crate::prelude::*;

#[derive(Default)]
pub struct LabelSideSelector;

impl LabelSideSelector {
    pub fn new() -> LabelSideSelector {
        LabelSideSelector
    }

    // MARK: - Simple Placement Strategies

    fn same_side(&self, lg: &mut LGraphArena, graph: LGraphId, label_side: LabelSide) {
        for layer in lg[graph].layers.clone() {
            for node in lg[layer].nodes.clone() {
                if lg[node].node_type == NodeType::LABEL {
                    self.apply_label_side_to_node(lg, node, label_side);
                }
                for edge in lg.node_outgoing_edges(node) {
                    self.apply_label_side_to_edge(lg, edge, label_side);
                }
            }
        }
    }

    fn based_on_direction(&self, lg: &mut LGraphArena, graph: LGraphId, side_for_rightward_edges: LabelSide) {
        for layer in lg[graph].layers.clone() {
            for node in lg[layer].nodes.clone() {
                if lg[node].node_type == NodeType::LABEL {
                    let side = if self.does_edge_point_right_node(lg, node) {
                        side_for_rightward_edges
                    } else {
                        side_for_rightward_edges.opposite()
                    };
                    self.apply_label_side_to_node(lg, node, side);
                }
                for edge in lg.node_outgoing_edges(node) {
                    let side = if self.does_edge_point_right_edge(lg, edge) {
                        side_for_rightward_edges
                    } else {
                        side_for_rightward_edges.opposite()
                    };
                    self.apply_label_side_to_edge(lg, edge, side);
                }
            }
        }
    }

    // MARK: - Smart Placement Strategy

    fn smart(&self, lg: &mut LGraphArena, graph: LGraphId, default_side: LabelSide) {
        let mut dummy_node_queue: Vec<LNodeId> = Vec::new();

        for layer in lg[graph].layers.clone() {
            let mut top_group = true;
            let mut label_dummies_in_queue = 0;

            for node in lg[layer].nodes.clone() {
                match lg[node].node_type {
                    NodeType::LABEL => {
                        label_dummies_in_queue += 1;
                        dummy_node_queue.push(node);
                    }
                    NodeType::LONG_EDGE => {
                        dummy_node_queue.push(node);
                    }
                    NodeType::NORMAL => {
                        self.smart_for_regular_node(lg, node, default_side);
                        if !dummy_node_queue.is_empty() {
                            self.smart_for_consecutive_dummy_node_run(
                                lg,
                                &mut dummy_node_queue,
                                label_dummies_in_queue,
                                top_group,
                                false,
                                default_side,
                            );
                        }
                        top_group = false;
                        label_dummies_in_queue = 0;
                    }
                    _ => {
                        if !dummy_node_queue.is_empty() {
                            self.smart_for_consecutive_dummy_node_run(
                                lg,
                                &mut dummy_node_queue,
                                label_dummies_in_queue,
                                top_group,
                                false,
                                default_side,
                            );
                        }
                        top_group = false;
                        label_dummies_in_queue = 0;
                    }
                }
            }

            if !dummy_node_queue.is_empty() {
                self.smart_for_consecutive_dummy_node_run(
                    lg,
                    &mut dummy_node_queue,
                    label_dummies_in_queue,
                    top_group,
                    true,
                    default_side,
                );
            }
        }
    }

    fn smart_for_consecutive_dummy_node_run(
        &self,
        lg: &mut LGraphArena,
        dummy_nodes: &mut Vec<LNodeId>,
        label_dummy_count: i32,
        top_group: bool,
        bottom_group: bool,
        default_side: LabelSide,
    ) {
        let first_is_label = dummy_nodes.first().map(|&n| lg[n].node_type == NodeType::LABEL).unwrap_or(false);
        let last_is_label = dummy_nodes.last().map(|&n| lg[n].node_type == NodeType::LABEL).unwrap_or(false);

        if top_group && (!bottom_group || dummy_nodes.len() > 1) && label_dummy_count == 1 && first_is_label {
            let first = dummy_nodes[0];
            self.apply_label_side_to_node(lg, first, LabelSide::ABOVE);
        } else if bottom_group && (!top_group || dummy_nodes.len() > 1) && label_dummy_count == 1 && last_is_label {
            let last = dummy_nodes[dummy_nodes.len() - 1];
            self.apply_label_side_to_node(lg, last, LabelSide::BELOW);
        } else if dummy_nodes.len() == 2 {
            let first = dummy_nodes.remove(0);
            self.apply_label_side_to_node(lg, first, LabelSide::ABOVE);
            let second = dummy_nodes.remove(0);
            self.apply_label_side_to_node(lg, second, LabelSide::BELOW);
        } else {
            self.apply_for_dummy_node_run_with_simple_loops(lg, dummy_nodes, label_dummy_count, default_side);
        }

        dummy_nodes.clear();
    }

    fn apply_for_dummy_node_run_with_simple_loops(
        &self,
        lg: &mut LGraphArena,
        dummy_nodes: &[LNodeId],
        _label_dummy_count: i32,
        default_side: LabelSide,
    ) {
        let mut label_dummy_run: Vec<LNodeId> = Vec::new();
        let mut prev_long_edge_source: Option<LNodeId> = None;
        let mut prev_long_edge_target: Option<LNodeId> = None;

        for &current_dummy in dummy_nodes {
            let curr_long_edge_source = self.get_long_edge_end_node(lg, current_dummy, true);
            let curr_long_edge_target = self.get_long_edge_end_node(lg, current_dummy, false);

            if prev_long_edge_source != curr_long_edge_source || prev_long_edge_target != curr_long_edge_target {
                self.apply_label_sides_to_label_dummy_run(lg, &mut label_dummy_run, default_side);
                prev_long_edge_source = curr_long_edge_source;
                prev_long_edge_target = curr_long_edge_target;
            }

            label_dummy_run.push(current_dummy);
        }

        self.apply_label_sides_to_label_dummy_run(lg, &mut label_dummy_run, default_side);
    }

    fn get_long_edge_end_node(&self, lg: &LGraphArena, label_dummy: LNodeId, source: bool) -> Option<LNodeId> {
        let property = if source { &InternalProperties::LONG_EDGE_SOURCE } else { &InternalProperties::LONG_EDGE_TARGET };
        let end_port = lg[label_dummy].props.get_as::<LPortId>(property);
        end_port.and_then(|p| lg[p].owner)
    }

    fn apply_label_sides_to_label_dummy_run(&self, lg: &mut LGraphArena, label_dummy_run: &mut Vec<LNodeId>, default_side: LabelSide) {
        if !label_dummy_run.is_empty() {
            if label_dummy_run.len() == 2 {
                self.apply_label_side_to_node(lg, label_dummy_run[0], LabelSide::ABOVE);
                self.apply_label_side_to_node(lg, label_dummy_run[1], LabelSide::BELOW);
            } else {
                for &dummy_node in label_dummy_run.iter() {
                    self.apply_label_side_to_node(lg, dummy_node, default_side);
                }
            }
            label_dummy_run.clear();
        }
    }

    fn smart_for_regular_node(&self, lg: &mut LGraphArena, node: LNodeId, default_side: LabelSide) {
        let mut end_label_queue: Vec<Vec<LLabelId>> = Vec::new();
        let mut current_port_side: Option<PortSide> = None;

        for port in lg[node].ports.clone() {
            if Some(lg[port].side) != current_port_side {
                if !end_label_queue.is_empty() {
                    if let Some(side) = current_port_side {
                        self.smart_for_regular_node_port_end_labels(lg, &mut end_label_queue, side, default_side);
                    }
                }
                end_label_queue.clear();
                current_port_side = Some(lg[port].side);
            }

            // (Also records MAX_EDGE_THICKNESS and END_LABEL_EDGE, as in Swift.)
            if let Some(port_end_labels) = EndLabelPreprocessor::gather_labels(lg, port) {
                end_label_queue.push(port_end_labels);
            }
        }

        if !end_label_queue.is_empty() {
            if let Some(current_port_side) = current_port_side {
                self.smart_for_regular_node_port_end_labels(lg, &mut end_label_queue, current_port_side, default_side);
            }
        }
    }

    fn smart_for_regular_node_port_end_labels(
        &self,
        lg: &mut LGraphArena,
        end_label_queue: &mut Vec<Vec<LLabelId>>,
        port_side: PortSide,
        default_side: LabelSide,
    ) {
        if end_label_queue.len() == 2 {
            if port_side == PortSide::NORTH || port_side == PortSide::EAST {
                let first = end_label_queue.remove(0);
                self.apply_label_side_to_labels(lg, &first, LabelSide::ABOVE);
                let second = end_label_queue.remove(0);
                self.apply_label_side_to_labels(lg, &second, LabelSide::BELOW);
            } else {
                let first = end_label_queue.remove(0);
                self.apply_label_side_to_labels(lg, &first, LabelSide::BELOW);
                let second = end_label_queue.remove(0);
                self.apply_label_side_to_labels(lg, &second, LabelSide::ABOVE);
            }
        } else {
            for label_list in end_label_queue.iter() {
                self.apply_label_side_to_labels(lg, label_list, default_side);
            }
        }
    }

    // MARK: - Helper Methods

    fn apply_label_side_to_node(&self, lg: &mut LGraphArena, label_dummy: LNodeId, side: LabelSide) {
        if lg[label_dummy].node_type == NodeType::LABEL {
            let effective_side = if lg.node_is_inline_edge_label(label_dummy) { LabelSide::INLINE } else { side };

            lg[label_dummy].props.set(&InternalProperties::LABEL_SIDE, effective_side);

            if effective_side != LabelSide::BELOW {
                let Some(origin_edge) = lg[label_dummy].props.get_as::<LEdgeId>(&InternalProperties::ORIGIN) else { return };
                let thickness = lg[origin_edge].props.get_as::<f64>(&LayeredOptions::EDGE_THICKNESS).unwrap_or(0.0);

                let mut port_pos: f64 = 0.0;
                if effective_side == LabelSide::ABOVE {
                    port_pos = lg[label_dummy].size.y - (thickness / 2.0).ceil();
                } else if effective_side == LabelSide::INLINE {
                    let Some(graph) = lg.node_graph(label_dummy) else { return };
                    let edge_label_spacing = lg[graph].props.get_as::<f64>(&LayeredOptions::SPACING_EDGE_LABEL).unwrap_or(0.0);
                    port_pos = (lg[label_dummy].size.y - edge_label_spacing - thickness).ceil() / 2.0;
                    lg[label_dummy].size.y -= edge_label_spacing;
                    lg[label_dummy].size.y -= thickness;
                }

                for port in lg[label_dummy].ports.clone() {
                    lg[port].position.y = port_pos;
                }
            }
        }
    }

    fn apply_label_side_to_edge(&self, lg: &mut LGraphArena, edge: LEdgeId, side: LabelSide) {
        for label in lg[edge].labels.clone() {
            lg[label].props.set(&InternalProperties::LABEL_SIDE, side);
        }
    }

    fn apply_label_side_to_labels(&self, lg: &mut LGraphArena, labels: &[LLabelId], side: LabelSide) {
        for &label in labels {
            lg[label].props.set(&InternalProperties::LABEL_SIDE, side);
        }
    }

    fn does_edge_point_right_edge(&self, lg: &LGraphArena, edge: LEdgeId) -> bool {
        !lg[edge].props.get_as::<bool>(&InternalProperties::REVERSED).unwrap_or(false)
    }

    fn does_edge_point_right_node(&self, lg: &LGraphArena, label_dummy: LNodeId) -> bool {
        let incoming = lg.node_incoming_edges(label_dummy).first().copied();
        let outgoing = lg.node_outgoing_edges(label_dummy).first().copied();

        let in_right = incoming.map(|e| self.does_edge_point_right_edge(lg, e)).unwrap_or(false);
        let out_right = outgoing.map(|e| self.does_edge_point_right_edge(lg, e)).unwrap_or(false);
        in_right || out_right
    }
}

impl ILayoutProcessor for LabelSideSelector {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        let mode = lg[layered_graph]
            .props
            .get_as::<EdgeLabelSideSelection>(&LayeredOptions::EDGE_LABELS_SIDE_SELECTION)
            .unwrap_or(EdgeLabelSideSelection::ALWAYS_DOWN);
        monitor.begin("Label side selection", 1.0);

        match mode {
            EdgeLabelSideSelection::ALWAYS_UP => self.same_side(lg, layered_graph, LabelSide::ABOVE),
            EdgeLabelSideSelection::ALWAYS_DOWN => self.same_side(lg, layered_graph, LabelSide::BELOW),
            EdgeLabelSideSelection::DIRECTION_UP => self.based_on_direction(lg, layered_graph, LabelSide::ABOVE),
            EdgeLabelSideSelection::DIRECTION_DOWN => self.based_on_direction(lg, layered_graph, LabelSide::BELOW),
            EdgeLabelSideSelection::SMART_UP => self.smart(lg, layered_graph, LabelSide::ABOVE),
            EdgeLabelSideSelection::SMART_DOWN => self.smart(lg, layered_graph, LabelSide::BELOW),
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LabelSideSelector"
    }
}
