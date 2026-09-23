//! Upleft's port of [elk-swift](https://github.com/lukilabs/elk-swift) at
//! 32f8042 (1.0.2), the Swift port of the Eclipse Layout Kernel that
//! beautiful-mermaid-swift lays Mermaid graphs out with.
//!
//! Copyright (c) Kiel University and others (the Eclipse Layout Kernel), and
//! the elk-swift authors. This crate is a derivative work and, like its
//! sources, is made available under the terms of the Eclipse Public License
//! 2.0 (see `LICENSE`). SPDX-License-Identifier: EPL-2.0
//!
//! # Layout
//!
//! One Rust module per Swift file. `org::eclipse::elk` mirrors elk-swift's
//! `ELK/org/eclipse/elk` package tree, `bridge` its `Bridge/` folder (the JSON
//! import/export and the public [`bridge::elk::Elk`] entry point).
//!
//! # Ownership
//!
//! ELK's object graph is heavily cross-linked. Graph elements live in arenas
//! and are referred to by typed indices: the input graph in
//! [`bridge::elk_graph_impl::ElkGraph`], the layered graph in
//! [`org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena`]. An index
//! plays the role of a Swift object reference (identity comparisons are index
//! comparisons). Property values that are Swift *classes* (`KVector`,
//! `KVectorChain`, `ElkPadding`, the random generator, …) are shared
//! `Rc<RefCell<_>>` values so aliasing behaves as it does in Swift; Swift value
//! types (arrays, sets, enums) are copied.
//!
//! # Fidelity
//!
//! Output must be bit-identical to elk-swift's. Floating-point expressions keep
//! their Swift evaluation order, sorting uses Swift's own algorithm
//! ([`swift::sort_by`]), and dynamic property casts (`as? T`) keep their
//! Swift semantics ([`org::eclipse::elk::graph::properties::property`]).

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case, clippy::all)]

pub mod array_deque;
pub mod bridge;
pub mod elk_swift;
pub mod org;
pub mod prelude;
pub mod swift;
