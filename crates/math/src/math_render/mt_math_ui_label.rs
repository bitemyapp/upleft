//! `MTMathUILabel.swift`: the label's display modes and alignment.
//!
//! `MTMathUILabel` itself is an `NSView` that Downright never instantiates;
//! only the two enums it defines, which `MTMathImage` takes, are ported.

/// Different display styles supported by the label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MTMathUILabelMode {
    /// Display mode. Equivalent to $$ in TeX
    #[default]
    Display,
    /// Text mode. Equivalent to $ in TeX.
    Text,
}

/// Horizontal text alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MTTextAlignment {
    /// Align left.
    #[default]
    Left,
    /// Align center.
    Center,
    /// Align right.
    Right,
}
