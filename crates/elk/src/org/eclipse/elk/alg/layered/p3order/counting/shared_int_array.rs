//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/counting/SharedIntArray.swift`.
//!
//! Reference-type wrapper for `[Int]`, matching Java's `int[]` reference
//! semantics: several crossing counters share one port-position array.
//! Cloning a `SharedIntArray` copies the reference, as assigning a Swift
//! class instance does.

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

#[derive(Clone, Default, Debug)]
pub struct SharedIntArray(Rc<RefCell<Vec<i64>>>);

impl SharedIntArray {
    /// `SharedIntArray()`: empty.
    pub fn new() -> SharedIntArray {
        SharedIntArray::default()
    }

    /// `SharedIntArray(repeating:count:)`.
    pub fn repeating(value: i64, count: usize) -> SharedIntArray {
        SharedIntArray(Rc::new(RefCell::new(vec![value; count])))
    }

    /// A fresh array holding `values` (`SharedIntArray(...)` then `.values = values`).
    pub fn from_values(values: Vec<i64>) -> SharedIntArray {
        SharedIntArray(Rc::new(RefCell::new(values)))
    }

    /// `values` (read).
    pub fn values(&self) -> Ref<'_, Vec<i64>> {
        self.0.borrow()
    }

    /// `values` (write).
    pub fn values_mut(&self) -> RefMut<'_, Vec<i64>> {
        self.0.borrow_mut()
    }

    /// `subscript(index)` (get); traps out of range.
    pub fn get(&self, index: usize) -> i64 {
        self.0.borrow()[index]
    }

    /// `subscript(index)` (set); traps out of range.
    pub fn set(&self, index: usize, value: i64) {
        self.0.borrow_mut()[index] = value;
    }

    /// `count`.
    pub fn count(&self) -> usize {
        self.0.borrow().len()
    }

    /// `===`.
    pub fn ptr_eq(&self, other: &SharedIntArray) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
