//! Foundation's `NSRange`, as MarkdownCore uses it.
//!
//! Swift's `NSRange` is a pair of signed `Int`s, and MarkdownCore does signed
//! arithmetic on them (`max(0, end - start)` is everywhere), so both fields are
//! `isize` here too. Every position is a UTF-16 offset (see `model`).
//!
//! `contains(offset:)` and `touches(offset:)` are Model.swift's extension; the
//! rest mirrors Foundation.

/// Foundation's `NSNotFound` (`Int.max`).
pub const NS_NOT_FOUND: isize = isize::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NSRange {
    pub location: isize,
    pub length: isize,
}

impl std::fmt::Debug for NSRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{{}, {}}}", self.location, self.length)
    }
}

impl NSRange {
    #[inline]
    pub const fn new(location: isize, length: isize) -> NSRange {
        NSRange { location, length }
    }

    /// `NSRange(location: NSNotFound, length: 0)`.
    pub const NOT_FOUND: NSRange = NSRange { location: NS_NOT_FOUND, length: 0 };

    #[inline]
    pub const fn upper_bound(&self) -> isize {
        self.location + self.length
    }

    /// `NSRange.lowerBound`.
    #[inline]
    pub const fn lower_bound(&self) -> isize {
        self.location
    }

    /// Model.swift: `offset >= location && offset < upperBound`.
    #[inline]
    pub const fn contains(&self, offset: isize) -> bool {
        offset >= self.location && offset < self.upper_bound()
    }

    /// Model.swift: inclusive of the upper bound.
    #[inline]
    pub const fn touches(&self, offset: isize) -> bool {
        offset >= self.location && offset <= self.upper_bound()
    }

    /// Foundation's `NSRange.union(_:)` (`NSUnionRange`).
    pub fn union(&self, other: NSRange) -> NSRange {
        let max = self.upper_bound().max(other.upper_bound());
        let location = self.location.min(other.location);
        NSRange::new(location, max - location)
    }

    /// Foundation's `NSRange.intersection(_:)`: `NSIntersectionRange`, with an
    /// empty result reported as `nil`.
    pub fn intersection(&self, other: NSRange) -> Option<NSRange> {
        let result = ns_intersection_range(*self, other);
        if result.length == 0 { None } else { Some(result) }
    }

    /// `Range<Int>(nsRange)` bounds, for iteration.
    #[inline]
    pub fn indices(&self) -> std::ops::Range<isize> {
        self.location..self.upper_bound()
    }

    /// The same range as `usize` bounds, for slicing a UTF-16 buffer.
    #[inline]
    pub fn as_usize_range(&self) -> std::ops::Range<usize> {
        self.location as usize..self.upper_bound() as usize
    }
}

/// `NSIntersectionRange`: `{0, 0}` when the ranges do not meet.
pub fn ns_intersection_range(a: NSRange, b: NSRange) -> NSRange {
    if a.upper_bound() < b.location || b.upper_bound() < a.location {
        return NSRange::new(0, 0);
    }
    let location = a.location.max(b.location);
    let max = a.upper_bound().min(b.upper_bound());
    NSRange::new(location, max - location)
}

/// `NSLocationInRange`.
#[inline]
pub fn ns_location_in_range(location: isize, range: NSRange) -> bool {
    location >= range.location && location - range.location < range.length
}
