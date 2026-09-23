//! Port of `core/math/ElkMargin.swift`.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

pub use super::spacing::Spacing;

/// `ElkMargin`: a `Spacing` subclass without state of its own. A distinct type
/// so that `as? ElkMargin` casts on property values behave as in Swift.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ElkMargin(pub Spacing);

pub type ElkMarginRef = Rc<RefCell<ElkMargin>>;

impl ElkMargin {
    /// `init(_ top, _ right, _ bottom, _ left)`.
    pub const fn new(top: f64, right: f64, bottom: f64, left: f64) -> ElkMargin {
        ElkMargin(Spacing::new(top, right, bottom, left))
    }

    /// `init(_ any)`.
    pub const fn uniform(any: f64) -> ElkMargin {
        ElkMargin(Spacing::uniform(any))
    }

    /// `init(_ leftRight, _ topBottom)`.
    pub const fn lr_tb(left_right: f64, top_bottom: f64) -> ElkMargin {
        ElkMargin(Spacing::lr_tb(left_right, top_bottom))
    }
}

impl Deref for ElkMargin {
    type Target = Spacing;
    fn deref(&self) -> &Spacing {
        &self.0
    }
}

impl DerefMut for ElkMargin {
    fn deref_mut(&mut self) -> &mut Spacing {
        &mut self.0
    }
}
