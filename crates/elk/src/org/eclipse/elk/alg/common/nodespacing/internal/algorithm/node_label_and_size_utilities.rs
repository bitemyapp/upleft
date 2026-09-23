//! Port of `alg/common/nodespacing/internal/algorithm/NodeLabelAndSizeUtilities.swift`.
//!
//! Various little methods that didn't quite fit into any of the other classes.
//! Also holds the two `ElkUtil` helpers on `PortAdapter`s this package uses
//! (`ElkUtil.computeInsidePart(_:_:)` and `ElkUtil.getLabelsBounds(_:)`),
//! ported here because they are defined on this crate's adapter types.

use std::cell::RefCell;
use std::rc::Rc;

use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LPortAdapter;
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::org::eclipse::elk::core::math::elk_rectangle::ElkRectangle;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::org::eclipse::elk::core::util::elk_util::ElkUtil;
use crate::prelude::*;

pub struct NodeLabelAndSizeUtilities;

impl NodeLabelAndSizeUtilities {
    // MARK: - Algorithm Phase Implementation Fragments

    pub fn setup_minimum_client_area_size(lg: &LGraphArena, node_context: &mut NodeContext) {
        let min_size = Self::get_minimum_client_area_size(lg, node_context);
        if let (Some(min_size), Some(container)) = (min_size, &node_context.inside_node_label_container) {
            container.borrow_mut().set_center_cell_minimum_size(min_size);
        }
    }

    pub fn setup_node_padding_for_ports_with_offset(lg: &LGraphArena, node_context: &mut NodeContext) {
        let node_container = Rc::clone(&node_context.node_container);
        let mut node_container = node_container.borrow_mut();
        let node_cell_padding = &mut node_container.cell.padding;

        for port_context_list in node_context.port_contexts.values() {
            for port_context in port_context_list {
                let mut port_border_offset = 0.0;
                if port_context.port.has_property(lg, &CoreOptions::PORT_BORDER_OFFSET) {
                    port_border_offset = port_context.port.get_property::<f64>(lg, &CoreOptions::PORT_BORDER_OFFSET).unwrap_or(0.0);

                    if port_border_offset < 0.0 {
                        match port_context.port.get_side(lg) {
                            PortSide::NORTH => node_cell_padding.top = swift::max(node_cell_padding.top, -port_border_offset),
                            PortSide::SOUTH => node_cell_padding.bottom = swift::max(node_cell_padding.bottom, -port_border_offset),
                            PortSide::EAST => node_cell_padding.right = swift::max(node_cell_padding.right, -port_border_offset),
                            PortSide::WEST => node_cell_padding.left = swift::max(node_cell_padding.left, -port_border_offset),
                            _ => {}
                        }
                    }
                }
                if PortLabelPlacement::is_fixed(node_context.port_labels_placement) {
                    let inside_part = Self::compute_inside_part(lg, &port_context.port, port_border_offset);
                    let size_opts = node_context
                        .node
                        .get_property::<SizeOptions>(lg, &CoreOptions::NODE_SIZE_OPTIONS)
                        .unwrap_or(SizeOptions::empty());
                    let symmetry = !size_opts.contains(SizeOptions::ASYMMETRICAL);
                    let inside_part_is_bigger;
                    match port_context.port.get_side(lg) {
                        PortSide::NORTH => {
                            inside_part_is_bigger = inside_part > node_cell_padding.top;
                            node_cell_padding.top = swift::max(node_cell_padding.top, inside_part);
                            if symmetry && inside_part_is_bigger {
                                node_cell_padding.top = swift::max(node_cell_padding.top, node_cell_padding.bottom);
                                node_cell_padding.bottom = node_cell_padding.top + port_border_offset;
                            }
                        }
                        PortSide::SOUTH => {
                            inside_part_is_bigger = inside_part > node_cell_padding.bottom;
                            node_cell_padding.bottom = swift::max(node_cell_padding.bottom, inside_part);
                            if symmetry && inside_part_is_bigger {
                                node_cell_padding.bottom = swift::max(node_cell_padding.bottom, node_cell_padding.top);
                                node_cell_padding.top = node_cell_padding.bottom + port_border_offset;
                            }
                        }
                        PortSide::EAST => {
                            inside_part_is_bigger = inside_part > node_cell_padding.right;
                            node_cell_padding.right = swift::max(node_cell_padding.right, inside_part);
                            if symmetry && inside_part_is_bigger {
                                node_cell_padding.right = swift::max(node_cell_padding.left, node_cell_padding.right);
                                node_cell_padding.left = node_cell_padding.right + port_border_offset;
                            }
                        }
                        PortSide::WEST => {
                            inside_part_is_bigger = inside_part > node_cell_padding.left;
                            node_cell_padding.left = swift::max(node_cell_padding.left, inside_part);
                            if symmetry && inside_part_is_bigger {
                                node_cell_padding.left = swift::max(node_cell_padding.left, node_cell_padding.right);
                                node_cell_padding.right = node_cell_padding.left + port_border_offset;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn offset_southern_ports_by_node_size(node_context: &mut NodeContext) {
        let node_height = node_context.node_size.y;

        for port_context in node_context.port_contexts.get_or_empty_mut(PortSide::SOUTH) {
            port_context.port_position.y += node_height;
        }
    }

    pub fn set_node_padding(lg: &mut LGraphArena, node_context: &NodeContext) {
        if !node_context.size_options.contains(SizeOptions::COMPUTE_PADDING) {
            return;
        }

        let node_rect = node_context.node_container.borrow().cell.cell_rectangle;
        let Some(container) = &node_context.inside_node_label_container else { return };
        let client_area = container.borrow().get_center_cell_rectangle();
        let mut node_padding = ElkPadding::default();

        node_padding.left = client_area.x - node_rect.x;
        node_padding.top = client_area.y - node_rect.y;
        node_padding.right = (node_rect.x + node_rect.width) - (client_area.x + client_area.width);
        node_padding.bottom = (node_rect.y + node_rect.height) - (client_area.y + client_area.height);

        node_context.node.set_padding(lg, &node_padding);
    }

    pub fn apply_stuff(lg: &mut LGraphArena, node_context: &NodeContext) {
        node_context.apply_node_size(lg);
        for port_context_list in node_context.port_contexts.values() {
            for pc in port_context_list {
                pc.apply_port_position(lg);
            }
        }
    }

    // MARK: - Minimum Size Things

    pub fn get_minimum_client_area_size(lg: &LGraphArena, node_context: &NodeContext) -> Option<KVector> {
        if node_context.size_constraints.contains(SizeConstraint::MINIMUM_SIZE)
            && node_context.size_options.contains(SizeOptions::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING)
        {
            Some(Self::get_minimum_node_or_client_area_size(lg, node_context))
        } else {
            None
        }
    }

    pub fn get_minimum_node_size(lg: &LGraphArena, node_context: &NodeContext) -> Option<KVector> {
        if node_context.size_constraints.contains(SizeConstraint::MINIMUM_SIZE) {
            if !node_context.size_options.contains(SizeOptions::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING) {
                return Some(Self::get_minimum_node_or_client_area_size(lg, node_context));
            }
        }
        None
    }

    pub fn get_minimum_node_or_client_area_size(lg: &LGraphArena, node_context: &NodeContext) -> KVector {
        let raw_min_size: Option<Rc<RefCell<KVector>>> =
            node_context.node.get_property(lg, &CoreOptions::NODE_SIZE_MINIMUM);
        let mut min_size = raw_min_size.map(|v| *v.borrow()).unwrap_or_default();

        if node_context.size_options.contains(SizeOptions::DEFAULT_MINIMUM_SIZE) {
            if min_size.x <= 0.0 {
                min_size.x = ElkUtil::DEFAULT_MIN_WIDTH;
            }
            if min_size.y <= 0.0 {
                min_size.y = ElkUtil::DEFAULT_MIN_HEIGHT;
            }
        }

        min_size
    }

    // MARK: - Utilities

    pub const EFFECTIVELY_FIXED_SIZE_CONSTRAINTS: SizeConstraint = SizeConstraint::PORT_LABELS;

    pub fn are_size_constraints_fixed(node_context: &NodeContext) -> bool {
        node_context.size_constraints.is_empty() || node_context.size_constraints == Self::EFFECTIVELY_FIXED_SIZE_CONSTRAINTS
    }

    pub fn is_first_outside_port_label_placed_differently(node_context: &NodeContext, port_side: PortSide) -> bool {
        let Some(port_contexts) = node_context.port_contexts.get(port_side) else { return false };
        if port_contexts.len() < 2 {
            return false;
        }

        let first_port = &port_contexts[0];

        let always_same_side = node_context.port_labels_placement.contains(PortLabelPlacement::ALWAYS_SAME_SIDE);
        let space_efficient = node_context.port_labels_placement.contains(PortLabelPlacement::SPACE_EFFICIENT);

        !first_port.labels_next_to_port && !always_same_side && (port_contexts.len() == 2 || space_efficient)
    }

    // MARK: - ElkUtil helpers on port adapters

    /// `ElkUtil.computeInsidePart(_ labelPos:, _ labelSize:, _ portSize:, _ labelSpacing:, _ portSide:)`.
    pub fn compute_inside_part_of_label(
        label_pos: KVector,
        label_size: KVector,
        port_size: KVector,
        _label_spacing: f64,
        port_side: PortSide,
    ) -> f64 {
        match port_side {
            PortSide::EAST | PortSide::WEST => {
                let inside_end = swift::min(label_pos.x + label_size.x, port_size.x);
                let inside_start = swift::max(label_pos.x, 0.0);
                swift::max(0.0, inside_end - inside_start)
            }
            PortSide::NORTH | PortSide::SOUTH => {
                let inside_end = swift::min(label_pos.y + label_size.y, port_size.y);
                let inside_start = swift::max(label_pos.y, 0.0);
                swift::max(0.0, inside_end - inside_start)
            }
            _ => 0.0,
        }
    }

    /// `ElkUtil.computeInsidePart(_ port: PortAdapter, _ portBorderOffset:)`: the
    /// maximum amount by which any label of the port (or the port itself)
    /// extends inside the node.
    pub fn compute_inside_part(lg: &LGraphArena, port: &LPortAdapter, port_border_offset: f64) -> f64 {
        let port_size = port.get_size(lg);
        let port_side = port.get_side(lg);
        let mut max_inside_part = 0.0;

        for &label in &lg[port.element].labels {
            let label_pos = lg[label].position;
            let label_size = lg[label].size;
            let inside_part = Self::compute_inside_part_of_label(label_pos, label_size, port_size, 0.0, port_side);
            max_inside_part = swift::max(max_inside_part, inside_part);
        }

        // The port itself extends inside by its size minus the border offset
        match port_side {
            PortSide::NORTH | PortSide::SOUTH => {
                max_inside_part = swift::max(max_inside_part, port_size.y + port_border_offset);
            }
            PortSide::EAST | PortSide::WEST => {
                max_inside_part = swift::max(max_inside_part, port_size.x + port_border_offset);
            }
            _ => {}
        }

        max_inside_part
    }

    /// `ElkUtil.getLabelsBounds(_ port: PortAdapter)`: the bounding box of all
    /// labels of the port, relative to the port's position.
    pub fn get_labels_bounds(lg: &LGraphArena, port: &LPortAdapter) -> ElkRectangle {
        let mut bounds = ElkRectangle::default();
        let mut initialized = false;

        for &label in &lg[port.element].labels {
            let label_pos = lg[label].position;
            let label_size = lg[label].size;

            if !initialized {
                bounds.x = label_pos.x;
                bounds.y = label_pos.y;
                bounds.width = label_size.x;
                bounds.height = label_size.y;
                initialized = true;
            } else {
                let right = swift::max(bounds.x + bounds.width, label_pos.x + label_size.x);
                let bottom = swift::max(bounds.y + bounds.height, label_pos.y + label_size.y);
                bounds.x = swift::min(bounds.x, label_pos.x);
                bounds.y = swift::min(bounds.y, label_pos.y);
                bounds.width = right - bounds.x;
                bounds.height = bottom - bounds.y;
            }
        }

        bounds
    }
}
