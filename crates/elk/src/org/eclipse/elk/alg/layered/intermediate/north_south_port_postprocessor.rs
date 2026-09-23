//! Port of `alg/layered/intermediate/NorthSouthPortPostprocessor.swift`.
//!
//! Removes the dummy nodes created by `NorthSouthPortPreprocessor` and
//! reconnects their edges to the original ports, adding the bend points (and,
//! for dummies of a single port, junction points) that route them there.

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::k_vector_chain::{kvector_chain_ref, KVectorChainRef};
use crate::org::eclipse::elk::core::options::edge_routing::EdgeRouting;
use crate::prelude::*;

#[derive(Default)]
pub struct NorthSouthPortPostprocessor;

/// `value as AnyObject?` identity of a property value, for `!==`: the object
/// a value refers to, or `None` for a missing value. Values that are not
/// references to graph objects have no stable identity (Swift boxes them
/// afresh), so they never compare identical.
fn object_identity(v: Option<PropValue>) -> Result<Option<(u8, u32)>, ()> {
    match v {
        None => Ok(None),
        Some(PropValue::LPort(p)) => Ok(Some((0, p.0))),
        Some(PropValue::LNode(n)) => Ok(Some((1, n.0))),
        Some(PropValue::LEdge(e)) => Ok(Some((2, e.0))),
        Some(PropValue::LLabel(l)) => Ok(Some((3, l.0))),
        Some(PropValue::LGraph(g)) => Ok(Some((4, g.0))),
        Some(PropValue::Layer(l)) => Ok(Some((5, l.0))),
        Some(_) => Err(()),
    }
}

impl NorthSouthPortPostprocessor {
    pub fn new() -> NorthSouthPortPostprocessor {
        NorthSouthPortPostprocessor
    }

    /// Adds `(x, y)` to the edge's `JUNCTION_POINTS` chain, creating it if needed.
    fn add_junction_point(lg: &mut LGraphArena, edge: LEdgeId, x: f64, y: f64) {
        let jp: KVectorChainRef = match lg[edge].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS) {
            Some(existing) => existing,
            None => {
                let jp = kvector_chain_ref(KVectorChain::new());
                lg[edge].props.set(&LayeredOptions::JUNCTION_POINTS, jp.clone());
                jp
            }
        };
        jp.borrow_mut().add(KVector::new(x, y));
    }

    /// `processInputPort(_:_:)`.
    fn process_input_port(lg: &mut LGraphArena, input_port: LPortId, add_junction_points: bool) {
        let Some(origin_port) = lg[input_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) else { return };
        let Some(node) = lg[input_port].owner else { return };

        let x = lg.port_absolute_anchor(origin_port).x;
        let y = lg[node].position.y;

        for in_edge in lg[input_port].incoming_edges.clone() {
            lg.edge_set_target(in_edge, Some(origin_port));
            lg[in_edge].bend_points.add_last_xy(x, y);

            if add_junction_points {
                Self::add_junction_point(lg, in_edge, x, y);
            }
        }
    }

    /// `processOutputPort(_:_:)`.
    fn process_output_port(lg: &mut LGraphArena, output_port: LPortId, add_junction_points: bool) {
        let Some(origin_port) = lg[output_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) else { return };
        let Some(node) = lg[output_port].owner else { return };

        let x = lg.port_absolute_anchor(origin_port).x;
        let y = lg[node].position.y;

        for out_edge in lg[output_port].outgoing_edges.clone() {
            lg.edge_set_source(out_edge, Some(origin_port));
            lg[out_edge].bend_points.add_first_xy(x, y);

            if add_junction_points {
                Self::add_junction_point(lg, out_edge, x, y);
            }
        }
    }

    /// `processSelfLoop(_:)`.
    fn process_self_loop(lg: &mut LGraphArena, dummy: LNodeId) {
        let Some(self_loop) = lg[dummy].props.get_as::<LEdgeId>(&InternalProperties::ORIGIN) else { return };
        let Some(input_port) = lg[dummy].ports.iter().copied().find(|&p| lg[p].side == PortSide::WEST) else { return };
        let Some(output_port) = lg[dummy].ports.iter().copied().find(|&p| lg[p].side == PortSide::EAST) else { return };
        let Some(origin_input_port) = lg[input_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) else { return };
        let Some(origin_output_port) = lg[output_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) else { return };

        lg.edge_set_source(self_loop, Some(origin_output_port));
        lg.edge_set_target(self_loop, Some(origin_input_port));

        let Some(output_node) = lg[output_port].owner else { return };
        let Some(input_node) = lg[input_port].owner else { return };

        let mut bend_point1 = lg[output_node].position;
        bend_point1.x = lg.port_absolute_anchor(origin_output_port).x;
        lg[self_loop].bend_points.add(bend_point1);

        let mut bend_point2 = lg[input_node].position;
        bend_point2.x = lg.port_absolute_anchor(origin_input_port).x;
        lg[self_loop].bend_points.add(bend_point2);
    }

    /// `processSplineInputPort(_:)`.
    fn process_spline_input_port(lg: &mut LGraphArena, input_port: LPortId) {
        let Some(origin_port) = lg[input_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) else { return };
        let Some(input_node) = lg[input_port].owner else { return };
        let y = lg[input_node].position.y;
        lg[origin_port].props.set(&InternalProperties::SPLINE_NS_PORT_Y_COORD, y);

        for in_edge in lg[input_port].incoming_edges.clone() {
            lg.edge_set_target(in_edge, Some(origin_port));
        }
    }

    /// `processSplineOutputPort(_:)`.
    fn process_spline_output_port(lg: &mut LGraphArena, output_port: LPortId) {
        let Some(origin_port) = lg[output_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) else { return };
        let Some(output_node) = lg[output_port].owner else { return };
        let y = lg[output_node].position.y;
        lg[origin_port].props.set(&InternalProperties::SPLINE_NS_PORT_Y_COORD, y);

        for out_edge in lg[output_port].outgoing_edges.clone() {
            lg.edge_set_source(out_edge, Some(origin_port));
        }
    }
}

impl ILayoutProcessor for NorthSouthPortPostprocessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Odd port side processing", 1.0);

        let routing = lg[layered_graph].props.get_as::<EdgeRouting>(&LayeredOptions::EDGE_ROUTING);

        for layer in lg[layered_graph].layers.clone() {
            let node_array = lg[layer].nodes.clone();
            for node in node_array {
                if lg[node].node_type != NodeType::NORTH_SOUTH_PORT {
                    continue;
                }

                if routing == Some(EdgeRouting::SPLINES) {
                    for port in lg[node].ports.clone() {
                        if !lg[port].incoming_edges.is_empty() {
                            Self::process_spline_input_port(lg, port);
                        }
                        if !lg[port].outgoing_edges.is_empty() {
                            Self::process_spline_output_port(lg, port);
                        }
                    }
                } else if matches!(lg[node].props.get(&InternalProperties::ORIGIN), Some(PropValue::LEdge(_))) {
                    Self::process_self_loop(lg, node);
                } else {
                    // Check if all ports were created for the same origin port
                    let same_origin_port = if lg[node].ports.len() >= 2 {
                        let ports = &lg[node].ports;
                        let mut all_same = true;
                        for i in 1..ports.len() {
                            let prev = object_identity(lg[ports[i - 1]].props.get(&InternalProperties::ORIGIN));
                            let curr = object_identity(lg[ports[i]].props.get(&InternalProperties::ORIGIN));
                            let identical = matches!((prev, curr), (Ok(a), Ok(b)) if a == b);
                            if !identical {
                                all_same = false;
                                break;
                            }
                        }
                        all_same
                    } else {
                        false
                    };

                    for port in lg[node].ports.clone() {
                        if !lg[port].incoming_edges.is_empty() {
                            Self::process_input_port(lg, port, same_origin_port);
                        }
                        if !lg[port].outgoing_edges.is_empty() {
                            Self::process_output_port(lg, port, same_origin_port);
                        }
                    }
                }

                // Remove the node
                lg.node_set_layer(node, None);
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "NorthSouthPortPostprocessor"
    }
}
