//! Port of `alg/common/nodespacing/internal/algorithm/PortPlacementCalculator.swift`.
//!
//! Actually places ports (into the port contexts' `port_position`).

use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::port_context::PortContext;
use crate::org::eclipse::elk::core::options::port_alignment::PortAlignment;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::org::eclipse::elk::graph::properties::keys;
use crate::prelude::*;

pub struct PortPlacementCalculator;

impl PortPlacementCalculator {
    /// `PORT_RATIO_OR_POSITION = Property<Double>("portRatioOrPosition", 0.0)`.
    pub const PORT_RATIO_OR_POSITION: Property = Property::with_default(keys::PORT_RATIO_OR_POSITION, || PropValue::Double(0.0));

    // MARK: - Horizontal Port Placement

    pub fn place_horizontal_ports(lg: &LGraphArena, node_context: &mut NodeContext) {
        match node_context.port_constraints {
            PortConstraints::FIXED_POS => {
                Self::place_horizontal_fixed_pos_ports(lg, node_context, PortSide::NORTH);
                Self::place_horizontal_fixed_pos_ports(lg, node_context, PortSide::SOUTH);
            }
            PortConstraints::FIXED_RATIO => {
                Self::place_horizontal_fixed_ratio_ports(lg, node_context, PortSide::NORTH);
                Self::place_horizontal_fixed_ratio_ports(lg, node_context, PortSide::SOUTH);
            }
            _ => {
                Self::place_horizontal_free_ports(lg, node_context, PortSide::NORTH);
                Self::place_horizontal_free_ports(lg, node_context, PortSide::SOUTH);
            }
        }
    }

    pub fn place_horizontal_fixed_pos_ports(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            port_context.port_position.y = Self::calculate_horizontal_port_y_coordinate(lg, port_context);
        }
    }

    pub fn place_horizontal_fixed_ratio_ports(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let node_width = node_context.node_size.x;

        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            let ratio: f64 = port_context.port.get_property::<f64>(lg, &Self::PORT_RATIO_OR_POSITION).unwrap_or(0.0);
            port_context.port_position.x = node_width * ratio;
            port_context.port_position.y = Self::calculate_horizontal_port_y_coordinate(lg, port_context);
        }
    }

    pub fn place_horizontal_free_ports(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let port_count = node_context.port_contexts.get_or_empty(port_side).len();
        if port_count == 0 {
            return;
        }

        let Some(inside_port_label_cell) = node_context.inside_port_label_cell(port_side) else { return };
        let (inside_port_label_cell_rectangle, inside_port_label_cell_padding, minimum_content_area_size) = {
            let c = inside_port_label_cell.borrow();
            (c.cell.cell_rectangle, c.cell.padding, c.minimum_content_area_size)
        };

        let mut port_alignment = node_context.get_port_alignment(lg, port_side);
        let available_space =
            inside_port_label_cell_rectangle.width - inside_port_label_cell_padding.left - inside_port_label_cell_padding.right;
        let mut calculated_port_placement_width = minimum_content_area_size.x;
        let mut current_x_pos = inside_port_label_cell_rectangle.x + inside_port_label_cell_padding.left;
        let mut space_between_ports = node_context.port_port_spacing;

        if (port_alignment == PortAlignment::DISTRIBUTED || port_alignment == PortAlignment::JUSTIFIED) && port_count == 1 {
            calculated_port_placement_width =
                Self::modified_port_placement_size(node_context, port_alignment, calculated_port_placement_width);
            port_alignment = PortAlignment::CENTER;
        }

        if available_space < calculated_port_placement_width && !node_context.size_options.contains(SizeOptions::PORTS_OVERHANG) {
            if port_alignment == PortAlignment::DISTRIBUTED {
                space_between_ports += (available_space - calculated_port_placement_width) / (port_count + 1) as f64;
                current_x_pos += space_between_ports;
            } else {
                space_between_ports += (available_space - calculated_port_placement_width) / (port_count as f64 - 1.0);
            }
        } else {
            if available_space < calculated_port_placement_width {
                calculated_port_placement_width =
                    Self::modified_port_placement_size(node_context, port_alignment, calculated_port_placement_width);
                port_alignment = PortAlignment::CENTER;
            }

            match port_alignment {
                PortAlignment::BEGIN => {}
                PortAlignment::CENTER => {
                    current_x_pos += (available_space - calculated_port_placement_width) / 2.0;
                }
                PortAlignment::END => {
                    current_x_pos += available_space - calculated_port_placement_width;
                }
                PortAlignment::DISTRIBUTED => {
                    let additional_space_between_ports =
                        (available_space - calculated_port_placement_width) / (port_count + 1) as f64;
                    space_between_ports += swift::max(0.0, additional_space_between_ports);
                    current_x_pos += space_between_ports;
                }
                PortAlignment::JUSTIFIED => {
                    let additional_space_between_ports =
                        (available_space - calculated_port_placement_width) / (port_count as f64 - 1.0);
                    space_between_ports += swift::max(0.0, additional_space_between_ports);
                }
            }
        }

        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            port_context.port_position.x = current_x_pos + port_context.port_margin.left;
            port_context.port_position.y = Self::calculate_horizontal_port_y_coordinate(lg, port_context);
            current_x_pos += port_context.port_margin.left
                + port_context.port.get_size(lg).x
                + port_context.port_margin.right
                + space_between_ports;
        }
    }

    pub fn calculate_horizontal_port_y_coordinate(lg: &LGraphArena, port_context: &PortContext) -> f64 {
        let port = &port_context.port;

        if port.has_property(lg, &CoreOptions::PORT_BORDER_OFFSET) {
            let offset: f64 = port.get_property::<f64>(lg, &CoreOptions::PORT_BORDER_OFFSET).unwrap_or(0.0);
            if port.get_side(lg) == PortSide::NORTH {
                -port.get_size(lg).y - offset
            } else {
                offset
            }
        } else if port.get_side(lg) == PortSide::NORTH {
            -port.get_size(lg).y
        } else {
            0.0
        }
    }

    // MARK: - Vertical Port Placement

    pub fn place_vertical_ports(lg: &LGraphArena, node_context: &mut NodeContext) {
        match node_context.port_constraints {
            PortConstraints::FIXED_POS => {
                Self::place_vertical_fixed_pos_ports(lg, node_context, PortSide::EAST);
                Self::place_vertical_fixed_pos_ports(lg, node_context, PortSide::WEST);
            }
            PortConstraints::FIXED_RATIO => {
                Self::place_vertical_fixed_ratio_ports(lg, node_context, PortSide::EAST);
                Self::place_vertical_fixed_ratio_ports(lg, node_context, PortSide::WEST);
            }
            _ => {
                Self::place_vertical_free_ports(lg, node_context, PortSide::EAST);
                Self::place_vertical_free_ports(lg, node_context, PortSide::WEST);
            }
        }
    }

    pub fn place_vertical_fixed_pos_ports(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let node_width = node_context.node_size.x;

        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            port_context.port_position.x = Self::calculate_vertical_port_x_coordinate(lg, port_context, node_width);
        }
    }

    pub fn place_vertical_fixed_ratio_ports(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let node_size = node_context.node_size;

        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            port_context.port_position.x = Self::calculate_vertical_port_x_coordinate(lg, port_context, node_size.x);
            let ratio: f64 = port_context.port.get_property::<f64>(lg, &Self::PORT_RATIO_OR_POSITION).unwrap_or(0.0);
            port_context.port_position.y = node_size.y * ratio;
        }
    }

    pub fn place_vertical_free_ports(lg: &LGraphArena, node_context: &mut NodeContext, port_side: PortSide) {
        let port_count = node_context.port_contexts.get_or_empty(port_side).len();
        if port_count == 0 {
            return;
        }

        let Some(inside_port_label_cell) = node_context.inside_port_label_cell(port_side) else { return };
        let (inside_port_label_cell_rectangle, inside_port_label_cell_padding, minimum_content_area_size) = {
            let c = inside_port_label_cell.borrow();
            (c.cell.cell_rectangle, c.cell.padding, c.minimum_content_area_size)
        };

        let mut port_alignment = node_context.get_port_alignment(lg, port_side);
        let available_space =
            inside_port_label_cell_rectangle.height - inside_port_label_cell_padding.top - inside_port_label_cell_padding.bottom;
        let mut calculated_port_placement_height = minimum_content_area_size.y;
        let mut current_y_pos = inside_port_label_cell_rectangle.y + inside_port_label_cell_padding.top;
        let mut space_between_ports = node_context.port_port_spacing;
        let node_width = node_context.node_size.x;

        if (port_alignment == PortAlignment::DISTRIBUTED || port_alignment == PortAlignment::JUSTIFIED) && port_count == 1 {
            calculated_port_placement_height =
                Self::modified_port_placement_size(node_context, port_alignment, calculated_port_placement_height);
            port_alignment = PortAlignment::CENTER;
        }

        if available_space < calculated_port_placement_height
            && !node_context.size_options.contains(SizeOptions::PORTS_OVERHANG)
        {
            if port_alignment == PortAlignment::DISTRIBUTED {
                space_between_ports += (available_space - calculated_port_placement_height) / (port_count + 1) as f64;
                current_y_pos += space_between_ports;
            } else {
                space_between_ports += (available_space - calculated_port_placement_height) / (port_count as f64 - 1.0);
            }
        } else {
            if available_space < calculated_port_placement_height {
                calculated_port_placement_height =
                    Self::modified_port_placement_size(node_context, port_alignment, calculated_port_placement_height);
                port_alignment = PortAlignment::CENTER;
            }

            match port_alignment {
                PortAlignment::BEGIN => {}
                PortAlignment::CENTER => {
                    current_y_pos += (available_space - calculated_port_placement_height) / 2.0;
                }
                PortAlignment::END => {
                    current_y_pos += available_space - calculated_port_placement_height;
                }
                PortAlignment::DISTRIBUTED => {
                    let additional_space_between_ports =
                        (available_space - calculated_port_placement_height) / (port_count + 1) as f64;
                    space_between_ports += swift::max(0.0, additional_space_between_ports);
                    current_y_pos += space_between_ports;
                }
                PortAlignment::JUSTIFIED => {
                    let additional_space_between_ports =
                        (available_space - calculated_port_placement_height) / (port_count as f64 - 1.0);
                    space_between_ports += swift::max(0.0, additional_space_between_ports);
                }
            }
        }

        for port_context in node_context.port_contexts.get_or_empty_mut(port_side) {
            port_context.port_position.x = Self::calculate_vertical_port_x_coordinate(lg, port_context, node_width);
            port_context.port_position.y = current_y_pos + port_context.port_margin.top;
            current_y_pos += port_context.port_margin.top
                + port_context.port.get_size(lg).y
                + port_context.port_margin.bottom
                + space_between_ports;
        }
    }

    pub fn calculate_vertical_port_x_coordinate(lg: &LGraphArena, port_context: &PortContext, node_width: f64) -> f64 {
        let port = &port_context.port;
        if port.has_property(lg, &CoreOptions::PORT_BORDER_OFFSET) {
            let offset: f64 = port.get_property::<f64>(lg, &CoreOptions::PORT_BORDER_OFFSET).unwrap_or(0.0);
            if port.get_side(lg) == PortSide::WEST {
                -port.get_size(lg).x - offset
            } else {
                node_width + offset
            }
        } else if port.get_side(lg) == PortSide::WEST {
            -port.get_size(lg).x
        } else {
            node_width
        }
    }

    // MARK: - Utilities

    pub fn modified_port_placement_size(
        node_context: &NodeContext,
        old_port_alignment: PortAlignment,
        current_port_placement_size: f64,
    ) -> f64 {
        if old_port_alignment == PortAlignment::DISTRIBUTED {
            current_port_placement_size - 2.0 * node_context.port_port_spacing
        } else {
            current_port_placement_size
        }
    }
}
