//! Port of `alg/common/nodespacing/NodeLabelAndSizeCalculator.swift`.
//!
//! Knows how to calculate the size of a node and how to place its ports.

use super::cellsystem::container_area::ContainerArea;
use super::internal::algorithm::cell_system_configurator::CellSystemConfigurator;
use super::internal::algorithm::horizontal_port_placement_size_calculator::HorizontalPortPlacementSizeCalculator;
use super::internal::algorithm::inside_port_label_cell_creator::InsidePortLabelCellCreator;
use super::internal::algorithm::label_placer::LabelPlacer;
use super::internal::algorithm::node_label_and_size_utilities::NodeLabelAndSizeUtilities;
use super::internal::algorithm::node_label_cell_creator::NodeLabelCellCreator;
use super::internal::algorithm::node_size_calculator::NodeSizeCalculator;
use super::internal::algorithm::port_context_creator::PortContextCreator;
use super::internal::algorithm::port_label_placement_calculator::PortLabelPlacementCalculator;
use super::internal::algorithm::port_placement_calculator::PortPlacementCalculator;
use super::internal::algorithm::vertical_port_placement_size_calculator::VerticalPortPlacementSizeCalculator;
use super::internal::node_context::NodeContext;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::{LGraphAdapter, LNodeAdapter};
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::prelude::*;

pub struct NodeLabelAndSizeCalculator;

impl NodeLabelAndSizeCalculator {
    /// `process(_ graph:)`: processes all direct children of the given graph.
    pub fn process_graph(lg: &mut LGraphArena, graph: &LGraphAdapter) {
        for node in graph.get_nodes(lg) {
            let _ = Self::process(lg, graph, &node, true, false);
        }
    }

    /// `process(_ graph:, _ node:, _ applyStuff:, _ ignoreInsidePortLabels:)`:
    /// processes the given node, which is assumed to be a child of the given
    /// graph, and returns the node size (the context's vector, which Swift
    /// also gave to the node when applying).
    pub fn process(
        lg: &mut LGraphArena,
        graph: &LGraphAdapter,
        node: &LNodeAdapter,
        apply_stuff: bool,
        ignore_inside_port_labels: bool,
    ) -> KVector {
        let mut node_context = NodeContext::new(lg, graph, *node);

        PortContextCreator::create_port_contexts(lg, &mut node_context, ignore_inside_port_labels);

        // PHASE 1: Setup All Cells
        let mut horizontal_layout_mode = true;
        let layout_direction: Option<Direction> = graph.get_property(lg, &CoreOptions::DIRECTION);
        if let Some(dir) = layout_direction {
            horizontal_layout_mode = dir == Direction::UNDEFINED || dir.is_horizontal();
        }

        NodeLabelCellCreator::create_node_label_cells(lg, &mut node_context, false, horizontal_layout_mode);
        InsidePortLabelCellCreator::create_inside_port_label_cells(&mut node_context);

        // PHASE 2: Setup Client Area Space and Node Cell Padding
        NodeLabelAndSizeUtilities::setup_minimum_client_area_size(lg, &mut node_context);
        NodeLabelAndSizeUtilities::setup_node_padding_for_ports_with_offset(lg, &mut node_context);

        // PHASE 3: Minimum Space Required to Place Ports
        HorizontalPortPlacementSizeCalculator::calculate_horizontal_port_placement_size(lg, &mut node_context);
        VerticalPortPlacementSizeCalculator::calculate_vertical_port_placement_size(lg, &mut node_context);

        // PHASE 4: Setup Cell System Size Contribution Flags
        CellSystemConfigurator::configure_cell_system_size_contributions(&node_context);

        // PHASE 5: Set Node Width and Place Horizontal Ports
        NodeSizeCalculator::set_node_width(lg, &mut node_context);
        PortPlacementCalculator::place_horizontal_ports(lg, &mut node_context);
        PortLabelPlacementCalculator::place_horizontal_port_labels(lg, &mut node_context);

        // PHASE 6: Set Node Height and Place Vertical Ports
        CellSystemConfigurator::update_vertical_inside_port_label_cell_padding(&node_context);
        NodeSizeCalculator::set_node_height(lg, &mut node_context);

        if !apply_stuff {
            return node_context.node_size;
        }

        NodeLabelAndSizeUtilities::offset_southern_ports_by_node_size(&mut node_context);
        PortPlacementCalculator::place_vertical_ports(lg, &mut node_context);
        PortLabelPlacementCalculator::place_vertical_port_labels(lg, &mut node_context);

        // PHASE 7: Place Labels and Apply Stuff
        LabelPlacer::place_labels(lg, &node_context);
        NodeLabelAndSizeUtilities::set_node_padding(lg, &node_context);
        NodeLabelAndSizeUtilities::apply_stuff(lg, &node_context);

        node_context.node_size
    }

    /// Computes the padding required to place inside non-center node labels.
    pub fn compute_inside_node_label_padding(
        lg: &LGraphArena,
        graph: &LGraphAdapter,
        node: &LNodeAdapter,
        layout_direction: Direction,
    ) -> ElkPadding {
        let mut node_context = NodeContext::new(lg, graph, *node);
        NodeLabelCellCreator::create_node_label_cells(lg, &mut node_context, true, !layout_direction.is_vertical());

        let Some(label_cell_container) = &node_context.inside_node_label_container else {
            return ElkPadding::default();
        };
        let label_cell_container = label_cell_container.borrow();
        let mut padding = ElkPadding::default();

        // Top
        for col in ContainerArea::ALL {
            if let Some(label_cell) = label_cell_container.get_cell(ContainerArea::BEGIN, col) {
                padding.top = swift::max(padding.top, label_cell.get_minimum_height());
            }
        }

        // Bottom
        for col in ContainerArea::ALL {
            if let Some(label_cell) = label_cell_container.get_cell(ContainerArea::END, col) {
                padding.bottom = swift::max(padding.bottom, label_cell.get_minimum_height());
            }
        }

        // Left
        for row in ContainerArea::ALL {
            if let Some(label_cell) = label_cell_container.get_cell(row, ContainerArea::BEGIN) {
                padding.left = swift::max(padding.left, label_cell.get_minimum_width());
            }
        }

        // Right
        for row in ContainerArea::ALL {
            if let Some(label_cell) = label_cell_container.get_cell(row, ContainerArea::END) {
                padding.right = swift::max(padding.right, label_cell.get_minimum_width());
            }
        }

        // Apply insets and gap where necessary
        let container_padding = label_cell_container.cell.padding;
        let gap = label_cell_container.get_gap();
        if padding.top > 0.0 {
            padding.top += container_padding.top;
            padding.top += gap;
        }

        if padding.bottom > 0.0 {
            padding.bottom += container_padding.bottom;
            padding.bottom += gap;
        }

        if padding.left > 0.0 {
            padding.left += container_padding.left;
            padding.left += gap;
        }

        if padding.right > 0.0 {
            padding.right += container_padding.right;
            padding.right += gap;
        }

        padding
    }
}
