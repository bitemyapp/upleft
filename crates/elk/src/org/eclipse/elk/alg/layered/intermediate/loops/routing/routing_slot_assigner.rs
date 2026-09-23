//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/routing/org_eclipse_elk_alg_layered_intermediate_loops_routing_RoutingSlotAssigner.swift`.
//!
//! Assigns routing slots (distances from the node) to hyper loops so that
//! they cross as little as possible.
//!
//! The Swift builds a crossing graph of `HyperEdgeSegment`s (one per loop)
//! and breaks its cycles with `OrthogonalRoutingGenerator.breakNonCriticalCycles`
//! (group C's orthogonal router). Only the segments' dependency lists, their
//! weights and their routing slots are used, so this module carries a
//! private, line-by-line port of exactly that part ([`segments`]): the
//! segment graph, `HyperEdgeSegmentDependency.createAndAddRegular`,
//! `remove`, `reverse`, and `HyperEdgeCycleDetector.detectCycles(_, false, nil)`.

use std::collections::VecDeque;

use self::segments::{SegId, SegmentGraph};
use super::super::self_hyper_loop::SlLoopId;
use super::super::self_loop_holder::SelfLoopHolder;
use super::super::self_loop_port::SlPortId;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::LGraphArena;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

#[derive(Default)]
pub struct RoutingSlotAssigner {
    hyper_edge_segments: Vec<SegId>,
    graph: SegmentGraph,
    /// `slLoopToSegmentMap`, indexed by loop.
    sl_loop_to_segment_map: Vec<Option<SegId>>,
    /// `slLoopActivityOverPorts`, indexed by loop.
    sl_loop_activity_over_ports: Vec<Option<Vec<bool>>>,
}

fn port_id(lg: &LGraphArena, sl_holder: &SelfLoopHolder, sl_port: SlPortId) -> i64 {
    lg[sl_holder.sl_port(sl_port).get_l_port()].id as i64
}

impl RoutingSlotAssigner {
    pub fn new() -> RoutingSlotAssigner {
        RoutingSlotAssigner::default()
    }

    pub fn assign_routing_slots(&mut self, lg: &LGraphArena, sl_holder: &mut SelfLoopHolder) {
        let label_crossing_matrix = compute_label_crossing_matrix(sl_holder);
        self.create_crossing_graph(lg, sl_holder, &label_crossing_matrix);

        // Swift version takes 1 parameter (no random)
        let segments = self.hyper_edge_segments.clone();
        self.graph.break_non_critical_cycles(&segments);

        self.do_assign_routing_slots(lg, sl_holder, &label_crossing_matrix);

        self.hyper_edge_segments = Vec::new();
        self.graph = SegmentGraph::default();
        self.sl_loop_to_segment_map = Vec::new();
        self.sl_loop_activity_over_ports = Vec::new();
    }

    // MARK: - Crossing Graph

    fn create_crossing_graph(&mut self, lg: &LGraphArena, sl_holder: &SelfLoopHolder, label_crossing_matrix: &[Vec<bool>]) {
        let sl_loops: Vec<SlLoopId> = sl_holder.sl_loop_ids().collect();

        self.hyper_edge_segments = Vec::new();
        self.graph = SegmentGraph::default();
        self.sl_loop_to_segment_map = vec![None; sl_loops.len()];

        for &sl_loop in &sl_loops {
            let segment = self.graph.new_segment();
            self.hyper_edge_segments.push(segment);
            self.sl_loop_to_segment_map[sl_loop.index()] = Some(segment);
        }

        self.sl_loop_activity_over_ports = vec![None; sl_loops.len()];
        self.compute_loop_activity(lg, sl_holder);

        // `0..<(slLoops.count - 1)` traps on an empty list.
        assert!(!sl_loops.is_empty(), "Range requires lowerBound <= upperBound");
        for first_idx in 0..(sl_loops.len() - 1) {
            let sl_loop1 = sl_loops[first_idx];
            for &sl_loop2 in &sl_loops[first_idx + 1..] {
                self.create_dependencies(lg, sl_holder, sl_loop1, sl_loop2, label_crossing_matrix);
            }
        }
    }

    fn compute_loop_activity(&mut self, lg: &LGraphArena, sl_holder: &SelfLoopHolder) {
        let l_ports_count = lg[sl_holder.get_l_node()].ports.len() as i64;

        for sl_loop in sl_holder.sl_loop_ids() {
            let mut loop_activity = vec![false; l_ports_count as usize];

            let l = sl_holder.sl_loop(sl_loop);
            let (Some(leftmost), Some(rightmost)) = (l.get_leftmost_port(), l.get_rightmost_port()) else { continue };
            let mut l_port_idx = port_id(lg, sl_holder, leftmost) - 1;
            let l_port_target_idx = port_id(lg, sl_holder, rightmost);

            while l_port_idx != l_port_target_idx {
                l_port_idx = (l_port_idx + 1) % l_ports_count;
                loop_activity[l_port_idx as usize] = true;
            }

            self.sl_loop_activity_over_ports[sl_loop.index()] = Some(loop_activity);
        }
    }

    fn create_dependencies(&mut self, lg: &LGraphArena, sl_holder: &SelfLoopHolder, sl_loop1: SlLoopId, sl_loop2: SlLoopId, label_crossing_matrix: &[Vec<bool>]) {
        let first_above_second_crossings = self.count_crossings(lg, sl_holder, sl_loop1, sl_loop2);
        let second_above_first_crossings = self.count_crossings(lg, sl_holder, sl_loop2, sl_loop1);

        let (Some(segment1), Some(segment2)) = (self.sl_loop_to_segment_map[sl_loop1.index()], self.sl_loop_to_segment_map[sl_loop2.index()]) else {
            return;
        };

        if first_above_second_crossings < second_above_first_crossings {
            self.graph.create_and_add_regular(segment1, segment2, second_above_first_crossings - first_above_second_crossings);
        } else if second_above_first_crossings < first_above_second_crossings {
            self.graph.create_and_add_regular(segment2, segment1, first_above_second_crossings - second_above_first_crossings);
        } else if first_above_second_crossings != 0 || labels_overlap_matrix(sl_holder, sl_loop1, sl_loop2, label_crossing_matrix) {
            self.graph.create_and_add_regular(segment1, segment2, 0);
            self.graph.create_and_add_regular(segment2, segment1, 0);
        }
    }

    fn count_crossings(&self, lg: &LGraphArena, sl_holder: &SelfLoopHolder, sl_upper_loop: SlLoopId, sl_lower_loop: SlLoopId) -> i64 {
        let Some(lower_loop_activity) = &self.sl_loop_activity_over_ports[sl_lower_loop.index()] else { return 0 };
        let mut crossings = 0;

        for &sl_port in sl_holder.sl_loop(sl_upper_loop).get_sl_ports() {
            if lower_loop_activity[port_id(lg, sl_holder, sl_port) as usize] {
                crossings += 1;
            }
        }

        crossings
    }

    // MARK: - Slot Assignment

    fn do_assign_routing_slots(&mut self, lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, label_crossing_matrix: &[Vec<bool>]) {
        self.assign_raw_routing_slots_to_segments();
        self.assign_raw_routing_slots_to_loops(sl_holder);
        self.shift_towards_node(lg, sl_holder, label_crossing_matrix);
    }

    fn assign_raw_routing_slots_to_segments(&mut self) {
        let g = &mut self.graph;
        let mut sinks: VecDeque<SegId> = VecDeque::new();

        for &segment in &self.hyper_edge_segments {
            let s = g.seg_mut(segment);
            s.in_weight = s.incoming.len() as i64;
            s.out_weight = s.outgoing.len() as i64;

            if s.out_weight == 0 {
                s.routing_slot = 0;
                sinks.push_back(segment);
            }
        }

        while let Some(segment) = sinks.pop_front() {
            let next_routing_slot = g.seg(segment).routing_slot + 1;

            for in_dependency in g.seg(segment).incoming.clone() {
                let Some(source_segment) = g.dep(in_dependency).source else { continue };
                let s = g.seg_mut(source_segment);
                s.routing_slot = swift::max(s.routing_slot, next_routing_slot);

                s.out_weight -= 1;
                if s.out_weight == 0 {
                    sinks.push_back(source_segment);
                }
            }
        }
    }

    fn assign_raw_routing_slots_to_loops(&self, sl_holder: &mut SelfLoopHolder) {
        for sl_loop in sl_holder.sl_loop_ids() {
            let Some(segment) = self.sl_loop_to_segment_map[sl_loop.index()] else { continue };
            let slot = self.graph.seg(segment).routing_slot;
            // A Swift `Set<PortSide>`; each side is updated independently.
            for port_side in sl_holder.sl_loop(sl_loop).get_occupied_port_sides().iter().collect::<Vec<_>>() {
                sl_holder.set_routing_slot(sl_loop, port_side, slot);
            }
        }
    }

    fn shift_towards_node(&self, lg: &LGraphArena, sl_holder: &mut SelfLoopHolder, label_crossing_matrix: &[Vec<bool>]) {
        let mut next_free_routing_slot_at_port = vec![0i64; lg[sl_holder.get_l_node()].ports.len()];

        self.shift_towards_node_on_side(lg, sl_holder, PortSide::NORTH, &mut next_free_routing_slot_at_port, label_crossing_matrix);
        self.shift_towards_node_on_side(lg, sl_holder, PortSide::EAST, &mut next_free_routing_slot_at_port, label_crossing_matrix);
        self.shift_towards_node_on_side(lg, sl_holder, PortSide::SOUTH, &mut next_free_routing_slot_at_port, label_crossing_matrix);
        self.shift_towards_node_on_side(lg, sl_holder, PortSide::WEST, &mut next_free_routing_slot_at_port, label_crossing_matrix);
    }

    fn shift_towards_node_on_side(
        &self,
        lg: &LGraphArena,
        sl_holder: &mut SelfLoopHolder,
        side: PortSide,
        next_free_routing_slot_at_port: &mut [i64],
        label_crossing_matrix: &[Vec<bool>],
    ) {
        let sl_loops: Vec<SlLoopId> = swift::sorted_by(
            sl_holder.sl_loop_ids().filter(|&l| sl_holder.sl_loop(l).get_occupied_port_sides().contains(side)),
            |a, b| sl_holder.sl_loop(*a).get_routing_slot(side) < sl_holder.sl_loop(*b).get_routing_slot(side),
        );

        let mut min_l_port_index = i64::MAX;
        let mut max_l_port_index = i64::MIN;
        for &l_port in &lg[sl_holder.get_l_node()].ports {
            if lg[l_port].side == side {
                min_l_port_index = swift::min(min_l_port_index, lg[l_port].id as i64);
                max_l_port_index = swift::max(max_l_port_index, lg[l_port].id as i64);
            }
        }

        if min_l_port_index == i64::MAX {
            for (i, &l) in sl_loops.iter().enumerate() {
                sl_holder.set_routing_slot(l, side, i as i64);
            }
        } else {
            let mut slot_assigned_to_label = vec![-1i64; label_crossing_matrix.len()];

            for &sl_loop in &sl_loops {
                let Some(active_at_port) = &self.sl_loop_activity_over_ports[sl_loop.index()] else { continue };
                let mut lowest_available_slot = 0;

                for port_index in min_l_port_index..=max_l_port_index {
                    if active_at_port[port_index as usize] {
                        lowest_available_slot = swift::max(lowest_available_slot, next_free_routing_slot_at_port[port_index as usize]);
                    }
                }

                if let Some(our_labels) = sl_holder.sl_loop(sl_loop).get_sl_labels() {
                    let our_label_idx = our_labels.id as usize;
                    // A Swift `Set<Int>`, only tested for membership.
                    let mut slots_with_label_conflicts: Vec<i64> = Vec::new();

                    for other_label_idx in 0..label_crossing_matrix.len() {
                        if label_crossing_matrix[our_label_idx][other_label_idx] {
                            slots_with_label_conflicts.push(slot_assigned_to_label[other_label_idx]);
                        }
                    }

                    while slots_with_label_conflicts.contains(&lowest_available_slot) {
                        lowest_available_slot += 1;
                    }
                }

                sl_holder.set_routing_slot(sl_loop, side, lowest_available_slot);
                for port_index in min_l_port_index..=max_l_port_index {
                    if active_at_port[port_index as usize] {
                        next_free_routing_slot_at_port[port_index as usize] = lowest_available_slot + 1;
                    }
                }

                if let Some(end_labels) = sl_holder.sl_loop(sl_loop).get_sl_labels() {
                    slot_assigned_to_label[end_labels.id as usize] = lowest_available_slot;
                }
            }
        }
    }
}

// MARK: - Label Crossing Matrix

fn compute_label_crossing_matrix(sl_holder: &mut SelfLoopHolder) -> Vec<Vec<bool>> {
    let mut label_id: i64 = 0;
    for sl_loop in sl_holder.sl_loop_ids() {
        if let Some(sl_labels) = sl_holder.sl_loop_mut(sl_loop).get_sl_labels_mut() {
            sl_labels.id = label_id;
            label_id += 1;
        }
    }

    let mut crossing_matrix = vec![vec![false; label_id as usize]; label_id as usize];

    let sl_loops: Vec<SlLoopId> = sl_holder.sl_loop_ids().collect();
    for sl1_idx in 0..sl_loops.len() {
        let sl_loop1 = sl_loops[sl1_idx];
        let Some(sl_labels1) = sl_holder.sl_loop(sl_loop1).get_sl_labels() else { continue };

        for &sl_loop2 in &sl_loops[sl1_idx + 1..] {
            let Some(sl_labels2) = sl_holder.sl_loop(sl_loop2).get_sl_labels() else { continue };

            let overlap = labels_overlap(sl_holder, sl_loop1, sl_loop2);
            crossing_matrix[sl_labels1.id as usize][sl_labels2.id as usize] = overlap;
            crossing_matrix[sl_labels2.id as usize][sl_labels1.id as usize] = overlap;
        }
    }

    crossing_matrix
}

fn labels_overlap(sl_holder: &SelfLoopHolder, sl_loop1: SlLoopId, sl_loop2: SlLoopId) -> bool {
    let (Some(sl_labels1), Some(sl_labels2)) = (sl_holder.sl_loop(sl_loop1).get_sl_labels(), sl_holder.sl_loop(sl_loop2).get_sl_labels()) else {
        return false;
    };

    if sl_labels1.get_side() != sl_labels2.get_side() || sl_labels1.get_side() == PortSide::EAST || sl_labels1.get_side() == PortSide::WEST {
        return false;
    }

    let start1 = sl_labels1.get_position().x;
    let end1 = start1 + sl_labels1.get_size().x;
    let start2 = sl_labels2.get_position().x;
    let end2 = start2 + sl_labels2.get_size().x;

    start1 <= end2 && end1 >= start2
}

fn labels_overlap_matrix(sl_holder: &SelfLoopHolder, sl_loop1: SlLoopId, sl_loop2: SlLoopId, label_crossing_matrix: &[Vec<bool>]) -> bool {
    let (Some(l1), Some(l2)) = (sl_holder.sl_loop(sl_loop1).get_sl_labels(), sl_holder.sl_loop(sl_loop2).get_sl_labels()) else {
        return false;
    };
    label_crossing_matrix[l1.id as usize][l2.id as usize]
}

/// The part of `HyperEdgeSegment`, `HyperEdgeSegmentDependency` and
/// `HyperEdgeCycleDetector` (all in `p5edges/orthogonal`) that the routing
/// slot assigner uses, over a small arena. Only regular dependencies occur
/// here, and cycle detection runs with `criticalOnly == false` and no random
/// generator.
pub(crate) mod segments {
    use std::collections::VecDeque;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct SegId(pub u32);

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct DepId(pub u32);

    /// `HyperEdgeSegment` (routing-relevant fields only).
    #[derive(Clone, Debug, Default)]
    pub struct Segment {
        pub routing_slot: i64,
        pub incoming: Vec<DepId>,
        pub outgoing: Vec<DepId>,
        pub in_weight: i64,
        pub out_weight: i64,
    }

    /// `HyperEdgeSegmentDependency` of type `REGULAR`.
    #[derive(Clone, Debug)]
    pub struct Dependency {
        pub source: Option<SegId>,
        pub target: Option<SegId>,
        pub weight: i64,
    }

    #[derive(Clone, Debug, Default)]
    pub struct SegmentGraph {
        segments: Vec<Segment>,
        deps: Vec<Dependency>,
    }

    impl SegmentGraph {
        /// `HyperEdgeSegment()`.
        pub fn new_segment(&mut self) -> SegId {
            let id = SegId(self.segments.len() as u32);
            self.segments.push(Segment::default());
            id
        }

        pub fn seg(&self, id: SegId) -> &Segment {
            &self.segments[id.0 as usize]
        }

        pub fn seg_mut(&mut self, id: SegId) -> &mut Segment {
            &mut self.segments[id.0 as usize]
        }

        pub fn dep(&self, id: DepId) -> &Dependency {
            &self.deps[id.0 as usize]
        }

        /// `HyperEdgeSegmentDependency.createAndAddRegular(_:_:_:)`.
        pub fn create_and_add_regular(&mut self, source: SegId, target: SegId, weight: i64) -> DepId {
            let id = DepId(self.deps.len() as u32);
            self.deps.push(Dependency { source: None, target: None, weight });
            self.set_source(id, Some(source));
            self.set_target(id, Some(target));
            id
        }

        /// `remove()`.
        pub fn remove(&mut self, dep: DepId) {
            self.set_source(dep, None);
            self.set_target(dep, None);
        }

        /// `reverse()`.
        pub fn reverse(&mut self, dep: DepId) {
            let old_source = self.dep(dep).source;
            let old_target = self.dep(dep).target;
            self.set_source(dep, old_target);
            self.set_target(dep, old_source);
        }

        fn set_source(&mut self, dep: DepId, new_source: Option<SegId>) {
            if let Some(source) = self.deps[dep.0 as usize].source {
                self.seg_mut(source).outgoing.retain(|&d| d != dep);
            }
            self.deps[dep.0 as usize].source = new_source;
            if let Some(source) = new_source {
                self.seg_mut(source).outgoing.push(dep);
            }
        }

        fn set_target(&mut self, dep: DepId, new_target: Option<SegId>) {
            if let Some(target) = self.deps[dep.0 as usize].target {
                self.seg_mut(target).incoming.retain(|&d| d != dep);
            }
            self.deps[dep.0 as usize].target = new_target;
            if let Some(target) = new_target {
                self.seg_mut(target).incoming.push(dep);
            }
        }

        /// `OrthogonalRoutingGenerator.breakNonCriticalCycles(_:)`.
        pub fn break_non_critical_cycles(&mut self, edge_segments: &[SegId]) {
            let cycle_dependencies = self.detect_cycles(edge_segments);

            for cycle_dependency in cycle_dependencies {
                if self.dep(cycle_dependency).weight == 0 {
                    self.remove(cycle_dependency);
                } else {
                    self.reverse(cycle_dependency);
                }
            }
        }

        /// `HyperEdgeCycleDetector.detectCycles(segments, false, nil)`.
        /// The per-segment dictionaries (`markBySegment`, …; `?? 0` for a
        /// missing key) are vectors indexed by segment.
        fn detect_cycles(&self, segments: &[SegId]) -> Vec<DepId> {
            let n = self.segments.len();
            let mut result = Vec::new();
            let mut sources: VecDeque<SegId> = VecDeque::new();
            let mut sinks: VecDeque<SegId> = VecDeque::new();
            let mut st = MarkState { mark: vec![0; n], in_weight: vec![0; n], out_weight: vec![0; n] };

            // initialize
            let mut next_mark: i64 = -1;
            for &segment in segments {
                let k = segment.0 as usize;
                st.mark[k] = next_mark;
                next_mark -= 1;

                // Only regular dependencies exist here: the critical weights are 0.
                let in_weight: i64 = self.seg(segment).incoming.iter().map(|&d| self.dep(d).weight).sum();
                let out_weight: i64 = self.seg(segment).outgoing.iter().map(|&d| self.dep(d).weight).sum();
                st.in_weight[k] = in_weight;
                st.out_weight[k] = out_weight;

                if out_weight == 0 {
                    sinks.push_back(segment);
                } else if in_weight == 0 {
                    sources.push_back(segment);
                }
            }

            // computeLinearOrderingMarks
            let mut unprocessed: Vec<SegId> = crate::swift::sorted_by(segments.iter().copied(), |a, b| st.mark[a.0 as usize] < st.mark[b.0 as usize]);
            let mut max_segments: Vec<SegId> = Vec::new();

            let mark_base = segments.len() as i64;
            let mut next_sink_mark = mark_base - 1;
            let mut next_source_mark = mark_base + 1;

            while !unprocessed.is_empty() {
                while let Some(sink) = sinks.pop_front() {
                    remove_from_unprocessed(sink, &mut unprocessed);
                    st.mark[sink.0 as usize] = next_sink_mark;
                    next_sink_mark -= 1;
                    self.update_neighbors(sink, &mut sources, &mut sinks, &mut st);
                }

                while let Some(source) = sources.pop_front() {
                    remove_from_unprocessed(source, &mut unprocessed);
                    st.mark[source.0 as usize] = next_source_mark;
                    next_source_mark += 1;
                    self.update_neighbors(source, &mut sources, &mut sinks, &mut st);
                }

                let mut max_outflow = i64::MIN;
                for &segment in &unprocessed {
                    let k = segment.0 as usize;
                    // (The critical-dependency shortcut never fires: no
                    // critical weights.)
                    let outflow = st.out_weight[k] - st.in_weight[k];
                    if outflow >= max_outflow {
                        if outflow > max_outflow {
                            max_segments.clear();
                            max_outflow = outflow;
                        }
                        max_segments.push(segment);
                    }
                }

                if !max_segments.is_empty() {
                    // NONDETERMINISTIC IN SWIFT: `nextRandomInt(count, nil, &nil)`
                    // is `Int.random(in: 0..<count)` (system RNG) when there
                    // is more than one candidate. The first candidate is used.
                    let index = 0;
                    let max_node = max_segments[index];
                    remove_from_unprocessed(max_node, &mut unprocessed);
                    st.mark[max_node.0 as usize] = next_source_mark;
                    next_source_mark += 1;
                    self.update_neighbors(max_node, &mut sources, &mut sinks, &mut st);
                    max_segments.clear();
                }
            }

            let shift_base = segments.len() as i64 + 1;
            for &node in segments {
                let k = node.0 as usize;
                if st.mark[k] < mark_base {
                    st.mark[k] += shift_base;
                }
            }

            for &source in segments {
                let source_mark = st.mark[source.0 as usize];
                for &out_dependency in &self.seg(source).outgoing {
                    let Some(target) = self.dep(out_dependency).target else { continue };
                    let target_mark = st.mark[target.0 as usize];
                    if source_mark > target_mark {
                        result.push(out_dependency);
                    }
                }
            }

            result
        }

        fn update_neighbors(&self, node: SegId, sources: &mut VecDeque<SegId>, sinks: &mut VecDeque<SegId>, st: &mut MarkState) {
            for &dep in &self.seg(node).outgoing {
                let Some(target) = self.dep(dep).target else { continue };
                let k = target.0 as usize;
                if st.mark[k] < 0 && self.dep(dep).weight > 0 {
                    st.in_weight[k] -= self.dep(dep).weight;
                    if st.in_weight[k] <= 0 && st.out_weight[k] > 0 {
                        sources.push_back(target);
                    }
                }
            }

            for &dep in &self.seg(node).incoming {
                let Some(source) = self.dep(dep).source else { continue };
                let k = source.0 as usize;
                if st.mark[k] < 0 && self.dep(dep).weight > 0 {
                    st.out_weight[k] -= self.dep(dep).weight;
                    if st.out_weight[k] <= 0 && st.in_weight[k] > 0 {
                        sinks.push_back(source);
                    }
                }
            }
        }
    }

    struct MarkState {
        mark: Vec<i64>,
        in_weight: Vec<i64>,
        out_weight: Vec<i64>,
    }

    fn remove_from_unprocessed(segment: SegId, unprocessed: &mut Vec<SegId>) {
        if let Some(index) = unprocessed.iter().position(|&s| s == segment) {
            unprocessed.remove(index);
        }
    }
}
