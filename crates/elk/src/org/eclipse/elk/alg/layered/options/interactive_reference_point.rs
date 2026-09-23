//! Port of `alg/layered/options/InteractiveReferencePoint.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum InteractiveReferencePoint {
    CENTER,
    TOP_LEFT,
}

impl InteractiveReferencePoint {
    pub const ALL: [InteractiveReferencePoint; 2] = [InteractiveReferencePoint::CENTER, InteractiveReferencePoint::TOP_LEFT];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            InteractiveReferencePoint::CENTER => "CENTER",
            InteractiveReferencePoint::TOP_LEFT => "TOP_LEFT",
        }
    }
}

crate::enum_ordinal!(InteractiveReferencePoint);
