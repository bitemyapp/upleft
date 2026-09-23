//! Port of `alg/layered/components/ComponentGroupModelOrderGraphPlacer.swift`.
//!
//! Like [`ComponentGroupGraphPlacer`], but components only ever join the last
//! group (a [`ModelOrderComponentGroup`]), and groups are stacked along the
//! layout direction while respecting the space blocked by south (east) port
//! connections.

use super::abstract_graph_placer::{move_graphs, offset_graphs, AbstractGraphPlacer};
use super::component_group_graph_placer::{ComponentGroupGraphPlacer, _needs_components_compactor};
use super::model_order_component_group::ModelOrderComponentGroup;
use crate::org::eclipse::elk::core::options::edge_routing::EdgeRouting;
use crate::prelude::*;

#[derive(Default)]
pub struct ComponentGroupModelOrderGraphPlacer {
    /// `componentGroups` (all `ModelOrderComponentGroup`s in this placer).
    pub component_groups: Vec<ModelOrderComponentGroup>,
}

impl ComponentGroupModelOrderGraphPlacer {
    pub fn new() -> ComponentGroupModelOrderGraphPlacer {
        ComponentGroupModelOrderGraphPlacer::default()
    }

    /// `addModelOrderComponent(_:)`: into the last group if it accepts it,
    /// else into a new group.
    pub fn add_model_order_component(&mut self, lg: &LGraphArena, component: LGraphId) {
        if let Some(group) = self.component_groups.last_mut() {
            if group.add(lg, component) {
                return;
            }
        }
        self.component_groups.push(ModelOrderComponentGroup::with_component(lg, component));
    }
}

impl AbstractGraphPlacer for ComponentGroupModelOrderGraphPlacer {
    fn combine(&mut self, lg: &mut LGraphArena, components: &[LGraphId], target: LGraphId) {
        self.component_groups.clear();
        lg[target].layerless_nodes.clear();

        if components.is_empty() {
            lg[target].size.x = 0.0;
            lg[target].size.y = 0.0;
            return;
        }

        let first_component = components[0];
        let props = lg[first_component].props.clone();
        lg[target].props.copy_properties(&props);

        // Construct component groups
        for &component in components {
            self.add_model_order_component(lg, component);
        }

        // Place components in each group
        let mut space_blocked_by_south_edges = KVector::default();
        let mut space_blocked_by_components = KVector::default();
        let mut offset = KVector::default();
        let mut max_size = KVector::default();
        let component_spacing = lg[first_component].props.get_typed::<f64>(&LayeredOptions::SPACING_COMPONENT_COMPONENT).unwrap_or(20.0);

        let direction = lg[target].props.get_typed::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::RIGHT);

        for group in &self.component_groups {
            // (The port side sets are visited in hash order in Swift; any
            // matching set sets the same value, so the order is irrelevant.)
            let port_sides = group.port_sides();
            if direction.is_horizontal() {
                offset.x = space_blocked_by_south_edges.x;
                if port_sides.iter().any(|s| s.contains(PortSide::NORTH)) {
                    offset.x = space_blocked_by_components.x;
                }
            } else if direction.is_vertical() {
                offset.y = space_blocked_by_south_edges.y;
                if port_sides.iter().any(|s| s.contains(PortSide::WEST)) {
                    offset.y = space_blocked_by_components.y;
                }
            }

            let group_size = ComponentGroupGraphPlacer::place_components(lg, &group.base, component_spacing);
            offset_graphs(lg, &group.base.get_components(), offset.x, offset.y);

            if direction.is_horizontal() {
                space_blocked_by_components.x = offset.x + group_size.x;
                max_size.x = swift::max(max_size.x, space_blocked_by_components.x);
                if port_sides.iter().any(|s| s.contains(PortSide::SOUTH)) {
                    space_blocked_by_south_edges.x = offset.x + group_size.x;
                }
                space_blocked_by_components.y = offset.y + group_size.y;
                offset.y = space_blocked_by_components.y;
                max_size.y = swift::max(max_size.y, offset.y);
            } else if direction.is_vertical() {
                space_blocked_by_components.y = offset.y + group_size.y;
                max_size.y = swift::max(max_size.y, space_blocked_by_components.y);
                if port_sides.iter().any(|s| s.contains(PortSide::EAST)) {
                    space_blocked_by_south_edges.y = offset.y + group_size.y;
                }
                space_blocked_by_components.x = offset.x + group_size.x;
                offset.x = space_blocked_by_components.x;
                max_size.x = swift::max(max_size.x, offset.x);
            }
        }

        lg[target].size.x = max_size.x - component_spacing;
        lg[target].size.y = max_size.y - component_spacing;

        // if compaction is desired, do so!
        let compaction_desired = lg[first_component].props.get_typed::<bool>(&LayeredOptions::COMPACTION_CONNECTED_COMPONENTS).unwrap_or(false);
        let edge_routing = lg[first_component].props.get_typed::<EdgeRouting>(&LayeredOptions::EDGE_ROUTING);
        if compaction_desired && edge_routing == Some(EdgeRouting::ORTHOGONAL) {
            _needs_components_compactor();
        }

        // finally move the components to the combined graph
        for group in &self.component_groups {
            move_graphs(lg, target, &group.base.get_components(), 0.0, 0.0);
        }
    }
}
