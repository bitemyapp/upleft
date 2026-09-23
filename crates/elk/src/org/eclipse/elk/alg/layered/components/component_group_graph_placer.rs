//! Port of `alg/layered/components/ComponentGroupGraphPlacer.swift`.
//!
//! Places components with regard to the external port sides they connect to:
//! components are collected into [`ComponentGroup`]s, each group's sectors
//! are filled, and the groups are placed diagonally one after another.

use super::abstract_graph_placer::{move_graphs, offset_graph, offset_graphs, AbstractGraphPlacer};
use super::component_group::{sides, ComponentGroup};
use crate::org::eclipse::elk::core::options::edge_routing::EdgeRouting;
use crate::prelude::*;

#[derive(Default)]
pub struct ComponentGroupGraphPlacer {
    pub component_groups: Vec<ComponentGroup>,
}

/// The components compactor (`ComponentsCompactor`) is only used with
/// `compaction.connectedComponents`, which the port does not support.
pub(crate) fn _needs_components_compactor() -> ! {
    unimplemented!("ComponentsCompactor (org.eclipse.elk.layered.compaction.connectedComponents) is not ported")
}

impl ComponentGroupGraphPlacer {
    pub fn new() -> ComponentGroupGraphPlacer {
        ComponentGroupGraphPlacer::default()
    }

    /// `addComponent(_:)`: into the first group that accepts it, else a new one.
    pub fn add_component(&mut self, lg: &LGraphArena, component: LGraphId) {
        for group in self.component_groups.iter_mut() {
            if group.add(lg, component) {
                return;
            }
        }
        self.component_groups.push(ComponentGroup::with_component(lg, component));
    }

    /// `placeComponents(_:spacing:)`: places the group's sectors and returns
    /// the group's size.
    pub fn place_components(lg: &mut LGraphArena, group: &ComponentGroup, spacing: f64) -> KVector {
        use sides::*;
        let max = swift::max::<f64>;

        let size_c = Self::place_components_in_rows(lg, &group.get_components_for(none()), spacing);
        let size_n = Self::place_components_horizontally(lg, &group.get_components_for(north()), spacing);
        let size_s = Self::place_components_horizontally(lg, &group.get_components_for(south()), spacing);
        let size_w = Self::place_components_vertically(lg, &group.get_components_for(west()), spacing);
        let size_e = Self::place_components_vertically(lg, &group.get_components_for(east()), spacing);
        let size_nw = Self::place_components_horizontally(lg, &group.get_components_for(north_west()), spacing);
        let size_ne = Self::place_components_horizontally(lg, &group.get_components_for(north_east()), spacing);
        let size_sw = Self::place_components_horizontally(lg, &group.get_components_for(south_west()), spacing);
        let size_se = Self::place_components_horizontally(lg, &group.get_components_for(east_south()), spacing);
        let size_we = Self::place_components_vertically(lg, &group.get_components_for(east_west()), spacing);
        let size_ns = Self::place_components_horizontally(lg, &group.get_components_for(north_south()), spacing);
        let size_nwe = Self::place_components_horizontally(lg, &group.get_components_for(north_east_west()), spacing);
        let size_swe = Self::place_components_horizontally(lg, &group.get_components_for(east_south_west()), spacing);
        let size_wns = Self::place_components_vertically(lg, &group.get_components_for(north_south_west()), spacing);
        let size_ens = Self::place_components_vertically(lg, &group.get_components_for(north_east_south()), spacing);
        let size_nesw = Self::place_components_horizontally(lg, &group.get_components_for(north_east_south_west()), spacing);

        let col_left_width = max(max(max(size_nw.x, size_w.x), size_sw.x), size_wns.x);
        let col_mid_width = max(max(max(size_n.x, size_c.x), size_s.x), size_nesw.x);
        let col_ns_width = size_ns.x;
        let col_right_width = max(max(max(size_ne.x, size_e.x), size_se.x), size_ens.x);
        let row_top_height = max(max(max(size_nw.y, size_n.y), size_ne.y), size_nwe.y);
        let row_mid_height = max(max(max(size_w.y, size_c.y), size_e.y), size_nesw.y);
        let row_we_height = size_we.y;
        let row_bottom_height = max(max(max(size_sw.y, size_s.y), size_se.y), size_swe.y);

        offset_graphs(lg, &group.get_components_for(none()), col_left_width + col_ns_width, row_top_height + row_we_height);
        offset_graphs(lg, &group.get_components_for(north_east_south_west()), col_left_width + col_ns_width, row_top_height + row_we_height);
        offset_graphs(lg, &group.get_components_for(north()), col_left_width + col_ns_width, 0.0);
        offset_graphs(lg, &group.get_components_for(south()), col_left_width + col_ns_width, row_top_height + row_we_height + row_mid_height);
        offset_graphs(lg, &group.get_components_for(west()), 0.0, row_top_height + row_we_height);
        offset_graphs(lg, &group.get_components_for(east()), col_left_width + col_ns_width + col_mid_width, row_top_height + row_we_height);
        offset_graphs(lg, &group.get_components_for(north_east()), col_left_width + col_ns_width + col_mid_width, 0.0);
        offset_graphs(lg, &group.get_components_for(south_west()), 0.0, row_top_height + row_we_height + row_mid_height);
        offset_graphs(
            lg,
            &group.get_components_for(east_south()),
            col_left_width + col_ns_width + col_mid_width,
            row_top_height + row_we_height + row_mid_height,
        );
        offset_graphs(lg, &group.get_components_for(east_west()), 0.0, row_top_height);
        offset_graphs(lg, &group.get_components_for(north_south()), col_left_width, 0.0);
        offset_graphs(lg, &group.get_components_for(east_south_west()), 0.0, row_top_height + row_we_height + row_mid_height);
        offset_graphs(lg, &group.get_components_for(north_east_south()), col_left_width + col_ns_width + col_mid_width, 0.0);

        let mut component_size = KVector::default();
        component_size.x = max(max(max(col_left_width + col_mid_width + col_ns_width + col_right_width, size_we.x), size_nwe.x), size_swe.x);
        component_size.y = max(max(max(row_top_height + row_mid_height + row_we_height + row_bottom_height, size_ns.y), size_wns.y), size_ens.y);
        component_size
    }

    /// `placeComponentsHorizontally(_:spacing:)`.
    pub fn place_components_horizontally(lg: &mut LGraphArena, components: &[LGraphId], spacing: f64) -> KVector {
        let mut size = KVector::default();
        for &component in components {
            offset_graph(lg, component, size.x, 0.0);
            size.x += lg[component].size.x + spacing;
            size.y = swift::max(size.y, lg[component].size.y);
        }
        if size.y > 0.0 {
            size.y += spacing;
        }
        size
    }

    /// `placeComponentsVertically(_:spacing:)`.
    pub fn place_components_vertically(lg: &mut LGraphArena, components: &[LGraphId], spacing: f64) -> KVector {
        let mut size = KVector::default();
        for &component in components {
            offset_graph(lg, component, 0.0, size.y);
            size.y += lg[component].size.y + spacing;
            size.x = swift::max(size.x, lg[component].size.x);
        }
        if size.x > 0.0 {
            size.x += spacing;
        }
        size
    }

    /// `placeComponentsInRows(_:spacing:)`.
    pub fn place_components_in_rows(lg: &mut LGraphArena, components: &[LGraphId], spacing: f64) -> KVector {
        if components.is_empty() {
            return KVector::default();
        }

        let mut max_row_width = 0.0;
        let mut total_area = 0.0;
        for &component in components {
            let component_size = lg[component].size;
            max_row_width = swift::max(max_row_width, component_size.x);
            total_area += component_size.x * component_size.y;
        }

        let first_component = components[0];
        let aspect_ratio = lg[first_component].props.get_typed::<f64>(&LayeredOptions::ASPECT_RATIO).unwrap_or(1.0);
        max_row_width = swift::max(max_row_width, f64::sqrt(total_area) * aspect_ratio);

        let (mut xpos, mut ypos, mut highest_box, mut broadest_row) = (0.0, 0.0, 0.0, spacing);
        for &graph in components {
            let size = lg[graph].size;
            if xpos + size.x > max_row_width {
                xpos = 0.0;
                ypos += highest_box + spacing;
                highest_box = 0.0;
            }
            offset_graph(lg, graph, xpos, ypos);
            broadest_row = swift::max(broadest_row, xpos + size.x);
            highest_box = swift::max(highest_box, size.y);
            xpos += size.x + spacing;
        }

        KVector::new(broadest_row + spacing, ypos + highest_box + spacing)
    }
}

impl AbstractGraphPlacer for ComponentGroupGraphPlacer {
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
            self.add_component(lg, component);
        }

        // Place components in each group
        let mut offset = KVector::default();
        let component_spacing = lg[first_component].props.get_typed::<f64>(&LayeredOptions::SPACING_COMPONENT_COMPONENT).unwrap_or(20.0);

        for group in &self.component_groups {
            let group_size = Self::place_components(lg, group, component_spacing);
            offset_graphs(lg, &group.get_components(), offset.x, offset.y);
            offset.x += group_size.x;
            offset.y += group_size.y;
        }

        lg[target].size.x = offset.x - component_spacing;
        lg[target].size.y = offset.y - component_spacing;

        // if compaction is desired, do so!
        let compaction_desired = lg[first_component].props.get_typed::<bool>(&LayeredOptions::COMPACTION_CONNECTED_COMPONENTS).unwrap_or(false);
        let edge_routing = lg[first_component].props.get_typed::<EdgeRouting>(&LayeredOptions::EDGE_ROUTING);
        if compaction_desired && edge_routing == Some(EdgeRouting::ORTHOGONAL) {
            _needs_components_compactor();
        }

        // finally move the components to the combined graph
        for group in &self.component_groups {
            move_graphs(lg, target, &group.get_components(), 0.0, 0.0);
        }
    }
}
