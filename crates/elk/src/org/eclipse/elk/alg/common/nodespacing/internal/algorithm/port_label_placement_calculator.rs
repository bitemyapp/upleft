//! Port of `alg/common/nodespacing/internal/algorithm/PortLabelPlacementCalculator.swift`.
//!
//! Knows how to place port labels (into the port label cells' rectangles,
//! relative to their ports).
//!
//! The constrained placements hand the label cells' rectangles to a
//! [`RectangleStripOverlapRemover`], which in Swift mutates those very
//! rectangle objects; the port writes the remover's results back into the
//! cells right after `removeOverlaps()` (nothing touches them in between).

use super::node_label_and_size_utilities::NodeLabelAndSizeUtilities;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::horizontal_label_alignment::HorizontalLabelAlignment;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::vertical_label_alignment::VerticalLabelAlignment;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::alg::common::overlaps::rectangle_strip_overlap_remover::{
    OverlapRemovalDirection, RectangleStripOverlapRemover,
};
use crate::org::eclipse::elk::core::math::elk_rectangle::ElkRectangle;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::prelude::*;

pub struct PortLabelPlacementCalculator;

impl PortLabelPlacementCalculator {
    pub fn place_horizontal_port_labels(lg: &LGraphArena, node_context: &mut NodeContext) {
        Self::place_port_labels(lg, node_context, PortSide::NORTH);
        Self::place_port_labels(lg, node_context, PortSide::SOUTH);
    }

    pub fn place_vertical_port_labels(lg: &LGraphArena, node_context: &mut NodeContext) {
        Self::place_port_labels(lg, node_context, PortSide::EAST);
        Self::place_port_labels(lg, node_context, PortSide::WEST);
    }

    pub fn place_port_labels(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let constrained_placement = !node_context.size_constraints.contains(SizeConstraint::PORT_LABELS)
            || node_context.port_constraints == PortConstraints::FIXED_POS;

        if node_context.port_labels_placement.contains(PortLabelPlacement::INSIDE) {
            if constrained_placement {
                Self::constrained_inside_port_label_placement(lg, node_context, port_side);
            } else {
                Self::simple_inside_port_label_placement(lg, node_context, port_side);
            }
        } else if node_context.port_labels_placement.contains(PortLabelPlacement::OUTSIDE) {
            if constrained_placement {
                Self::constrained_outside_port_label_placement(lg, node_context, port_side);
            } else {
                Self::simple_outside_port_label_placement(lg, node_context, port_side);
            }
        }
    }

    // MARK: - Simple Inside Port Labels

    pub fn simple_inside_port_label_placement(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let mut inside_north_or_south_port_label_area_height: f64 = 0.0;

        let label_border_offset = Self::port_label_border_offset_for_port_side(node_context, port_side);
        let port_label_spacing_horizontal = node_context.port_label_spacing_horizontal;
        let port_label_spacing_vertical = node_context.port_label_spacing_vertical;
        let port_labels_treat_as_group = node_context.port_labels_treat_as_group;

        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            let labels_next_to_port = port_context.labels_next_to_port;
            let port = port_context.port;
            let Some(port_label_cell) = port_context.port_label_cell.as_mut() else { continue };
            if !port_label_cell.has_labels() {
                continue;
            }

            let port_size = port.get_size(lg);
            let port_border_offset: f64 = if port.has_property(lg, &CoreOptions::PORT_BORDER_OFFSET) {
                port.get_property::<f64>(lg, &CoreOptions::PORT_BORDER_OFFSET).unwrap_or(0.0)
            } else {
                0.0
            };

            let min_width = port_label_cell.get_minimum_width();
            let min_height = port_label_cell.get_minimum_height();
            let first_label_height = port_label_cell.labels.first().map(|l| l.get_size(lg).y);
            let port_label_cell_rect = &mut port_label_cell.cell.cell_rectangle;
            port_label_cell_rect.width = min_width;
            port_label_cell_rect.height = min_height;

            let alignments;
            match port_side {
                PortSide::NORTH => {
                    port_label_cell_rect.x = if labels_next_to_port {
                        (port_size.x - port_label_cell_rect.width) / 2.0
                    } else {
                        port_size.x + port_label_spacing_horizontal
                    };
                    port_label_cell_rect.y = port_size.y + port_border_offset + label_border_offset;
                    alignments = Some((HorizontalLabelAlignment::CENTER, VerticalLabelAlignment::TOP));
                }
                PortSide::SOUTH => {
                    port_label_cell_rect.x = if labels_next_to_port {
                        (port_size.x - port_label_cell_rect.width) / 2.0
                    } else {
                        port_size.x + port_label_spacing_horizontal
                    };
                    port_label_cell_rect.y = -port_border_offset - label_border_offset - port_label_cell_rect.height;
                    alignments = Some((HorizontalLabelAlignment::CENTER, VerticalLabelAlignment::BOTTOM));
                }
                PortSide::EAST => {
                    port_label_cell_rect.x = -port_border_offset - label_border_offset - port_label_cell_rect.width;
                    if labels_next_to_port {
                        let label_height = if port_labels_treat_as_group {
                            port_label_cell_rect.height
                        } else {
                            first_label_height.unwrap_or(0.0)
                        };
                        port_label_cell_rect.y = (port_size.y - label_height) / 2.0;
                    } else {
                        port_label_cell_rect.y = port_size.y + port_label_spacing_vertical;
                    }
                    alignments = Some((HorizontalLabelAlignment::RIGHT, VerticalLabelAlignment::CENTER));
                }
                PortSide::WEST => {
                    port_label_cell_rect.x = port_size.x + port_border_offset + label_border_offset;
                    if labels_next_to_port {
                        let label_height = if port_labels_treat_as_group {
                            port_label_cell_rect.height
                        } else {
                            first_label_height.unwrap_or(0.0)
                        };
                        port_label_cell_rect.y = (port_size.y - label_height) / 2.0;
                    } else {
                        port_label_cell_rect.y = port_size.y + port_label_spacing_vertical;
                    }
                    alignments = Some((HorizontalLabelAlignment::LEFT, VerticalLabelAlignment::CENTER));
                }
                _ => {
                    alignments = None;
                }
            }
            let rect_height = port_label_cell_rect.height;
            if let Some((h, v)) = alignments {
                port_label_cell.set_horizontal_alignment(h);
                port_label_cell.set_vertical_alignment(v);
            }

            if port_side == PortSide::NORTH || port_side == PortSide::SOUTH {
                inside_north_or_south_port_label_area_height =
                    swift::max(inside_north_or_south_port_label_area_height, rect_height);
            }
        }

        if inside_north_or_south_port_label_area_height > 0.0 {
            if let Some(cell) = node_context.inside_port_label_cell(port_side) {
                cell.borrow_mut().minimum_content_area_size.y = inside_north_or_south_port_label_area_height;
            }
        }
    }

    pub fn port_label_border_offset_for_port_side(node_context: &NodeContext, port_side: PortSide) -> f64 {
        let padding = node_context.node_container.borrow().cell.padding;
        match port_side {
            PortSide::NORTH => padding.top + node_context.port_label_spacing_vertical,
            PortSide::SOUTH => padding.bottom + node_context.port_label_spacing_vertical,
            PortSide::EAST => padding.right + node_context.port_label_spacing_horizontal,
            PortSide::WEST => padding.left + node_context.port_label_spacing_horizontal,
            _ => 0.0,
        }
    }

    // MARK: - Constrained Inside Port Labels

    pub fn constrained_inside_port_label_placement(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        if node_context.port_contexts.get(port_side).is_none() {
            return;
        }

        if port_side == PortSide::EAST || port_side == PortSide::WEST {
            Self::simple_inside_port_label_placement(lg, node_context, port_side);
            return;
        }

        let overlap_removal_direction =
            if port_side == PortSide::NORTH { OverlapRemovalDirection::DOWN } else { OverlapRemovalDirection::UP };
        let vertical_label_alignment =
            if port_side == PortSide::NORTH { VerticalLabelAlignment::TOP } else { VerticalLabelAlignment::BOTTOM };

        let Some(inside_port_label_container) = node_context.inside_port_label_cell(port_side).cloned() else { return };
        let (label_container_rect, container_padding) = {
            let c = inside_port_label_container.borrow();
            (c.cell.cell_rectangle, c.cell.padding)
        };
        let left_border = label_container_rect.x
            + swift::max_of(&[
                container_padding.left,
                node_context.surrounding_port_margins.left,
                node_context.node_label_spacing,
            ]);
        let right_border = label_container_rect.x + label_container_rect.width
            - swift::max_of(&[
                container_padding.right,
                node_context.surrounding_port_margins.right,
                node_context.node_label_spacing,
            ]);

        let mut overlap_remover = RectangleStripOverlapRemover::create(overlap_removal_direction)
            .with_gap(node_context.port_label_spacing_horizontal, node_context.port_label_spacing_vertical);

        let start_coordinate: f64 = if port_side == PortSide::NORTH { -f64::MAX } else { f64::MAX };

        let mut current_start_coordinate = start_coordinate;

        // (port context position, overlap remover id)
        let mut added: Vec<(usize, usize)> = Vec::new();
        for (index, port_context) in node_context.port_contexts.get_or_empty_mut(port_side).iter_mut().enumerate() {
            let port = port_context.port;
            let port_position = port_context.port_position;
            let Some(port_label_cell) = port_context.port_label_cell.as_mut() else { continue };
            if !port_label_cell.has_labels() {
                continue;
            }

            let port_size = port.get_size(lg);
            let min_width = port_label_cell.get_minimum_width();
            let min_height = port_label_cell.get_minimum_height();
            port_label_cell.cell.cell_rectangle.width = min_width;
            port_label_cell.cell.cell_rectangle.height = min_height;

            port_label_cell.set_vertical_alignment(vertical_label_alignment);
            port_label_cell.set_horizontal_alignment(HorizontalLabelAlignment::RIGHT);

            Self::center_port_label(&mut port_label_cell.cell.cell_rectangle, port_position, port_size, left_border, right_border);

            added.push((index, overlap_remover.add_rectangle(port_label_cell.cell.cell_rectangle)));

            current_start_coordinate = if port_side == PortSide::NORTH {
                swift::max(current_start_coordinate, port_position.y + port.get_size(lg).y)
            } else {
                swift::min(current_start_coordinate, port_position.y)
            };
        }

        let adjusted_start_coordinate = current_start_coordinate
            + if port_side == PortSide::NORTH {
                node_context.port_label_spacing_vertical
            } else {
                -node_context.port_label_spacing_vertical
            };

        let strip_height = overlap_remover.with_start_coordinate(adjusted_start_coordinate).remove_overlaps();
        Self::write_back(node_context, port_side, &overlap_remover, &added);

        if strip_height > 0.0 {
            inside_port_label_container.borrow_mut().minimum_content_area_size.y = strip_height;
        }

        Self::make_relative_to_ports(node_context, port_side);
    }

    pub fn center_port_label(
        port_label_cell_rect: &mut ElkRectangle,
        port_position: KVector,
        port_size: KVector,
        min_x: f64,
        max_x: f64,
    ) {
        port_label_cell_rect.x = port_position.x - (port_label_cell_rect.width - port_size.x) / 2.0;

        let actual_min_x = swift::min(min_x, port_position.x);
        let actual_max_x = swift::max(max_x, port_position.x + port_size.x);

        if port_label_cell_rect.x < actual_min_x {
            port_label_cell_rect.x = actual_min_x;
        } else if port_label_cell_rect.x + port_label_cell_rect.width > actual_max_x {
            port_label_cell_rect.x = actual_max_x - port_label_cell_rect.width;
        }
    }

    // MARK: - Simple Outside Port Labels

    pub fn simple_outside_port_label_placement(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        if node_context.port_contexts.get(port_side).is_none() {
            return;
        }

        let place_first_port_differently =
            NodeLabelAndSizeUtilities::is_first_outside_port_label_placed_differently(node_context, port_side);

        let always_above = node_context.port_labels_placement.contains(PortLabelPlacement::ALWAYS_OTHER_SAME_SIDE);
        let port_label_spacing_horizontal = node_context.port_label_spacing_horizontal;
        let port_label_spacing_vertical = node_context.port_label_spacing_vertical;
        let port_labels_treat_as_group = node_context.port_labels_treat_as_group;

        let mut should_place_first_port_differently = place_first_port_differently;

        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            let labels_next_to_port = port_context.labels_next_to_port;
            let port = port_context.port;
            let Some(port_label_cell) = port_context.port_label_cell.as_mut() else { continue };
            if !port_label_cell.has_labels() {
                continue;
            }

            let port_size = port.get_size(lg);

            let min_width = port_label_cell.get_minimum_width();
            let min_height = port_label_cell.get_minimum_height();
            let first_label_height = port_label_cell.labels.first().map(|l| l.get_size(lg).y);
            let mut rect = port_label_cell.cell.cell_rectangle;
            rect.width = min_width;
            rect.height = min_height;
            let mut h_align = port_label_cell.horizontal_alignment;
            let mut v_align = port_label_cell.vertical_alignment;

            match port_side {
                PortSide::NORTH => {
                    if labels_next_to_port {
                        rect.x = (port_size.x - rect.width) / 2.0;
                        h_align = HorizontalLabelAlignment::CENTER;
                    } else if should_place_first_port_differently || always_above {
                        rect.x = -rect.width - port_label_spacing_horizontal;
                        h_align = HorizontalLabelAlignment::RIGHT;
                    } else {
                        rect.x = port_size.x + port_label_spacing_horizontal;
                        h_align = HorizontalLabelAlignment::LEFT;
                    }
                    rect.y = -rect.height - port_label_spacing_vertical;
                    v_align = VerticalLabelAlignment::BOTTOM;
                }
                PortSide::SOUTH => {
                    if labels_next_to_port {
                        rect.x = (port_size.x - rect.width) / 2.0;
                        h_align = HorizontalLabelAlignment::CENTER;
                    } else if should_place_first_port_differently || always_above {
                        rect.x = -rect.width - port_label_spacing_horizontal;
                        h_align = HorizontalLabelAlignment::RIGHT;
                    } else {
                        rect.x = port_size.x + port_label_spacing_horizontal;
                        h_align = HorizontalLabelAlignment::LEFT;
                    }
                    rect.y = port_size.y + port_label_spacing_vertical;
                    v_align = VerticalLabelAlignment::TOP;
                }
                PortSide::EAST => {
                    if labels_next_to_port {
                        let label_height =
                            if port_labels_treat_as_group { rect.height } else { first_label_height.unwrap_or(0.0) };
                        rect.y = (port_size.y - label_height) / 2.0;
                        v_align = VerticalLabelAlignment::CENTER;
                    } else if should_place_first_port_differently || always_above {
                        rect.y = -rect.height - port_label_spacing_vertical;
                        v_align = VerticalLabelAlignment::BOTTOM;
                    } else {
                        rect.y = port_size.y + port_label_spacing_vertical;
                        v_align = VerticalLabelAlignment::TOP;
                    }
                    rect.x = port_size.x + port_label_spacing_horizontal;
                    h_align = HorizontalLabelAlignment::LEFT;
                }
                PortSide::WEST => {
                    if labels_next_to_port {
                        let label_height =
                            if port_labels_treat_as_group { rect.height } else { first_label_height.unwrap_or(0.0) };
                        rect.y = (port_size.y - label_height) / 2.0;
                        v_align = VerticalLabelAlignment::CENTER;
                    } else if should_place_first_port_differently || always_above {
                        rect.y = -rect.height - port_label_spacing_vertical;
                        v_align = VerticalLabelAlignment::BOTTOM;
                    } else {
                        rect.y = port_size.y + port_label_spacing_vertical;
                        v_align = VerticalLabelAlignment::TOP;
                    }
                    rect.x = -rect.width - port_label_spacing_horizontal;
                    h_align = HorizontalLabelAlignment::RIGHT;
                }
                _ => {}
            }

            port_label_cell.cell.cell_rectangle = rect;
            port_label_cell.horizontal_alignment = h_align;
            port_label_cell.vertical_alignment = v_align;

            should_place_first_port_differently = false;
        }
    }

    // MARK: - Constrained Outside Port Labels

    pub fn constrained_outside_port_label_placement(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let Some(port_contexts) = node_context.port_contexts.get(port_side) else { return };

        if port_contexts.len() <= 2 || port_side == PortSide::EAST || port_side == PortSide::WEST {
            Self::simple_outside_port_label_placement(lg, node_context, port_side);
            return;
        }

        let port_with_special_needs = node_context.port_labels_placement.contains(PortLabelPlacement::SPACE_EFFICIENT);

        let overlap_removal_direction =
            if port_side == PortSide::NORTH { OverlapRemovalDirection::UP } else { OverlapRemovalDirection::DOWN };
        let vertical_label_alignment =
            if port_side == PortSide::NORTH { VerticalLabelAlignment::BOTTOM } else { VerticalLabelAlignment::TOP };

        let mut overlap_remover = RectangleStripOverlapRemover::create(overlap_removal_direction)
            .with_gap(node_context.port_label_spacing_vertical, node_context.port_label_spacing_horizontal);

        let start_coordinate: f64 = if port_side == PortSide::NORTH { f64::MAX } else { -f64::MAX };

        let mut current_start_coordinate = start_coordinate;
        let mut has_special_needs_port = port_with_special_needs;
        let port_label_spacing_horizontal = node_context.port_label_spacing_horizontal;

        // (port context position, overlap remover id)
        let mut added: Vec<(usize, usize)> = Vec::new();
        for (index, port_context) in node_context.port_contexts.get_or_empty_mut(port_side).iter_mut().enumerate() {
            let port = port_context.port;
            let port_position = port_context.port_position;
            let Some(port_label_cell) = port_context.port_label_cell.as_mut() else { continue };
            if !port_label_cell.has_labels() {
                continue;
            }

            let port_size = port.get_size(lg);
            let min_width = port_label_cell.get_minimum_width();
            let min_height = port_label_cell.get_minimum_height();
            port_label_cell.cell.cell_rectangle.width = min_width;
            port_label_cell.cell.cell_rectangle.height = min_height;

            if has_special_needs_port {
                port_label_cell.cell.cell_rectangle.x =
                    port_position.x - port_label_cell.get_minimum_width() - port_label_spacing_horizontal;
                has_special_needs_port = false;
            } else {
                port_label_cell.cell.cell_rectangle.x = port_position.x + port_size.x + port_label_spacing_horizontal;
            }

            port_label_cell.set_vertical_alignment(vertical_label_alignment);
            port_label_cell.set_horizontal_alignment(HorizontalLabelAlignment::RIGHT);

            added.push((index, overlap_remover.add_rectangle(port_label_cell.cell.cell_rectangle)));

            current_start_coordinate = if port_side == PortSide::NORTH {
                swift::min(current_start_coordinate, port_position.y)
            } else {
                swift::max(current_start_coordinate, port_position.y + port.get_size(lg).y)
            };
        }

        let adjusted_start_coordinate = current_start_coordinate
            + if port_side == PortSide::NORTH {
                -node_context.port_label_spacing_vertical
            } else {
                node_context.port_label_spacing_vertical
            };

        overlap_remover.with_start_coordinate(adjusted_start_coordinate).remove_overlaps();
        Self::write_back(node_context, port_side, &overlap_remover, &added);

        Self::make_relative_to_ports(node_context, port_side);
    }

    /// Copies the overlap remover's rectangles back into the label cells they
    /// came from (Swift's remover mutates the cells' own rectangle objects).
    fn write_back(
        node_context: &mut NodeContext,
        port_side: PortSide,
        overlap_remover: &RectangleStripOverlapRemover,
        added: &[(usize, usize)],
    ) {
        let rectangles = overlap_remover.original_rectangles();
        let port_contexts = node_context.port_contexts.get_or_empty_mut(port_side);
        for &(index, id) in added {
            if let Some(cell) = port_contexts[index].port_label_cell.as_mut() {
                cell.cell.cell_rectangle = rectangles[id];
            }
        }
    }

    /// The final loop of both constrained placements: label cell rectangles
    /// become relative to their ports.
    fn make_relative_to_ports(node_context: &mut NodeContext, port_side: PortSide) {
        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            let port_position = port_context.port_position;
            let Some(port_label_cell) = port_context.port_label_cell.as_mut() else { continue };
            if !port_label_cell.has_labels() {
                continue;
            }

            let rect = &mut port_label_cell.cell.cell_rectangle;
            rect.x -= port_position.x;
            rect.y -= port_position.y;
        }
    }
}
