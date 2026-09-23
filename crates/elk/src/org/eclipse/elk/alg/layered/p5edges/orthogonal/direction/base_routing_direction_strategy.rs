//! Port of `alg/layered/p5edges/orthogonal/direction/BaseRoutingDirectionStrategy.swift`.
//!
//! The Swift class hierarchy (`BaseRoutingDirectionStrategy` with the
//! `WestToEast`, `NorthToSouth` and `SouthToNorth` subclasses) becomes one
//! struct whose [`RoutingStrategyKind`] selects the overrides. Only
//! `WestToEastRoutingStrategy` overrides anything in elk-swift; the other two
//! subclasses are empty and behave like the base class, whose "abstract"
//! methods are `assertionFailure` (a no-op in release builds) plus a dummy
//! return value.

use std::collections::HashSet;

use super::routing_direction::RoutingDirection;
use super::west_to_east_routing_strategy::WestToEastRoutingStrategy;
use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal::hyper_edge_segment::{HyperEdgeSegmentGraph, SegmentId};
use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal::orthogonal_routing_generator::TOLERANCE;
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChainRef;
use crate::prelude::*;

/// Which Swift class a strategy (or a segment's strategy reference) is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RoutingStrategyKind {
    /// `BaseRoutingDirectionStrategy()` itself (used by `HyperEdgeSegment()`).
    Base,
    WestToEast,
    NorthToSouth,
    SouthToNorth,
}

impl RoutingStrategyKind {
    /// `getPortPositionOnHyperNode(_:)`.
    pub fn get_port_position_on_hyper_node(self, lg: &LGraphArena, port: LPortId) -> f64 {
        match self {
            RoutingStrategyKind::WestToEast => WestToEastRoutingStrategy::get_port_position_on_hyper_node(lg, port),
            // assertionFailure("Subclass must override getPortPositionOnHyperNode")
            _ => 0.0,
        }
    }

    /// `getSourcePortSide()`.
    pub fn get_source_port_side(self) -> PortSide {
        match self {
            RoutingStrategyKind::WestToEast => WestToEastRoutingStrategy::get_source_port_side(),
            _ => PortSide::UNDEFINED,
        }
    }

    /// `getTargetPortSide()`.
    pub fn get_target_port_side(self) -> PortSide {
        match self {
            RoutingStrategyKind::WestToEast => WestToEastRoutingStrategy::get_target_port_side(),
            _ => PortSide::UNDEFINED,
        }
    }
}

/// `Set<KVector>` of junction points (value equality: `x == x' && y == y'`).
/// Keys are the bit patterns with `-0.0` folded into `0.0` (Swift hashes
/// both zeros alike); vectors with a NaN coordinate never compare equal to
/// anything, so they are never found.
#[derive(Clone, Debug, Default)]
pub struct JunctionPointSet {
    keys: HashSet<(u64, u64)>,
}

impl JunctionPointSet {
    fn key(v: &KVector) -> Option<(u64, u64)> {
        if v.x.is_nan() || v.y.is_nan() {
            return None;
        }
        let norm = |d: f64| if d == 0.0 { 0u64 } else { d.to_bits() };
        Some((norm(v.x), norm(v.y)))
    }

    pub fn contains(&self, v: &KVector) -> bool {
        Self::key(v).is_some_and(|k| self.keys.contains(&k))
    }

    pub fn insert(&mut self, v: &KVector) {
        if let Some(k) = Self::key(v) {
            self.keys.insert(k);
        }
    }

    pub fn clear(&mut self) {
        self.keys.clear();
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

pub struct BaseRoutingDirectionStrategy {
    pub kind: RoutingStrategyKind,
    /// set of already created junction points, to avoid multiple points at the same position.
    pub created_junction_points: JunctionPointSet,
}

impl Default for BaseRoutingDirectionStrategy {
    fn default() -> Self {
        BaseRoutingDirectionStrategy::new()
    }
}

impl BaseRoutingDirectionStrategy {
    /// `BaseRoutingDirectionStrategy()`.
    pub fn new() -> BaseRoutingDirectionStrategy {
        BaseRoutingDirectionStrategy::with_kind(RoutingStrategyKind::Base)
    }

    pub fn with_kind(kind: RoutingStrategyKind) -> BaseRoutingDirectionStrategy {
        BaseRoutingDirectionStrategy { kind, created_junction_points: JunctionPointSet::default() }
    }

    /// `forRoutingDirection(_:)`.
    pub fn for_routing_direction(direction: RoutingDirection) -> BaseRoutingDirectionStrategy {
        match direction {
            RoutingDirection::WEST_TO_EAST => BaseRoutingDirectionStrategy::with_kind(RoutingStrategyKind::WestToEast),
            RoutingDirection::NORTH_TO_SOUTH => BaseRoutingDirectionStrategy::with_kind(RoutingStrategyKind::NorthToSouth),
            RoutingDirection::SOUTH_TO_NORTH => BaseRoutingDirectionStrategy::with_kind(RoutingStrategyKind::SouthToNorth),
        }
    }

    /// `addJunctionPointIfNecessary(_:_:_:_:)`.
    pub fn add_junction_point_if_necessary(
        &mut self,
        lg: &mut LGraphArena,
        edge: LEdgeId,
        segments: &HyperEdgeSegmentGraph,
        segment: SegmentId,
        pos: KVector,
        vertical: bool,
    ) {
        let p = if vertical { pos.y } else { pos.x };

        // If we already have this junction point, don't bother
        if self.created_junction_points.contains(&pos) {
            return;
        }

        let seg = &segments[segment];

        // Whether the point lies somewhere inside the edge segment (without boundaries)
        let point_inside_edge_segment = p > seg.get_start_coordinate() && p < seg.get_end_coordinate();

        // Check if the point lies somewhere at the segment's boundary
        let mut point_at_segment_boundary = false;
        let in_coords = seg.get_incoming_connection_coordinates();
        let out_coords = seg.get_outgoing_connection_coordinates();
        if let (Some(&in_first), Some(&out_first), Some(&in_last), Some(&out_last)) = (in_coords.first(), out_coords.first(), in_coords.last(), out_coords.last()) {
            // Is the bend point at the start and joins another edge at the same position?
            point_at_segment_boundary = point_at_segment_boundary || ((p - in_first).abs() < TOLERANCE && (p - out_first).abs() < TOLERANCE);

            // Is the bend point at the end and joins another edge at the same position?
            point_at_segment_boundary = point_at_segment_boundary || ((p - in_last).abs() < TOLERANCE && (p - out_last).abs() < TOLERANCE);
        }

        if point_inside_edge_segment || point_at_segment_boundary {
            // create a new junction point for the edge at the bend point's position.
            // `JUNCTION_POINTS_KEY` has no default, so the typed read yields
            // the stored chain (a shared reference) or nothing.
            let junction_points: KVectorChainRef = match lg[edge].props.get_typed::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS) {
                Some(existing) => existing,
                None => {
                    let jp = crate::org::eclipse::elk::core::math::k_vector_chain::kvector_chain_ref(KVectorChain::new());
                    lg[edge].props.set(&LayeredOptions::JUNCTION_POINTS, jp.clone());
                    jp
                }
            };

            let jpoint = pos;
            junction_points.borrow_mut().add(jpoint);
            self.created_junction_points.insert(&jpoint);
        }
    }

    /// `clearCreatedJunctionPoints()`.
    pub fn clear_created_junction_points(&mut self) {
        self.created_junction_points.clear();
    }

    /// `getCreatedJunctionPoints()`.
    pub fn get_created_junction_points(&self) -> &JunctionPointSet {
        &self.created_junction_points
    }

    /// `getPortPositionOnHyperNode(_:)`.
    pub fn get_port_position_on_hyper_node(&self, lg: &LGraphArena, port: LPortId) -> f64 {
        self.kind.get_port_position_on_hyper_node(lg, port)
    }

    /// `getSourcePortSide()`.
    pub fn get_source_port_side(&self) -> PortSide {
        self.kind.get_source_port_side()
    }

    /// `getTargetPortSide()`.
    pub fn get_target_port_side(&self) -> PortSide {
        self.kind.get_target_port_side()
    }

    /// `calculateBendPoints(_:_:_:)`.
    pub fn calculate_bend_points(&mut self, lg: &mut LGraphArena, segments: &HyperEdgeSegmentGraph, hyper_node: SegmentId, start_pos: f64, edge_spacing: f64) {
        match self.kind {
            RoutingStrategyKind::WestToEast => WestToEastRoutingStrategy::calculate_bend_points(self, lg, segments, hyper_node, start_pos, edge_spacing),
            // assertionFailure("Subclass must override calculateBendPoints")
            _ => {}
        }
    }
}
