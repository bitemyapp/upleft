//! Port of `alg/layered/options/LayeringStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum LayeringStrategy {
    NETWORK_SIMPLEX,
    LONGEST_PATH,
    LONGEST_PATH_SOURCE,
    COFFMAN_GRAHAM,
    INTERACTIVE,
    STRETCH_WIDTH,
    MIN_WIDTH,
    BF_MODEL_ORDER,
    DF_MODEL_ORDER,
}

impl LayeringStrategy {
    pub const ALL: [LayeringStrategy; 9] = [LayeringStrategy::NETWORK_SIMPLEX, LayeringStrategy::LONGEST_PATH, LayeringStrategy::LONGEST_PATH_SOURCE, LayeringStrategy::COFFMAN_GRAHAM, LayeringStrategy::INTERACTIVE, LayeringStrategy::STRETCH_WIDTH, LayeringStrategy::MIN_WIDTH, LayeringStrategy::BF_MODEL_ORDER, LayeringStrategy::DF_MODEL_ORDER];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            LayeringStrategy::NETWORK_SIMPLEX => "NETWORK_SIMPLEX",
            LayeringStrategy::LONGEST_PATH => "LONGEST_PATH",
            LayeringStrategy::LONGEST_PATH_SOURCE => "LONGEST_PATH_SOURCE",
            LayeringStrategy::COFFMAN_GRAHAM => "COFFMAN_GRAHAM",
            LayeringStrategy::INTERACTIVE => "INTERACTIVE",
            LayeringStrategy::STRETCH_WIDTH => "STRETCH_WIDTH",
            LayeringStrategy::MIN_WIDTH => "MIN_WIDTH",
            LayeringStrategy::BF_MODEL_ORDER => "BF_MODEL_ORDER",
            LayeringStrategy::DF_MODEL_ORDER => "DF_MODEL_ORDER",
        }
    }
}

crate::enum_ordinal!(LayeringStrategy);
