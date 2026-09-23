//! Port of `alg/layered/p5edges/orthogonal/HyperEdgeCycleDetector.swift`.
//!
//! Finds the dependencies to remove or reverse to make the segment dependency
//! graph acyclic (a greedy linear ordering after Eades, Lin and Smyth).
//!
//! The Swift keeps marks and weights in `[ObjectIdentifier: Int]` dictionaries
//! that are only ever looked up (a missing entry reads as 0); here they are
//! dense arrays indexed by segment id, initialised to 0.

use std::collections::VecDeque;

use super::hyper_edge_segment::{DependencyId, HyperEdgeSegmentGraph, SegmentId};
use super::hyper_edge_segment_dependency::DependencyType;
use crate::swift;

/// Per-run bookkeeping (`markBySegment`, `inWeightBySegment`, …).
struct Weights {
    mark: Vec<i64>,
    in_weight: Vec<i64>,
    out_weight: Vec<i64>,
    critical_in_weight: Vec<i64>,
    critical_out_weight: Vec<i64>,
}

impl Weights {
    fn new(n: usize) -> Weights {
        Weights { mark: vec![0; n], in_weight: vec![0; n], out_weight: vec![0; n], critical_in_weight: vec![0; n], critical_out_weight: vec![0; n] }
    }
}

pub struct HyperEdgeCycleDetector;

impl HyperEdgeCycleDetector {
    /// `detectCycles(_:_:_:)` with no `random` (what every caller passes).
    pub fn detect_cycles(graph: &HyperEdgeSegmentGraph, segments: &[SegmentId], critical_only: bool) -> Vec<DependencyId> {
        Self::detect_cycles_with_random(graph, segments, critical_only, None)
    }

    /// `detectCycles(_:_:_:)`. `random` is the Swift `random: Any?` already
    /// converted by `seed(from:)` (see [`HyperEdgeCycleDetector::seed_from_u64`]
    /// and friends); no caller in elk-swift passes one.
    pub fn detect_cycles_with_random(graph: &HyperEdgeSegmentGraph, segments: &[SegmentId], critical_only: bool, random: Option<u64>) -> Vec<DependencyId> {
        let mut result: Vec<DependencyId> = Vec::new();
        let mut sources: VecDeque<SegmentId> = VecDeque::new();
        let mut sinks: VecDeque<SegmentId> = VecDeque::new();

        let mut w = Weights::new(graph.segments.len());

        Self::initialize(graph, segments, &mut sources, &mut sinks, critical_only, &mut w);

        Self::compute_linear_ordering_marks(graph, segments, &mut sources, &mut sinks, critical_only, random, &mut w);

        for &source in segments {
            let source_mark = w.mark[source.index()];
            for &out_dependency in graph[source].get_outgoing_segment_dependencies() {
                let dep = &graph[out_dependency];
                if !critical_only || dep.get_type() == DependencyType::CRITICAL {
                    let Some(target) = dep.get_target() else { continue };

                    let target_mark = w.mark[target.index()];
                    if source_mark > target_mark {
                        result.push(out_dependency);
                    }
                }
            }
        }

        result
    }

    /// `initialize(...)`.
    fn initialize(
        graph: &HyperEdgeSegmentGraph,
        segments: &[SegmentId],
        sources: &mut VecDeque<SegmentId>,
        sinks: &mut VecDeque<SegmentId>,
        critical_only: bool,
        w: &mut Weights,
    ) {
        let mut next_mark: i64 = -1;

        for &segment in segments {
            let key = segment.index();
            w.mark[key] = next_mark;
            next_mark -= 1;

            let seg = &graph[segment];
            let critical_incoming: i64 = seg
                .get_incoming_segment_dependencies()
                .iter()
                .filter(|&&d| graph[d].get_type() == DependencyType::CRITICAL)
                .fold(0, |acc, &d| acc + graph[d].get_weight());

            let critical_outgoing: i64 = seg
                .get_outgoing_segment_dependencies()
                .iter()
                .filter(|&&d| graph[d].get_type() == DependencyType::CRITICAL)
                .fold(0, |acc, &d| acc + graph[d].get_weight());

            let mut in_weight = critical_incoming;
            let mut out_weight = critical_outgoing;

            if !critical_only {
                in_weight = seg.get_incoming_segment_dependencies().iter().fold(0, |acc, &d| acc + graph[d].get_weight());
                out_weight = seg.get_outgoing_segment_dependencies().iter().fold(0, |acc, &d| acc + graph[d].get_weight());
            }

            w.in_weight[key] = in_weight;
            w.critical_in_weight[key] = critical_incoming;
            w.out_weight[key] = out_weight;
            w.critical_out_weight[key] = critical_outgoing;

            if out_weight == 0 {
                sinks.push_back(segment);
            } else if in_weight == 0 {
                sources.push_back(segment);
            }
        }
    }

    /// `computeLinearOrderingMarks(...)`.
    fn compute_linear_ordering_marks(
        graph: &HyperEdgeSegmentGraph,
        segments: &[SegmentId],
        sources: &mut VecDeque<SegmentId>,
        sinks: &mut VecDeque<SegmentId>,
        critical_only: bool,
        random: Option<u64>,
        w: &mut Weights,
    ) {
        // Sorted by the initial (distinct) marks -1, -2, …: the reverse of `segments`.
        let mut unprocessed: Vec<SegmentId> = segments.to_vec();
        {
            let marks = &w.mark;
            crate::swift::sort_by(&mut unprocessed, |a, b| marks[a.index()] < marks[b.index()]);
        }
        let mut max_segments: Vec<SegmentId> = Vec::new();

        let mark_base = segments.len() as i64;
        let mut next_sink_mark = mark_base - 1;
        let mut next_source_mark = mark_base + 1;
        // `random.map(seed(from:))`
        let mut seeded_random_state: Option<u64> = random;

        while !unprocessed.is_empty() {
            while let Some(sink) = sinks.pop_front() {
                Self::remove_from_unprocessed(sink, &mut unprocessed);
                w.mark[sink.index()] = next_sink_mark;
                next_sink_mark -= 1;

                Self::update_neighbors(graph, sink, sources, sinks, critical_only, w);
            }

            while let Some(source) = sources.pop_front() {
                Self::remove_from_unprocessed(source, &mut unprocessed);
                w.mark[source.index()] = next_source_mark;
                next_source_mark += 1;

                Self::update_neighbors(graph, source, sources, sinks, critical_only, w);
            }

            let mut max_outflow = i64::MIN;
            for &segment in &unprocessed {
                let key = segment.index();

                if !critical_only && w.critical_out_weight[key] > 0 && w.critical_in_weight[key] <= 0 {
                    max_segments.clear();
                    max_segments.push(segment);
                    break;
                }

                let outflow = w.out_weight[key] - w.in_weight[key];
                if outflow >= max_outflow {
                    if outflow > max_outflow {
                        max_segments.clear();
                        max_outflow = outflow;
                    }
                    max_segments.push(segment);
                }
            }

            if !max_segments.is_empty() {
                let index = Self::next_random_int(max_segments.len(), random, &mut seeded_random_state);
                let max_node = max_segments[index];
                Self::remove_from_unprocessed(max_node, &mut unprocessed);
                w.mark[max_node.index()] = next_source_mark;
                next_source_mark += 1;

                Self::update_neighbors(graph, max_node, sources, sinks, critical_only, w);
                max_segments.clear();
            }
        }

        let shift_base = segments.len() as i64 + 1;
        for &node in segments {
            let key = node.index();
            let mark = w.mark[key];
            if mark < mark_base {
                w.mark[key] = mark + shift_base;
            }
        }
    }

    /// `updateNeighbors(...)`.
    fn update_neighbors(
        graph: &HyperEdgeSegmentGraph,
        node: SegmentId,
        sources: &mut VecDeque<SegmentId>,
        sinks: &mut VecDeque<SegmentId>,
        critical_only: bool,
        w: &mut Weights,
    ) {
        for &d in graph[node].get_outgoing_segment_dependencies() {
            let dep = &graph[d];
            if !critical_only || dep.get_type() == DependencyType::CRITICAL {
                let Some(target) = dep.get_target() else { continue };

                let target_key = target.index();
                if w.mark[target_key] < 0 && dep.get_weight() > 0 {
                    w.in_weight[target_key] -= dep.get_weight();
                    if dep.get_type() == DependencyType::CRITICAL {
                        w.critical_in_weight[target_key] -= dep.get_weight();
                    }

                    if w.in_weight[target_key] <= 0 && w.out_weight[target_key] > 0 {
                        sources.push_back(target);
                    }
                }
            }
        }

        for &d in graph[node].get_incoming_segment_dependencies() {
            let dep = &graph[d];
            if !critical_only || dep.get_type() == DependencyType::CRITICAL {
                let Some(source) = dep.get_source() else { continue };

                let source_key = source.index();
                if w.mark[source_key] < 0 && dep.get_weight() > 0 {
                    w.out_weight[source_key] -= dep.get_weight();
                    if dep.get_type() == DependencyType::CRITICAL {
                        w.critical_out_weight[source_key] -= dep.get_weight();
                    }

                    if w.out_weight[source_key] <= 0 && w.in_weight[source_key] > 0 {
                        sinks.push_back(source);
                    }
                }
            }
        }
    }

    /// `removeFromUnprocessed(_:_:)`.
    fn remove_from_unprocessed(segment: SegmentId, unprocessed: &mut Vec<SegmentId>) {
        if let Some(index) = unprocessed.iter().position(|&s| s == segment) {
            unprocessed.remove(index);
        }
    }

    /// `nextRandomInt(_:_:_:)`.
    ///
    /// NONDETERMINISTIC IN SWIFT: with no `random` and no seeded state the
    /// Swift falls through to `Int.random(in: 0..<bound)` — the system RNG,
    /// different on every run — whenever several segments tie for the maximal
    /// outflow (every elk-swift caller passes no `random`; Java ELK draws from
    /// the layout's seeded `Random`). The port takes the first candidate
    /// (index 0) in that branch and never draws from the graph's `Random`; the
    /// seeded branches are ported as written.
    pub fn next_random_int(bound: usize, random: Option<u64>, seeded_state: &mut Option<u64>) -> usize {
        if bound <= 1 {
            return 0;
        }

        if let Some(mut state) = *seeded_state {
            state = 2862933555777941757u64.wrapping_mul(state).wrapping_add(3037000493);
            *seeded_state = Some(state);
            let upper53 = state >> 11;
            let unit = upper53 as f64 / (1u64 << 53) as f64;
            let index = (unit * bound as f64) as i64;
            return swift::min(index, bound as i64 - 1) as usize;
        }

        if let Some(rng) = random {
            *seeded_state = Some(rng);
            return Self::next_random_int(bound, Some(rng), seeded_state);
        }

        0
    }

    /// `seed(from:)` for a `UInt64` (`UInt32`/`UInt` widen the same way).
    pub fn seed_from_u64(v: u64) -> u64 {
        v
    }

    /// `seed(from:)` for an `Int64`/`Int32`/`Int`: the bit pattern.
    pub fn seed_from_i64(v: i64) -> u64 {
        v as u64
    }

    /// `seed(from:)` for a `Double`: the bit pattern. (Strings and other
    /// values seed from `hashValue`, which Swift randomises per process.)
    pub fn seed_from_f64(v: f64) -> u64 {
        v.to_bits()
    }
}
