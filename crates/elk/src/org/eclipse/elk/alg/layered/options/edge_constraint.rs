//! Port of `alg/layered/options/EdgeConstraint.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum EdgeConstraint {
    NONE,
    INCOMING_ONLY,
    OUTGOING_ONLY,
}

impl EdgeConstraint {
    pub const ALL: [EdgeConstraint; 3] = [EdgeConstraint::NONE, EdgeConstraint::INCOMING_ONLY, EdgeConstraint::OUTGOING_ONLY];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeConstraint::NONE => "NONE",
            EdgeConstraint::INCOMING_ONLY => "INCOMING_ONLY",
            EdgeConstraint::OUTGOING_ONLY => "OUTGOING_ONLY",
        }
    }
}

crate::enum_ordinal!(EdgeConstraint);
