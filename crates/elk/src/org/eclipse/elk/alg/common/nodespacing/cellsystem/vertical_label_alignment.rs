//! Port of `alg/common/nodespacing/cellsystem/VerticalLabelAlignment.swift`.

/// Vertical alignment of labels.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VerticalLabelAlignment {
    /// Labels are top-aligned.
    TOP,
    /// Labels are centered.
    CENTER,
    /// Labels are bottom-aligned.
    BOTTOM,
}
