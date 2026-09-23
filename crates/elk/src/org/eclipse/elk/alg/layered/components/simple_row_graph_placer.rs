//! Port of `alg/layered/components/SimpleRowGraphPlacer.swift`.
//!
//! Places components into rows bounded by a width derived from the aspect
//! ratio, ignoring external port connections. The target graph must not be
//! one of the components unless there is only one.

use super::abstract_graph_placer::{move_graph, move_graphs, offset_graph, AbstractGraphPlacer};
use super::component_group_graph_placer::_needs_components_compactor;
use crate::prelude::*;

#[derive(Default)]
pub struct SimpleRowGraphPlacer;

impl SimpleRowGraphPlacer {
    pub fn new() -> SimpleRowGraphPlacer {
        SimpleRowGraphPlacer
    }

    /// `combine(_:target:)` with the (overridable) `placeComponents`.
    /// `sortComponents` never reorders anything in elk-swift (the sort was
    /// dropped because the array is not `inout`), so it is not called.
    pub(crate) fn combine_with(
        lg: &mut LGraphArena,
        components: &[LGraphId],
        target: LGraphId,
        place_components: fn(&mut LGraphArena, &[LGraphId], LGraphId, f64, f64),
    ) {
        if components.len() == 1 {
            let source = components[0];
            if source != target {
                lg[target].layerless_nodes.clear();
                move_graph(lg, target, source, 0.0, 0.0);
                let props = lg[source].props.clone();
                lg[target].props.copy_properties(&props);
                lg[target].padding = lg[source].padding;
                lg[target].size.x = lg[source].size.x;
                lg[target].size.y = lg[source].size.y;
            }
            return;
        } else if components.is_empty() {
            lg[target].layerless_nodes.clear();
            lg[target].size.x = 0.0;
            lg[target].size.y = 0.0;
            return;
        }

        let first_component = components[0];
        lg[target].layerless_nodes.clear();
        let props = lg[first_component].props.clone();
        lg[target].props.copy_properties(&props);

        // determine the maximal row width by the maximal box width and the total area
        let mut max_row_width = 0.0;
        let mut total_area = 0.0;
        for &graph in components {
            let size = lg[graph].size;
            max_row_width = swift::max(max_row_width, size.x);
            total_area += size.x * size.y;
        }
        let aspect_ratio = lg[target].props.get_typed::<f64>(&LayeredOptions::ASPECT_RATIO).unwrap_or(1.6);
        max_row_width = swift::max(max_row_width, f64::sqrt(total_area) * aspect_ratio);
        let component_spacing = lg[target].props.get_typed::<f64>(&LayeredOptions::SPACING_COMPONENT_COMPONENT).unwrap_or(20.0);

        place_components(lg, components, target, max_row_width, component_spacing);

        // if compaction is desired, do so!
        let compaction_desired = lg[first_component].props.get_typed::<bool>(&LayeredOptions::COMPACTION_CONNECTED_COMPONENTS).unwrap_or(false);
        if compaction_desired {
            _needs_components_compactor();
        }

        // finally move the components to the combined graph
        move_graphs(lg, target, components, 0.0, 0.0);
    }

    /// `placeComponents(_:target:maxRowWidth:componentSpacing:)`.
    pub fn place_components(lg: &mut LGraphArena, components: &[LGraphId], target: LGraphId, max_row_width: f64, component_spacing: f64) {
        let mut xpos = 0.0;
        let mut ypos = 0.0;
        let mut highest_box = 0.0;
        let mut broadest_row = component_spacing;

        for &graph in components {
            let size = lg[graph].size;
            if xpos + size.x > max_row_width {
                // place the graph into the next row
                xpos = 0.0;
                ypos += highest_box + component_spacing;
                highest_box = 0.0;
            }

            let offset = lg[graph].offset;
            offset_graph(lg, graph, xpos + offset.x, ypos + offset.y);
            lg[graph].offset = KVector::default();
            broadest_row = swift::max(broadest_row, xpos + size.x);
            highest_box = swift::max(highest_box, size.y);
            xpos += size.x + component_spacing;
        }

        lg[target].size.x = broadest_row;
        lg[target].size.y = ypos + highest_box;
    }
}

impl AbstractGraphPlacer for SimpleRowGraphPlacer {
    fn combine(&mut self, lg: &mut LGraphArena, components: &[LGraphId], target: LGraphId) {
        Self::combine_with(lg, components, target, Self::place_components);
    }
}
