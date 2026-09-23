//! Port of `alg/layered/intermediate/CommentPostprocessor.swift`.

use crate::prelude::*;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;

#[derive(Default)]
pub struct CommentPostprocessor;

impl CommentPostprocessor {
    pub fn new() -> CommentPostprocessor {
        CommentPostprocessor
    }

    pub fn process_node(&mut self, lg: &mut LGraphArena, node: LNodeId, top_boxes: Option<&[LNodeId]>, bottom_boxes: Option<&[LNodeId]>) {
        let node_pos = lg[node].position;
        let node_size = lg[node].size;
        let margin = lg[node].margin;
        let comment_comment_spacing = lg[node].props.get_as::<f64>(&LayeredOptions::SPACING_COMMENT_COMMENT).unwrap_or(0.0);

        if let Some(top_boxes) = top_boxes.filter(|b| !b.is_empty()) {
            let mut boxes_width = comment_comment_spacing * (top_boxes.len() as f64 - 1.0);
            let mut max_height = 0.0;
            for &b in top_boxes {
                boxes_width += lg[b].size.x;
                max_height = swift::max(max_height, lg[b].size.y);
            }
            let mut x_start = node_pos.x - (boxes_width - node_size.x) / 2.0;
            let base_line = node_pos.y - margin.top + max_height;
            let anchor_inc = node_size.x / (top_boxes.len() + 1) as f64;
            let mut anchor_x = anchor_inc;
            for &b in top_boxes {
                lg[b].position.x = x_start;
                lg[b].position.y = base_line - lg[b].size.y;
                let x_start_next = x_start + lg[b].size.x + comment_comment_spacing;
                let box_port = self.get_box_port(lg, b);
                lg[box_port].position.x = lg[b].size.x / 2.0 - lg[box_port].anchor.x;
                lg[box_port].position.y = lg[b].size.y;
                if let Some(node_port) = lg[b].props.get_as::<LPortId>(&InternalProperties::COMMENT_CONN_PORT) {
                    if lg.port_degree(node_port) == 1 {
                        lg[node_port].position.x = anchor_x - lg[node_port].anchor.x;
                        lg[node_port].position.y = 0.0;
                        lg[node_port].owner = Some(node);
                    }
                }
                anchor_x += anchor_inc;
                x_start = x_start_next;
            }
        }

        if let Some(bottom_boxes) = bottom_boxes.filter(|b| !b.is_empty()) {
            let mut boxes_width = comment_comment_spacing * (bottom_boxes.len() as f64 - 1.0);
            let mut max_height = 0.0;
            for &b in bottom_boxes {
                boxes_width += lg[b].size.x;
                max_height = swift::max(max_height, lg[b].size.y);
            }
            let mut x_start = node_pos.x - (boxes_width - node_size.x) / 2.0;
            let base_line = node_pos.y + node_size.y + margin.bottom - max_height;
            let anchor_inc = node_size.x / (bottom_boxes.len() + 1) as f64;
            let mut anchor_x = anchor_inc;
            for &b in bottom_boxes {
                lg[b].position.x = x_start;
                lg[b].position.y = base_line;
                let x_start_next = x_start + lg[b].size.x + comment_comment_spacing;
                let box_port = self.get_box_port(lg, b);
                lg[box_port].position.x = lg[b].size.x / 2.0 - lg[box_port].anchor.x;
                lg[box_port].position.y = 0.0;
                if let Some(node_port) = lg[b].props.get_as::<LPortId>(&InternalProperties::COMMENT_CONN_PORT) {
                    if lg.port_degree(node_port) == 1 {
                        lg[node_port].position.x = anchor_x - lg[node_port].anchor.x;
                        lg[node_port].position.y = node_size.y;
                        lg[node_port].owner = Some(node);
                    }
                }
                anchor_x += anchor_inc;
                x_start = x_start_next;
            }
        }
    }

    /// `getBoxPort(_:)`: reconnects the box's edge (by direct assignment).
    pub fn get_box_port(&mut self, lg: &mut LGraphArena, comment_box: LNodeId) -> LPortId {
        let Some(node_port) = lg[comment_box].props.get_as::<LPortId>(&InternalProperties::COMMENT_CONN_PORT) else {
            return match lg[comment_box].ports.first() {
                Some(&p) => p,
                None => lg.new_port(),
            };
        };
        for port in lg[comment_box].ports.clone() {
            if let Some(&edge) = lg[port].outgoing_edges.first() {
                lg[edge].target = Some(node_port);
                return port;
            }
            if let Some(&edge) = lg[port].incoming_edges.first() {
                lg[edge].source = Some(node_port);
                return port;
            }
        }
        match lg[comment_box].ports.first() {
            Some(&p) => p,
            None => lg.new_port(),
        }
    }
}

impl ILayoutProcessor for CommentPostprocessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Comment post-processing", 1.0);
        for layer in lg[layered_graph].layers.clone() {
            let mut boxes: Vec<LNodeId> = Vec::new();
            for node in lg[layer].nodes.clone() {
                let top_boxes = lg[node].props.get_as::<Vec<LNodeId>>(&InternalProperties::TOP_COMMENTS);
                let bottom_boxes = lg[node].props.get_as::<Vec<LNodeId>>(&InternalProperties::BOTTOM_COMMENTS);
                if top_boxes.is_some() || bottom_boxes.is_some() {
                    self.process_node(lg, node, top_boxes.as_deref(), bottom_boxes.as_deref());
                    if let Some(t) = top_boxes {
                        boxes.extend(t);
                    }
                    if let Some(b) = bottom_boxes {
                        boxes.extend(b);
                    }
                }
            }
            lg[layer].nodes.extend(boxes);
        }
        monitor.done();
    }

    fn name(&self) -> &'static str {
        "CommentPostprocessor"
    }
}
