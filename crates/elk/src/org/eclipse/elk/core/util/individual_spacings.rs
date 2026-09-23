//! Port of `core/util/IndividualSpacings.swift`.
//!
//! An `IndividualSpacings` object is a property holder stored under
//! `CoreOptions.SPACING_INDIVIDUAL`; the port keeps it as a
//! [`PropertyMap`] inside a `PropValue::Object` (read with
//! `props.get_object::<PropertyMap>(&CoreOptions::SPACING_INDIVIDUAL)`, as the
//! layered code does). `toString`/`parse` (serialisation through
//! `LayoutMetaDataService`) and the `NodeAdapterProtocol` overload (a protocol
//! nothing implements) are unreachable and not ported.

use crate::bridge::elk_graph_impl::{ElkGraph, ElkNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LNodeAdapter;
use crate::prelude::*;

/// `IndividualSpacings` (static helpers).
pub struct IndividualSpacings;

impl IndividualSpacings {
    /// `getIndividualOrInherited(_ node: NodeAdapter, _ property:)`: the
    /// node's individual override, else its graph adapter's value (stored or
    /// default), else the property default.
    pub fn get_individual_or_inherited(lg: &LGraphArena, node: &LNodeAdapter, property: &Property) -> Option<PropValue> {
        let mut result: Option<PropValue> = None;

        if node.has_property(lg, &CoreOptions::SPACING_INDIVIDUAL) {
            // `node.getProperty(SPACING_INDIVIDUAL) as? IPropertyHolder`
            if let Some(individual_spacings) = lg[node.element].props.get_object::<PropertyMap>(&CoreOptions::SPACING_INDIVIDUAL) {
                if individual_spacings.has(property) {
                    result = individual_spacings.get(property);
                }
            }
        }

        // use the common value from the parent graph
        if result.is_none() {
            if let Some(graph) = node.get_graph() {
                result = graph.get_property_value(lg, property);
            }
        }

        // if the result is still nil, use the property's default value
        if result.is_none() {
            result = property.default_value();
        }

        result
    }

    /// `getIndividualOrInherited(_ node: ElkNode, _ property:)`: the node's
    /// individual override, else its parent's value (stored or default).
    pub fn get_individual_or_inherited_elk(graph: &ElkGraph, node: ElkNodeId, property: &Property) -> Option<PropValue> {
        let mut result: Option<PropValue> = None;

        let props = &graph[node].props;
        if props.has(&CoreOptions::SPACING_INDIVIDUAL) {
            if let Some(individual_spacings) = props.get_object::<PropertyMap>(&CoreOptions::SPACING_INDIVIDUAL) {
                if individual_spacings.has(property) {
                    result = individual_spacings.get(property);
                }
            }
        }

        // use the common value
        if result.is_none() {
            if let Some(parent) = graph[node].parent {
                result = graph[parent].props.get(property);
            }
        }

        result
    }

    /// `serializedOptionSeparator`.
    pub const SERIALIZED_OPTION_SEPARATOR: &'static str = ";,;";
}
