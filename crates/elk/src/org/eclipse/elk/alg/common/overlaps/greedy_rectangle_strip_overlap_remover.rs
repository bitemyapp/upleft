//! Port of `alg/common/overlaps/GreedyRectangleStripOverlapRemover.swift`.
//!
//! Removes rectangle overlaps by greedily choosing the smallest y position
//! that won't cause overlaps.

use super::i_rectangle_strip_overlap_removal_strategy::IRectangleStripOverlapRemovalStrategy;
use super::rectangle_strip_overlap_remover::RectangleStripOverlapRemover;
use crate::swift;

#[derive(Default)]
pub struct GreedyRectangleStripOverlapRemover;

impl GreedyRectangleStripOverlapRemover {
    pub fn new() -> GreedyRectangleStripOverlapRemover {
        GreedyRectangleStripOverlapRemover
    }
}

impl IRectangleStripOverlapRemovalStrategy for GreedyRectangleStripOverlapRemover {
    fn remove_overlaps(&mut self, overlap_remover: &mut RectangleStripOverlapRemover) -> f64 {
        let vertical_gap = overlap_remover.get_vertical_gap();
        let nodes = overlap_remover.get_rectangle_nodes_mut();
        // `alreadyPlacedNodes.contains(ObjectIdentifier(node))`, by node position.
        let mut already_placed_nodes = vec![false; nodes.len()];
        let mut strip_size: f64 = 0.0;

        for curr in 0..nodes.len() {
            let mut y_pos: f64 = 0.0;

            // Sort the node's list of overlapping nodes by y coordinate (the
            // current y: placed nodes already have their new one)
            let sorted_overlapping =
                swift::sorted_by(nodes[curr].overlapping_nodes.iter().copied(), |&n1, &n2| nodes[n1].rectangle.y < nodes[n2].rectangle.y);

            for overlap_node in sorted_overlapping {
                if already_placed_nodes[overlap_node] {
                    let curr_rect = nodes[curr].rectangle;
                    let overlap_rect = nodes[overlap_node].rectangle;

                    if y_pos < overlap_rect.y + overlap_rect.height + vertical_gap
                        && y_pos + curr_rect.height + vertical_gap > overlap_rect.y
                    {
                        y_pos = overlap_rect.y + overlap_rect.height + vertical_gap;
                    }
                }
            }

            nodes[curr].rectangle.y = y_pos;
            already_placed_nodes[curr] = true;

            strip_size = swift::max(strip_size, nodes[curr].rectangle.y + nodes[curr].rectangle.height);
        }

        strip_size
    }
}
