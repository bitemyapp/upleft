//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/org_eclipse_elk_alg_layered_intermediate_loops_SelfLoopPort.swift`.
//!
//! Self loop ports live in their [`SelfLoopHolder`](super::self_loop_holder::SelfLoopHolder)'s
//! port list and are referred to by [`SlPortId`] (the Swift object reference).

use super::self_loop_edge::SlEdgeId;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LPortId};

/// A `SelfLoopPort` reference: its index in the holder's port list.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SlPortId(pub u32);

impl SlPortId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug)]
pub struct SelfLoopPort {
    l_port: LPortId,
    had_only_self_loops_value: bool,
    incoming_sl_edges: Vec<SlEdgeId>,
    outgoing_sl_edges: Vec<SlEdgeId>,
    hidden_value: bool,
}

impl SelfLoopPort {
    pub fn new(lg: &LGraphArena, l_port: LPortId) -> SelfLoopPort {
        // Check if the port is only incident to self loops
        let had_only_self_loops_value = lg.port_connected_edges(l_port).into_iter().all(|e| lg.edge_is_self_loop(e));
        SelfLoopPort { l_port, had_only_self_loops_value, incoming_sl_edges: Vec::new(), outgoing_sl_edges: Vec::new(), hidden_value: false }
    }

    pub fn get_l_port(&self) -> LPortId {
        self.l_port
    }

    pub fn had_only_self_loops(&self) -> bool {
        self.had_only_self_loops_value
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden_value
    }

    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden_value = hidden;
    }

    pub fn get_incoming_sl_edges(&self) -> &[SlEdgeId] {
        &self.incoming_sl_edges
    }

    pub fn get_outgoing_sl_edges(&self) -> &[SlEdgeId] {
        &self.outgoing_sl_edges
    }

    pub(crate) fn append_incoming_sl_edge(&mut self, edge: SlEdgeId) {
        self.incoming_sl_edges.push(edge);
    }

    pub(crate) fn append_outgoing_sl_edge(&mut self, edge: SlEdgeId) {
        self.outgoing_sl_edges.push(edge);
    }

    pub fn get_sl_net_flow(&self) -> i64 {
        self.incoming_sl_edges.len() as i64 - self.outgoing_sl_edges.len() as i64
    }
}
