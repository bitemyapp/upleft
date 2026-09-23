//! Port of `alg/layered/options/GroupOrderStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum GroupOrderStrategy {
    ONLY_WITHIN_GROUP,
    MODEL_ORDER,
    ENFORCED,
}

impl GroupOrderStrategy {
    pub const ALL: [GroupOrderStrategy; 3] = [GroupOrderStrategy::ONLY_WITHIN_GROUP, GroupOrderStrategy::MODEL_ORDER, GroupOrderStrategy::ENFORCED];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            GroupOrderStrategy::ONLY_WITHIN_GROUP => "ONLY_WITHIN_GROUP",
            GroupOrderStrategy::MODEL_ORDER => "MODEL_ORDER",
            GroupOrderStrategy::ENFORCED => "ENFORCED",
        }
    }
}

crate::enum_ordinal!(GroupOrderStrategy);
