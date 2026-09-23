//! Port of `core/math/ElkPadding.swift`.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

pub use super::spacing::Spacing;

/// `ElkPadding`: a `Spacing` subclass without state of its own. A distinct type
/// so that `as? ElkPadding` casts on property values behave as in Swift.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ElkPadding(pub Spacing);

pub type ElkPaddingRef = Rc<RefCell<ElkPadding>>;

impl ElkPadding {
    /// `init(_ top, _ right, _ bottom, _ left)`.
    pub const fn new(top: f64, right: f64, bottom: f64, left: f64) -> ElkPadding {
        ElkPadding(Spacing::new(top, right, bottom, left))
    }

    /// `init(_ any)`.
    pub const fn uniform(any: f64) -> ElkPadding {
        ElkPadding(Spacing::uniform(any))
    }

    /// `init(_ leftRight, _ topBottom)`.
    pub const fn lr_tb(left_right: f64, top_bottom: f64) -> ElkPadding {
        ElkPadding(Spacing::lr_tb(left_right, top_bottom))
    }
}

impl Deref for ElkPadding {
    type Target = Spacing;
    fn deref(&self) -> &Spacing {
        &self.0
    }
}

impl DerefMut for ElkPadding {
    fn deref_mut(&mut self) -> &mut Spacing {
        &mut self.0
    }
}
