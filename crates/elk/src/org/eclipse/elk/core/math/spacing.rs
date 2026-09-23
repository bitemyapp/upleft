//! Port of `core/math/Spacing.swift`.

/// Top/right/bottom/left spacing. Swift's `Spacing` is a class; `ElkMargin`,
/// `ElkPadding`, `LMargin` and `LPadding` are its subclasses. Here they are all
/// this one value type (see the aliases in the sibling modules).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spacing {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

impl Spacing {
    /// `init(_ top, _ right, _ bottom, _ left)`.
    pub const fn new(top: f64, right: f64, bottom: f64, left: f64) -> Spacing {
        Spacing { top, bottom, left, right }
    }

    pub const fn uniform(any: f64) -> Spacing {
        Spacing::new(any, any, any, any)
    }

    /// `ElkMargin(leftRight, topBottom)` / `ElkPadding(leftRight, topBottom)`.
    pub const fn lr_tb(left_right: f64, top_bottom: f64) -> Spacing {
        Spacing::new(top_bottom, left_right, top_bottom, left_right)
    }

    pub fn set(&mut self, other: &Spacing) {
        self.set4(other.top, other.right, other.bottom, other.left);
    }

    pub fn set4(&mut self, top: f64, right: f64, bottom: f64, left: f64) {
        self.top = top;
        self.right = right;
        self.bottom = bottom;
        self.left = left;
    }

    pub fn set_left_right(&mut self, val: f64) {
        self.left = val;
        self.right = val;
    }

    pub fn set_top_bottom(&mut self, val: f64) {
        self.top = val;
        self.bottom = val;
    }

    pub fn horizontal(&self) -> f64 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f64 {
        self.top + self.bottom
    }

    /// `copy(_:)`.
    pub fn copy_from(&mut self, other: &Spacing) -> &mut Self {
        self.left = other.left;
        self.right = other.right;
        self.top = other.top;
        self.bottom = other.bottom;
        self
    }

    /// `add(_:)`.
    pub fn add(&mut self, other: &Spacing) -> &mut Self {
        self.left += other.left;
        self.right += other.right;
        self.top += other.top;
        self.bottom += other.bottom;
        self
    }
}
