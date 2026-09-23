//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/org_eclipse_elk_alg_layered_intermediate_loops_SelfLoopEdge.swift`.
//!
//! Self loop edges live in their [`SelfLoopHolder`]'s edge list and are
//! referred to by [`SlEdgeId`] (Swift hashes them by object identity).

use super::self_hyper_loop::SlLoopId;
use super::self_loop_holder::SelfLoopHolder;
use super::self_loop_port::SlPortId;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena};
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::options::port_side::PortSide;

/// A `SelfLoopEdge` reference: its index in the holder's edge list.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SlEdgeId(pub u32);

impl SlEdgeId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug)]
pub struct SelfLoopEdge {
    l_edge: LEdgeId,
    sl_hyper_loop: Option<SlLoopId>,
    sl_source: SlPortId,
    sl_target: SlPortId,
}

impl SelfLoopEdge {
    pub fn get_l_edge(&self) -> LEdgeId {
        self.l_edge
    }

    pub fn get_sl_hyper_loop(&self) -> Option<SlLoopId> {
        self.sl_hyper_loop
    }

    pub(crate) fn set_sl_hyper_loop(&mut self, sl_loop: SlLoopId) {
        self.sl_hyper_loop = Some(sl_loop);
    }

    pub fn get_sl_source(&self) -> SlPortId {
        self.sl_source
    }

    pub fn get_sl_target(&self) -> SlPortId {
        self.sl_target
    }

    /// `isInline()`.
    pub fn is_inline(&self, lg: &LGraphArena) -> bool {
        for &label in &lg[self.l_edge].labels {
            if lg[label].props.get_as::<bool>(&LayeredOptions::EDGE_LABELS_INLINE) == Some(true) {
                return true;
            }
        }
        false
    }
}

impl SelfLoopHolder {
    /// `SelfLoopEdge(lEdge, slSource, slTarget)`: registers the edge with its
    /// source and target self loop ports.
    pub(crate) fn new_sl_edge(&mut self, l_edge: LEdgeId, sl_source: SlPortId, sl_target: SlPortId) -> SlEdgeId {
        let id = SlEdgeId(self.sl_edges.len() as u32);
        self.sl_edges.push(SelfLoopEdge { l_edge, sl_hyper_loop: None, sl_source, sl_target });
        self.sl_ports_list[sl_source.index()].append_outgoing_sl_edge(id);
        self.sl_ports_list[sl_target.index()].append_incoming_sl_edge(id);
        id
    }

    /// `SelfLoopEdge.getLabelSide()`.
    pub fn sl_edge_label_side(&self, sl_edge: SlEdgeId) -> PortSide {
        let Some(sl_loop) = self.sl_edge(sl_edge).sl_hyper_loop else { return PortSide::UNDEFINED };
        let Some(labels) = self.sl_loop(sl_loop).get_sl_labels() else { return PortSide::UNDEFINED };
        labels.get_side()
    }
}
