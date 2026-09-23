//! Port of `core/math/KVector.swift`.
//!
//! In Swift `KVector` is a class; here it is a `Copy` value. Code that relies
//! on two owners sharing one vector keeps it in an `Rc<RefCell<KVector>>`
//! (see [`KVectorRef`]); everywhere else the owner's field is mutated in place.

use std::cell::RefCell;
use std::rc::Rc;

/// A shared, mutable vector: a Swift `KVector` reference held by more than one owner.
pub type KVectorRef = Rc<RefCell<KVector>>;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KVector {
    pub x: f64,
    pub y: f64,
}

impl KVector {
    pub const DEFAULT_FUZZINESS: f64 = 0.05;

    pub const fn new(x: f64, y: f64) -> KVector {
        KVector { x, y }
    }

    /// `KVector(start, end)`: the vector from `start` to `end`.
    pub fn between(start: KVector, end: KVector) -> KVector {
        KVector { x: end.x - start.x, y: end.y - start.y }
    }

    /// `KVector(angle)`: a normalized vector for an angle in radians.
    pub fn from_angle(angle: f64) -> KVector {
        KVector { x: angle.cos(), y: angle.sin() }
    }

    pub fn clone_vec(&self) -> KVector {
        *self
    }

    pub fn equals_fuzzily(&self, other: &KVector) -> bool {
        self.equals_fuzzily_with(other, Self::DEFAULT_FUZZINESS)
    }

    pub fn equals_fuzzily_with(&self, other: &KVector, fuzzyness: f64) -> bool {
        (self.x - other.x).abs() <= fuzzyness && (self.y - other.y).abs() <= fuzzyness
    }

    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn square_length(&self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    pub fn reset(&mut self) -> &mut Self {
        self.x = 0.0;
        self.y = 0.0;
        self
    }

    pub fn set(&mut self, other: KVector) -> &mut Self {
        self.x = other.x;
        self.y = other.y;
        self
    }

    pub fn set_xy(&mut self, x: f64, y: f64) -> &mut Self {
        self.x = x;
        self.y = y;
        self
    }

    pub fn add(&mut self, v: KVector) -> &mut Self {
        self.x += v.x;
        self.y += v.y;
        self
    }

    pub fn add_xy(&mut self, dx: f64, dy: f64) -> &mut Self {
        self.x += dx;
        self.y += dy;
        self
    }

    /// `KVector.sum(vs...)`: starts from `(0, 0)` and adds in order.
    pub fn sum(vs: &[KVector]) -> KVector {
        let mut sum = KVector::default();
        for v in vs {
            sum.x += v.x;
            sum.y += v.y;
        }
        sum
    }

    pub fn sub(&mut self, v: KVector) -> &mut Self {
        self.x -= v.x;
        self.y -= v.y;
        self
    }

    pub fn sub_xy(&mut self, dx: f64, dy: f64) -> &mut Self {
        self.x -= dx;
        self.y -= dy;
        self
    }

    pub fn diff(v1: KVector, v2: KVector) -> KVector {
        KVector::new(v1.x - v2.x, v1.y - v2.y)
    }

    pub fn scale(&mut self, scale: f64) -> &mut Self {
        self.x *= scale;
        self.y *= scale;
        self
    }

    pub fn scale_xy(&mut self, scalex: f64, scaley: f64) -> &mut Self {
        self.x *= scalex;
        self.y *= scaley;
        self
    }

    pub fn normalize(&mut self) -> &mut Self {
        let length = self.length();
        if length > 0.0 {
            self.x /= length;
            self.y /= length;
        }
        self
    }

    pub fn scale_to_length(&mut self, length: f64) -> &mut Self {
        self.normalize();
        self.scale(length);
        self
    }

    pub fn negate(&mut self) -> &mut Self {
        self.x = -self.x;
        self.y = -self.y;
        self
    }

    pub fn to_degrees(&self) -> f64 {
        self.to_radians() * 180.0 / std::f64::consts::PI
    }

    pub fn to_radians(&self) -> f64 {
        let length = self.length();
        if !(length > 0.0) {
            return 0.0;
        }
        if self.x >= 0.0 && self.y >= 0.0 {
            (self.y / length).asin()
        } else if self.x < 0.0 {
            std::f64::consts::PI - (self.y / length).asin()
        } else {
            2.0 * std::f64::consts::PI + (self.y / length).asin()
        }
    }

    pub fn distance(&self, v2: KVector) -> f64 {
        let dx = self.x - v2.x;
        let dy = self.y - v2.y;
        ((dx * dx) + (dy * dy)).sqrt()
    }

    pub fn dot_product(&self, v2: KVector) -> f64 {
        self.x * v2.x + self.y * v2.y
    }

    pub fn cross_product(v: KVector, w: KVector) -> f64 {
        v.x * w.y - v.y * w.x
    }

    pub fn rotate(&mut self, angle: f64) -> &mut Self {
        let new_x = self.x * angle.cos() - self.y * angle.sin();
        self.y = self.x * angle.sin() + self.y * angle.cos();
        self.x = new_x;
        self
    }

    pub fn angle(&self, other: KVector) -> f64 {
        (self.dot_product(other) / (self.length() * other.length())).acos()
    }

    pub fn bound(&mut self, lowx: f64, lowy: f64, highx: f64, highy: f64) -> &mut Self {
        if !(highx >= lowx && highy >= lowy) {
            return self;
        }
        if self.x < lowx {
            self.x = lowx;
        } else if self.x > highx {
            self.x = highx;
        }
        if self.y < lowy {
            self.y = lowy;
        } else if self.y > highy {
            self.y = highy;
        }
        self
    }

    pub fn is_nan(&self) -> bool {
        self.x.is_nan() || self.y.is_nan()
    }

    pub fn is_infinite(&self) -> bool {
        self.x.is_infinite() || self.y.is_infinite()
    }

    pub fn added(&self, other: KVector) -> KVector {
        KVector::new(self.x + other.x, self.y + other.y)
    }

    pub fn subtracted(&self, other: KVector) -> KVector {
        KVector::new(self.x - other.x, self.y - other.y)
    }

    pub fn scaled(&self, factor: f64) -> KVector {
        KVector::new(self.x * factor, self.y * factor)
    }

    pub fn scaled_to_length(&self, length: f64) -> KVector {
        let mut clone = *self;
        clone.scale_to_length(length);
        clone
    }

    /// `KVector` equality is exact on both coordinates (`==`).
    pub fn swift_eq(&self, other: &KVector) -> bool {
        self.x == other.x && self.y == other.y
    }

    /// Swift `description`: `"(x,y)"` with Swift's shortest double formatting.
    pub fn description(&self) -> String {
        format!("({},{})", crate::swift::describe_double(self.x), crate::swift::describe_double(self.y))
    }
}

/// Wraps a vector as a shared Swift reference.
pub fn kvector_ref(v: KVector) -> KVectorRef {
    Rc::new(RefCell::new(v))
}
