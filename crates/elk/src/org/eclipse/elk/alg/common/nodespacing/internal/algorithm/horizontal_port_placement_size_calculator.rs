//! Port of `alg/common/nodespacing/internal/algorithm/HorizontalPortPlacementSizeCalculator.swift`.
//!
//! Calculates the space required to set up the northern and southern ports
//! (and their labels).

use super::node_label_and_size_utilities::NodeLabelAndSizeUtilities;
use super::port_placement_calculator::PortPlacementCalculator;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::port_context::PortContext;
use crate::org::eclipse::elk::core::options::port_alignment::PortAlignment;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::prelude::*;

pub struct HorizontalPortPlacementSizeCalculator;

impl HorizontalPortPlacementSizeCalculator {
    pub fn calculate_horizontal_port_placement_size(lg: &LGraphArena, node_context: &mut NodeContext) {
        match node_context.port_constraints {
            PortConstraints::FIXED_POS => {
                Self::calculate_horizontal_node_size_required_by_fixed_pos_ports(lg, node_context, PortSide::NORTH);
                Self::calculate_horizontal_node_size_required_by_fixed_pos_ports(lg, node_context, PortSide::SOUTH);
            }
            PortConstraints::FIXED_RATIO => {
                Self::calculate_horizontal_node_size_required_by_fixed_ratio_ports(lg, node_context, PortSide::NORTH);
                Self::calculate_horizontal_node_size_required_by_fixed_ratio_ports(lg, node_context, PortSide::SOUTH);
            }
            _ => {
                Self::calculate_horizontal_node_size_required_by_free_ports(lg, node_context, PortSide::NORTH);
                Self::calculate_horizontal_node_size_required_by_free_ports(lg, node_context, PortSide::SOUTH);
            }
        }
    }

    // MARK: - Fixed Position

    pub fn calculate_horizontal_node_size_required_by_fixed_pos_ports(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        port_side: PortSide,
    ) {
        let mut rightmost_port_border: f64 = 0.0;

        for port_context in node_context.port_contexts.get_or_empty(port_side) {
            rightmost_port_border =
                swift::max(rightmost_port_border, port_context.port_position.x + port_context.port.get_size(lg).x);
        }

        if let Some(cell) = node_context.inside_port_label_cell(port_side) {
            let mut cell = cell.borrow_mut();
            cell.cell.padding.left = 0.0;
            cell.minimum_content_area_size.x = rightmost_port_border;
        }
    }

    // MARK: - Fixed Ratio

    pub fn calculate_horizontal_node_size_required_by_fixed_ratio_ports(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        port_side: PortSide,
    ) {
        let Some(cell) = node_context.inside_port_label_cell(port_side).cloned() else { return };

        if node_context.port_contexts.get_or_empty(port_side).is_empty() {
            let mut cell = cell.borrow_mut();
            cell.cell.padding.left = 0.0;
            cell.cell.padding.right = 0.0;
            return;
        }

        let port_labels_inside = node_context.port_labels_placement.contains(PortLabelPlacement::INSIDE);
        let mut min_width: f64 = 0.0;

        if node_context.size_constraints.contains(SizeConstraint::PORT_LABELS) {
            Self::setup_port_margins(lg, node_context, port_side);
        }

        let port_contexts = node_context.port_contexts.get_or_empty(port_side);
        let mut previous_port_context: Option<&PortContext> = None;
        let mut previous_port_ratio: f64 = 0.0;
        let mut previous_port_width: f64 = 0.0;

        for current_port_context in port_contexts {
            let current_port_ratio: f64 = current_port_context
                .port
                .get_property::<f64>(lg, &PortPlacementCalculator::PORT_RATIO_OR_POSITION)
                .unwrap_or(0.0);
            let current_port_width = current_port_context.port.get_size(lg).x;

            match previous_port_context {
                None => {
                    let surrounding_port_margins = node_context.surrounding_port_margins;
                    if surrounding_port_margins.left > 0.0 {
                        min_width = swift::max(
                            min_width,
                            Self::min_size_required_to_respect_spacing(
                                surrounding_port_margins.left + current_port_context.port_margin.left,
                                0.0,
                                current_port_ratio,
                            ),
                        );
                    }
                }
                Some(prev_port_context) => {
                    let required_space = previous_port_width
                        + prev_port_context.port_margin.right
                        + node_context.port_port_spacing
                        + current_port_context.port_margin.left;
                    min_width = swift::max(
                        min_width,
                        Self::min_size_required_to_respect_spacing(required_space, previous_port_ratio, current_port_ratio),
                    );
                }
            }

            previous_port_context = Some(current_port_context);
            previous_port_ratio = current_port_ratio;
            previous_port_width = current_port_width;
        }

        let surrounding_port_margins = node_context.surrounding_port_margins;
        if surrounding_port_margins.right > 0.0 {
            let mut required_space = previous_port_width + surrounding_port_margins.right;

            if port_labels_inside {
                if let Some(prev_port_context) = previous_port_context {
                    required_space += prev_port_context.port_margin.right;
                }
            }

            min_width = swift::max(
                min_width,
                Self::min_size_required_to_respect_spacing(required_space, previous_port_ratio, 1.0),
            );
        }

        let mut cell = cell.borrow_mut();
        cell.cell.padding.left = 0.0;
        cell.minimum_content_area_size.x = min_width;
    }

    pub const EQUALITY_TOLERANCE: f64 = 0.01;

    /// (Swift `assert(secondRatio >= firstRatio)` is a no-op in release builds.)
    pub fn min_size_required_to_respect_spacing(spacing: f64, first_ratio: f64, second_ratio: f64) -> f64 {
        if (first_ratio - second_ratio).abs() < Self::EQUALITY_TOLERANCE {
            0.0
        } else {
            spacing / (second_ratio - first_ratio)
        }
    }

    // MARK: - Free

    pub fn calculate_horizontal_node_size_required_by_free_ports(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        port_side: PortSide,
    ) {
        let Some(cell) = node_context.inside_port_label_cell(port_side).cloned() else { return };

        if node_context.port_contexts.get_or_empty(port_side).is_empty() {
            let mut cell = cell.borrow_mut();
            cell.cell.padding.left = 0.0;
            cell.cell.padding.right = 0.0;
            return;
        }

        {
            let mut cell = cell.borrow_mut();
            cell.cell.padding.left = node_context.surrounding_port_margins.left;
            cell.cell.padding.right = node_context.surrounding_port_margins.right;
        }

        if node_context.size_constraints.contains(SizeConstraint::PORT_LABELS) {
            Self::setup_port_margins(lg, node_context, port_side);
        }

        let mut width = Self::port_width_plus_port_port_spacing(lg, node_context, port_side);

        if node_context.get_port_alignment(lg, port_side) == PortAlignment::DISTRIBUTED {
            width += 2.0 * node_context.port_port_spacing;
        }

        cell.borrow_mut().minimum_content_area_size.x = width;
    }

    pub fn setup_port_margins(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let count = node_context.port_contexts.get_or_empty(port_side).len();

        let placement = node_context.port_labels_placement;
        let port_labels_outside = placement.contains(PortLabelPlacement::OUTSIDE);
        let always_same_side = placement.contains(PortLabelPlacement::ALWAYS_SAME_SIDE);
        let always_same_side_above = placement.contains(PortLabelPlacement::ALWAYS_OTHER_SAME_SIDE);
        let space_efficient = placement.contains(PortLabelPlacement::SPACE_EFFICIENT);
        let uniform_port_spacing = node_context.size_options.contains(SizeOptions::UNIFORM_PORT_SPACING);

        let space_efficient_port_labels = !always_same_side && !always_same_side_above && (space_efficient || count == 2);

        Self::compute_horizontal_port_margins(lg, node_context, port_side, port_labels_outside);

        let port_contexts = node_context.port_contexts.get_or_empty_mut(port_side);

        // Positions of the leftmost / rightmost contexts (the same one if there is only one).
        let mut leftmost_port_context: Option<usize> = None;
        let mut rightmost_port_context: Option<usize> = None;

        if port_labels_outside {
            if count > 0 {
                leftmost_port_context = Some(0);
                rightmost_port_context = Some(count - 1);
            }

            if let Some(l) = leftmost_port_context {
                port_contexts[l].port_margin.left = 0.0;
            }
            if let Some(r) = rightmost_port_context {
                port_contexts[r].port_margin.right = 0.0;
            }

            if space_efficient_port_labels {
                if let Some(l) = leftmost_port_context {
                    if !port_contexts[l].labels_next_to_port {
                        port_contexts[l].port_margin.right = 0.0;
                    }
                }
            }
        }

        if uniform_port_spacing {
            Self::unify_port_margins(port_contexts);

            if port_labels_outside {
                if let Some(l) = leftmost_port_context {
                    port_contexts[l].port_margin.left = 0.0;
                }
                if let Some(r) = rightmost_port_context {
                    port_contexts[r].port_margin.right = 0.0;
                }
            }
        }
    }

    pub fn compute_horizontal_port_margins(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        port_side: PortSide,
        _port_labels_outside: bool,
    ) {
        let port_label_spacing_horizontal = node_context.port_label_spacing_horizontal;
        let fixed = PortLabelPlacement::is_fixed(node_context.port_labels_placement);

        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            let label_width = port_context.port_label_cell.as_ref().map(|c| c.get_minimum_width()).unwrap_or(0.0);

            if label_width > 0.0 {
                if port_context.labels_next_to_port {
                    let port_width = port_context.port.get_size(lg).x;
                    if label_width > port_width {
                        let overhang = (label_width - port_width) / 2.0;
                        port_context.port_margin.left = overhang;
                        port_context.port_margin.right = overhang;
                    }
                } else {
                    port_context.port_margin.right = port_label_spacing_horizontal + label_width;
                }
            } else if fixed {
                let labels_bounds = NodeLabelAndSizeUtilities::get_labels_bounds(lg, &port_context.port);
                if labels_bounds.x < 0.0 {
                    port_context.port_margin.left = -labels_bounds.x;
                }
                if labels_bounds.x + labels_bounds.width > port_context.port.get_size(lg).x {
                    port_context.port_margin.right = labels_bounds.x + labels_bounds.width - port_context.port.get_size(lg).x;
                }
            }
        }
    }

    pub fn unify_port_margins(port_contexts: &mut [PortContext]) {
        let mut max_left: f64 = 0.0;
        let mut max_right: f64 = 0.0;

        for port_context in port_contexts.iter() {
            max_left = swift::max(max_left, port_context.port_margin.left);
            max_right = swift::max(max_right, port_context.port_margin.right);
        }

        for port_context in port_contexts.iter_mut() {
            port_context.port_margin.left = max_left;
            port_context.port_margin.right = max_right;
        }
    }

    pub fn port_width_plus_port_port_spacing(lg: &LGraphArena, node_context: &NodeContext, port_side: PortSide) -> f64 {
        let mut result: f64 = 0.0;

        let port_contexts = node_context.port_contexts.get_or_empty(port_side);
        let count = port_contexts.len();
        for (index, port_context) in port_contexts.iter().enumerate() {
            result += port_context.port_margin.left + port_context.port.get_size(lg).x + port_context.port_margin.right;

            if index < count - 1 {
                result += node_context.port_port_spacing;
            }
        }

        result
    }
}
