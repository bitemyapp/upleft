//! Port of `alg/layered/options/LayerConstraint.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum LayerConstraint {
    NONE,
    FIRST,
    FIRST_SEPARATE,
    LAST,
    LAST_SEPARATE,
}

impl LayerConstraint {
    pub const ALL: [LayerConstraint; 5] = [LayerConstraint::NONE, LayerConstraint::FIRST, LayerConstraint::FIRST_SEPARATE, LayerConstraint::LAST, LayerConstraint::LAST_SEPARATE];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            LayerConstraint::NONE => "NONE",
            LayerConstraint::FIRST => "FIRST",
            LayerConstraint::FIRST_SEPARATE => "FIRST_SEPARATE",
            LayerConstraint::LAST => "LAST",
            LayerConstraint::LAST_SEPARATE => "LAST_SEPARATE",
        }
    }
}

crate::enum_ordinal!(LayerConstraint);
