//! Port of `alg/common/nodespacing/internal/algorithm/VerticalPortPlacementSizeCalculator.swift`.
//!
//! Calculates the space required to set up the eastern and western ports (and
//! their labels).

use super::horizontal_port_placement_size_calculator::HorizontalPortPlacementSizeCalculator;
use super::node_label_and_size_utilities::NodeLabelAndSizeUtilities;
use super::port_placement_calculator::PortPlacementCalculator;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::port_context::PortContext;
use crate::org::eclipse::elk::core::options::port_alignment::PortAlignment;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::prelude::*;

pub struct VerticalPortPlacementSizeCalculator;

impl VerticalPortPlacementSizeCalculator {
    pub fn calculate_vertical_port_placement_size(lg: &LGraphArena, node_context: &mut NodeContext) {
        match node_context.port_constraints {
            PortConstraints::FIXED_POS => {
                Self::calculate_vertical_node_size_required_by_fixed_pos_ports(lg, node_context, PortSide::EAST);
                Self::calculate_vertical_node_size_required_by_fixed_pos_ports(lg, node_context, PortSide::WEST);
            }
            PortConstraints::FIXED_RATIO => {
                Self::calculate_vertical_node_size_required_by_fixed_ratio_ports(lg, node_context, PortSide::EAST);
                Self::calculate_vertical_node_size_required_by_fixed_ratio_ports(lg, node_context, PortSide::WEST);
            }
            _ => {
                Self::calculate_vertical_node_size_required_by_free_ports(lg, node_context, PortSide::EAST);
                Self::calculate_vertical_node_size_required_by_free_ports(lg, node_context, PortSide::WEST);
            }
        }
    }

    // MARK: - Fixed Position

    pub fn calculate_vertical_node_size_required_by_fixed_pos_ports(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        port_side: PortSide,
    ) {
        let mut bottommost_port_border: f64 = 0.0;

        for port_context in node_context.port_contexts.get_or_empty(port_side) {
            let port_y = port_context.port_position.y;
            let port_height = port_context.port.get_size(lg).y;
            bottommost_port_border = swift::max(bottommost_port_border, port_y + port_height);
        }

        let Some(cell) = node_context.inside_port_label_cell(port_side) else { return };
        let mut cell = cell.borrow_mut();
        cell.cell.padding.top = 0.0;
        cell.minimum_content_area_size.y = bottommost_port_border;
    }

    // MARK: - Fixed Ratio

    pub fn calculate_vertical_node_size_required_by_fixed_ratio_ports(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        port_side: PortSide,
    ) {
        let Some(cell) = node_context.inside_port_label_cell(port_side).cloned() else { return };

        if node_context.port_contexts.get(port_side).is_none() {
            let mut cell = cell.borrow_mut();
            cell.cell.padding.top = 0.0;
            cell.cell.padding.bottom = 0.0;
            return;
        }

        let port_labels_inside = node_context.port_labels_placement.contains(PortLabelPlacement::INSIDE);
        let mut min_height: f64 = 0.0;

        if node_context.size_constraints.contains(SizeConstraint::PORT_LABELS) {
            Self::setup_port_margins(lg, node_context, port_side);
        }

        let port_contexts = node_context.port_contexts.get_or_empty(port_side);
        let mut previous_port_context: Option<&PortContext> = None;
        let mut previous_port_ratio: f64 = 0.0;
        let mut previous_port_height: f64 = 0.0;

        for current_port_context in port_contexts {
            let current_port_ratio: f64 = current_port_context
                .port
                .get_property::<f64>(lg, &PortPlacementCalculator::PORT_RATIO_OR_POSITION)
                .unwrap_or(0.0);
            let current_port_height = current_port_context.port.get_size(lg).y;

            if let Some(prev) = previous_port_context {
                let required_space = previous_port_height
                    + prev.port_margin.bottom
                    + node_context.port_port_spacing
                    + current_port_context.port_margin.top;
                min_height = swift::max(
                    min_height,
                    HorizontalPortPlacementSizeCalculator::min_size_required_to_respect_spacing(
                        required_space,
                        previous_port_ratio,
                        current_port_ratio,
                    ),
                );
            } else {
                let surrounding_port_margins = node_context.surrounding_port_margins;
                if surrounding_port_margins.top > 0.0 {
                    let required_space = surrounding_port_margins.top + current_port_context.port_margin.top;
                    min_height = swift::max(
                        min_height,
                        HorizontalPortPlacementSizeCalculator::min_size_required_to_respect_spacing(
                            required_space,
                            0.0,
                            current_port_ratio,
                        ),
                    );
                }
            }

            previous_port_context = Some(current_port_context);
            previous_port_ratio = current_port_ratio;
            previous_port_height = current_port_height;
        }

        let surrounding_port_margins = node_context.surrounding_port_margins;
        if surrounding_port_margins.bottom > 0.0 {
            let mut required_space = previous_port_height + surrounding_port_margins.bottom;

            if port_labels_inside {
                required_space += previous_port_context.map(|p| p.port_margin.bottom).unwrap_or(0.0);
            }

            min_height = swift::max(
                min_height,
                HorizontalPortPlacementSizeCalculator::min_size_required_to_respect_spacing(
                    required_space,
                    previous_port_ratio,
                    1.0,
                ),
            );
        }

        let mut cell = cell.borrow_mut();
        cell.cell.padding.top = 0.0;
        cell.minimum_content_area_size.y = min_height;
    }

    // MARK: - Free

    pub fn calculate_vertical_node_size_required_by_free_ports(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        port_side: PortSide,
    ) {
        let Some(cell) = node_context.inside_port_label_cell(port_side).cloned() else { return };

        if node_context.port_contexts.get(port_side).is_none() {
            let mut cell = cell.borrow_mut();
            cell.cell.padding.top = 0.0;
            cell.cell.padding.bottom = 0.0;
            return;
        }

        {
            let mut cell = cell.borrow_mut();
            cell.cell.padding.top = node_context.surrounding_port_margins.top;
            cell.cell.padding.bottom = node_context.surrounding_port_margins.bottom;
        }

        if node_context.size_constraints.contains(SizeConstraint::PORT_LABELS) {
            Self::setup_port_margins(lg, node_context, port_side);
        }

        let mut height = Self::port_height_plus_port_port_spacing(lg, node_context, port_side);

        if node_context.get_port_alignment(lg, port_side) == PortAlignment::DISTRIBUTED {
            height += 2.0 * node_context.port_port_spacing;
        }

        cell.borrow_mut().minimum_content_area_size.y = height;
    }

    pub fn setup_port_margins(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let Some(port_contexts) = node_context.port_contexts.get(port_side) else { return };
        let count = port_contexts.len();

        let placement = node_context.port_labels_placement;
        let port_labels_outside = placement.contains(PortLabelPlacement::OUTSIDE);
        let always_same_side = placement.contains(PortLabelPlacement::ALWAYS_SAME_SIDE);
        let always_same_side_above = placement.contains(PortLabelPlacement::ALWAYS_OTHER_SAME_SIDE);
        let space_efficient = placement.contains(PortLabelPlacement::SPACE_EFFICIENT);
        let uniform_port_spacing = node_context.size_options.contains(SizeOptions::UNIFORM_PORT_SPACING);

        let space_efficient_port_labels = !always_same_side && !always_same_side_above && (space_efficient || count == 2);

        Self::compute_vertical_port_margins(lg, node_context, port_side, port_labels_outside);

        let port_contexts = node_context.port_contexts.get_or_empty_mut(port_side);

        // Positions of the topmost / bottommost contexts (the same one if there is only one).
        let mut topmost_port_context: Option<usize> = None;
        let mut bottommost_port_context: Option<usize> = None;

        if port_labels_outside {
            topmost_port_context = Some(0);
            bottommost_port_context = Some(count - 1);

            port_contexts[0].port_margin.top = 0.0;
            port_contexts[count - 1].port_margin.bottom = 0.0;

            if space_efficient_port_labels && !port_contexts[0].labels_next_to_port {
                port_contexts[0].port_margin.bottom = 0.0;
            }
        }

        if uniform_port_spacing {
            Self::unify_port_margins(port_contexts);

            if port_labels_outside {
                if let Some(t) = topmost_port_context {
                    port_contexts[t].port_margin.top = 0.0;
                }
                if let Some(b) = bottommost_port_context {
                    port_contexts[b].port_margin.bottom = 0.0;
                }
            }
        }
    }

    pub fn compute_vertical_port_margins(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        port_side: PortSide,
        _port_labels_outside: bool,
    ) {
        let port_labels_treat_as_group = node_context.port_labels_treat_as_group;
        let port_label_spacing_vertical = node_context.port_label_spacing_vertical;
        let fixed = PortLabelPlacement::is_fixed(node_context.port_labels_placement);

        let Some(port_contexts) = node_context.port_contexts.get_mut(port_side) else { return };

        for port_context in port_contexts {
            let label_height = port_context.port_label_cell.as_ref().map(|c| c.get_minimum_height()).unwrap_or(0.0);

            if label_height > 0.0 {
                if port_context.labels_next_to_port {
                    let port_height = port_context.port.get_size(lg).y;
                    if label_height > port_height {
                        let label_count = port_context.port_label_cell.as_ref().map(|c| c.labels.len()).unwrap_or(0);
                        if port_labels_treat_as_group || label_count == 1 {
                            let overhang = (label_height - port_height) / 2.0;
                            port_context.port_margin.top = overhang;
                            port_context.port_margin.bottom = overhang;
                        } else {
                            let first_label_height = port_context
                                .port_label_cell
                                .as_ref()
                                .and_then(|c| c.labels.first())
                                .map(|l| l.get_size(lg).y)
                                .unwrap_or(0.0);
                            let first_label_overhang = (first_label_height - port_height) / 2.0;

                            port_context.port_margin.top = swift::max(0.0, first_label_overhang);
                            port_context.port_margin.bottom = label_height - first_label_overhang - port_height;
                        }
                    }
                } else {
                    port_context.port_margin.bottom = port_label_spacing_vertical + label_height;
                }
            } else if fixed {
                let labels_bounds = NodeLabelAndSizeUtilities::get_labels_bounds(lg, &port_context.port);
                if labels_bounds.y < 0.0 {
                    port_context.port_margin.top = -labels_bounds.y;
                }
                if labels_bounds.y + labels_bounds.height > port_context.port.get_size(lg).y {
                    port_context.port_margin.bottom = labels_bounds.y + labels_bounds.height - port_context.port.get_size(lg).y;
                }
            }
        }
    }

    pub fn unify_port_margins(port_contexts: &mut [PortContext]) {
        let mut max_top: f64 = 0.0;
        let mut max_bottom: f64 = 0.0;

        for port_context in port_contexts.iter() {
            max_top = swift::max(max_top, port_context.port_margin.top);
            max_bottom = swift::max(max_bottom, port_context.port_margin.bottom);
        }

        for port_context in port_contexts.iter_mut() {
            port_context.port_margin.top = max_top;
            port_context.port_margin.bottom = max_bottom;
        }
    }

    pub fn port_height_plus_port_port_spacing(lg: &LGraphArena, node_context: &NodeContext, port_side: PortSide) -> f64 {
        let mut result: f64 = 0.0;

        let Some(port_contexts) = node_context.port_contexts.get(port_side) else { return result };

        let count = port_contexts.len();
        for (index, port_context) in port_contexts.iter().enumerate() {
            result += port_context.port_margin.top + port_context.port.get_size(lg).y + port_context.port_margin.bottom;
            if index < count - 1 {
                result += node_context.port_port_spacing;
            }
        }

        result
    }
}
