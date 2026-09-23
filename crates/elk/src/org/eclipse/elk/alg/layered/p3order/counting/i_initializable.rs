//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/counting/org_eclipse_elk_alg_layered_p3order_counting_IInitializable.swift`.
//!
//! In elk-swift the protocol is empty: `GraphInfoHolder.initializeByTraversal`
//! calls each component's `initAt…` methods concretely, in its own fixed
//! order, instead of Java's generic `IInitializable.init` traversal. The port
//! does the same, so this is only a marker.

/// `IInitializable`: a component initialised by one traversal of the node
/// order (layers → nodes → ports → edges).
pub trait IInitializable {}
