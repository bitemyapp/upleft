//! Port of `alg/common/nodespacing/internal/algorithm/NodeLabelCellCreator.swift`.
//!
//! Knows how to take all of a node's labels and create the appropriate grid cells.

use std::rc::Rc;

use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::cell::CellRef;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::container_area::ContainerArea;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::grid_container_cell::GridContainerCell;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::label_cell::{LabelCell, LabelCellRef};
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::strip_container_cell::{Strip, StripContainerCell};
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_label_location::NodeLabelLocation;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LLabelAdapter;
use crate::org::eclipse::elk::core::options::node_label_placement::NodeLabelPlacement;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::prelude::*;

pub struct NodeLabelCellCreator;

impl NodeLabelCellCreator {
    /// Iterates over all of the node's labels and creates all required cell
    /// containers and label cells.
    pub fn create_node_label_cells(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        only_inside: bool,
        horizontal_layout_mode: bool,
    ) {
        Self::create_node_label_cell_containers(node_context, only_inside);

        for label in node_context.node.get_labels(lg) {
            Self::handle_node_label(lg, node_context, label, only_inside, horizontal_layout_mode);
        }
    }

    /// Handles the given node label by adding it to the corresponding node label cell.
    pub fn handle_node_label(
        lg: &LGraphArena,
        node_context: &mut NodeContext,
        label: LLabelAdapter,
        only_inside: bool,
        horizontal_layout_mode: bool,
    ) {
        // Find the effective label location
        let label_placement = if label.has_property(lg, &CoreOptions::NODE_LABELS_PLACEMENT) {
            label
                .get_property::<NodeLabelPlacement>(lg, &CoreOptions::NODE_LABELS_PLACEMENT)
                .unwrap_or(node_context.node_label_placement)
        } else {
            node_context.node_label_placement
        };
        let label_location = NodeLabelLocation::from_node_label_placement(label_placement);

        if label_location == NodeLabelLocation::UNDEFINED {
            return;
        }

        if only_inside && !label_location.is_inside_location() {
            return;
        }

        Self::retrieve_node_label_cell(node_context, label_location, horizontal_layout_mode).borrow_mut().add_label(lg, label);
    }

    // Cell Creation and Retrieval

    /// Creates all node label containers.
    pub fn create_node_label_cell_containers(node_context: &mut NodeContext, only_inside: bool) {
        let symmetry = !node_context.size_options.contains(SizeOptions::ASYMMETRICAL);
        let tabular_node_labels = node_context.size_options.contains(SizeOptions::FORCE_TABULAR_NODE_LABELS);

        // Inside container
        let inside = GridContainerCell::new_ref(tabular_node_labels, symmetry, node_context.label_cell_spacing);
        inside.borrow_mut().cell.padding.copy_from(&node_context.node_labels_padding);
        node_context.inside_node_label_container = Some(Rc::clone(&inside));
        node_context.node_container_middle_row.borrow_mut().set_cell(ContainerArea::CENTER, Some(CellRef::Grid(inside)));

        // Outside containers, if requested
        if !only_inside {
            let north_container = StripContainerCell::new_ref(Strip::HORIZONTAL, symmetry, node_context.label_cell_spacing);
            north_container.borrow_mut().cell.padding.bottom = node_context.node_label_spacing;
            node_context.outside_node_label_containers[PortSide::NORTH.ordinal()] = Some(north_container);

            let south_container = StripContainerCell::new_ref(Strip::HORIZONTAL, symmetry, node_context.label_cell_spacing);
            south_container.borrow_mut().cell.padding.top = node_context.node_label_spacing;
            node_context.outside_node_label_containers[PortSide::SOUTH.ordinal()] = Some(south_container);

            let west_container = StripContainerCell::new_ref(Strip::VERTICAL, symmetry, node_context.label_cell_spacing);
            west_container.borrow_mut().cell.padding.right = node_context.node_label_spacing;
            node_context.outside_node_label_containers[PortSide::WEST.ordinal()] = Some(west_container);

            let east_container = StripContainerCell::new_ref(Strip::VERTICAL, symmetry, node_context.label_cell_spacing);
            east_container.borrow_mut().cell.padding.left = node_context.node_label_spacing;
            node_context.outside_node_label_containers[PortSide::EAST.ordinal()] = Some(east_container);
        }
    }

    /// Retrieves the node label cell for the given location. If it doesn't
    /// exist yet, it is created.
    pub fn retrieve_node_label_cell(
        node_context: &mut NodeContext,
        node_label_location: NodeLabelLocation,
        horizontal_layout_mode: bool,
    ) -> LabelCellRef {
        if let Some(existing) = &node_context.node_label_cells[node_label_location.ordinal()] {
            return Rc::clone(existing);
        }

        // The node label cell doesn't exist yet, so create one and add it to the relevant container
        let new_label_cell =
            LabelCell::with_location(node_context.label_label_spacing, node_label_location, horizontal_layout_mode).into_ref();
        node_context.node_label_cells[node_label_location.ordinal()] = Some(Rc::clone(&new_label_cell));

        // Find the correct container and add the cell to it
        if node_label_location.is_inside_location() {
            if let Some(container) = &node_context.inside_node_label_container {
                container.borrow_mut().set_cell(
                    node_label_location.get_container_row(),
                    node_label_location.get_container_column(),
                    Some(CellRef::Label(Rc::clone(&new_label_cell))),
                );
            }
        } else {
            let outside_side = node_label_location.get_outside_side();
            // Swift: `assertionFailure` (a no-op in release builds) if missing.
            let Some(container_cell) = node_context.outside_node_label_container(outside_side).cloned() else {
                return new_label_cell;
            };

            match outside_side {
                PortSide::NORTH | PortSide::SOUTH => {
                    new_label_cell.borrow_mut().cell.set_contributes_to_minimum_height(true);
                    container_cell
                        .borrow_mut()
                        .set_cell(node_label_location.get_container_column(), Some(CellRef::Label(Rc::clone(&new_label_cell))));
                }
                PortSide::WEST | PortSide::EAST => {
                    new_label_cell.borrow_mut().cell.set_contributes_to_minimum_width(true);
                    container_cell
                        .borrow_mut()
                        .set_cell(node_label_location.get_container_row(), Some(CellRef::Label(Rc::clone(&new_label_cell))));
                }
                _ => {}
            }
        }

        new_label_cell
    }
}
