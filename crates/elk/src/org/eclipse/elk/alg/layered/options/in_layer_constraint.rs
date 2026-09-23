//! Port of `alg/layered/options/InLayerConstraint.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum InLayerConstraint {
    NONE,
    TOP,
    BOTTOM,
}

impl InLayerConstraint {
    pub const ALL: [InLayerConstraint; 3] = [InLayerConstraint::NONE, InLayerConstraint::TOP, InLayerConstraint::BOTTOM];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            InLayerConstraint::NONE => "NONE",
            InLayerConstraint::TOP => "TOP",
            InLayerConstraint::BOTTOM => "BOTTOM",
        }
    }
}

crate::enum_ordinal!(InLayerConstraint);
