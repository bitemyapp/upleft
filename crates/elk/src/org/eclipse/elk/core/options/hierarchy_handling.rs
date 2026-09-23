//! Port of `core/options/HierarchyHandling.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum HierarchyHandling {
    INHERIT,
    INCLUDE_CHILDREN,
    SEPARATE_CHILDREN,
}

impl HierarchyHandling {
    pub const ALL: [HierarchyHandling; 3] = [HierarchyHandling::INHERIT, HierarchyHandling::INCLUDE_CHILDREN, HierarchyHandling::SEPARATE_CHILDREN];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            HierarchyHandling::INHERIT => "INHERIT",
            HierarchyHandling::INCLUDE_CHILDREN => "INCLUDE_CHILDREN",
            HierarchyHandling::SEPARATE_CHILDREN => "SEPARATE_CHILDREN",
        }
    }
}

crate::enum_ordinal!(HierarchyHandling);
