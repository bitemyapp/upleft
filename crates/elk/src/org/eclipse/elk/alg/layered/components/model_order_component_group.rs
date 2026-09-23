//! Port of `alg/layered/components/ModelOrderComponentGroup.swift`.
//!
//! A component group that additionally keeps the order in which components
//! were added, and refuses components whose port sides would break the model
//! order of the components already in it.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::component_group::{constraints, ext_port_connections, ComponentGroup, ConstraintMap};
use crate::prelude::*;

/// `ModelOrderComponentGroup.modelOrderConstraints`: constraints that apply
/// in addition to `ComponentGroup.constraints` when the key is inserted into
/// a group that already contains the value.
pub fn model_order_constraints() -> &'static ConstraintMap {
    static MAP: OnceLock<ConstraintMap> = OnceLock::new();
    MAP.get_or_init(|| {
        use super::component_group::sides::*;
        let mut map: ConstraintMap = HashMap::new();
        let mut add = |key: EnumSet<PortSide>, value: EnumSet<PortSide>| map.entry(key).or_default().push(value);

        // Key is inserted in component group with value
        add(north(), none());
        add(west(), none());
        add(north_east(), none());
        add(north_west(), none());
        add(north_south_west(), none());
        add(north_east_west(), none());
        add(north_west(), north());
        add(none(), east());
        add(north(), east());
        add(west(), east());
        add(north_east(), east());
        add(north_south(), east());
        add(north_west(), east());
        add(north_south_west(), east());
        add(north_east_west(), east());
        add(east_west(), east());
        add(none(), south());
        add(north(), south());
        add(east(), south());
        add(west(), south());
        add(north_east(), south());
        add(north_south(), south());
        add(north_west(), south());
        add(east_west(), south());
        add(south_west(), south());
        add(north_south_west(), south());
        add(north_east_south(), south());
        add(north_east_west(), south());
        add(north(), west());
        add(north_east(), west());
        add(north_west(), west());
        add(north_east_west(), west());
        add(north(), north_east());
        add(west(), north_east());
        add(north_west(), north_east());
        add(north_east(), north_east());
        add(north_south_west(), north_east());
        // NW has nothing since it is in the first slot
        // Only conflicts since it is in the last slot
        add(none(), east_south());
        add(north(), east_south());
        add(east(), east_south());
        add(south(), east_south());
        add(west(), east_south());
        add(north_east(), east_south());
        add(north_south(), east_south());
        add(north_west(), east_south());
        add(south_west(), east_south());
        add(east_west(), east_south());
        add(north_east_west(), east_south());
        add(north_south_west(), east_south());
        add(north_east_south_west(), east_south());
        add(none(), south_west());
        add(north(), south_west());
        add(east(), south_west());
        add(west(), south_west());
        add(north_east(), south_west());
        add(north_south(), south_west());
        add(north_west(), south_west());
        add(east_west(), south_west());
        add(north_east_west(), south_west());
        add(north_east_south(), south_west());
        add(north_east_south_west(), south_west());
        add(north(), east_west());
        add(west(), east_west());
        add(north_east(), east_west());
        add(north_west(), east_west());
        add(south_west(), east_west());
        add(north_east_west(), east_west());
        add(north_south_west(), east_west());
        // NEW no additional conflicts
        add(none(), east_south_west());
        add(north(), east_south_west());
        add(east(), east_south_west());
        add(west(), east_south_west());
        add(north_east(), east_south_west());
        add(north_south(), east_south_west());
        add(north_west(), east_south_west());
        add(east_west(), east_south_west());
        add(north_east_west(), east_south_west());
        add(north(), north_south_west());
        add(east(), north_south_west());
        add(south(), north_south_west());
        add(north_east(), north_south_west());
        add(none(), north_east_south());
        add(north(), north_east_south());
        add(south(), north_east_south());
        add(west(), north_east_south());
        add(north_east(), north_east_south());
        add(north_south(), north_east_south());
        add(north_west(), north_east_south());
        add(north_west(), north_east_south_west());
        add(north_east(), north_east_south_west());
        // Conflicts that seem solvable but that arise since the order of C, EW, W, E is fix
        add(east_west(), none());
        add(east_west(), west());
        add(east_west(), east());
        // Conflicts that seem solvable but that arise since the order of C, NS, N, S is fix
        add(north_south(), none());
        add(north_south(), north());
        add(north_south(), south());

        map
    })
}

#[derive(Clone, Debug, Default)]
pub struct ModelOrderComponentGroup {
    pub base: ComponentGroup,
    pub component_order: Vec<LGraphId>,
}

impl ModelOrderComponentGroup {
    /// `ModelOrderComponentGroup()`.
    pub fn new() -> ModelOrderComponentGroup {
        ModelOrderComponentGroup::default()
    }

    /// `ModelOrderComponentGroup(_ component:)`: adds the component, then
    /// appends it to `componentOrder` a second time, as elk-swift does.
    pub fn with_component(lg: &LGraphArena, component: LGraphId) -> ModelOrderComponentGroup {
        let mut group = ModelOrderComponentGroup::new();
        group.add(lg, component);
        group.component_order.push(component);
        group
    }

    /// `add(_:)`.
    pub fn add(&mut self, lg: &LGraphArena, component: LGraphId) -> bool {
        if self.can_add(lg, component) {
            let port_connections = ext_port_connections(lg, component);
            self.base.append(port_connections, component);
            self.component_order.push(component);
            true
        } else {
            false
        }
    }

    /// `canAdd(_:)`: the `ComponentGroup` constraints plus the model order
    /// constraints.
    pub fn can_add(&self, lg: &LGraphArena, component: LGraphId) -> bool {
        let candidate_sides = ext_port_connections(lg, component);

        if let Some(super_constraints) = constraints().get(&candidate_sides) {
            for &constraint in super_constraints {
                if self.base.has_components(constraint) {
                    return false;
                }
            }
        }

        if let Some(mo_constraints) = model_order_constraints().get(&candidate_sides) {
            for &constraint in mo_constraints {
                if self.base.has_components(constraint) {
                    return false;
                }
            }
        }

        true
    }

    /// `getComponentOrder()`.
    pub fn get_component_order(&self) -> &[LGraphId] {
        &self.component_order
    }

    /// `portSides`.
    pub fn port_sides(&self) -> Vec<EnumSet<PortSide>> {
        self.base.get_port_sides()
    }
}
