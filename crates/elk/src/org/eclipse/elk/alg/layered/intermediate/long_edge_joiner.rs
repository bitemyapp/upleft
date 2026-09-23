//! Port of `alg/layered/intermediate/LongEdgeJoiner.swift`.
//!
//! Removes long-edge dummy nodes, joining the edges they split back together
//! (bend points, labels and junction points are merged into the surviving
//! edge).

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::k_vector_chain::{kvector_chain_ref, KVectorChainRef};
use crate::prelude::*;

#[derive(Default)]
pub struct LongEdgeJoiner;

impl LongEdgeJoiner {
    pub fn new() -> LongEdgeJoiner {
        LongEdgeJoiner
    }

    /// `joinAt(_:_:)`.
    ///
    /// The Swift reads `inputPort.incomingEdges` and `outputPort.outgoingEdges`
    /// into local arrays (value copies) and then always takes their *first*
    /// element; with more than one edge through the dummy, the later
    /// iterations see the already detached dropped edge and skip. Ported as is.
    pub fn join_at(lg: &mut LGraphArena, long_edge_dummy: LNodeId, add_unnecessary_bendpoints: bool) {
        let input_port = lg[long_edge_dummy].ports.iter().copied().find(|&p| lg[p].side == PortSide::WEST);
        let output_port = lg[long_edge_dummy].ports.iter().copied().find(|&p| lg[p].side == PortSide::EAST);
        let (Some(input_port), Some(output_port)) = (input_port, output_port) else {
            // Defensive: skip dummy nodes with unexpected port sides
            return;
        };
        let input_port_edges = lg[input_port].incoming_edges.clone();
        let output_port_edges = lg[output_port].outgoing_edges.clone();
        let mut edge_count = input_port_edges.len();

        let Some(&first_port) = lg[long_edge_dummy].ports.first() else { return };
        let unnecessary_bendpoint = lg.port_absolute_anchor(first_port);

        while edge_count > 0 {
            edge_count -= 1;

            let (Some(&surviving_edge), Some(&dropped_edge)) = (input_port_edges.first(), output_port_edges.first()) else { break };

            let Some(target_port) = lg[dropped_edge].target else { continue };
            let dropped_edge_list_index = lg[target_port].incoming_edges.iter().position(|&e| e == dropped_edge).unwrap_or(0);
            lg.edge_set_target_and_insert_at_index(surviving_edge, Some(target_port), dropped_edge_list_index);

            lg.edge_set_source(dropped_edge, None);
            lg.edge_set_target(dropped_edge, None);

            if add_unnecessary_bendpoints {
                lg[surviving_edge].bend_points.add(unnecessary_bendpoint);
            }

            let dropped_bend_points = lg[dropped_edge].bend_points.clone();
            for bend_point in dropped_bend_points.iter() {
                lg[surviving_edge].bend_points.add(*bend_point);
            }

            let dropped_labels = lg[dropped_edge].labels.clone();
            lg[surviving_edge].labels.extend(dropped_labels);

            let surviving_junction_points = lg[surviving_edge].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS);
            let dropped_junction_points = lg[dropped_edge].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS);
            if let Some(dropped_jp) = dropped_junction_points {
                // The chains are shared references: appending to an existing
                // chain changes it wherever else it is stored.
                let sjp = match surviving_junction_points {
                    Some(existing) => existing,
                    None => {
                        let sjp = kvector_chain_ref(KVectorChain::new());
                        lg[surviving_edge].props.set(&LayeredOptions::JUNCTION_POINTS, sjp.clone());
                        sjp
                    }
                };
                // Snapshot first: `sjp` and `droppedJP` may be the same chain.
                let points = dropped_jp.borrow().to_array();
                for jp in points {
                    sjp.borrow_mut().add(jp);
                }
            }
        }
    }
}

impl ILayoutProcessor for LongEdgeJoiner {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Edge joining", 1.0);

        let add_unnecessary_bendpoints = lg[layered_graph].props.get_as::<bool>(&LayeredOptions::UNNECESSARY_BENDPOINTS).unwrap_or(false);

        for li in 0..lg[layered_graph].layers.len() {
            let layer = lg[layered_graph].layers[li];
            let mut i = 0;
            while i < lg[layer].nodes.len() {
                let node = lg[layer].nodes[i];
                if lg[node].node_type == NodeType::LONG_EDGE {
                    Self::join_at(lg, node, add_unnecessary_bendpoints);
                    // `layer.nodes.remove(at:)`: the node keeps its `layer`.
                    lg[layer].nodes.remove(i);
                } else {
                    i += 1;
                }
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LongEdgeJoiner"
    }
}
