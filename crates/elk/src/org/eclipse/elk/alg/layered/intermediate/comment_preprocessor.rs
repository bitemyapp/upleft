//! Port of `alg/layered/intermediate/CommentPreprocessor.swift`.
//!
//! Faithful to two elk-swift quirks: the comment box lists are Swift arrays
//! (values), so `boxList.append(box)` never reaches the stored
//! `TOP_COMMENTS`/`BOTTOM_COMMENTS` properties (they stay empty); and the
//! box's edge is detached by assigning `edge.target`/`edge.source` and
//! `port.node` directly, which leaves the ports' edge lists and the node's
//! port list untouched.

use crate::prelude::*;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;

#[derive(Default)]
pub struct CommentPreprocessor;

impl CommentPreprocessor {
    pub fn new() -> CommentPreprocessor {
        CommentPreprocessor
    }

    fn is_comment(lg: &LGraphArena, node: Option<LNodeId>) -> bool {
        node.is_some_and(|n| lg[n].props.get_as::<bool>(&LayeredOptions::COMMENT_BOX).unwrap_or(false))
    }

    pub fn process_box(&mut self, lg: &mut LGraphArena, box_node: LNodeId, edge: LEdgeId, opposite_port: LPortId, real_node: LNodeId) {
        let mut top_first;
        let mut only_top = false;
        let mut only_bottom = false;
        if lg[real_node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED).is_side_fixed() {
            let mut has_north = false;
            let mut has_south = false;
            'port_loop: for &port1 in &lg[real_node].ports {
                for port2 in lg.port_connected_ports(port1) {
                    if !Self::is_comment(lg, lg[port2].owner) {
                        if lg[port1].side == PortSide::NORTH {
                            has_north = true;
                            break 'port_loop;
                        }
                        if lg[port1].side == PortSide::SOUTH {
                            has_south = true;
                            break 'port_loop;
                        }
                    }
                }
            }
            only_top = has_south && !has_north;
            only_bottom = has_north && !has_south;
        }

        if !only_top && !only_bottom && !lg[real_node].labels.is_empty() {
            let mut label_pos = 0.0;
            for &label in &lg[real_node].labels {
                label_pos += lg[label].position.y + lg[label].size.y / 2.0;
            }
            label_pos /= lg[real_node].labels.len() as f64;
            top_first = label_pos >= lg[real_node].size.y / 2.0;
        } else {
            top_first = !only_bottom;
        }
        let _ = &mut top_first;

        // `boxList` is a copy; appending to it changes no property.
        let props = &mut lg[real_node].props;
        if top_first {
            match props.get_as::<Vec<LNodeId>>(&InternalProperties::TOP_COMMENTS) {
                Some(_top) => {
                    if !only_top && props.get_as::<Vec<LNodeId>>(&InternalProperties::BOTTOM_COMMENTS).is_none() {
                        props.set(&InternalProperties::BOTTOM_COMMENTS, Vec::<LNodeId>::new());
                    }
                }
                None => props.set(&InternalProperties::TOP_COMMENTS, Vec::<LNodeId>::new()),
            }
        } else {
            match props.get_as::<Vec<LNodeId>>(&InternalProperties::BOTTOM_COMMENTS) {
                Some(_bottom) => {
                    if !only_bottom && props.get_as::<Vec<LNodeId>>(&InternalProperties::TOP_COMMENTS).is_none() {
                        props.set(&InternalProperties::TOP_COMMENTS, Vec::<LNodeId>::new());
                    }
                }
                None => props.set(&InternalProperties::BOTTOM_COMMENTS, Vec::<LNodeId>::new()),
            }
        }

        lg[box_node].props.set(&InternalProperties::COMMENT_CONN_PORT, PropValue::LPort(opposite_port));

        if lg[edge].target == Some(opposite_port) {
            lg[edge].target = None;
            if lg.port_degree(opposite_port) == 0 {
                lg[opposite_port].owner = None;
            }
            self.remove_hierarchical_port_dummy_node(lg, opposite_port);
        } else {
            lg[edge].source = None;
            if lg.port_degree(opposite_port) == 0 {
                lg[opposite_port].owner = None;
            }
        }
        lg[edge].bend_points.clear();
    }

    pub fn remove_hierarchical_port_dummy_node(&mut self, lg: &mut LGraphArena, opposite_port: LPortId) {
        if let Some(dummy) = lg[opposite_port].props.get_as::<LNodeId>(&InternalProperties::PORT_DUMMY) {
            if let Some(layer) = lg[dummy].layer {
                if let Some(index) = lg[layer].nodes.iter().position(|&n| n == dummy) {
                    lg[layer].nodes.remove(index);
                }
                if lg[layer].nodes.is_empty() {
                    if let Some(graph) = lg[dummy].graph {
                        if let Some(layer_index) = lg[graph].layers.iter().position(|&l| l == layer) {
                            lg[graph].layers.remove(layer_index);
                        }
                    }
                }
            }
        }
    }
}

impl ILayoutProcessor for CommentPreprocessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Comment pre-processing", 1.0);
        let mut nodes_to_remove: Vec<usize> = Vec::new();
        for (index, node) in lg[layered_graph].layerless_nodes.clone().into_iter().enumerate() {
            if !lg[node].props.get_as::<bool>(&LayeredOptions::COMMENT_BOX).unwrap_or(false) {
                continue;
            }
            let mut edge_count = 0;
            let mut edge: Option<LEdgeId> = None;
            let mut opposite_port: Option<LPortId> = None;
            for &port in &lg[node].ports {
                edge_count += lg.port_degree(port);
                if lg[port].incoming_edges.len() == 1 {
                    edge = Some(lg[port].incoming_edges[0]);
                    opposite_port = lg[edge.unwrap()].source;
                }
                if lg[port].outgoing_edges.len() == 1 {
                    edge = Some(lg[port].outgoing_edges[0]);
                    opposite_port = lg[edge.unwrap()].target;
                }
            }
            let real_node = opposite_port.and_then(|p| lg[p].owner);
            if let (Some(e), Some(op), Some(real)) = (edge, opposite_port, real_node) {
                if edge_count == 1 && lg.port_degree(op) == 1 && !Self::is_comment(lg, lg[op].owner) {
                    self.process_box(lg, node, e, op, real);
                    nodes_to_remove.push(index);
                    continue;
                }
            }
            let mut rev_edges: Vec<LEdgeId> = Vec::new();
            for &port in &lg[node].ports {
                for &outedge in &lg[port].outgoing_edges {
                    if !lg[outedge].target.is_none_or(|t| lg[t].outgoing_edges.is_empty()) {
                        rev_edges.push(outedge);
                    }
                }
                for &inedge in &lg[port].incoming_edges {
                    if !lg[inedge].source.is_none_or(|s| lg[s].incoming_edges.is_empty()) {
                        rev_edges.push(inedge);
                    }
                }
            }
            for re in rev_edges {
                lg.edge_reverse(re, layered_graph, true);
            }
        }
        for index in nodes_to_remove.into_iter().rev() {
            lg[layered_graph].layerless_nodes.remove(index);
        }
        monitor.done();
    }

    fn name(&self) -> &'static str {
        "CommentPreprocessor"
    }
}
