//! Port of `alg/common/overlaps/RectangleStripOverlapRemover.swift`.
//!
//! Removes overlaps between a strip of rectangles by moving them along one
//! axis.
//!
//! Aliasing: Swift's `addRectangle` keeps the caller's `ElkRectangle` object
//! (`originalRectangle`) and, for `UP`/`DOWN`, `importRectangle` returns that
//! very object as the working `rectangle` — so the algorithm mutates the
//! caller's rectangle directly (the greedy strategy sets its `y`, and later
//! comparisons read that updated `y`). The port keeps one rectangle per node
//! for `UP`/`DOWN` (`original_rectangle` is then the working `rectangle`) and a
//! separate copy for `LEFT`/`RIGHT`. [`add_rectangle`](RectangleStripOverlapRemover::add_rectangle)
//! returns an id; after [`remove_overlaps`](RectangleStripOverlapRemover::remove_overlaps)
//! the caller writes [`original_rectangles`](RectangleStripOverlapRemover::original_rectangles)
//! back into its own objects. Every caller leaves its rectangles untouched
//! between adding them and removing overlaps, so this write-back is exact.

use super::greedy_rectangle_strip_overlap_remover::GreedyRectangleStripOverlapRemover;
use super::i_rectangle_strip_overlap_removal_strategy::IRectangleStripOverlapRemovalStrategy;
use crate::org::eclipse::elk::core::math::elk_rectangle::ElkRectangle;
use crate::swift;

/// `OverlapRemovalDirection`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum OverlapRemovalDirection {
    UP,
    DOWN,
    LEFT,
    RIGHT,
}

/// `RectangleNode`.
#[derive(Clone, Debug)]
pub struct RectangleNode {
    /// The order in which the rectangle was added (the caller's handle).
    pub id: usize,
    /// For `LEFT`/`RIGHT` only: the caller's rectangle. For `UP`/`DOWN` the
    /// caller's rectangle is `rectangle` itself.
    original_rectangle: ElkRectangle,
    shares_original: bool,
    /// The (transformed) rectangle the algorithm works on.
    pub rectangle: ElkRectangle,
    /// Positions (in the sorted `rectangle_nodes`) of the overlapping nodes.
    pub overlapping_nodes: Vec<usize>,
}

impl RectangleNode {
    pub fn get_rectangle(&self) -> &ElkRectangle {
        &self.rectangle
    }

    pub fn get_overlapping_nodes(&self) -> &[usize] {
        &self.overlapping_nodes
    }

    /// `originalRectangle`.
    pub fn original_rectangle(&self) -> &ElkRectangle {
        if self.shares_original { &self.rectangle } else { &self.original_rectangle }
    }

    fn original_rectangle_mut(&mut self) -> &mut ElkRectangle {
        if self.shares_original { &mut self.rectangle } else { &mut self.original_rectangle }
    }
}

pub struct RectangleStripOverlapRemover {
    pub overlap_removal_direction: OverlapRemovalDirection,
    pub gap_vertical: f64,
    pub gap_horizontal: f64,
    pub start_coordinate: f64,
    pub overlap_removal_strategy: Option<Box<dyn IRectangleStripOverlapRemovalStrategy>>,
    pub rectangle_nodes: Vec<RectangleNode>,
}

impl RectangleStripOverlapRemover {
    pub const DEFAULT_GAP: f64 = 5.0;

    /// `create(for:)`.
    pub fn create(direction: OverlapRemovalDirection) -> RectangleStripOverlapRemover {
        RectangleStripOverlapRemover {
            overlap_removal_direction: direction,
            gap_vertical: Self::DEFAULT_GAP,
            gap_horizontal: Self::DEFAULT_GAP,
            start_coordinate: 0.0,
            overlap_removal_strategy: None,
            rectangle_nodes: Vec::new(),
        }
    }

    // MARK: - Configuration

    /// `withGap(_ horizontalGap:, _ verticalGap:)`.
    pub fn with_gap(mut self, horizontal_gap: f64, vertical_gap: f64) -> RectangleStripOverlapRemover {
        self.gap_horizontal = horizontal_gap;
        self.gap_vertical = vertical_gap;
        self
    }

    pub fn with_start_coordinate(&mut self, coordinate: f64) -> &mut RectangleStripOverlapRemover {
        self.start_coordinate = coordinate;
        self
    }

    pub fn with_overlap_removal_strategy(
        &mut self,
        strategy: Box<dyn IRectangleStripOverlapRemovalStrategy>,
    ) -> &mut RectangleStripOverlapRemover {
        self.overlap_removal_strategy = Some(strategy);
        self
    }

    /// `addRectangle(_:)`: returns the rectangle's id (its index in
    /// [`original_rectangles`](Self::original_rectangles)).
    pub fn add_rectangle(&mut self, rectangle: ElkRectangle) -> usize {
        let id = self.rectangle_nodes.len();
        let node = match self.overlap_removal_direction {
            OverlapRemovalDirection::UP | OverlapRemovalDirection::DOWN => RectangleNode {
                id,
                original_rectangle: ElkRectangle::default(),
                shares_original: true,
                rectangle,
                overlapping_nodes: Vec::new(),
            },
            OverlapRemovalDirection::LEFT | OverlapRemovalDirection::RIGHT => RectangleNode {
                id,
                original_rectangle: rectangle,
                shares_original: false,
                rectangle: self.import_rectangle(&rectangle),
                overlapping_nodes: Vec::new(),
            },
        };
        self.rectangle_nodes.push(node);
        id
    }

    // MARK: - Getters

    pub fn get_horizontal_gap(&self) -> f64 {
        self.gap_horizontal
    }

    pub fn get_vertical_gap(&self) -> f64 {
        self.gap_vertical
    }

    pub fn get_rectangle_nodes(&self) -> &[RectangleNode] {
        &self.rectangle_nodes
    }

    pub fn get_rectangle_nodes_mut(&mut self) -> &mut Vec<RectangleNode> {
        &mut self.rectangle_nodes
    }

    /// The caller's rectangles (as mutated by the algorithm), by id.
    pub fn original_rectangles(&self) -> Vec<ElkRectangle> {
        let mut result = vec![ElkRectangle::default(); self.rectangle_nodes.len()];
        for node in &self.rectangle_nodes {
            result[node.id] = *node.original_rectangle();
        }
        result
    }

    // MARK: - Coordinate Transformation

    /// `importRectangle(_:)` (for `UP`/`DOWN` Swift returns the same object;
    /// see `add_rectangle`).
    pub fn import_rectangle(&self, rectangle: &ElkRectangle) -> ElkRectangle {
        match self.overlap_removal_direction {
            OverlapRemovalDirection::UP | OverlapRemovalDirection::DOWN => *rectangle,
            OverlapRemovalDirection::LEFT | OverlapRemovalDirection::RIGHT => {
                ElkRectangle::new(rectangle.y, 0.0, rectangle.height, rectangle.width)
            }
        }
    }

    fn export_rectangle(direction: OverlapRemovalDirection, start_coordinate: f64, rectangle_node: &mut RectangleNode, _strip_size: f64) {
        let rectangle = rectangle_node.rectangle;

        match direction {
            OverlapRemovalDirection::UP => {
                rectangle_node.original_rectangle_mut().y = start_coordinate - rectangle.height - rectangle.y;
            }
            OverlapRemovalDirection::DOWN => {
                rectangle_node.original_rectangle_mut().y += start_coordinate;
            }
            OverlapRemovalDirection::LEFT => {
                rectangle_node.original_rectangle_mut().x = start_coordinate - rectangle.height - rectangle.y;
            }
            OverlapRemovalDirection::RIGHT => {
                rectangle_node.original_rectangle_mut().x = start_coordinate + rectangle.y;
            }
        }
    }

    // MARK: - Actual Algorithm

    /// `removeOverlaps()`: returns the strip size.
    pub fn remove_overlaps(&mut self) -> f64 {
        if self.overlap_removal_strategy.is_none() {
            self.overlap_removal_strategy = Some(Box::new(GreedyRectangleStripOverlapRemover::new()));
        }

        swift::sort_by(&mut self.rectangle_nodes, |a, b| Self::compare_left_rectangle_borders(a, b));

        self.compute_overlaps();
        let Some(mut strategy) = self.overlap_removal_strategy.take() else { return 0.0 };
        let strip_size = strategy.remove_overlaps(self);
        self.overlap_removal_strategy = Some(strategy);

        let direction = self.overlap_removal_direction;
        let start_coordinate = self.start_coordinate;
        for node in &mut self.rectangle_nodes {
            Self::export_rectangle(direction, start_coordinate, node, strip_size);
        }

        strip_size
    }

    pub fn compute_overlaps(&mut self) {
        let nodes = &mut self.rectangle_nodes;
        // `OverlapSortedSet` ordered by `compareRightRectangleBorders`, as node positions.
        let mut intersecting_nodes: Vec<usize> = Vec::new();
        let mut scanline_pos: f64;

        for curr_node in 0..nodes.len() {
            scanline_pos = nodes[curr_node].rectangle.x;

            while let Some(&intersecting_rectangle) = intersecting_nodes.first() {
                let r = nodes[intersecting_rectangle].rectangle;
                if r.x + r.width < scanline_pos {
                    // `remove(_:)` removes the first identical element: the first one.
                    intersecting_nodes.remove(0);
                } else {
                    break;
                }
            }

            for &intersecting_node in &intersecting_nodes {
                nodes[intersecting_node].overlapping_nodes.push(curr_node);
                nodes[curr_node].overlapping_nodes.push(intersecting_node);
            }

            // `add(_:)`: insert after every element that compares less.
            let mut index = 0;
            while index < intersecting_nodes.len()
                && Self::compare_right_rectangle_borders(&nodes[intersecting_nodes[index]], &nodes[curr_node])
            {
                index += 1;
            }
            intersecting_nodes.insert(index, curr_node);
        }
    }

    // MARK: - Utility Methods

    pub fn compare_left_rectangle_borders(rn1: &RectangleNode, rn2: &RectangleNode) -> bool {
        rn1.rectangle.x < rn2.rectangle.x
    }

    pub fn compare_right_rectangle_borders(rn1: &RectangleNode, rn2: &RectangleNode) -> bool {
        rn1.rectangle.x + rn1.rectangle.width < rn2.rectangle.x + rn2.rectangle.width
    }
}
