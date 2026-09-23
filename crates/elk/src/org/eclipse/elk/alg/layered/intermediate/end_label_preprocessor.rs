//! Port of `alg/layered/intermediate/EndLabelPreprocessor.swift`.
//!
//! Places the head and tail labels of edges around the ports they attach to,
//! in one label cell per port, stores the cells in the node's
//! `InternalProperties.END_LABELS` property and extends the node margins.
//! `EndLabelSorter` may reorder a cell's labels and `EndLabelPostprocessor`
//! applies the cells after node placement.
//!
//! The Swift property value is a `[LPort: LabelCell]` dictionary whose label
//! cells are shared class instances (the sorter mutates them in place). The
//! port stores an [`EndLabelCells`] (port/cell pairs in port order, cells
//! shared as `Rc<RefCell<LabelCell>>`) in a `PropValue::Object`. Its readers
//! only look cells up by port or apply each cell independently, so the
//! dictionary's hash order is unobservable.

use std::rc::Rc;

use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::horizontal_label_alignment::HorizontalLabelAlignment;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::label_cell::{LabelCell, LabelCellRef};
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::vertical_label_alignment::VerticalLabelAlignment;
use crate::org::eclipse::elk::alg::common::overlaps::rectangle_strip_overlap_remover::OverlapRemovalDirection;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LGraphAdapters;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::elk_rectangle::ElkRectangle;
use crate::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use crate::org::eclipse::elk::core::options::label_side::LabelSide;
use crate::prelude::*;

/// The `[LPort: LabelCell]` value of `InternalProperties.END_LABELS`.
#[derive(Debug, Default)]
pub struct EndLabelCells(pub Vec<(LPortId, LabelCellRef)>);

impl EndLabelCells {
    /// `map[port]`.
    pub fn get(&self, port: LPortId) -> Option<&LabelCellRef> {
        self.0.iter().find(|(p, _)| *p == port).map(|(_, c)| c)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `map.values`.
    pub fn values(&self) -> impl Iterator<Item = &LabelCellRef> {
        self.0.iter().map(|(_, c)| c)
    }
}

#[derive(Default)]
pub struct EndLabelPreprocessor;

impl EndLabelPreprocessor {
    pub const NO_INCIDENT_EDGE_THICKNESS: f64 = -1.0;

    pub fn new() -> EndLabelPreprocessor {
        EndLabelPreprocessor
    }

    pub fn process_node(
        &self,
        lg: &mut LGraphArena,
        node: LNodeId,
        edge_label_spacing: f64,
        label_label_spacing: f64,
        vertical_layout: bool,
    ) {
        // Iterate over all ports and collect their labels in label cells
        let port_count = lg[node].ports.len();
        let mut port_label_cells: Vec<Option<LabelCellRef>> = vec![None; port_count];

        for port_index in 0..port_count {
            let port = lg[node].ports[port_index];
            lg[port].id = port_index as i32;

            let labels = Self::gather_labels(lg, port);
            port_label_cells[port_index] = self.create_configured_label_cell(lg, labels, label_label_spacing, vertical_layout);
        }

        // Actually go off and place them labels!
        self.place_labels_of_node(lg, node, &port_label_cells, label_label_spacing, edge_label_spacing, vertical_layout);

        // Turn the array into a map and save that in the node
        let mut port_to_label_cell_map: Vec<(LPortId, LabelCellRef)> = Vec::new();
        for (index, label_cell) in port_label_cells.iter().enumerate() {
            if let Some(label_cell) = label_cell {
                port_to_label_cell_map.push((lg[node].ports[index], Rc::clone(label_cell)));
            }
        }

        if !port_to_label_cell_map.is_empty() {
            lg[node].props.set(&InternalProperties::END_LABELS, PropValue::object(Rc::new(EndLabelCells(port_to_label_cell_map))));

            // Update the node's margins
            self.update_node_margins(lg, node, &port_label_cells);
        }
    }

    /// Creates label cell for the given port with the given labels, if any.
    pub fn create_configured_label_cell(
        &self,
        lg: &LGraphArena,
        labels: Option<Vec<LLabelId>>,
        label_label_spacing: f64,
        vertical_layout: bool,
    ) -> Option<LabelCellRef> {
        let labels = labels?;
        if labels.is_empty() {
            return None;
        }

        // Create the new label cell and setup its alignments depending on the port's side
        let mut label_cell = LabelCell::with_mode(label_label_spacing, !vertical_layout);

        for label in labels {
            label_cell.add_label(lg, LGraphAdapters::adapt_label(label));
        }

        // Setup the label cell's size
        label_cell.cell.cell_rectangle.height = label_cell.get_minimum_height();
        label_cell.cell.cell_rectangle.width = label_cell.get_minimum_width();

        Some(label_cell.into_ref())
    }

    // MARK: - Label Gathering

    /// Returns a list that contains all end labels to be placed at the given
    /// port, or `None` if no edge is incident to it.
    pub fn gather_labels(lg: &mut LGraphArena, port: LPortId) -> Option<Vec<LLabelId>> {
        let mut labels: Vec<LLabelId> = Vec::new();

        // Gather labels of the port itself
        let mut max_edge_thickness = Self::gather_labels_into(lg, port, &mut labels);

        // If it has a dummy associated with it, go through the dummy's ports
        if let Some(dummy_node) = lg[port].props.get_as::<LNodeId>(&InternalProperties::PORT_DUMMY) {
            for dummy_port in lg[dummy_node].ports.clone() {
                if let Some(origin) = lg[dummy_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) {
                    if origin == port {
                        max_edge_thickness = swift::max(max_edge_thickness, Self::gather_labels_into(lg, dummy_port, &mut labels));
                    }
                }
            }
        }

        // Only save the maximum edge thickness if we'll be interested in it later
        if !labels.is_empty() {
            lg[port].props.set(&InternalProperties::MAX_EDGE_THICKNESS, max_edge_thickness);
        }

        if max_edge_thickness != Self::NO_INCIDENT_EDGE_THICKNESS { Some(labels) } else { None }
    }

    /// Puts all relevant end labels of edges connected to the given port into
    /// the given list and returns the maximum incident edge thickness.
    pub fn gather_labels_into(lg: &mut LGraphArena, port: LPortId, target_list: &mut Vec<LLabelId>) -> f64 {
        let mut max_edge_thickness: f64 = -1.0;

        for incident_edge in lg.port_connected_edges(port) {
            let thickness: f64 = lg[incident_edge].props.get_as::<f64>(&LayeredOptions::EDGE_THICKNESS).unwrap_or(0.0);
            max_edge_thickness = swift::max(max_edge_thickness, thickness);

            // An outgoing edge contributes its tail labels, an incoming edge its head labels
            let wanted = if lg[incident_edge].source == Some(port) { EdgeLabelPlacement::TAIL } else { EdgeLabelPlacement::HEAD };
            let edge_labels: Vec<LLabelId> = lg[incident_edge]
                .labels
                .iter()
                .copied()
                .filter(|&label| lg[label].props.get_as::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT) == Some(wanted))
                .collect();
            target_list.extend_from_slice(&edge_labels);

            for label in edge_labels {
                if !lg[label].props.has(&InternalProperties::END_LABEL_EDGE) {
                    lg[label].props.set(&InternalProperties::END_LABEL_EDGE, PropValue::LEdge(incident_edge));
                }
            }
        }

        max_edge_thickness
    }

    // MARK: - Label Placement

    /// `placeLabels(node:portLabelCells:...)`: places end labels of all of the node's ports.
    pub fn place_labels_of_node(
        &self,
        lg: &LGraphArena,
        node: LNodeId,
        port_label_cells: &[Option<LabelCellRef>],
        _label_label_spacing: f64,
        edge_label_spacing: f64,
        _vertical_layout: bool,
    ) {
        for &port in &lg[node].ports {
            if let Some(label_cell) = &port_label_cells[lg[port].id as usize] {
                self.place_labels_at_port(lg, port, label_cell, edge_label_spacing);
            }
        }
    }

    /// `placeLabels(port:labelCell:edgeLabelSpacing:)`: places the edge end
    /// labels that are to be placed near the given port.
    pub fn place_labels_at_port(&self, lg: &LGraphArena, port: LPortId, label_cell: &LabelCellRef, edge_label_spacing: f64) {
        // Some necessary position information
        let Some(owner_node) = lg[port].owner else { return };
        let node_size = lg[owner_node].size;
        let node_margin = lg[owner_node].margin;
        let port_pos = lg[port].position;
        let port_anchor = KVector::sum(&[port_pos, lg[port].anchor]);

        let label_side = self.get_label_side(lg, &label_cell.borrow());
        let max_edge_thickness = self.max_edge_thickness(lg, port);
        let mut cell = label_cell.borrow_mut();
        let mut rect = cell.cell.cell_rectangle;

        // Calculate cell position depending on port side
        match lg[port].side {
            PortSide::NORTH => {
                cell.set_vertical_alignment(VerticalLabelAlignment::BOTTOM);
                rect.y = -node_margin.top - edge_label_spacing - rect.height;

                if label_side == LabelSide::ABOVE {
                    cell.set_horizontal_alignment(HorizontalLabelAlignment::RIGHT);
                    rect.x = port_anchor.x - max_edge_thickness - edge_label_spacing - rect.width;
                } else {
                    cell.set_horizontal_alignment(HorizontalLabelAlignment::LEFT);
                    rect.x = port_anchor.x + max_edge_thickness + edge_label_spacing;
                }
            }
            PortSide::EAST => {
                cell.set_horizontal_alignment(HorizontalLabelAlignment::LEFT);
                rect.x = node_size.x + node_margin.right + edge_label_spacing;

                if label_side == LabelSide::ABOVE {
                    cell.set_vertical_alignment(VerticalLabelAlignment::BOTTOM);
                    rect.y = port_anchor.y - max_edge_thickness - edge_label_spacing - rect.height;
                } else {
                    cell.set_vertical_alignment(VerticalLabelAlignment::TOP);
                    rect.y = port_anchor.y + max_edge_thickness + edge_label_spacing;
                }
            }
            PortSide::SOUTH => {
                cell.set_vertical_alignment(VerticalLabelAlignment::TOP);
                rect.y = node_size.y + node_margin.bottom + edge_label_spacing;

                if label_side == LabelSide::ABOVE {
                    cell.set_horizontal_alignment(HorizontalLabelAlignment::RIGHT);
                    rect.x = port_anchor.x - max_edge_thickness - edge_label_spacing - rect.width;
                } else {
                    cell.set_horizontal_alignment(HorizontalLabelAlignment::LEFT);
                    rect.x = port_anchor.x + max_edge_thickness + edge_label_spacing;
                }
            }
            PortSide::WEST => {
                cell.set_horizontal_alignment(HorizontalLabelAlignment::RIGHT);
                rect.x = -node_margin.left - edge_label_spacing - rect.width;

                if label_side == LabelSide::ABOVE {
                    cell.set_vertical_alignment(VerticalLabelAlignment::BOTTOM);
                    rect.y = port_anchor.y - max_edge_thickness - edge_label_spacing - rect.height;
                } else {
                    cell.set_vertical_alignment(VerticalLabelAlignment::TOP);
                    rect.y = port_anchor.y + max_edge_thickness + edge_label_spacing;
                }
            }
            PortSide::UNDEFINED => {}
        }

        cell.cell.cell_rectangle = rect;
    }

    // MARK: - Node Margins

    /// Updates the node's margins to account for its end labels.
    pub fn update_node_margins(&self, lg: &mut LGraphArena, node: LNodeId, label_cells: &[Option<LabelCellRef>]) {
        let node_margin = lg[node].margin;
        let node_size = lg[node].size;

        // Calculate the rectangle that describes the node's current margin
        let mut node_margin_rectangle = ElkRectangle::new(
            -node_margin.left,
            -node_margin.top,
            node_margin.left + node_size.x + node_margin.right,
            node_margin.top + node_size.y + node_margin.bottom,
        );

        // Union the rectangle with each rectangle that describes a label cell
        for label_cell in label_cells.iter().flatten() {
            node_margin_rectangle.union(&label_cell.borrow().cell.cell_rectangle);
        }

        // Reapply the new rectangle to the margin
        let node_margin = &mut lg[node].margin;
        node_margin.left = -node_margin_rectangle.x;
        node_margin.top = -node_margin_rectangle.y;
        node_margin.right = node_margin_rectangle.width - node_margin.left - node_size.x;
        node_margin.bottom = node_margin_rectangle.height - node_margin.top - node_size.y;
    }

    // MARK: - Utility Methods

    /// Retrieve the side of the edge the labels of the given cell should be
    /// placed at (the first label's `InternalProperties.LABEL_SIDE`, default
    /// `ABOVE`).
    pub fn get_label_side(&self, lg: &LGraphArena, label_cell: &LabelCell) -> LabelSide {
        let Some(first_label) = label_cell.get_labels().first() else { return LabelSide::ABOVE };
        let side_value: Option<LabelSide> = first_label.get_property(lg, &InternalProperties::LABEL_SIDE);
        side_value.unwrap_or(LabelSide::ABOVE)
    }

    /// Returns the maximum thickness of all edges incident to the port.
    pub fn max_edge_thickness(&self, lg: &LGraphArena, port: LPortId) -> f64 {
        lg[port].props.get_as::<f64>(&InternalProperties::MAX_EDGE_THICKNESS).unwrap_or(0.0)
    }

    /// Returns the overlap removal direction appropriate for the given port side.
    pub fn port_side_to_overlap_removal_direction(&self, port_side: PortSide) -> OverlapRemovalDirection {
        match port_side {
            PortSide::NORTH => OverlapRemovalDirection::UP,
            PortSide::SOUTH => OverlapRemovalDirection::DOWN,
            PortSide::EAST => OverlapRemovalDirection::RIGHT,
            PortSide::WEST => OverlapRemovalDirection::LEFT,
            PortSide::UNDEFINED => OverlapRemovalDirection::DOWN,
        }
    }
}

impl ILayoutProcessor for EndLabelPreprocessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("End label pre-processing", 1.0);

        let edge_label_spacing: f64 =
            lg[layered_graph].props.get_as::<f64>(&LayeredOptions::SPACING_EDGE_LABEL).unwrap_or(0.0);
        let label_label_spacing: f64 =
            lg[layered_graph].props.get_as::<f64>(&LayeredOptions::SPACING_LABEL_LABEL).unwrap_or(0.0);
        let direction = lg[layered_graph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
        let vertical_layout = direction.is_vertical();

        // We iterate over each node and place the end labels of its incident edges
        let nodes: Vec<LNodeId> = lg[layered_graph].layers.iter().flat_map(|&l| lg[l].nodes.iter().copied()).collect();
        for node in nodes {
            self.process_node(lg, node, edge_label_spacing, label_label_spacing, vertical_layout);
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "EndLabelPreprocessor"
    }
}
