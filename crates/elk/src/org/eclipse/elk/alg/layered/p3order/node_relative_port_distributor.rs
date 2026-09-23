//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_NodeRelativePortDistributor.swift`.
//!
//! Port ranks relative to their node: every node consumes one unit of rank.

use super::abstract_barycenter_port_distributor::{AbstractBarycenterPortDistributor, BarycenterPortDistributorKind};
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
use crate::org::eclipse::elk::core::options::port_side::PortSide;

pub struct NodeRelativePortDistributor;

impl NodeRelativePortDistributor {
    /// `NodeRelativePortDistributor(_ numLayers:)`.
    pub fn new(num_layers: i64) -> AbstractBarycenterPortDistributor {
        AbstractBarycenterPortDistributor::new(BarycenterPortDistributorKind::NodeRelative, num_layers)
    }

    /// `NodeRelativePortDistributor()`.
    pub fn new_default() -> AbstractBarycenterPortDistributor {
        Self::new(0)
    }

    /// The `calculatePortRanks(_:_:_:)` override.
    pub fn calculate_port_ranks(pd: &mut AbstractBarycenterPortDistributor, lg: &LGraphArena, node: LNodeId, rank_sum: f32, port_type: PortType) -> f32 {
        match port_type {
            PortType::INPUT => {
                let mut input_count = 0i64;
                let mut north_input_count = 0i64;
                for &port in &lg[node].ports {
                    if !lg[port].incoming_edges.is_empty() {
                        input_count += 1;
                        if lg[port].side == PortSide::NORTH {
                            north_input_count += 1;
                        }
                    }
                }

                let incr = 1.0f32 / (input_count + 1) as f32;
                let mut north_pos = rank_sum + north_input_count as f32 * incr;
                let mut rest_pos = rank_sum + 1.0 - incr;
                for &port in &lg[node].ports {
                    if lg[port].incoming_edges.is_empty() {
                        continue;
                    }
                    if lg[port].side == PortSide::NORTH {
                        pd.set_port_rank(lg[port].id as i64, north_pos);
                        north_pos -= incr;
                    } else {
                        pd.set_port_rank(lg[port].id as i64, rest_pos);
                        rest_pos -= incr;
                    }
                }
            }
            PortType::OUTPUT => {
                let mut output_count = 0i64;
                for &port in &lg[node].ports {
                    if !lg[port].outgoing_edges.is_empty() {
                        output_count += 1;
                    }
                }

                let incr = 1.0f32 / (output_count + 1) as f32;
                let mut pos = rank_sum + incr;
                for &port in &lg[node].ports {
                    if lg[port].outgoing_edges.is_empty() {
                        continue;
                    }
                    pd.set_port_rank(lg[port].id as i64, pos);
                    pos += incr;
                }
            }
            PortType::UNDEFINED => {}
        }

        // Java: consumed rank is always 1 for node-relative strategy.
        1.0
    }
}
