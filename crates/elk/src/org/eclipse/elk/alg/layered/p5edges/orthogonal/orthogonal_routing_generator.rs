//! Port of `alg/layered/p5edges/orthogonal/OrthogonalRoutingGenerator.swift`.
//!
//! Routes the edges between two layers orthogonally: builds hyperedge
//! segments, orders them by dependencies (breaking cycles, splitting segments
//! on critical cycles), assigns routing slots and lets the routing direction
//! strategy create bend points and junction points.

use std::collections::{HashMap, VecDeque};

use super::direction::base_routing_direction_strategy::BaseRoutingDirectionStrategy;
use super::direction::routing_direction::RoutingDirection;
use super::hyper_edge_cycle_detector::HyperEdgeCycleDetector;
use super::hyper_edge_segment::{HyperEdgeSegmentGraph, SegmentId};
use super::hyper_edge_segment_dependency::HyperEdgeSegmentDependency;
use super::hyper_edge_segment_splitter::HyperEdgeSegmentSplitter;
use crate::prelude::*;

/// differences below this tolerance value are treated as zero.
pub const TOLERANCE: f64 = 1e-3;

/// a special return value used by the conflict counting method.
pub const CRITICAL_CONFLICTS_DETECTED: i64 = -1;

/// factor for edge spacing used to determine the conflictThreshold (determined experimentally).
pub const CONFLICT_THRESHOLD_FACTOR: f64 = 0.5;
/// factor to compute criticalConflictThreshold (determined experimentally).
pub const CRITICAL_CONFLICT_THRESHOLD_FACTOR: f64 = 0.2;

/// weight penalty for (non-critical) conflicts.
pub const CONFLICT_PENALTY: i64 = 1;
/// weight penalty for crossings.
pub const CROSSING_PENALTY: i64 = 16;

pub struct OrthogonalRoutingGenerator {
    /// we'll be using this thing to split hyper edge segments, if necessary.
    pub segment_splitter: Option<HyperEdgeSegmentSplitter>,
    /// routing direction strategy.
    pub routing_strategy: BaseRoutingDirectionStrategy,
    /// spacing between edges.
    pub edge_spacing: f64,
    /// Threshold at which horizontal line segments are considered to be too close to one another.
    pub conflict_threshold: f64,
    /// Threshold at which horizontal line segments are considered to overlap.
    pub critical_conflict_threshold: f64,
    /// prefix of debug output files.
    pub debug_prefix: Option<String>,
}

impl Default for OrthogonalRoutingGenerator {
    /// `init()`.
    fn default() -> Self {
        OrthogonalRoutingGenerator {
            segment_splitter: None,
            routing_strategy: BaseRoutingDirectionStrategy::new(),
            edge_spacing: 0.0,
            conflict_threshold: 0.0,
            critical_conflict_threshold: 0.0,
            debug_prefix: None,
        }
    }
}

impl OrthogonalRoutingGenerator {
    /// `init(_:_:_:)`.
    pub fn new(direction: RoutingDirection, edge_spacing: f64, debug_prefix: Option<&str>) -> OrthogonalRoutingGenerator {
        OrthogonalRoutingGenerator {
            segment_splitter: None,
            routing_strategy: BaseRoutingDirectionStrategy::for_routing_direction(direction),
            edge_spacing,
            conflict_threshold: CONFLICT_THRESHOLD_FACTOR * edge_spacing,
            critical_conflict_threshold: 0.0,
            debug_prefix: debug_prefix.map(str::to_string),
        }
    }

    /// `routeEdges(_:_:_:_:_:_:)`: routes the edges between two layers and
    /// returns the number of routing slots used.
    #[allow(clippy::too_many_arguments)]
    pub fn route_edges(
        &mut self,
        _monitor: &mut dyn IElkProgressMonitor,
        lg: &mut LGraphArena,
        _layered_graph: LGraphId,
        source_layer_nodes: Option<&[LNodeId]>,
        _source_layer_index: i64,
        target_layer_nodes: Option<&[LNodeId]>,
        start_pos: f64,
    ) -> i64 {
        // Keep track of our hyperedge segments, and which ports they were created for
        let mut graph = HyperEdgeSegmentGraph::new();
        let mut port_to_edge_segment_map: HashMap<LPortId, SegmentId> = HashMap::new();
        let mut edge_segments: Vec<SegmentId> = Vec::new();

        // create hyperedge segments for eastern output ports of the left layer and for western output ports of
        // the right layer
        let source_side = self.routing_strategy.get_source_port_side();
        let target_side = self.routing_strategy.get_target_port_side();
        self.create_hyper_edge_segments(lg, &mut graph, source_layer_nodes, source_side, &mut edge_segments, &mut port_to_edge_segment_map);
        self.create_hyper_edge_segments(lg, &mut graph, target_layer_nodes, target_side, &mut edge_segments, &mut port_to_edge_segment_map);

        // Our critical conflict threshold is a fraction of the minimum distance between two horizontal hyperedge
        // segments
        self.critical_conflict_threshold = CRITICAL_CONFLICT_THRESHOLD_FACTOR * Self::minimum_horizontal_segment_distance(&graph, &edge_segments);

        // create dependencies for the hyperedge segment ordering graph and note how many critical dependencies have
        // been created
        let mut critical_dependency_count = 0;
        let n = edge_segments.len();
        for first_idx in 0..n.saturating_sub(1) {
            let first_segment = edge_segments[first_idx];
            for second_idx in (first_idx + 1)..n {
                critical_dependency_count += self.create_dependency_if_necessary(&mut graph, first_segment, edge_segments[second_idx]);
            }
        }

        // if there are at least two critical dependencies, there may be critical cycles that need to be broken
        if critical_dependency_count >= 2 {
            self.break_critical_cycles(&mut graph, &mut edge_segments);
        }

        // break non-critical cycles
        Self::break_non_critical_cycles(&mut graph, &edge_segments);

        // assign ranks to the edge segments
        Self::topological_numbering(&mut graph, &edge_segments);

        // set bend points with appropriate coordinates
        let mut rank_count: i64 = -1;
        for &node in &edge_segments {
            // edges that are just straight lines don't take up a slot and don't need bend points
            if (graph[node].get_start_coordinate() - graph[node].get_end_coordinate()).abs() < TOLERANCE {
                continue;
            }

            rank_count = swift::max(rank_count, graph[node].get_routing_slot());

            self.routing_strategy.calculate_bend_points(lg, &graph, node, start_pos, self.edge_spacing);
        }

        // release the created resources
        self.routing_strategy.clear_created_junction_points();
        rank_count + 1
    }

    /// `createHyperEdgeSegments(_:_:_:_:)`.
    fn create_hyper_edge_segments(
        &self,
        lg: &LGraphArena,
        graph: &mut HyperEdgeSegmentGraph,
        nodes: Option<&[LNodeId]>,
        port_side: PortSide,
        hyper_edges: &mut Vec<SegmentId>,
        port_to_hyper_edge_segment_map: &mut HashMap<LPortId, SegmentId>,
    ) {
        let Some(nodes) = nodes else { return };

        for &node in nodes {
            // `node.getPorts(.OUTPUT, portSide)`
            for &port in &lg[node].ports {
                if lg[port].side != port_side || lg[port].outgoing_edges.is_empty() {
                    continue;
                }
                if !port_to_hyper_edge_segment_map.contains_key(&port) {
                    let new_hyper_edge = graph.new_segment(self.routing_strategy.kind);
                    hyper_edges.push(new_hyper_edge);
                    graph.add_port_positions(lg, new_hyper_edge, port, port_to_hyper_edge_segment_map);
                }
            }
        }
    }

    /// `minimumHorizontalSegmentDistance(_:)`: the minimum distance between any
    /// two adjacent source connections and any two adjacent target connections.
    fn minimum_horizontal_segment_distance(graph: &HyperEdgeSegmentGraph, edge_segments: &[SegmentId]) -> f64 {
        let mut all_incoming: Vec<f64> = Vec::new();
        let mut all_outgoing: Vec<f64> = Vec::new();

        for &segment in edge_segments {
            all_incoming.extend_from_slice(graph[segment].get_incoming_connection_coordinates());
            all_outgoing.extend_from_slice(graph[segment].get_outgoing_connection_coordinates());
        }

        let min_incoming_distance = Self::minimum_difference(all_incoming);
        let min_outgoing_distance = Self::minimum_difference(all_outgoing);

        swift::min(min_incoming_distance, min_outgoing_distance)
    }

    /// `minimumDifference(_:)`: the smallest difference between any two
    /// distinct numbers, or `Double.greatestFiniteMagnitude`.
    ///
    /// Swift sorts `Array(Set(numbers))`; the set's (seeded) order does not
    /// survive the sort of distinct values, so sorting and then dropping
    /// equal neighbours is the same (`-0.0`/`0.0` are one set element and give
    /// the same differences either way).
    fn minimum_difference(mut numbers: Vec<f64>) -> f64 {
        swift::sort(&mut numbers);
        numbers.dedup_by(|a, b| a == b);

        let mut min_difference = f64::MAX;

        if numbers.len() >= 2 {
            for i in 1..numbers.len() {
                let diff = numbers[i] - numbers[i - 1];
                min_difference = swift::min(min_difference, diff);
            }
        }

        min_difference
    }

    /// `createDependencyIfNecessary(_:_:)`: creates a dependency between the
    /// two segments if one is needed; returns the number of critical ones.
    pub fn create_dependency_if_necessary(&self, graph: &mut HyperEdgeSegmentGraph, he1: SegmentId, he2: SegmentId) -> i64 {
        // check if at least one of the two nodes is just a straight line; those don't
        // create dependencies since they don't take up a slot
        if (graph[he1].get_start_coordinate() - graph[he1].get_end_coordinate()).abs() < TOLERANCE
            || (graph[he2].get_start_coordinate() - graph[he2].get_end_coordinate()).abs() < TOLERANCE
        {
            return 0;
        }

        // compare number of conflicts for both variants
        let conflicts1 = self.count_conflicts(graph[he1].get_outgoing_connection_coordinates(), graph[he2].get_incoming_connection_coordinates());
        let conflicts2 = self.count_conflicts(graph[he2].get_outgoing_connection_coordinates(), graph[he1].get_incoming_connection_coordinates());

        let critical_conflicts_detected = conflicts1 == CRITICAL_CONFLICTS_DETECTED || conflicts2 == CRITICAL_CONFLICTS_DETECTED;
        let mut critical_dependency_count = 0;

        if critical_conflicts_detected {
            if conflicts1 == CRITICAL_CONFLICTS_DETECTED {
                HyperEdgeSegmentDependency::create_and_add_critical(graph, he2, he1);
                critical_dependency_count += 1;
            }

            if conflicts2 == CRITICAL_CONFLICTS_DETECTED {
                HyperEdgeSegmentDependency::create_and_add_critical(graph, he1, he2);
                critical_dependency_count += 1;
            }
        } else {
            let (s1, s2) = (&graph[he1], &graph[he2]);
            let mut crossings1 = Self::count_crossings(s1.get_outgoing_connection_coordinates(), s2.get_start_coordinate(), s2.get_end_coordinate());
            crossings1 += Self::count_crossings(s2.get_incoming_connection_coordinates(), s1.get_start_coordinate(), s1.get_end_coordinate());
            let mut crossings2 = Self::count_crossings(s2.get_outgoing_connection_coordinates(), s1.get_start_coordinate(), s1.get_end_coordinate());
            crossings2 += Self::count_crossings(s1.get_incoming_connection_coordinates(), s2.get_start_coordinate(), s2.get_end_coordinate());

            let dep_value1 = CONFLICT_PENALTY * conflicts1 + CROSSING_PENALTY * crossings1;
            let dep_value2 = CONFLICT_PENALTY * conflicts2 + CROSSING_PENALTY * crossings2;

            if dep_value1 < dep_value2 {
                HyperEdgeSegmentDependency::create_and_add_regular(graph, he1, he2, dep_value2 - dep_value1);
            } else if dep_value1 > dep_value2 {
                HyperEdgeSegmentDependency::create_and_add_regular(graph, he2, he1, dep_value1 - dep_value2);
            } else if dep_value1 > 0 && dep_value2 > 0 {
                HyperEdgeSegmentDependency::create_and_add_regular(graph, he1, he2, 0);
                HyperEdgeSegmentDependency::create_and_add_regular(graph, he2, he1, 0);
            }
        }

        critical_dependency_count
    }

    /// `countConflicts(_:_:)`.
    pub fn count_conflicts(&self, posis1: &[f64], posis2: &[f64]) -> i64 {
        let mut conflicts = 0;

        if !posis1.is_empty() && !posis2.is_empty() {
            let mut iter1_index = 0;
            let mut iter2_index = 0;
            let mut pos1 = posis1[iter1_index];
            let mut pos2 = posis2[iter2_index];
            let mut has_more = true;

            loop {
                if pos1 > pos2 - self.critical_conflict_threshold && pos1 < pos2 + self.critical_conflict_threshold {
                    return -1;
                } else if pos1 > pos2 - self.conflict_threshold && pos1 < pos2 + self.conflict_threshold {
                    conflicts += 1;
                }

                if pos1 <= pos2 && iter1_index + 1 < posis1.len() {
                    iter1_index += 1;
                    pos1 = posis1[iter1_index];
                } else if pos2 <= pos1 && iter2_index + 1 < posis2.len() {
                    iter2_index += 1;
                    pos2 = posis2[iter2_index];
                } else {
                    has_more = false;
                }
                if !has_more {
                    break;
                }
            }
        }

        conflicts
    }

    /// `countCrossings(_:_:_:)`.
    pub fn count_crossings(posis: &[f64], start: f64, end: f64) -> i64 {
        let mut crossings = 0;
        for &pos in posis {
            if pos > end {
                break;
            } else if pos >= start {
                crossings += 1;
            }
        }
        crossings
    }

    /// `breakCriticalCycles(_:)`.
    fn break_critical_cycles(&mut self, graph: &mut HyperEdgeSegmentGraph, edge_segments: &mut Vec<SegmentId>) {
        let cycle_dependencies = HyperEdgeCycleDetector::detect_cycles(graph, edge_segments, true);

        // Lazy initialisation
        if self.segment_splitter.is_none() {
            self.segment_splitter = Some(HyperEdgeSegmentSplitter::new());
        }

        let Some(splitter) = self.segment_splitter else { return };
        splitter.split_segments(self, graph, &cycle_dependencies, edge_segments, self.critical_conflict_threshold);
    }

    /// `breakNonCriticalCycles(_:)`: removes (weight 0) or reverses the
    /// dependencies that close cycles.
    pub fn break_non_critical_cycles(graph: &mut HyperEdgeSegmentGraph, edge_segments: &[SegmentId]) {
        let cycle_dependencies = HyperEdgeCycleDetector::detect_cycles(graph, edge_segments, false);

        for cycle_dependency in cycle_dependencies {
            if graph[cycle_dependency].get_weight() == 0 {
                graph.dependency_remove(cycle_dependency);
            } else {
                graph.dependency_reverse(cycle_dependency);
            }
        }
    }

    /// `topologicalNumbering(_:)`.
    fn topological_numbering(graph: &mut HyperEdgeSegmentGraph, segments: &[SegmentId]) {
        let mut sources: VecDeque<SegmentId> = VecDeque::new();
        let mut rightward_targets: VecDeque<SegmentId> = VecDeque::new();

        for &node in segments {
            let in_count = graph[node].get_incoming_segment_dependencies().len() as i64;
            let out_count = graph[node].get_outgoing_segment_dependencies().len() as i64;
            graph[node].set_in_weight(in_count);
            graph[node].set_out_weight(out_count);

            if graph[node].get_in_weight() == 0 {
                sources.push_back(node);
            }

            if graph[node].get_out_weight() == 0 && graph[node].get_incoming_connection_coordinates().is_empty() {
                rightward_targets.push_back(node);
            }
        }

        let mut max_rank: i64 = -1;

        // assign ranks using topological numbering
        while let Some(node) = sources.pop_front() {
            for di in 0..graph[node].outgoing_segment_deps.len() {
                let dep = graph[node].outgoing_segment_deps[di];
                let Some(target) = graph[dep].get_target() else { continue };
                let slot = swift::max(graph[target].get_routing_slot(), graph[node].get_routing_slot() + 1);
                graph[target].set_routing_slot(slot);
                max_rank = swift::max(max_rank, graph[target].get_routing_slot());

                let w = graph[target].get_in_weight() - 1;
                graph[target].set_in_weight(w);
                if graph[target].get_in_weight() == 0 {
                    sources.push_back(target);
                }
            }
        }

        // Move hyperedge segments with horizontal segments only pointing rightwards as far right as possible
        if max_rank > -1 {
            for &node in &rightward_targets {
                graph[node].set_routing_slot(max_rank);
            }

            while let Some(node) = rightward_targets.pop_front() {
                for di in 0..graph[node].incoming_segment_deps.len() {
                    let dep = graph[node].incoming_segment_deps[di];
                    let Some(source) = graph[dep].get_source() else { continue };
                    if !graph[source].get_incoming_connection_coordinates().is_empty() {
                        continue;
                    }

                    let slot = swift::min(graph[source].get_routing_slot(), graph[node].get_routing_slot() - 1);
                    graph[source].set_routing_slot(slot);

                    let w = graph[source].get_out_weight() - 1;
                    graph[source].set_out_weight(w);
                    if graph[source].get_out_weight() == 0 {
                        rightward_targets.push_back(source);
                    }
                }
            }
        }
    }
}
