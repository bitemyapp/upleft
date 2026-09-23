//! Port of `alg/layered/p5edges/orthogonal/HyperEdgeSegment.swift`.
//!
//! Hyperedge segments and their dependencies form a cross-linked object graph
//! in Swift (segments list their dependencies, dependencies point at their
//! segments, split partners point at each other). Here both live in a
//! [`HyperEdgeSegmentGraph`] arena and refer to each other by
//! [`SegmentId`] / [`DependencyId`]; Swift's `===` is id equality. A graph is
//! created per routing run (`OrthogonalRoutingGenerator.routeEdges`, or a
//! self-loop routing-slot assignment) and dropped afterwards, as the Swift
//! objects are.
//!
//! The Swift class is `Comparable`/`Hashable` by `mark`, but nothing reachable
//! compares or hashes segments (the cycle detector and the splitter key by
//! `ObjectIdentifier`), so that is not modelled.

use std::ops::{Index, IndexMut};

use super::direction::base_routing_direction_strategy::RoutingStrategyKind;
use super::hyper_edge_segment_dependency::HyperEdgeSegmentDependency;
use crate::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct SegmentId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct DependencyId(pub u32);

impl SegmentId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl DependencyId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug)]
pub struct HyperEdgeSegment {
    /// The routing strategy the segment was created with (only its kind
    /// matters to a segment: it asks it for port sides and positions).
    pub routing_strategy: RoutingStrategyKind,
    /// ports represented by this hypernode.
    pub ports: Vec<LPortId>,
    /// mark value used for cycle breaking (to be accessed directly).
    pub mark: i64,
    /// the routing slot determines the horizontal distance to the preceding layer.
    pub routing_slot: i64,
    /// start position of this edge segment (in horizontal layouts, this is the topmost y coordinate).
    pub start_position: f64,
    /// end position of this edge segment (in horizontal layouts, this is the bottommost y coordinate).
    pub end_position: f64,
    /// sorted list of coordinates where incoming connections enter this segment.
    pub incoming_connection_coordinates: Vec<f64>,
    /// sorted list of coordinates where outgoing connections leave this segment.
    pub outgoing_connection_coordinates: Vec<f64>,
    /// list of outgoing dependencies to other edge segments.
    pub outgoing_segment_deps: Vec<DependencyId>,
    pub out_dep_weight: i64,
    pub critical_out_dep_weight: i64,
    /// list of incoming dependencies from other edge segments.
    pub incoming_segment_deps: Vec<DependencyId>,
    pub in_dep_weight: i64,
    pub critical_in_dep_weight: i64,
    /// if this segment is the result of a split segment, this is the other segment.
    pub split_partner: Option<SegmentId>,
    /// the segment that caused this segment to be split, if any (only set on one of the split partners).
    pub split_by: Option<SegmentId>,
}

impl HyperEdgeSegment {
    /// `init(_ routingStrategy:)` (not yet in any arena).
    pub fn new(routing_strategy: RoutingStrategyKind) -> HyperEdgeSegment {
        HyperEdgeSegment {
            routing_strategy,
            ports: Vec::new(),
            mark: 0,
            routing_slot: 0,
            start_position: f64::NAN,
            end_position: f64::NAN,
            incoming_connection_coordinates: Vec::new(),
            outgoing_connection_coordinates: Vec::new(),
            outgoing_segment_deps: Vec::new(),
            out_dep_weight: 0,
            critical_out_dep_weight: 0,
            incoming_segment_deps: Vec::new(),
            in_dep_weight: 0,
            critical_in_dep_weight: 0,
            split_partner: None,
            split_by: None,
        }
    }

    /// `insertSorted(_:_:)`: inserts `value` unless a value equal to it as a
    /// `Float` is present.
    pub fn insert_sorted(list: &mut Vec<f64>, value: f64) {
        let mut insert_index = list.len();
        for i in 0..list.len() {
            let next = list[i] as f32;
            if next == value as f32 {
                // an exactly equal value is already present in the list
                return;
            } else if next as f64 > value {
                insert_index = i;
                break;
            }
        }
        list.insert(insert_index, value);
    }

    pub fn get_ports(&self) -> &[LPortId] {
        &self.ports
    }

    pub fn get_routing_slot(&self) -> i64 {
        self.routing_slot
    }

    pub fn set_routing_slot(&mut self, slot: i64) {
        self.routing_slot = slot;
    }

    pub fn get_start_coordinate(&self) -> f64 {
        self.start_position
    }

    pub fn get_end_coordinate(&self) -> f64 {
        self.end_position
    }

    pub fn get_incoming_connection_coordinates(&self) -> &[f64] {
        &self.incoming_connection_coordinates
    }

    pub fn set_incoming_connection_coordinates(&mut self, coords: Vec<f64>) {
        self.incoming_connection_coordinates = coords;
    }

    pub fn get_outgoing_connection_coordinates(&self) -> &[f64] {
        &self.outgoing_connection_coordinates
    }

    pub fn set_outgoing_connection_coordinates(&mut self, coords: Vec<f64>) {
        self.outgoing_connection_coordinates = coords;
    }

    pub fn get_outgoing_segment_dependencies(&self) -> &[DependencyId] {
        &self.outgoing_segment_deps
    }

    pub fn get_incoming_segment_dependencies(&self) -> &[DependencyId] {
        &self.incoming_segment_deps
    }

    pub fn get_out_weight(&self) -> i64 {
        self.out_dep_weight
    }

    pub fn set_out_weight(&mut self, out_weight: i64) {
        self.out_dep_weight = out_weight;
    }

    pub fn get_critical_out_weight(&self) -> i64 {
        self.critical_out_dep_weight
    }

    pub fn set_critical_out_weight(&mut self, out_weight: i64) {
        self.critical_out_dep_weight = out_weight;
    }

    pub fn get_in_weight(&self) -> i64 {
        self.in_dep_weight
    }

    pub fn set_in_weight(&mut self, in_weight: i64) {
        self.in_dep_weight = in_weight;
    }

    pub fn get_critical_in_weight(&self) -> i64 {
        self.critical_in_dep_weight
    }

    pub fn set_critical_in_weight(&mut self, in_weight: i64) {
        self.critical_in_dep_weight = in_weight;
    }

    pub fn get_split_partner(&self) -> Option<SegmentId> {
        self.split_partner
    }

    pub fn set_split_partner(&mut self, split_partner: Option<SegmentId>) {
        self.split_partner = split_partner;
    }

    pub fn get_split_by(&self) -> Option<SegmentId> {
        self.split_by
    }

    pub fn set_split_by(&mut self, split_by: Option<SegmentId>) {
        self.split_by = split_by;
    }

    /// `getLength()`.
    pub fn get_length(&self) -> f64 {
        self.get_end_coordinate() - self.get_start_coordinate()
    }

    /// `representsHyperedge()`.
    pub fn represents_hyperedge(&self) -> bool {
        self.incoming_connection_coordinates.len() + self.outgoing_connection_coordinates.len() > 2
    }

    /// `isDummy()`.
    pub fn is_dummy(&self) -> bool {
        self.split_partner.is_some() && self.split_by.is_none()
    }

    /// `recomputeExtent()`.
    pub fn recompute_extent(&mut self) {
        self.start_position = f64::NAN;
        self.end_position = f64::NAN;

        let incoming = std::mem::take(&mut self.incoming_connection_coordinates);
        self.recompute_extent_from_positions(&incoming);
        self.incoming_connection_coordinates = incoming;
        let outgoing = std::mem::take(&mut self.outgoing_connection_coordinates);
        self.recompute_extent_from_positions(&outgoing);
        self.outgoing_connection_coordinates = outgoing;
    }

    /// `recomputeExtentFromPositions(_:)`.
    pub fn recompute_extent_from_positions(&mut self, positions: &[f64]) {
        // this code assumes that the positions are sorted ascendingly
        let (Some(&first), Some(&last)) = (positions.first(), positions.last()) else { return };
        // set new start position
        if self.start_position.is_nan() {
            self.start_position = first;
        } else {
            self.start_position = swift::min(self.start_position, first);
        }

        // set new end position
        if self.end_position.is_nan() {
            self.end_position = last;
        } else {
            self.end_position = swift::max(self.end_position, last);
        }
    }

    /// `simulateSplit()`: the two segments a split would create. They are
    /// only rated (by coordinates) and never enter the arena; their mutual
    /// `splitPartner` links are not needed by any caller and are left unset.
    pub fn simulate_split(&self) -> (HyperEdgeSegment, HyperEdgeSegment) {
        let mut new_split = HyperEdgeSegment::new(self.routing_strategy);
        let mut new_split_partner = HyperEdgeSegment::new(self.routing_strategy);

        new_split.incoming_connection_coordinates.extend_from_slice(&self.incoming_connection_coordinates);
        new_split.split_by = self.split_by;
        new_split.recompute_extent();

        new_split_partner.outgoing_connection_coordinates.extend_from_slice(&self.outgoing_connection_coordinates);
        new_split_partner.recompute_extent();

        (new_split, new_split_partner)
    }
}

/// The arena of one routing run's segments and dependencies.
#[derive(Clone, Debug, Default)]
pub struct HyperEdgeSegmentGraph {
    pub segments: Vec<HyperEdgeSegment>,
    pub dependencies: Vec<HyperEdgeSegmentDependency>,
}

impl Index<SegmentId> for HyperEdgeSegmentGraph {
    type Output = HyperEdgeSegment;
    #[inline]
    fn index(&self, id: SegmentId) -> &HyperEdgeSegment {
        &self.segments[id.index()]
    }
}

impl IndexMut<SegmentId> for HyperEdgeSegmentGraph {
    #[inline]
    fn index_mut(&mut self, id: SegmentId) -> &mut HyperEdgeSegment {
        &mut self.segments[id.index()]
    }
}

impl Index<DependencyId> for HyperEdgeSegmentGraph {
    type Output = HyperEdgeSegmentDependency;
    #[inline]
    fn index(&self, id: DependencyId) -> &HyperEdgeSegmentDependency {
        &self.dependencies[id.index()]
    }
}

impl IndexMut<DependencyId> for HyperEdgeSegmentGraph {
    #[inline]
    fn index_mut(&mut self, id: DependencyId) -> &mut HyperEdgeSegmentDependency {
        &mut self.dependencies[id.index()]
    }
}

impl HyperEdgeSegmentGraph {
    pub fn new() -> HyperEdgeSegmentGraph {
        HyperEdgeSegmentGraph::default()
    }

    /// `HyperEdgeSegment(routingStrategy)`.
    pub fn new_segment(&mut self, routing_strategy: RoutingStrategyKind) -> SegmentId {
        let id = SegmentId(self.segments.len() as u32);
        self.segments.push(HyperEdgeSegment::new(routing_strategy));
        id
    }

    /// `HyperEdgeSegment()`: a segment with a plain `BaseRoutingDirectionStrategy`.
    pub fn new_default_segment(&mut self) -> SegmentId {
        self.new_segment(RoutingStrategyKind::Base)
    }

    /// `segment.addPortPositions(port, &hyperEdgeSegmentMap)`: adds the port
    /// and, recursively, every port connected to it that has no segment yet.
    pub fn add_port_positions(
        &mut self,
        lg: &LGraphArena,
        segment: SegmentId,
        port: LPortId,
        hyper_edge_segment_map: &mut std::collections::HashMap<LPortId, SegmentId>,
    ) {
        hyper_edge_segment_map.insert(port, segment);
        let seg = &mut self[segment];
        seg.ports.push(port);
        let port_pos = seg.routing_strategy.get_port_position_on_hyper_node(lg, port);

        // add the new port position to the respective list
        if lg[port].side == seg.routing_strategy.get_source_port_side() {
            HyperEdgeSegment::insert_sorted(&mut seg.incoming_connection_coordinates, port_pos);
        } else {
            HyperEdgeSegment::insert_sorted(&mut seg.outgoing_connection_coordinates, port_pos);
        }

        // update start and end coordinates
        seg.recompute_extent();

        // add connected ports (`port.getConnectedPorts()`: predecessors, then successors)
        let p = &lg[port];
        for &e in p.incoming_edges.iter() {
            if let Some(other_port) = lg[e].source {
                if !hyper_edge_segment_map.contains_key(&other_port) {
                    self.add_port_positions(lg, segment, other_port, hyper_edge_segment_map);
                }
            }
        }
        for &e in p.outgoing_edges.iter() {
            if let Some(other_port) = lg[e].target {
                if !hyper_edge_segment_map.contains_key(&other_port) {
                    self.add_port_positions(lg, segment, other_port, hyper_edge_segment_map);
                }
            }
        }
    }

    /// `segment.splitAt(_:)`: splits the segment and returns the new partner.
    pub fn split_at(&mut self, segment: SegmentId, split_position: f64) -> SegmentId {
        let partner = self.new_segment(self[segment].routing_strategy);
        self[segment].split_partner = Some(partner);
        self[partner].set_split_partner(Some(segment));

        // Move all target positions over to the new segment
        let outgoing = std::mem::take(&mut self[segment].outgoing_connection_coordinates);
        self[partner].outgoing_connection_coordinates.extend_from_slice(&outgoing);

        // Link the two
        self[segment].outgoing_connection_coordinates.push(split_position);
        self[partner].incoming_connection_coordinates.push(split_position);

        // Recompute their outer coordinates
        self[segment].recompute_extent();
        self[partner].recompute_extent();

        // Clear dependencies so they can be regenerated later
        while let Some(&dep) = self[segment].incoming_segment_deps.first() {
            self.dependency_remove(dep);
        }

        while let Some(&dep) = self[segment].outgoing_segment_deps.first() {
            self.dependency_remove(dep);
        }

        partner
    }

    /// `removeOutgoingSegmentDependency(_:)`.
    pub fn remove_outgoing_segment_dependency(&mut self, segment: SegmentId, dep: DependencyId) {
        self[segment].outgoing_segment_deps.retain(|&d| d != dep);
    }

    /// `removeIncomingSegmentDependency(_:)`.
    pub fn remove_incoming_segment_dependency(&mut self, segment: SegmentId, dep: DependencyId) {
        self[segment].incoming_segment_deps.retain(|&d| d != dep);
    }
}
