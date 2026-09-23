//! Port of `alg/layered/options/LayerUnzippingStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum LayerUnzippingStrategy {
    NONE,
    ALTERNATING,
}

impl LayerUnzippingStrategy {
    pub const ALL: [LayerUnzippingStrategy; 2] = [LayerUnzippingStrategy::NONE, LayerUnzippingStrategy::ALTERNATING];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            LayerUnzippingStrategy::NONE => "NONE",
            LayerUnzippingStrategy::ALTERNATING => "ALTERNATING",
        }
    }
}

crate::enum_ordinal!(LayerUnzippingStrategy);
