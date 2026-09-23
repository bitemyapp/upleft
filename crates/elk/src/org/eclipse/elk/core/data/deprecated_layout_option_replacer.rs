//! Port of `core/data/DeprecatedLayoutOptionReplacer.swift`.

use crate::bridge::elk_graph_impl::{ElkElement, ElkGraph};
use crate::org::eclipse::elk::core::options::core_options as CoreOptions;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;

pub struct DeprecatedLayoutOptionReplacer;

impl DeprecatedLayoutOptionReplacer {
    pub fn visit(&self, graph: &mut ElkGraph, element: ElkElement) {
        if graph.element_props(element).has(&CoreOptions::PORT_LABELS_NEXT_TO_PORT_IF_POSSIBLE) {
            let props = graph.element_props_mut(element);
            let mut port_labels = props.get_as::<PortLabelPlacement>(&CoreOptions::PORT_LABELS_PLACEMENT).unwrap_or_default();
            port_labels.insert(PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE);
            props.set(&CoreOptions::PORT_LABELS_PLACEMENT, port_labels);
            props.set_opt(&CoreOptions::PORT_LABELS_NEXT_TO_PORT_IF_POSSIBLE, None);
        }
        if graph.element_props(element).has(&CoreOptions::NODE_SIZE_OPTIONS) {
            let props = graph.element_props_mut(element);
            if let Some(mut size_opts) = props.get_as::<SizeOptions>(&CoreOptions::NODE_SIZE_OPTIONS) {
                if size_opts.contains(SizeOptions::SPACE_EFFICIENT_PORT_LABELS) {
                    let mut port_labels = props.get_as::<PortLabelPlacement>(&CoreOptions::PORT_LABELS_PLACEMENT).unwrap_or_default();
                    port_labels.insert(PortLabelPlacement::SPACE_EFFICIENT);
                    props.set(&CoreOptions::PORT_LABELS_PLACEMENT, port_labels);
                    size_opts.remove(SizeOptions::SPACE_EFFICIENT_PORT_LABELS);
                    props.set(&CoreOptions::NODE_SIZE_OPTIONS, size_opts);
                }
            }
        }
    }
}
