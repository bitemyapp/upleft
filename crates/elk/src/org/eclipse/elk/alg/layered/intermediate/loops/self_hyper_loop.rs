//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/org_eclipse_elk_alg_layered_intermediate_loops_SelfHyperLoop.swift`.
//!
//! A hyper loop lives in its [`SelfLoopHolder`]'s loop list (which is
//! `getSLHyperLoops()`) and is referred to by [`SlLoopId`]. The methods that
//! reach other objects of the holder (`addSelfLoopEdge`, `setRoutingSlot`,
//! `computePortsPerSide`) are holder methods here.

use super::self_hyper_loop_labels::SelfHyperLoopLabels;
use super::self_loop_edge::SlEdgeId;
use super::self_loop_holder::SelfLoopHolder;
use super::self_loop_port::SlPortId;
use super::self_loop_type::SelfLoopType;
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

/// A `SelfHyperLoop` reference: its index in the holder's loop list.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SlLoopId(pub u32);

impl SlLoopId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Default)]
pub struct SelfHyperLoop {
    // Structural properties
    sl_ports: Vec<SlPortId>,
    /// Swift `Set<SelfLoopEdge>`.
    // NONDETERMINISTIC IN SWIFT: the set is iterated by SelfLoopPreProcessor
    // (removing edges: order-independent), SelfLoopPostProcessor (re-adding
    // edges to their ports: the order of the edges in the ports' lists) and
    // OrthogonalSelfLoopRouter (per-edge bend points; node margins by
    // `max`, order-dependent only in the sign of a zero). Java ELK used a
    // HashSet as well. Kept in insertion order (BFS discovery order).
    sl_edges: Vec<SlEdgeId>,
    sl_labels: Option<SelfHyperLoopLabels>,

    // Routing properties
    self_loop_type: Option<SelfLoopType>,
    /// Swift `[PortSide: [SelfLoopPort]]?`, kept in key insertion order.
    // NONDETERMINISTIC IN SWIFT: `RoutingDirector.determineTwoSideOpposingLoopRoutes`
    // takes `Array(getSLPortsBySide().keys)` and prefers option 1 (built
    // from `sides[0]`) on a penalty tie, so the dictionary's key order
    // decides the route. Java ELK built the map with `Multimaps.index`
    // (keys in order of first appearance in `slPorts`); that order is used.
    // The other iterations (`PortRestorer.processFourSideLoops`, which
    // appends to per-side lists) are order-independent.
    sl_ports_by_side: Option<Vec<(PortSide, Vec<SlPortId>)>>,
    leftmost_port: Option<SlPortId>,
    rightmost_port: Option<SlPortId>,
    /// Swift `Set<PortSide>`: only `contains`, `first` on a one-element set,
    /// and per-side independent updates iterate it.
    occupied_port_sides: EnumSet<PortSide>,
    routing_slot: [i64; 5],
}

impl SelfHyperLoop {
    pub fn new() -> SelfHyperLoop {
        SelfHyperLoop::default()
    }

    // MARK: - Accessors

    pub fn get_sl_ports(&self) -> &[SlPortId] {
        &self.sl_ports
    }

    /// `sortSLPorts(by:)`.
    pub fn sort_sl_ports(&mut self, comparator: impl FnMut(&SlPortId, &SlPortId) -> bool) {
        swift::sort_by(&mut self.sl_ports, comparator);
    }

    pub fn get_sl_edges(&self) -> &[SlEdgeId] {
        &self.sl_edges
    }

    pub fn get_sl_labels(&self) -> Option<&SelfHyperLoopLabels> {
        self.sl_labels.as_ref()
    }

    pub fn get_sl_labels_mut(&mut self) -> Option<&mut SelfHyperLoopLabels> {
        self.sl_labels.as_mut()
    }

    pub fn get_self_loop_type(&self) -> Option<SelfLoopType> {
        self.self_loop_type
    }

    /// `getSLPortsBySide()`: `(side, ports)` in key order (see the field).
    pub fn get_sl_ports_by_side_map(&self) -> &[(PortSide, Vec<SlPortId>)] {
        self.sl_ports_by_side.as_deref().unwrap_or(&[])
    }

    /// `getSLPortsBySide().keys`.
    pub fn sl_ports_by_side_keys(&self) -> Vec<PortSide> {
        self.get_sl_ports_by_side_map().iter().map(|(s, _)| *s).collect()
    }

    /// `getSLPortsBySide(_ portSide:)`.
    pub fn get_sl_ports_by_side(&self, port_side: PortSide) -> &[SlPortId] {
        self.get_sl_ports_by_side_map().iter().find(|(s, _)| *s == port_side).map_or(&[], |(_, p)| p.as_slice())
    }

    pub fn has_sl_ports_on_side(&self, port_side: PortSide) -> bool {
        match &self.sl_ports_by_side {
            Some(map) => map.iter().find(|(s, _)| *s == port_side).is_some_and(|(_, p)| !p.is_empty()),
            None => false,
        }
    }

    pub fn get_leftmost_port(&self) -> Option<SlPortId> {
        self.leftmost_port
    }

    pub fn set_leftmost_port(&mut self, port: SlPortId) {
        self.leftmost_port = Some(port);
    }

    pub fn get_rightmost_port(&self) -> Option<SlPortId> {
        self.rightmost_port
    }

    pub fn set_rightmost_port(&mut self, port: SlPortId) {
        self.rightmost_port = Some(port);
    }

    pub fn get_occupied_port_sides(&self) -> EnumSet<PortSide> {
        self.occupied_port_sides
    }

    pub fn add_occupied_port_side(&mut self, side: PortSide) {
        self.occupied_port_sides.insert(side);
    }

    pub fn get_routing_slot(&self, port_side: PortSide) -> i64 {
        self.routing_slot[port_side.ordinal()]
    }
}

impl SelfLoopHolder {
    /// `SelfHyperLoop(slHolder)`: appends a new loop to the holder's list.
    pub(crate) fn new_sl_loop(&mut self) -> SlLoopId {
        let id = SlLoopId(self.sl_hyper_loops.len() as u32);
        self.sl_hyper_loops.push(SelfHyperLoop::new());
        id
    }

    /// `SelfHyperLoop.computePortsPerSide()`: fills `slPortsBySide` and
    /// determines the self loop type. Called by `SelfLoopPortRestorer`.
    pub fn compute_ports_per_side(&mut self, lg: &LGraphArena, sl_loop: SlLoopId) {
        let mut ports_by_side: Vec<(PortSide, Vec<SlPortId>)> = Vec::new();
        for &sl_port in &self.sl_hyper_loops[sl_loop.index()].sl_ports {
            let port_side = lg[self.sl_port(sl_port).get_l_port()].side;
            match ports_by_side.iter_mut().find(|(s, _)| *s == port_side) {
                Some((_, ports)) => ports.push(sl_port),
                None => ports_by_side.push((port_side, vec![sl_port])),
            }
        }
        let sides: EnumSet<PortSide> = ports_by_side.iter().map(|(s, _)| *s).collect();
        let l = &mut self.sl_hyper_loops[sl_loop.index()];
        l.sl_ports_by_side = Some(ports_by_side);
        l.self_loop_type = SelfLoopType::from_port_sides(sides);
    }

    /// `SelfHyperLoop.addSelfLoopEdge(_:)`.
    pub(crate) fn add_self_loop_edge(&mut self, lg: &LGraphArena, sl_loop: SlLoopId, sl_edge: SlEdgeId) {
        // `slEdges.insert(slEdge).inserted`: an edge only ever joins one loop,
        // and records it, so it is in this loop's set iff it points here.
        if self.sl_edge(sl_edge).get_sl_hyper_loop() == Some(sl_loop) {
            return;
        }
        self.sl_hyper_loops[sl_loop.index()].sl_edges.push(sl_edge);
        self.sl_edges[sl_edge.index()].set_sl_hyper_loop(sl_loop);

        let sl_source = self.sl_edge(sl_edge).get_sl_source();
        let l = &mut self.sl_hyper_loops[sl_loop.index()];
        if !l.sl_ports.contains(&sl_source) {
            l.sl_ports.push(sl_source);
        }

        let sl_target = self.sl_edge(sl_edge).get_sl_target();
        let l = &mut self.sl_hyper_loops[sl_loop.index()];
        if !l.sl_ports.contains(&sl_target) {
            l.sl_ports.push(sl_target);
        }

        // Check if we need to take care of any edge labels
        let l_labels = &lg[self.sl_edge(sl_edge).get_l_edge()].labels;
        if !l_labels.is_empty() {
            let l_node = self.get_l_node();
            let l = &mut self.sl_hyper_loops[sl_loop.index()];
            if l.sl_labels.is_none() {
                l.sl_labels = Some(SelfHyperLoopLabels::new(lg, l_node));
            }
            if let Some(labels) = l.sl_labels.as_mut() {
                labels.add_l_labels(lg, l_labels);
            }
        }
    }

    /// `SelfHyperLoop.setRoutingSlot(_:_:)`: also raises the holder's routing
    /// slot count for the side.
    pub fn set_routing_slot(&mut self, sl_loop: SlLoopId, port_side: PortSide, slot: i64) {
        self.sl_hyper_loops[sl_loop.index()].routing_slot[port_side.ordinal()] = slot;

        let slot_count = &mut self.routing_slot_count;
        slot_count[port_side.ordinal()] = swift::max(slot_count[port_side.ordinal()], slot + 1);
    }
}
