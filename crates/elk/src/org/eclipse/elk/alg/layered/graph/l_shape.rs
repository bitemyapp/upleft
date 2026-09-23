//! Port of `alg/layered/graph/LShape.swift`.
//!
//! `position` and `size` are fields of the node, port, and label records.
//! Swift's `getPosition()` returns the element's own `KVector` object, which
//! callers mutate; the port mutates the field.
