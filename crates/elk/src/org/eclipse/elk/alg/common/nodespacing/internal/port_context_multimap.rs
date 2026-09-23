//! Port of `alg/common/nodespacing/internal/PortContextMultimap.swift`: the
//! `TreeMultimap<PortSide, PortContext>` of a node context.
//!
//! Keys are iterated in `PortSide` ordinal order. Values are kept sorted per
//! key by `comparePortContexts` (NORTH/EAST: ascending volatile id,
//! SOUTH/WEST: descending), inserted with elk-swift's `TreeMultimap.put`
//! binary search (a new value goes before existing equal ones). Unlike Guava's
//! `TreeMultimap`, elk-swift keeps comparator-equal values.

use super::port_context::PortContext;
use crate::prelude::*;

#[derive(Clone, Debug, Default)]
pub struct PortContextMultimap {
    /// Per `PortSide` ordinal; a side's list is non-empty iff Swift's
    /// `storage[side]` exists.
    buckets: [Vec<PortContext>; 5],
}

impl PortContextMultimap {
    pub fn new() -> PortContextMultimap {
        PortContextMultimap::default()
    }

    /// The value comparator (`lhs < rhs`).
    fn value_less(lg: &LGraphArena, lhs: &PortContext, rhs: &PortContext) -> bool {
        let lhs_side = lhs.port.get_side(lg);
        let side_cmp = lhs_side.ordinal() as i64 - rhs.port.get_side(lg).ordinal() as i64;
        if side_cmp != 0 {
            return side_cmp < 0;
        }
        match lhs_side {
            PortSide::NORTH | PortSide::EAST => lhs.port.get_volatile_id(lg) < rhs.port.get_volatile_id(lg),
            PortSide::SOUTH | PortSide::WEST => lhs.port.get_volatile_id(lg) > rhs.port.get_volatile_id(lg),
            _ => false,
        }
    }

    /// `put(_:_:)`: inserts a port context, maintaining sorted order.
    pub fn put(&mut self, lg: &LGraphArena, side: PortSide, port_context: PortContext) {
        let arr = &mut self.buckets[side.ordinal()];
        if arr.is_empty() {
            arr.push(port_context);
        } else {
            let mut lo = 0;
            let mut hi = arr.len();
            while lo < hi {
                let mid = (lo + hi) / 2;
                if Self::value_less(lg, &arr[mid], &port_context) {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            arr.insert(lo, port_context);
        }
    }

    /// `self[side]`: `nil` if the side has no entries.
    pub fn get(&self, side: PortSide) -> Option<&Vec<PortContext>> {
        let arr = &self.buckets[side.ordinal()];
        if arr.is_empty() { None } else { Some(arr) }
    }

    /// Mutable `self[side]`.
    pub fn get_mut(&mut self, side: PortSide) -> Option<&mut Vec<PortContext>> {
        let arr = &mut self.buckets[side.ordinal()];
        if arr.is_empty() { None } else { Some(arr) }
    }

    /// `self[side] ?? []`.
    pub fn get_or_empty(&self, side: PortSide) -> &[PortContext] {
        &self.buckets[side.ordinal()]
    }

    /// `self[side] ?? []`, mutable.
    pub fn get_or_empty_mut(&mut self, side: PortSide) -> &mut [PortContext] {
        &mut self.buckets[side.ordinal()]
    }

    /// `values`: the non-empty lists in key order.
    pub fn values(&self) -> impl Iterator<Item = &Vec<PortContext>> {
        self.buckets.iter().filter(|b| !b.is_empty())
    }

    /// `values`, mutable.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Vec<PortContext>> {
        self.buckets.iter_mut().filter(|b| !b.is_empty())
    }

    /// Sequence iteration: `(side, list)` pairs in key order.
    pub fn iter(&self) -> impl Iterator<Item = (PortSide, &Vec<PortContext>)> {
        self.buckets.iter().enumerate().filter(|(_, b)| !b.is_empty()).map(|(i, b)| (PortSide::ALL[i], b))
    }
}
