//! Port of `alg/layered/p5edges/orthogonal/direction/WestToEastRoutingStrategy.swift`:
//! the overrides of `BaseRoutingDirectionStrategy` for left-to-right routing.

use super::base_routing_direction_strategy::BaseRoutingDirectionStrategy;
use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal::hyper_edge_segment::{HyperEdgeSegmentGraph, SegmentId};
use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal::orthogonal_routing_generator::TOLERANCE;
use crate::prelude::*;

/// Marker for the Swift class; its methods are the associated functions.
#[derive(Clone, Copy, Debug, Default)]
pub struct WestToEastRoutingStrategy;

impl WestToEastRoutingStrategy {
    /// `getPortPositionOnHyperNode(_:)`.
    pub fn get_port_position_on_hyper_node(lg: &LGraphArena, port: LPortId) -> f64 {
        let Some(node) = lg[port].owner else { return 0.0 };
        lg[node].position.y + lg[port].position.y + lg[port].anchor.y
    }

    /// `getSourcePortSide()`.
    pub fn get_source_port_side() -> PortSide {
        PortSide::EAST
    }

    /// `getTargetPortSide()`.
    pub fn get_target_port_side() -> PortSide {
        PortSide::WEST
    }

    /// `calculateBendPoints(_:_:_:)`.
    pub fn calculate_bend_points(
        strategy: &mut BaseRoutingDirectionStrategy,
        lg: &mut LGraphArena,
        segments: &HyperEdgeSegmentGraph,
        segment: SegmentId,
        start_pos: f64,
        edge_spacing: f64,
    ) {
        // We don't do anything with dummy segments; they are dealt with when their partner is processed
        if segments[segment].is_dummy() {
            return;
        }

        // Calculate coordinates for each port's bend points
        let segment_x = start_pos + segments[segment].get_routing_slot() as f64 * edge_spacing;

        for &port in segments[segment].get_ports() {
            let source_y = lg.port_absolute_anchor(port).y;

            for edge in lg[port].outgoing_edges.clone() {
                if lg.edge_is_self_loop(edge) {
                    continue;
                }
                let Some(target) = lg[edge].target else { continue };
                let target_y = lg.port_absolute_anchor(target).y;

                if (source_y - target_y).abs() > TOLERANCE {
                    // We'll update these if we find that the segment was split
                    let mut current_x = segment_x;
                    let mut current_segment = segment;

                    let mut bend = KVector::new(current_x, source_y);
                    lg[edge].bend_points.add(bend);
                    strategy.add_junction_point_if_necessary(lg, edge, segments, current_segment, bend, true);

                    // If this segment was split, we need two additional bend points
                    if let Some(split_partner) = segments[segment].get_split_partner() {
                        let split_y = segments[split_partner].get_incoming_connection_coordinates()[0];

                        bend = KVector::new(current_x, split_y);
                        lg[edge].bend_points.add(bend);
                        strategy.add_junction_point_if_necessary(lg, edge, segments, current_segment, bend, true);

                        // Advance to the split partner's routing slot
                        current_x = start_pos + segments[split_partner].get_routing_slot() as f64 * edge_spacing;
                        current_segment = split_partner;

                        bend = KVector::new(current_x, split_y);
                        lg[edge].bend_points.add(bend);
                        strategy.add_junction_point_if_necessary(lg, edge, segments, current_segment, bend, true);
                    }

                    bend = KVector::new(current_x, target_y);
                    lg[edge].bend_points.add(bend);
                    strategy.add_junction_point_if_necessary(lg, edge, segments, current_segment, bend, true);
                }
            }
        }
    }
}
