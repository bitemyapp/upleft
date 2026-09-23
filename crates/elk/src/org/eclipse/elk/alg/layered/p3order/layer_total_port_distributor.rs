//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_LayerTotalPortDistributor.swift`.
//!
//! Port ranks over the whole layer: every port consumes one unit of rank.

use super::abstract_barycenter_port_distributor::{AbstractBarycenterPortDistributor, BarycenterPortDistributorKind};
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
use crate::org::eclipse::elk::core::options::port_side::PortSide;

pub struct LayerTotalPortDistributor;

impl LayerTotalPortDistributor {
    /// `LayerTotalPortDistributor(_ numLayers:)`.
    pub fn new(num_layers: i64) -> AbstractBarycenterPortDistributor {
        AbstractBarycenterPortDistributor::new(BarycenterPortDistributorKind::LayerTotal, num_layers)
    }

    /// `LayerTotalPortDistributor()`.
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

                let mut north_pos = rank_sum + north_input_count as f32;
                let mut rest_pos = rank_sum + input_count as f32;
                for &port in &lg[node].ports {
                    if lg[port].incoming_edges.is_empty() {
                        continue;
                    }
                    if lg[port].side == PortSide::NORTH {
                        pd.set_port_rank(lg[port].id as i64, north_pos);
                        north_pos -= 1.0;
                    } else {
                        pd.set_port_rank(lg[port].id as i64, rest_pos);
                        rest_pos -= 1.0;
                    }
                }
                input_count as f32
            }
            PortType::OUTPUT => {
                let mut pos = 0i64;
                for &port in &lg[node].ports {
                    if lg[port].outgoing_edges.is_empty() {
                        continue;
                    }
                    pos += 1;
                    let rank = rank_sum + pos as f32;
                    pd.set_port_rank(lg[port].id as i64, rank);
                }
                pos as f32
            }
            PortType::UNDEFINED => 0.0,
        }
    }
}
