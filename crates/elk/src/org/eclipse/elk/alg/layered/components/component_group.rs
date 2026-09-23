//! Port of `alg/layered/components/ComponentGroup.swift`.
//!
//! A group of connected components placed together. The group is divided into
//! nine sectors; the external port sides a component connects to determine the
//! sector(s) it occupies, and a component can only join a group if no
//! component with a conflicting combination of port sides is there already.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::prelude::*;

/// The `Set<PortSide>` constants of elk-swift's `PortSide.swift`
/// (`.none`, `.north`, … `.northEastSouthWest`).
pub mod sides {
    use crate::prelude::*;

    const fn bits(n: bool, e: bool, s: bool, w: bool) -> [bool; 4] {
        [n, e, s, w]
    }

    fn make(b: [bool; 4]) -> EnumSet<PortSide> {
        let mut set = EnumSet::new();
        if b[0] {
            set.insert(PortSide::NORTH);
        }
        if b[1] {
            set.insert(PortSide::EAST);
        }
        if b[2] {
            set.insert(PortSide::SOUTH);
        }
        if b[3] {
            set.insert(PortSide::WEST);
        }
        set
    }

    pub fn none() -> EnumSet<PortSide> {
        make(bits(false, false, false, false))
    }
    pub fn north() -> EnumSet<PortSide> {
        make(bits(true, false, false, false))
    }
    pub fn east() -> EnumSet<PortSide> {
        make(bits(false, true, false, false))
    }
    pub fn south() -> EnumSet<PortSide> {
        make(bits(false, false, true, false))
    }
    pub fn west() -> EnumSet<PortSide> {
        make(bits(false, false, false, true))
    }
    pub fn north_south() -> EnumSet<PortSide> {
        make(bits(true, false, true, false))
    }
    pub fn east_west() -> EnumSet<PortSide> {
        make(bits(false, true, false, true))
    }
    pub fn north_west() -> EnumSet<PortSide> {
        make(bits(true, false, false, true))
    }
    pub fn north_east() -> EnumSet<PortSide> {
        make(bits(true, true, false, false))
    }
    pub fn south_west() -> EnumSet<PortSide> {
        make(bits(false, false, true, true))
    }
    pub fn east_south() -> EnumSet<PortSide> {
        make(bits(false, true, true, false))
    }
    pub fn north_east_west() -> EnumSet<PortSide> {
        make(bits(true, true, false, true))
    }
    pub fn east_south_west() -> EnumSet<PortSide> {
        make(bits(false, true, true, true))
    }
    pub fn north_south_west() -> EnumSet<PortSide> {
        make(bits(true, false, true, true))
    }
    pub fn north_east_south() -> EnumSet<PortSide> {
        make(bits(true, true, true, false))
    }
    pub fn north_east_south_west() -> EnumSet<PortSide> {
        make(bits(true, true, true, true))
    }
}

/// A `[Set<PortSide>: [Set<PortSide>]]` constraint table. Only looked up,
/// never iterated, so a `HashMap` is exact.
pub type ConstraintMap = HashMap<EnumSet<PortSide>, Vec<EnumSet<PortSide>>>;

/// `ComponentGroup.constraints`: for a candidate's port sides, the port side
/// sets that must not be present in the group yet.
pub fn constraints() -> &'static ConstraintMap {
    static MAP: OnceLock<ConstraintMap> = OnceLock::new();
    MAP.get_or_init(|| {
        use sides::*;
        let mut map: ConstraintMap = HashMap::new();
        let mut add = |key: EnumSet<PortSide>, value: EnumSet<PortSide>| map.entry(key).or_default().push(value);

        add(none(), north_east_south_west());
        add(west(), north_east_south_west());
        add(west(), north_south_west());
        add(east(), north_east_south());
        add(east(), north_east_south_west());
        add(north(), north_east_south_west());
        add(north(), north_east_west());
        add(south(), east_south_west());
        add(south(), north_east_south_west());
        add(north_south(), east_west());
        add(north_south(), north_east_south_west());
        add(north_south(), north_east_west());
        add(north_south(), east_south_west());
        add(east_west(), north_south());
        add(east_west(), north_south_west());
        add(east_west(), north_east_south());
        add(east_west(), north_east_south_west());
        add(north_west(), north_west());
        add(north_west(), north_east_west());
        add(north_west(), north_south_west());
        add(north_east(), north_east());
        add(north_east(), north_east_west());
        add(north_east(), north_east_south());
        add(south_west(), south_west());
        add(south_west(), east_south_west());
        add(south_west(), north_south_west());
        add(east_south(), east_south());
        add(east_south(), east_south_west());
        add(east_south(), north_east_south());
        add(north_east_west(), north());
        add(north_east_west(), north_south());
        add(north_east_west(), north_west());
        add(north_east_west(), north_east());
        add(north_east_west(), north_east_south_west());
        add(north_east_west(), north_east_west());
        add(north_east_west(), north_south_west());
        add(north_east_west(), north_east_south());
        add(east_south_west(), south());
        add(east_south_west(), north_south());
        add(east_south_west(), south_west());
        add(east_south_west(), east_south());
        add(east_south_west(), east_south_west());
        add(east_south_west(), north_south_west());
        add(east_south_west(), north_east_south());
        add(east_south_west(), north_east_south_west());
        add(north_south_west(), west());
        add(north_south_west(), east_west());
        add(north_south_west(), north_west());
        add(north_south_west(), south_west());
        add(north_south_west(), north_east_west());
        add(north_south_west(), east_south_west());
        add(north_south_west(), north_south_west());
        add(north_south_west(), north_east_south_west());
        add(north_east_south(), east());
        add(north_east_south(), east_west());
        add(north_east_south(), north_east());
        add(north_east_south(), east_south());
        add(north_east_south(), north_east_west());
        add(north_east_south(), east_south_west());
        add(north_east_south(), north_east_south());
        add(north_east_south(), north_east_south_west());
        add(north_east_south_west(), none());
        add(north_east_south_west(), west());
        add(north_east_south_west(), east());
        add(north_east_south_west(), north());
        add(north_east_south_west(), south());
        add(north_east_south_west(), north_south());
        add(north_east_south_west(), east_west());
        add(north_east_south_west(), north_east_west());
        add(north_east_south_west(), east_south_west());
        add(north_east_south_west(), north_south_west());
        add(north_east_south_west(), north_east_south());
        add(north_east_south_west(), north_east_south_west());

        map
    })
}

/// `component.getProperty(InternalProperties.EXT_PORT_CONNECTIONS) ?? Set<PortSide>()`.
pub fn ext_port_connections(lg: &LGraphArena, component: LGraphId) -> EnumSet<PortSide> {
    lg[component].props.get_typed::<EnumSet<PortSide>>(&InternalProperties::EXT_PORT_CONNECTIONS).unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
pub struct ComponentGroup {
    /// `components: [Set<PortSide>: [LGraph]]`.
    ///
    /// NONDETERMINISTIC IN SWIFT: the placers move the groups' components
    /// into the target graph in the order of `getComponents()`, which
    /// flattens this dictionary in hash order (a per-process seed), so the
    /// order of the combined graph's `layerlessNodes` varies between runs
    /// (positions do not). The port keeps the keys in insertion order.
    pub components: Vec<(EnumSet<PortSide>, Vec<LGraphId>)>,
}

impl ComponentGroup {
    /// `ComponentGroup()`.
    pub fn new() -> ComponentGroup {
        ComponentGroup::default()
    }

    /// `ComponentGroup(_ component:)`.
    pub fn with_component(lg: &LGraphArena, component: LGraphId) -> ComponentGroup {
        let mut group = ComponentGroup::new();
        group.add(lg, component);
        group
    }

    /// `components[key, default: []].append(component)`.
    pub(crate) fn append(&mut self, key: EnumSet<PortSide>, component: LGraphId) {
        match self.components.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(component),
            None => self.components.push((key, vec![component])),
        }
    }

    /// `!components[key, default: []].isEmpty`.
    pub(crate) fn has_components(&self, key: EnumSet<PortSide>) -> bool {
        self.components.iter().any(|(k, list)| *k == key && !list.is_empty())
    }

    /// `add(_:)`: adds the component if `can_add` allows it.
    pub fn add(&mut self, lg: &LGraphArena, component: LGraphId) -> bool {
        if self.can_add(lg, component) {
            let key = ext_port_connections(lg, component);
            self.append(key, component);
            true
        } else {
            false
        }
    }

    /// `canAdd(_:)`.
    pub fn can_add(&self, lg: &LGraphArena, component: LGraphId) -> bool {
        let candidate_sides = ext_port_connections(lg, component);
        let Some(constraints) = constraints().get(&candidate_sides) else { return true };
        for &constraint in constraints {
            if self.has_components(constraint) {
                return false;
            }
        }
        true
    }

    /// `getPortSides()` / `portSides`.
    pub fn get_port_sides(&self) -> Vec<EnumSet<PortSide>> {
        self.components.iter().map(|(k, _)| *k).collect()
    }

    /// `getComponents()`: all components, key by key.
    pub fn get_components(&self) -> Vec<LGraphId> {
        self.components.iter().flat_map(|(_, list)| list.iter().copied()).collect()
    }

    /// `getComponents(_ connections:)`.
    pub fn get_components_for(&self, connections: EnumSet<PortSide>) -> Vec<LGraphId> {
        self.components.iter().find(|(k, _)| *k == connections).map(|(_, list)| list.clone()).unwrap_or_default()
    }
}
