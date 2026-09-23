//! Port of `alg/common/nodespacing/cellsystem/HorizontalLabelAlignment.swift`.

/// Horizontal alignment of labels.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum HorizontalLabelAlignment {
    /// Labels are left-aligned.
    LEFT,
    /// Labels are centered.
    CENTER,
    /// Labels are right-aligned.
    RIGHT,
}
