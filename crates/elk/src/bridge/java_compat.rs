//! Port of `Bridge/JavaCompat.swift`: the Java library pieces elk-swift's
//! transpiled code leans on.

use std::marker::PhantomData;

/// Enums with a declaration-order ordinal (Swift `CaseIterable`/`ordinal`).
pub trait EnumOrdinal: Copy + Eq {
    const COUNT: usize;
    fn ordinal(self) -> usize;
    fn from_ordinal(i: usize) -> Self;
}

/// `EnumSet<E>` (a Swift `Set<E>` of a small enum). Stored as a bit set.
///
/// Swift iterates a `Set` in hash order, which is seeded per process. Any
/// elk-swift code that iterates such a set in an order-dependent way is
/// therefore nondeterministic in Swift; here iteration is in ordinal order.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnumSet<E: EnumOrdinal> {
    bits: u64,
    _marker: PhantomData<E>,
}

impl<E: EnumOrdinal> Default for EnumSet<E> {
    fn default() -> Self {
        EnumSet { bits: 0, _marker: PhantomData }
    }
}

impl<E: EnumOrdinal + std::fmt::Debug> std::fmt::Debug for EnumSet<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl<E: EnumOrdinal> EnumSet<E> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn of(items: &[E]) -> Self {
        let mut s = Self::default();
        for &i in items {
            s.insert(i);
        }
        s
    }

    pub fn all() -> Self {
        let mut s = Self::default();
        for i in 0..E::COUNT {
            s.insert(E::from_ordinal(i));
        }
        s
    }

    pub fn contains(&self, e: E) -> bool {
        self.bits & (1 << e.ordinal()) != 0
    }

    /// Returns whether the element was newly inserted.
    pub fn insert(&mut self, e: E) -> bool {
        let had = self.contains(e);
        self.bits |= 1 << e.ordinal();
        !had
    }

    /// Returns whether the element was present.
    pub fn remove(&mut self, e: E) -> bool {
        let had = self.contains(e);
        self.bits &= !(1 << e.ordinal());
        had
    }

    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }

    pub fn len(&self) -> usize {
        self.bits.count_ones() as usize
    }

    pub fn union(&self, other: &Self) -> Self {
        EnumSet { bits: self.bits | other.bits, _marker: PhantomData }
    }

    pub fn intersection(&self, other: &Self) -> Self {
        EnumSet { bits: self.bits & other.bits, _marker: PhantomData }
    }

    pub fn subtracting(&self, other: &Self) -> Self {
        EnumSet { bits: self.bits & !other.bits, _marker: PhantomData }
    }

    pub fn is_subset(&self, other: &Self) -> bool {
        self.bits & !other.bits == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = E> + '_ {
        (0..E::COUNT).filter(move |i| self.bits & (1 << i) != 0).map(E::from_ordinal)
    }
}

impl<E: EnumOrdinal> FromIterator<E> for EnumSet<E> {
    fn from_iter<I: IntoIterator<Item = E>>(iter: I) -> Self {
        let mut s = Self::default();
        for e in iter {
            s.insert(e);
        }
        s
    }
}

/// Implements [`EnumOrdinal`] for the port's generated enums (which carry `ALL`).
#[macro_export]
macro_rules! enum_ordinal {
    ($($t:ty),* $(,)?) => {$(
        impl $crate::bridge::java_compat::EnumOrdinal for $t {
            const COUNT: usize = <$t>::ALL.len();
            fn ordinal(self) -> usize { self as usize }
            fn from_ordinal(i: usize) -> Self { <$t>::ALL[i] }
        }
    )*};
}

/// `DoubleMath` (Guava).
pub struct DoubleMath;

impl DoubleMath {
    pub fn fuzzy_equals(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    /// Returns -1, 0, or 1.
    pub fn fuzzy_compare(a: f64, b: f64, tolerance: f64) -> i32 {
        if Self::fuzzy_equals(a, b, tolerance) {
            return 0;
        }
        if a < b { -1 } else { 1 }
    }
}

pub struct Strings;

impl Strings {
    pub fn is_null_or_empty(s: Option<&str>) -> bool {
        s.is_none_or(|s| s.is_empty())
    }
}

/// `Double.toRadians()` / `toDegrees()` from the `Double` extension.
pub fn to_radians(d: f64) -> f64 {
    d * std::f64::consts::PI / 180.0
}

pub fn to_degrees(d: f64) -> f64 {
    d * 180.0 / std::f64::consts::PI
}
