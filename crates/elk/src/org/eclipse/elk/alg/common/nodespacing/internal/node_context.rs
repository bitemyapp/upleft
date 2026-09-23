//! Port of `alg/common/nodespacing/internal/NodeContext.swift`.
//!
//! Data holder passed around the size calculation so the calculation classes
//! need no state of their own. The cells are shared (`Rc<RefCell<_>>`)
//! between the maps here and the containers of the cell system, as in Swift.
//! The dictionaries keyed by `PortSide` / `NodeLabelLocation` are arrays
//! indexed by ordinal; the only order-dependent iteration over them
//! (`LabelPlacer` over `nodeLabelCells`) is order-independent, see there.

use std::rc::Rc;

use super::node_label_location::NodeLabelLocation;
use super::port_context_multimap::PortContextMultimap;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::atomic_cell::AtomicCellRef;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::cell::CellRef;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::container_area::ContainerArea;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::grid_container_cell::GridContainerCellRef;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::label_cell::LabelCellRef;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::strip_container_cell::{
    Strip, StripContainerCell, StripContainerCellRef,
};
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::{LGraphAdapter, LNodeAdapter};
use crate::org::eclipse::elk::core::math::elk_margin::ElkMargin;
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::org::eclipse::elk::core::options::node_label_placement::NodeLabelPlacement;
use crate::org::eclipse::elk::core::options::port_alignment::PortAlignment;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::org::eclipse::elk::core::util::individual_spacings::IndividualSpacings;
use crate::prelude::*;

pub struct NodeContext {
    // Convenience access to things
    /// The node we calculate stuff for.
    pub node: LNodeAdapter,
    /// The node's size (the context's own vector; applied at the end).
    pub node_size: KVector,
    /// Whether this node has stuff inside it or not.
    pub treat_as_compound_node: bool,
    /// The node's size constraints.
    pub size_constraints: SizeConstraint,
    /// The node's size options.
    pub size_options: SizeOptions,
    /// Port constraints set on the node.
    pub port_constraints: PortConstraints,
    /// Whether port labels are placed inside or outside.
    pub port_labels_placement: PortLabelPlacement,
    /// Whether to treat port labels as a group when centering them next to eastern or western ports.
    pub port_labels_treat_as_group: bool,
    /// Where node labels are placed by default.
    pub node_label_placement: NodeLabelPlacement,
    /// Space to leave around the node label area. (In Swift the same object as
    /// the property value it was read from; only ever read.)
    pub node_labels_padding: ElkPadding,
    /// Space between a node and its outside labels.
    pub node_label_spacing: f64,
    /// Space between two labels.
    pub label_label_spacing: f64,
    /// Space between two different label cells.
    pub label_cell_spacing: f64,
    /// Space between a port and another port.
    pub port_port_spacing: f64,
    /// Horizontal space between a port and its labels.
    pub port_label_spacing_horizontal: f64,
    /// Vertical space between a port and its labels.
    pub port_label_spacing_vertical: f64,
    /// Margin to leave around the set of ports on each side (only ever read).
    pub surrounding_port_margins: ElkMargin,
    /// Whether node is being laid out in top-down layout mode.
    pub topdown_layout: bool,

    // More contexts
    /// Context objects that hold more information about each port.
    pub port_contexts: PortContextMultimap,

    // The cell system
    /// The main cell that holds all the cells that make up the node.
    pub node_container: StripContainerCellRef,
    /// The main cell's middle row, which will contain further cells.
    pub node_container_middle_row: StripContainerCellRef,
    /// The node's area reserved for inside node labels (and the client area).
    pub inside_node_label_container: Option<GridContainerCellRef>,
    /// `[PortSide: AtomicCell]`: the cells that describe the space required
    /// for ports and inside port labels, by `PortSide` ordinal.
    pub inside_port_label_cells: [Option<AtomicCellRef>; 5],
    /// `[PortSide: StripContainerCell]`: the containers of outside node label
    /// cells, by `PortSide` ordinal.
    pub outside_node_label_containers: [Option<StripContainerCellRef>; 5],
    /// `[NodeLabelLocation: LabelCell]`, by `NodeLabelLocation` ordinal.
    pub node_label_cells: [Option<LabelCellRef>; NodeLabelLocation::COUNT],
}

impl NodeContext {
    /// `init(parentGraph:node:)`.
    pub fn new(lg: &LGraphArena, _parent_graph: &LGraphAdapter, node: LNodeAdapter) -> NodeContext {
        let node_size = node.get_size(lg);

        // Top-down layout
        let topdown_layout = node.get_property::<bool>(lg, &CoreOptions::TOPDOWN_LAYOUT).unwrap_or(false);

        // Compound node
        let treat_as_compound_node = node.is_compound_node(lg)
            || node.get_property::<bool>(lg, &CoreOptions::INSIDE_SELF_LOOPS_ACTIVATE).unwrap_or(false);

        // Core size settings
        let size_constraints =
            node.get_property::<SizeConstraint>(lg, &CoreOptions::NODE_SIZE_CONSTRAINTS).unwrap_or(SizeConstraint::empty());
        let size_options = node.get_property::<SizeOptions>(lg, &CoreOptions::NODE_SIZE_OPTIONS).unwrap_or(SizeOptions::empty());
        let port_constraints =
            node.get_property::<PortConstraints>(lg, &CoreOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
        let port_labels_placement = node
            .get_property::<PortLabelPlacement>(lg, &CoreOptions::PORT_LABELS_PLACEMENT)
            .unwrap_or(PortLabelPlacement::empty());
        // (Swift: an invalid placement is kept as is.)

        let port_labels_treat_as_group =
            node.get_property::<bool>(lg, &CoreOptions::PORT_LABELS_TREAT_AS_GROUP).unwrap_or(true);
        let node_label_placement = node
            .get_property::<NodeLabelPlacement>(lg, &CoreOptions::NODE_LABELS_PLACEMENT)
            .unwrap_or(NodeLabelPlacement::empty());

        // Copy spacings for convenience
        let inherited = |p: &Property| IndividualSpacings::get_individual_or_inherited(lg, &node, p);
        let as_double = |v: Option<PropValue>| v.and_then(|v| v.cast::<f64>()).unwrap_or(0.0);

        let node_labels_padding = inherited(&CoreOptions::NODE_LABELS_PADDING)
            .and_then(|v| v.cast::<std::rc::Rc<std::cell::RefCell<ElkPadding>>>())
            .map(|p| *p.borrow())
            .unwrap_or_default();
        let node_label_spacing = as_double(inherited(&CoreOptions::SPACING_LABEL_NODE));
        let label_label_spacing = as_double(inherited(&CoreOptions::SPACING_LABEL_LABEL));
        let port_port_spacing = as_double(inherited(&CoreOptions::SPACING_PORT_PORT));
        let port_label_spacing_horizontal = as_double(inherited(&CoreOptions::SPACING_LABEL_PORT_HORIZONTAL));
        let port_label_spacing_vertical = as_double(inherited(&CoreOptions::SPACING_LABEL_PORT_VERTICAL));
        let surrounding_port_margins = inherited(&CoreOptions::SPACING_PORTS_SURROUNDING)
            .and_then(|v| v.cast::<std::rc::Rc<std::cell::RefCell<ElkMargin>>>())
            .map(|m| *m.borrow())
            .unwrap_or_default();

        let label_cell_spacing = 2.0 * label_label_spacing;

        // Create main cells (the others will be created later)
        let symmetry = !size_options.contains(SizeOptions::ASYMMETRICAL);
        let node_container = StripContainerCell::new_ref(Strip::VERTICAL, symmetry, 0.0);

        let node_container_middle_row = StripContainerCell::new_ref(Strip::HORIZONTAL, symmetry, 0.0);
        node_container
            .borrow_mut()
            .set_cell(ContainerArea::CENTER, Some(CellRef::Strip(Rc::clone(&node_container_middle_row))));

        NodeContext {
            node,
            node_size,
            treat_as_compound_node,
            size_constraints,
            size_options,
            port_constraints,
            port_labels_placement,
            port_labels_treat_as_group,
            node_label_placement,
            node_labels_padding,
            node_label_spacing,
            label_label_spacing,
            label_cell_spacing,
            port_port_spacing,
            port_label_spacing_horizontal,
            port_label_spacing_vertical,
            surrounding_port_margins,
            topdown_layout,
            port_contexts: PortContextMultimap::new(),
            node_container,
            node_container_middle_row,
            inside_node_label_container: None,
            inside_port_label_cells: Default::default(),
            outside_node_label_containers: Default::default(),
            node_label_cells: Default::default(),
        }
    }

    /// `insidePortLabelCells[side]`.
    pub fn inside_port_label_cell(&self, side: PortSide) -> Option<&AtomicCellRef> {
        self.inside_port_label_cells[side.ordinal()].as_ref()
    }

    /// `outsideNodeLabelContainers[side]`.
    pub fn outside_node_label_container(&self, side: PortSide) -> Option<&StripContainerCellRef> {
        self.outside_node_label_containers[side.ordinal()].as_ref()
    }

    /// `nodeLabelCells[location]`.
    pub fn node_label_cell(&self, location: NodeLabelLocation) -> Option<&LabelCellRef> {
        self.node_label_cells[location.ordinal()].as_ref()
    }

    // MARK: - Application

    /// `applyNodeSize()`: `node.setSize(nodeSize)`. Swift makes node and
    /// context share the vector; the context's vector is only returned (and
    /// discarded) by `NodeLabelAndSizeCalculator.process` afterwards, so a
    /// copy is exact.
    pub fn apply_node_size(&self, lg: &mut LGraphArena) {
        self.node.set_size(lg, self.node_size);
    }

    // MARK: - Utility Methods

    /// `getPortAlignment(portSide:)`: the port alignment that applies to the
    /// given side of the node.
    pub fn get_port_alignment(&self, lg: &LGraphArena, port_side: PortSide) -> PortAlignment {
        let mut alignment: Option<PortAlignment> = None;
        let node = &self.node;

        let specific = match port_side {
            PortSide::NORTH => Some(&CoreOptions::PORT_ALIGNMENT_NORTH),
            PortSide::SOUTH => Some(&CoreOptions::PORT_ALIGNMENT_SOUTH),
            PortSide::EAST => Some(&CoreOptions::PORT_ALIGNMENT_EAST),
            PortSide::WEST => Some(&CoreOptions::PORT_ALIGNMENT_WEST),
            _ => None,
        };
        if let Some(p) = specific {
            if node.has_property(lg, p) {
                alignment = node.get_property::<PortAlignment>(lg, p);
            }
        }

        // Fall back to basic port alignment if we haven't found a more specific one yet
        if alignment.is_none() {
            alignment = node.get_property::<PortAlignment>(lg, &CoreOptions::PORT_ALIGNMENT_DEFAULT);
        }

        alignment.unwrap_or(PortAlignment::BEGIN)
    }
}
