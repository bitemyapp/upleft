//! Port of `alg/layered/intermediate/preserveorder/CMGroupModelOrderCalculator.swift`.
//!
//! The model order of an element (node, port or edge) for crossing
//! minimization, offset by its group when group orders are enforced.

use crate::org::eclipse::elk::alg::layered::graph::l_graph_element::LElement;
use crate::org::eclipse::elk::alg::layered::options::group_order_strategy::GroupOrderStrategy;
use crate::prelude::*;

/// The property map of any layered-graph element.
pub(crate) fn element_props(lg: &LGraphArena, e: LElement) -> &PropertyMap {
    match e {
        LElement::Graph(g) => &lg[g].props,
        LElement::Layer(l) => &lg[l].props,
        LElement::Node(n) => &lg[n].props,
        LElement::Port(p) => &lg[p].props,
        LElement::Edge(e) => &lg[e].props,
        LElement::Label(l) => &lg[l].props,
    }
}

#[derive(Default)]
pub struct CMGroupModelOrderCalculator;

impl CMGroupModelOrderCalculator {
    pub fn new() -> CMGroupModelOrderCalculator {
        CMGroupModelOrderCalculator
    }

    /// `calculateModelOrderOrGroupModelOrder(_:_:_:_:)`: `MODEL_ORDER`
    /// (`-1` without one), plus `offset × groupId` if both elements are in
    /// enforced groups.
    pub fn calculate_model_order_or_group_model_order(lg: &LGraphArena, element: LElement, other: LElement, parent: LGraphId, offset: i64) -> i64 {
        Self::calculate_from_props(element_props(lg, element), element_props(lg, other), &lg[parent].props, offset)
    }

    /// The same on the elements' property maps.
    pub fn calculate_from_props(element: &PropertyMap, other: &PropertyMap, parent: &PropertyMap, offset: i64) -> i64 {
        let enforce_group_model_order = parent.get_as::<GroupOrderStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CM_GROUP_ORDER_STRATEGY)
            == Some(GroupOrderStrategy::ENFORCED);

        let Some(element_model_order) = element.get_as::<i64>(&InternalProperties::MODEL_ORDER) else { return -1 };

        if enforce_group_model_order {
            // `as? [Int] ?? []`
            let enforced_orders: Vec<i64> = parent
                .get_object::<Vec<i64>>(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CM_ENFORCED_GROUP_ORDERS)
                .map(|v| (*v).clone())
                .unwrap_or_default();
            let element_group_id = element.get_as::<i64>(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CROSSING_MINIMIZATION_ID);
            let other_group_id = other.get_as::<i64>(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CROSSING_MINIMIZATION_ID);
            if let (Some(element_group_id), Some(other_group_id)) = (element_group_id, other_group_id) {
                if enforced_orders.contains(&element_group_id) && enforced_orders.contains(&other_group_id) {
                    return offset * element_group_id + element_model_order;
                }
            }
        }

        element_model_order
    }
}
