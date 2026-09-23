//! Port of `alg/layered/p5edges/orthogonal/HyperEdgeSegmentSplitter.swift`.
//!
//! Breaks critical cycles by splitting hyperedge segments at free areas.
//! The Swift splitter keeps a reference to the routing generator that created
//! it (for `createDependencyIfNecessary` and its thresholds); here the
//! generator is passed to [`HyperEdgeSegmentSplitter::split_segments`].

use std::collections::HashSet;

use super::hyper_edge_segment::{DependencyId, HyperEdgeSegment, HyperEdgeSegmentGraph, SegmentId};
use super::hyper_edge_segment_dependency::HyperEdgeSegmentDependency;
use super::orthogonal_routing_generator::OrthogonalRoutingGenerator;
use crate::swift;

/// `FreeArea`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreeArea {
    pub start_position: f64,
    pub end_position: f64,
    pub size: f64,
}

impl FreeArea {
    pub fn new(start_position: f64, end_position: f64) -> FreeArea {
        // assert(endPosition >= startPosition): a no-op in release builds
        FreeArea { start_position, end_position, size: end_position - start_position }
    }
}

/// `AreaRating`.
#[derive(Clone, Copy, Debug, Default)]
pub struct AreaRating {
    pub dependencies: i64,
    pub crossings: i64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HyperEdgeSegmentSplitter;

impl HyperEdgeSegmentSplitter {
    pub fn new() -> HyperEdgeSegmentSplitter {
        HyperEdgeSegmentSplitter
    }

    /// `splitSegments(_:_:_:)` (the `inout` overload).
    pub fn split_segments(
        &self,
        routing_generator: &OrthogonalRoutingGenerator,
        graph: &mut HyperEdgeSegmentGraph,
        dependencies_to_resolve: &[DependencyId],
        segments: &mut Vec<SegmentId>,
        critical_conflict_threshold: f64,
    ) {
        if dependencies_to_resolve.is_empty() {
            return;
        }

        let mut free_areas = self.find_free_areas(graph, segments, critical_conflict_threshold);
        let segments_to_split = self.decide_which_segments_to_split(graph, dependencies_to_resolve);

        let sorted = swift::sorted_by(segments_to_split, |a, b| graph[*a].get_length() < graph[*b].get_length());
        for segment in sorted {
            self.split(routing_generator, graph, segment, segments, &mut free_areas, critical_conflict_threshold);
        }
    }

    /// `findFreeAreas(_:_:)`.
    pub fn find_free_areas(&self, graph: &HyperEdgeSegmentGraph, segments: &[SegmentId], critical_conflict_threshold: f64) -> Vec<FreeArea> {
        let mut free_areas: Vec<FreeArea> = Vec::new();
        let mut coordinates: Vec<f64> = Vec::new();

        for &segment in segments {
            coordinates.extend_from_slice(graph[segment].get_incoming_connection_coordinates());
            coordinates.extend_from_slice(graph[segment].get_outgoing_connection_coordinates());
        }

        swift::sort(&mut coordinates);

        if coordinates.len() < 2 {
            return free_areas;
        }

        for i in 1..coordinates.len() {
            if coordinates[i] - coordinates[i - 1] >= 2.0 * critical_conflict_threshold {
                free_areas.push(FreeArea::new(coordinates[i - 1] + critical_conflict_threshold, coordinates[i] - critical_conflict_threshold));
            }
        }

        free_areas
    }

    /// `decideWhichSegmentsToSplit(_:)`.
    pub fn decide_which_segments_to_split(&self, graph: &mut HyperEdgeSegmentGraph, dependencies: &[DependencyId]) -> Vec<SegmentId> {
        let mut segments_to_split: Vec<SegmentId> = Vec::new();
        let mut selected: HashSet<SegmentId> = HashSet::new();

        for &dependency in dependencies {
            let (Some(source_segment), Some(target_segment)) = (graph[dependency].get_source(), graph[dependency].get_target()) else { continue };

            if selected.contains(&source_segment) || selected.contains(&target_segment) {
                continue;
            }

            let mut segment_to_split = source_segment;
            let mut segment_causing_split = target_segment;

            if graph[source_segment].represents_hyperedge() && !graph[target_segment].represents_hyperedge() {
                segment_to_split = target_segment;
                segment_causing_split = source_segment;
            }

            segments_to_split.push(segment_to_split);
            selected.insert(segment_to_split);
            graph[segment_to_split].set_split_by(Some(segment_causing_split));
        }

        segments_to_split
    }

    /// `split(_:_:_:_:)`.
    pub fn split(
        &self,
        routing_generator: &OrthogonalRoutingGenerator,
        graph: &mut HyperEdgeSegmentGraph,
        segment: SegmentId,
        segments: &mut Vec<SegmentId>,
        free_areas: &mut Vec<FreeArea>,
        critical_conflict_threshold: f64,
    ) {
        let split_position = self.compute_position_to_split_and_update_free_areas(graph, segment, free_areas, critical_conflict_threshold);
        let partner = graph.split_at(segment, split_position);
        segments.push(partner);
        self.update_dependencies(routing_generator, graph, segment, segments);
    }

    /// `updateDependencies(_:_:)`.
    pub fn update_dependencies(&self, routing_generator: &OrthogonalRoutingGenerator, graph: &mut HyperEdgeSegmentGraph, segment: SegmentId, segments: &[SegmentId]) {
        let (Some(split_causing_segment), Some(split_partner)) = (graph[segment].get_split_by(), graph[segment].get_split_partner()) else { return };

        HyperEdgeSegmentDependency::create_and_add_critical(graph, segment, split_causing_segment);
        HyperEdgeSegmentDependency::create_and_add_critical(graph, split_causing_segment, split_partner);

        for &other_segment in segments {
            if other_segment != split_causing_segment && other_segment != segment && other_segment != split_partner {
                routing_generator.create_dependency_if_necessary(graph, other_segment, segment);
                routing_generator.create_dependency_if_necessary(graph, other_segment, split_partner);
            }
        }
    }

    /// `computePositionToSplitAndUpdateFreeAreas(_:_:_:)`.
    pub fn compute_position_to_split_and_update_free_areas(
        &self,
        graph: &HyperEdgeSegmentGraph,
        segment: SegmentId,
        free_areas: &mut Vec<FreeArea>,
        critical_conflict_threshold: f64,
    ) -> f64 {
        let mut first_possible_area_index: i64 = -1;
        let mut last_possible_area_index: i64 = -1;

        let seg = &graph[segment];
        for (i, curr_area) in free_areas.iter().enumerate() {
            if curr_area.start_position > seg.get_end_coordinate() {
                break;
            } else if curr_area.end_position >= seg.get_start_coordinate() {
                if first_possible_area_index < 0 {
                    first_possible_area_index = i as i64;
                }
                last_possible_area_index = i as i64;
            }
        }

        let mut split_position = Self::center_of_segment(seg);

        if first_possible_area_index >= 0 {
            let best_area_index = self.choose_best_area_index(graph, segment, free_areas, first_possible_area_index as usize, last_possible_area_index as usize);
            split_position = Self::center_of_area(&free_areas[best_area_index]);
            self.use_area(free_areas, best_area_index, critical_conflict_threshold);
        }

        split_position
    }

    /// `chooseBestAreaIndex(_:_:_:_:)`.
    pub fn choose_best_area_index(&self, graph: &HyperEdgeSegmentGraph, segment: SegmentId, free_areas: &[FreeArea], from_index: usize, to_index: usize) -> usize {
        let mut best_area_index = from_index;

        if from_index < to_index {
            let (mut split_segment, mut split_partner) = graph[segment].simulate_split();

            let mut best_area = free_areas[best_area_index];
            let mut best_rating = self.rate_area(graph, segment, &mut split_segment, &mut split_partner, &best_area);

            for i in (from_index + 1)..=to_index {
                let curr_area = free_areas[i];
                let curr_rating = self.rate_area(graph, segment, &mut split_segment, &mut split_partner, &curr_area);

                if self.is_better(&curr_area, &curr_rating, &best_area, &best_rating) {
                    best_area = curr_area;
                    best_rating = curr_rating;
                    best_area_index = i;
                }
            }
        }

        best_area_index
    }

    /// `rateArea(_:_:_:_:)`. Like the Swift, only the simulated segments'
    /// connection coordinates are replaced; their extents stay as
    /// `simulateSplit` computed them.
    pub fn rate_area(
        &self,
        graph: &HyperEdgeSegmentGraph,
        segment: SegmentId,
        split_segment: &mut HyperEdgeSegment,
        split_partner: &mut HyperEdgeSegment,
        area: &FreeArea,
    ) -> AreaRating {
        let area_centre = Self::center_of_area(area);

        split_segment.set_outgoing_connection_coordinates(vec![area_centre]);
        split_partner.set_incoming_connection_coordinates(vec![area_centre]);

        let mut rating = AreaRating::default();

        for &dependency in graph[segment].get_incoming_segment_dependencies() {
            let Some(other_segment) = graph[dependency].get_source() else { continue };
            self.update_considering_both_orderings(&mut rating, split_segment, &graph[other_segment]);
            self.update_considering_both_orderings(&mut rating, split_partner, &graph[other_segment]);
        }

        for &dependency in graph[segment].get_outgoing_segment_dependencies() {
            let Some(other_segment) = graph[dependency].get_target() else { continue };
            self.update_considering_both_orderings(&mut rating, split_segment, &graph[other_segment]);
            self.update_considering_both_orderings(&mut rating, split_partner, &graph[other_segment]);
        }

        rating.dependencies += 2;
        if let Some(split_by) = graph[segment].get_split_by() {
            rating.crossings += self.count_crossings_for_single_ordering(split_segment, &graph[split_by]);
            rating.crossings += self.count_crossings_for_single_ordering(&graph[split_by], split_partner);
        }

        rating
    }

    /// `updateConsideringBothOrderings(_:_:_:)`.
    pub fn update_considering_both_orderings(&self, rating: &mut AreaRating, s1: &HyperEdgeSegment, s2: &HyperEdgeSegment) {
        let crossings_s1_left_of_s2 = self.count_crossings_for_single_ordering(s1, s2);
        let crossings_s2_left_of_s1 = self.count_crossings_for_single_ordering(s2, s1);

        if crossings_s1_left_of_s2 == crossings_s2_left_of_s1 {
            if crossings_s1_left_of_s2 > 0 {
                rating.dependencies += 2;
                rating.crossings += crossings_s1_left_of_s2;
            }
        } else {
            rating.dependencies += 1;
            rating.crossings += swift::min(crossings_s1_left_of_s2, crossings_s2_left_of_s1);
        }
    }

    /// `countCrossingsForSingleOrdering(_:_:)`.
    pub fn count_crossings_for_single_ordering(&self, left: &HyperEdgeSegment, right: &HyperEdgeSegment) -> i64 {
        OrthogonalRoutingGenerator::count_crossings(left.get_outgoing_connection_coordinates(), right.get_start_coordinate(), right.get_end_coordinate())
            + OrthogonalRoutingGenerator::count_crossings(right.get_incoming_connection_coordinates(), left.get_start_coordinate(), left.get_end_coordinate())
    }

    /// `isBetter(_:_:_:_:)`.
    pub fn is_better(&self, curr_area: &FreeArea, curr_rating: &AreaRating, best_area: &FreeArea, best_rating: &AreaRating) -> bool {
        if curr_rating.crossings < best_rating.crossings {
            return true;
        } else if curr_rating.crossings == best_rating.crossings {
            if curr_rating.dependencies < best_rating.dependencies {
                return true;
            } else if curr_rating.dependencies == best_rating.dependencies && curr_area.size > best_area.size {
                return true;
            }
        }

        false
    }

    /// `useArea(_:_:_:)`.
    pub fn use_area(&self, free_areas: &mut Vec<FreeArea>, used_area_index: usize, critical_conflict_threshold: f64) {
        let old_area = free_areas.remove(used_area_index);

        if old_area.size / 2.0 >= critical_conflict_threshold {
            let mut insert_index = used_area_index;
            let old_area_centre = Self::center_of_area(&old_area);

            let new_end1 = old_area_centre - critical_conflict_threshold;
            if old_area.start_position <= new_end1 {
                free_areas.insert(insert_index, FreeArea::new(old_area.start_position, new_end1));
                insert_index += 1;
            }

            let new_start2 = old_area_centre + critical_conflict_threshold;
            if new_start2 <= old_area.end_position {
                free_areas.insert(insert_index, FreeArea::new(new_start2, old_area.end_position));
            }
        }
    }

    /// `center(_ segment:)`.
    pub fn center_of_segment(segment: &HyperEdgeSegment) -> f64 {
        Self::center(segment.get_start_coordinate(), segment.get_end_coordinate())
    }

    /// `center(_ area:)`.
    pub fn center_of_area(area: &FreeArea) -> f64 {
        Self::center(area.start_position, area.end_position)
    }

    /// `center(_:_:)`.
    pub fn center(p1: f64, p2: f64) -> f64 {
        (p1 + p2) / 2.0
    }
}
