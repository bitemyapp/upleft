//! Port of `core/util/Pair.swift`.
//!
//! A pair of optional values. `Pair` is a class in elk-swift; code that shares
//! one `Pair` between owners and mutates it must wrap it in `Rc<RefCell<_>>`
//! (as `ComponentsProcessor`'s DFS accumulator is modelled by a `&mut`).
//! `isEqual`/`hash` compare `String(describing:)` renderings in Swift and are
//! not ported (nothing in the layered pipeline uses them).

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pair<F, S> {
    /// The first element.
    pub first: Option<F>,
    /// The second element.
    pub second: Option<S>,
}

impl<F, S> Pair<F, S> {
    /// `Pair()` / `Pair.create()`: both elements `nil`.
    pub fn create() -> Pair<F, S> {
        Pair { first: None, second: None }
    }

    /// `Pair(first:second:)` / `Pair.of(_:_:)`.
    pub fn of(first: F, second: S) -> Pair<F, S> {
        Pair { first: Some(first), second: Some(second) }
    }

    /// `Pair.fromMap(_:)` over any sequence of entries (Swift iterates a
    /// dictionary, in hash order; callers pass the entries in the order they
    /// need).
    pub fn from_entries(entries: impl IntoIterator<Item = (F, S)>) -> Vec<Pair<F, S>> {
        entries.into_iter().map(|(k, v)| Pair::of(k, v)).collect()
    }

    pub fn set_first(&mut self, value: F) {
        self.first = Some(value);
    }

    pub fn get_first(&self) -> Option<&F> {
        self.first.as_ref()
    }

    pub fn set_second(&mut self, value: S) {
        self.second = Some(value);
    }

    pub fn get_second(&self) -> Option<&S> {
        self.second.as_ref()
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.first = None;
        self.second = None;
    }
}

/// `Pair.FirstComparator` / `Pair.SecondComparator`: only `nil`-ness is
/// compared (`nil` first); returns -1, 0 or 1.
pub fn compare_first<F, S>(o1: &Pair<F, S>, o2: &Pair<F, S>) -> i32 {
    match (&o1.first, &o2.first) {
        (None, None) => 0,
        (None, _) => -1,
        (_, None) => 1,
        _ => 0,
    }
}

pub fn compare_second<F, S>(o1: &Pair<F, S>, o2: &Pair<F, S>) -> i32 {
    match (&o1.second, &o2.second) {
        (None, None) => 0,
        (None, _) => -1,
        (_, None) => 1,
        _ => 0,
    }
}
