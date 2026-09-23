//! Port of `alg/layered/intermediate/preserveorder/ModelOrderPortComparator.swift`.
//!
//! elk-swift only ports a placeholder of the Java comparator: ports compare
//! by side, and ports on the same side by the hash of their object identity.

use std::collections::HashMap;
use std::rc::Rc;

use crate::org::eclipse::elk::alg::layered::options::ordering_strategy::OrderingStrategy;
use crate::prelude::*;

pub struct ModelOrderPortComparator {
    pub previous_layer: Vec<LNodeId>,
    pub graph: Option<LGraphId>,
    pub strategy: OrderingStrategy,
    pub target_node_model_order: Option<Rc<HashMap<LNodeId, i64>>>,
    pub port_model_order: bool,
}

impl Default for ModelOrderPortComparator {
    fn default() -> Self {
        ModelOrderPortComparator { previous_layer: Vec::new(), graph: None, strategy: OrderingStrategy::NONE, target_node_model_order: None, port_model_order: false }
    }
}

impl ModelOrderPortComparator {
    /// `init(_:_:_:_:_:)` with the previous layer's node list.
    pub fn new(
        graph: LGraphId,
        previous_layer: Vec<LNodeId>,
        strategy: OrderingStrategy,
        target_node_model_order: Option<Rc<HashMap<LNodeId, i64>>>,
        port_model_order: bool,
    ) -> ModelOrderPortComparator {
        ModelOrderPortComparator { previous_layer, graph: Some(graph), strategy, target_node_model_order, port_model_order }
    }

    /// The convenience `init` taking the previous `Layer`.
    pub fn with_layer(
        lg: &LGraphArena,
        graph: LGraphId,
        previous_layer: LayerId,
        strategy: OrderingStrategy,
        target_node_model_order: Option<Rc<HashMap<LNodeId, i64>>>,
        port_model_order: bool,
    ) -> ModelOrderPortComparator {
        Self::new(graph, lg[previous_layer].nodes.clone(), strategy, target_node_model_order, port_model_order)
    }

    pub fn compare(&self, lg: &LGraphArena, p1: LPortId, p2: LPortId) -> i64 {
        if p1 == p2 {
            return 0;
        }
        let (s1, s2) = (lg[p1].side, lg[p2].side);
        if s1 != s2 {
            return if self.side_order(s1) < self.side_order(s2) { -1 } else { 1 };
        }
        // NONDETERMINISTIC IN SWIFT: `ObjectIdentifier(p1).hashValue <
        // ObjectIdentifier(p2).hashValue` (a seeded hash of the heap address).
        // The port orders by arena index (creation order). The only reachable
        // callers are `LayerSweepCrossingMinimizer.countModelOrderPortChanges`,
        // whose count is multiplied by the port influence (0 unless set), and
        // `ModelOrderNodeComparator` with `beforePorts`, which nothing creates.
        if p1.0 < p2.0 { -1 } else { 1 }
    }

    pub fn side_order(&self, side: PortSide) -> i64 {
        match side {
            PortSide::NORTH => 0,
            PortSide::EAST => 1,
            PortSide::SOUTH => 2,
            PortSide::WEST => 3,
            PortSide::UNDEFINED => -1,
        }
    }
}
