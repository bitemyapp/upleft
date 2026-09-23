//! Port of `alg/common/overlaps/IRectangleStripOverlapRemovalStrategy.swift`.

use super::rectangle_strip_overlap_remover::RectangleStripOverlapRemover;

/// Classes implementing this protocol know how to remove overlaps between a
/// strip of rectangles.
pub trait IRectangleStripOverlapRemovalStrategy {
    /// Removes overlaps for the given overlap remover and returns the height
    /// of the resulting strip of rectangles.
    fn remove_overlaps(&mut self, overlap_remover: &mut RectangleStripOverlapRemover) -> f64;
}
