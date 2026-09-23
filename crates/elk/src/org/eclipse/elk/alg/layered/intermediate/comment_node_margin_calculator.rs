//! Port of `alg/layered/intermediate/CommentNodeMarginCalculator.swift`.

use crate::prelude::*;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;

#[derive(Default)]
pub struct CommentNodeMarginCalculator;

impl CommentNodeMarginCalculator {
    pub fn new() -> CommentNodeMarginCalculator {
        CommentNodeMarginCalculator
    }

    pub fn process_comments(&mut self, lg: &mut LGraphArena, node: LNodeId) {
        let top_boxes = lg[node].props.get_as::<Vec<LNodeId>>(&InternalProperties::TOP_COMMENTS);
        let bottom_boxes = lg[node].props.get_as::<Vec<LNodeId>>(&InternalProperties::BOTTOM_COMMENTS);
        if top_boxes.is_none() && bottom_boxes.is_none() {
            return;
        }
        let comment_comment_spacing = lg[node].props.get_as::<f64>(&LayeredOptions::SPACING_COMMENT_COMMENT).unwrap_or(0.0);
        let comment_node_spacing = lg[node].props.get_as::<f64>(&LayeredOptions::SPACING_COMMENT_NODE).unwrap_or(0.0);
        let mut margin = lg[node].margin;

        let mut top_width = 0.0;
        if let Some(top_boxes) = &top_boxes {
            let mut max_height = 0.0;
            for &b in top_boxes {
                max_height = swift::max(max_height, lg[b].size.y);
                top_width += lg[b].size.x;
            }
            top_width += comment_comment_spacing * (0i64.max(top_boxes.len() as i64 - 1)) as f64;
            margin.top += max_height + comment_node_spacing;
        }
        let mut bottom_width = 0.0;
        if let Some(bottom_boxes) = &bottom_boxes {
            let mut max_height = 0.0;
            for &b in bottom_boxes {
                max_height = swift::max(max_height, lg[b].size.y);
                bottom_width += lg[b].size.x;
            }
            bottom_width += comment_comment_spacing * (0i64.max(bottom_boxes.len() as i64 - 1)) as f64;
            margin.bottom += max_height + comment_node_spacing;
        }
        let max_comment_width = swift::max(top_width, bottom_width);
        if max_comment_width > lg[node].size.x {
            let protrusion = (max_comment_width - lg[node].size.x) / 2.0;
            margin.left = swift::max(margin.left, protrusion);
            margin.right = swift::max(margin.right, protrusion);
        }
        lg[node].margin = margin;
    }
}

impl ILayoutProcessor for CommentNodeMarginCalculator {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Node margin calculation", 1.0);
        let nodes: Vec<LNodeId> = lg[layered_graph].layers.iter().flat_map(|&l| lg[l].nodes.clone()).collect();
        for node in nodes {
            self.process_comments(lg, node);
        }
        monitor.done();
    }

    fn name(&self) -> &'static str {
        "CommentNodeMarginCalculator"
    }
}
