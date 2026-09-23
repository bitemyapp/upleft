//! Port of `alg/layered/components/ModelOrderRowGraphPlacer.swift`.
//!
//! A [`SimpleRowGraphPlacer`] that keeps the (model) order of the components
//! and starts new rows as external port connections demand.

use super::abstract_graph_placer::{offset_graph, AbstractGraphPlacer};
use super::simple_row_graph_placer::SimpleRowGraphPlacer;
use crate::prelude::*;

#[derive(Default)]
pub struct ModelOrderRowGraphPlacer;

impl ModelOrderRowGraphPlacer {
    pub fn new() -> ModelOrderRowGraphPlacer {
        ModelOrderRowGraphPlacer
    }

    /// `placeComponents(_:target:maxRowWidth:componentSpacing:)`.
    pub fn place_components(lg: &mut LGraphArena, components: &[LGraphId], target: LGraphId, max_row_width: f64, component_spacing: f64) {
        let mut xpos = 0.0;
        let mut ypos = 0.0;
        let mut highest_box = 0.0;
        let mut broadest_row = component_spacing;
        let mut last_component: Option<LGraphId> = None;
        let mut start_x_of_row = 0.0;

        for &graph in components {
            let size = lg[graph].size;
            let ext_port_connections = lg[graph].props.get_typed::<EnumSet<PortSide>>(&InternalProperties::EXT_PORT_CONNECTIONS).unwrap_or_default();

            let last_has_east = last_component.is_some_and(|c| {
                lg[c].props.get_as::<EnumSet<PortSide>>(&InternalProperties::EXT_PORT_CONNECTIONS).unwrap_or_default().contains(PortSide::EAST)
            });
            if (xpos + size.x > max_row_width && !ext_port_connections.contains(PortSide::NORTH)) || last_has_east || ext_port_connections.contains(PortSide::WEST) {
                // Components with NORTH connection are allowed to violate the width constraint.
                // Previous EAST ports and WEST ports require a new row.
                xpos = start_x_of_row;
                ypos += highest_box + component_spacing;
                highest_box = 0.0;
            }

            let offset = lg[graph].offset;
            // North ports should be placed such that they don't intersect with prior components.
            if ext_port_connections.contains(PortSide::NORTH) {
                xpos = broadest_row + component_spacing;
            }

            offset_graph(lg, graph, xpos + offset.x, ypos + offset.y);
            broadest_row = swift::max(broadest_row, xpos + size.x);

            // South ports block of everything below them.
            if ext_port_connections.contains(PortSide::SOUTH) {
                start_x_of_row = swift::max(start_x_of_row, xpos + size.x + component_spacing);
            }

            highest_box = swift::max(highest_box, size.y);
            xpos += size.x + component_spacing;
            last_component = Some(graph);
        }

        lg[target].size.x = broadest_row;
        lg[target].size.y = ypos + highest_box;
    }
}

impl AbstractGraphPlacer for ModelOrderRowGraphPlacer {
    /// `SimpleRowGraphPlacer.combine` with this placer's `placeComponents`
    /// (and its no-op `sortComponents`).
    fn combine(&mut self, lg: &mut LGraphArena, components: &[LGraphId], target: LGraphId) {
        SimpleRowGraphPlacer::combine_with(lg, components, target, Self::place_components);
    }
}
