//! Port of `alg/common/nodespacing/internal/PortContext.swift`.
//!
//! Data holder for one port of a node being sized. The position of a port
//! calculated as part of the algorithm is first stored in `port_position` and
//! only applied at the end of the algorithm, if required.
//!
//! Swift's `PortContext` is a class (hashed by a UUID, never used as a set or
//! map key). Each context is referenced only from its node context's
//! `portContexts` multimap, so the port stores it by value there. Its
//! `parentNodeContext` is only read in `init`; the port passes those settings
//! in instead of keeping the back reference.

use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::label_cell::LabelCell;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LPortAdapter;
use crate::org::eclipse::elk::core::math::elk_margin::ElkMargin;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::prelude::*;

#[derive(Clone, Debug)]
pub struct PortContext {
    /// The port we calculate stuff for.
    pub port: LPortAdapter,
    /// The port's position, to be modified by the algorithm and possibly applied later.
    pub port_position: KVector,
    /// Whether the port's labels need to be placed next to the port.
    pub labels_next_to_port: bool,
    /// Margin around the port to assume when placing the port. If node labels
    /// are taken into consideration, this will for example include the label
    /// cell. When placing the ports, this is the size the port will be assumed
    /// to have.
    pub port_margin: ElkMargin,
    /// The cell we place our port labels in.
    pub port_label_cell: Option<LabelCell>,
}

impl PortContext {
    /// `init(_ parentNodeContext:, _ port:)`, given the parent node context's
    /// `portLabelsPlacement` and `treatAsCompoundNode`.
    pub fn new(
        lg: &LGraphArena,
        port_labels_placement: PortLabelPlacement,
        treat_as_compound_node: bool,
        port: LPortAdapter,
    ) -> PortContext {
        let port_position = port.get_position(lg);

        let port_labels_next_to_port = port_labels_placement.contains(PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE);

        let labels_next_to_port = if port_labels_placement.contains(PortLabelPlacement::INSIDE) {
            if treat_as_compound_node {
                port_labels_next_to_port && !port.has_compound_connections(lg)
            } else {
                true
            }
        } else if port_labels_placement.contains(PortLabelPlacement::OUTSIDE) {
            if port_labels_next_to_port {
                let p = &lg[port.element];
                !(!p.incoming_edges.is_empty() || !p.outgoing_edges.is_empty())
            } else {
                false
            }
        } else {
            false
        };

        PortContext { port, port_position, labels_next_to_port, port_margin: ElkMargin::default(), port_label_cell: None }
    }

    /// `applyPortPosition()`: `port.setPosition(portPosition)`. Swift makes the
    /// port and this context share the vector; the context is dropped right
    /// after, so a copy is exact.
    pub fn apply_port_position(&self, lg: &mut LGraphArena) {
        self.port.set_position(lg, self.port_position);
    }
}
