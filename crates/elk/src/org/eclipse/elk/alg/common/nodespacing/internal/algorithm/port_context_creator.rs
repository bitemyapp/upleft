//! Port of `alg/common/nodespacing/internal/algorithm/PortContextCreator.swift`.
//!
//! Creates port context objects and assigns volatile IDs to all ports.

use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::label_cell::LabelCell;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::port_context::PortContext;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LPortAdapter;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::prelude::*;

pub struct PortContextCreator;

impl PortContextCreator {
    /// Creates and initializes port context objects for each of the node's ports.
    pub fn create_port_contexts(lg: &mut LGraphArena, node_context: &mut NodeContext, ignore_inside_port_labels: bool) {
        let im_port_labels =
            !ignore_inside_port_labels || !node_context.port_labels_placement.contains(PortLabelPlacement::INSIDE);

        let mut volatile_id = 0;
        for port in node_context.node.get_ports(lg) {
            // Swift: `assertionFailure` (a no-op in release builds), then skip.
            if port.get_side(lg) == PortSide::UNDEFINED {
                continue;
            }

            port.set_volatile_id(lg, volatile_id);
            volatile_id += 1;

            Self::create_port_context(lg, node_context, port, im_port_labels);
        }
    }

    /// Creates a port context for the given adapter and initializes it properly.
    /// (Swift puts the context into the multimap before creating its label
    /// cell; the multimap order does not depend on the label cell.)
    pub fn create_port_context(lg: &LGraphArena, node_context: &mut NodeContext, port: LPortAdapter, im_port_labels: bool) {
        let mut port_context =
            PortContext::new(lg, node_context.port_labels_placement, node_context.treat_as_compound_node, port);
        let side = port.get_side(lg);

        // If the port has labels and if port labels are to be placed, we need to remember them
        if im_port_labels && !PortLabelPlacement::is_fixed(node_context.port_labels_placement) {
            let mut port_label_cell = LabelCell::new(node_context.label_label_spacing);
            for label in port.get_labels(lg) {
                port_label_cell.add_label(lg, label);
            }
            port_context.port_label_cell = Some(port_label_cell);
        }

        node_context.port_contexts.put(lg, side, port_context);
    }
}
