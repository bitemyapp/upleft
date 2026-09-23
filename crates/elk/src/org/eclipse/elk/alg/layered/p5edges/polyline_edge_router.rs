//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p5edges/org_eclipse_elk_alg_layered_p5edges_PolylineEdgeRouter.swift`.
//!
//! Edge router module that draws edges with non-orthogonal line segments.
//! Sets horizontal coordinates for nodes and routes edges with bend points.

use crate::org::eclipse::elk::alg::layered::intermediate::intermediate_processor_strategy::IntermediateProcessorStrategy;
use crate::org::eclipse::elk::alg::layered::layered_phases::LayeredPhases;
use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal::direction::base_routing_direction_strategy::JunctionPointSet;
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::alg::layout_processor_configuration::LayoutProcessorConfiguration;
use crate::org::eclipse::elk::core::math::k_vector_chain::{kvector_chain_ref, KVectorChainRef};
use crate::prelude::*;

const MIN_VERT_DIFF: f64 = 1.0;
const LAYER_SPACE_FAC: f64 = 0.4;

#[derive(Default)]
pub struct PolylineEdgeRouter {
    /// `createdJunctionPoints: Set<KVector>` (value equality); only
    /// membership is ever asked, so its hash order does not matter.
    created_junction_points: JunctionPointSet,
}

impl PolylineEdgeRouter {
    pub fn new() -> PolylineEdgeRouter {
        PolylineEdgeRouter::default()
    }

    /// Predicate: is the node an external port dummy on west or east side?
    fn is_external_west_or_east_port(lg: &LGraphArena, node: LNodeId) -> bool {
        let ext_port_side: Option<PortSide> = lg[node].props.get_typed(&InternalProperties::EXT_PORT_SIDE);
        lg[node].node_type == NodeType::EXTERNAL_PORT && (ext_port_side == Some(PortSide::WEST) || ext_port_side == Some(PortSide::EAST))
    }

    // MARK: - Processor configurations

    fn baseline_processor_configuration() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_before(LayeredPhases::P3_NODE_ORDERING, IntermediateProcessorStrategy::INVERTED_PORT_PROCESSOR);
        c
    }

    fn north_south_port_processing_additions() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_before(LayeredPhases::P3_NODE_ORDERING, IntermediateProcessorStrategy::NORTH_SOUTH_PORT_PREPROCESSOR)
            .add_after(LayeredPhases::P5_EDGE_ROUTING, IntermediateProcessorStrategy::NORTH_SOUTH_PORT_POSTPROCESSOR);
        c
    }

    fn self_loop_processing_additions() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_before(LayeredPhases::P1_CYCLE_BREAKING, IntermediateProcessorStrategy::SELF_LOOP_PREPROCESSOR)
            .add_after(LayeredPhases::P5_EDGE_ROUTING, IntermediateProcessorStrategy::SELF_LOOP_POSTPROCESSOR)
            .add_before(LayeredPhases::P4_NODE_PLACEMENT, IntermediateProcessorStrategy::SELF_LOOP_PORT_RESTORER)
            .add_before(LayeredPhases::P4_NODE_PLACEMENT, IntermediateProcessorStrategy::SELF_LOOP_ROUTER);
        c
    }

    fn center_edge_label_processing_additions() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_before(LayeredPhases::P2_LAYERING, IntermediateProcessorStrategy::LABEL_DUMMY_INSERTER)
            .add_before(LayeredPhases::P4_NODE_PLACEMENT, IntermediateProcessorStrategy::LABEL_DUMMY_SWITCHER)
            .add_before(LayeredPhases::P4_NODE_PLACEMENT, IntermediateProcessorStrategy::LABEL_SIDE_SELECTOR)
            .add_after(LayeredPhases::P5_EDGE_ROUTING, IntermediateProcessorStrategy::LABEL_DUMMY_REMOVER);
        c
    }

    fn end_edge_label_processing_additions() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_before(LayeredPhases::P4_NODE_PLACEMENT, IntermediateProcessorStrategy::LABEL_SIDE_SELECTOR)
            .add_before(LayeredPhases::P4_NODE_PLACEMENT, IntermediateProcessorStrategy::END_LABEL_PREPROCESSOR)
            .add_after(LayeredPhases::P5_EDGE_ROUTING, IntermediateProcessorStrategy::END_LABEL_POSTPROCESSOR);
        c
    }

    // MARK: - Edge Routing

    /// `processNode(_:_:_:)`.
    fn process_node(&mut self, lg: &mut LGraphArena, node: LNodeId, layer_left_x_pos: f64, max_acceptable_x_diff: f64) {
        let layer_right_x_pos = layer_left_x_pos + lg[node].layer.map_or(0.0, |l| lg[l].size.x);

        for port in lg[node].ports.clone() {
            let mut absolute_port_anchor = lg.port_absolute_anchor(port);

            if lg[node].node_type == NodeType::NORTH_SOUTH_PORT {
                if let Some(corresponding_port) = lg[port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) {
                    absolute_port_anchor.x = lg.port_absolute_anchor(corresponding_port).x;
                    lg[node].position.x = absolute_port_anchor.x;
                }
            }

            let mut bend_point = KVector::new(0.0, absolute_port_anchor.y);

            if lg[port].side == PortSide::EAST {
                bend_point.x = layer_right_x_pos;
            } else if lg[port].side == PortSide::WEST {
                bend_point.x = layer_left_x_pos;
            } else {
                continue;
            }

            let x_distance = (absolute_port_anchor.x - bend_point.x).abs();
            if x_distance <= max_acceptable_x_diff && !Self::is_in_layer_dummy(lg, node) {
                continue;
            }

            let add_junction_point = lg[port].outgoing_edges.len() + lg[port].incoming_edges.len() > 1;

            for e in lg.port_connected_edges(port) {
                let other_port = if lg[e].source == Some(port) { lg[e].target } else { lg[e].source };
                let Some(other_port) = other_port else { continue };
                if (lg.port_absolute_anchor(other_port).y - bend_point.y).abs() > MIN_VERT_DIFF {
                    self.add_bend_point(lg, e, bend_point, add_junction_point, port);
                }
            }
        }
    }

    /// `processInLayerEdge(_:_:_:)`.
    fn process_in_layer_edge(lg: &mut LGraphArena, edge: LEdgeId, layer_x_pos: f64, edge_spacing: f64) {
        let (Some(source_port), Some(target_port)) = (lg[edge].source, lg[edge].target) else { return };

        let source_anchor_y = lg.port_absolute_anchor(source_port).y;
        let mid_y = (source_anchor_y + lg.port_absolute_anchor(target_port).y) / 2.0;

        if lg[source_port].side == PortSide::EAST {
            let layer_width = lg[source_port].owner.and_then(|n| lg[n].layer).map_or(0.0, |l| lg[l].size.x);
            let bx = layer_x_pos + layer_width + edge_spacing;
            lg[edge].bend_points.insert(0, KVector::new(bx, mid_y));
        } else {
            lg[edge].bend_points.insert(0, KVector::new(layer_x_pos - edge_spacing, mid_y));
        }
    }

    // MARK: - Utility

    /// `calculateWestInLayerEdgeYDiff(_:)`.
    fn calculate_west_in_layer_edge_y_diff(lg: &LGraphArena, layer: LayerId) -> f64 {
        let mut max_y_diff: f64 = 0.0;

        for &node in &lg[layer].nodes {
            for outgoing_edge in lg.node_outgoing_edges(node) {
                let (Some(sp), Some(tp)) = (lg[outgoing_edge].source, lg[outgoing_edge].target) else { continue };

                if Some(layer) == lg[tp].owner.and_then(|n| lg[n].layer) && lg[sp].side == PortSide::WEST {
                    let source_pos = lg.port_absolute_anchor(sp).y;
                    let target_pos = lg.port_absolute_anchor(tp).y;
                    max_y_diff = swift::max(max_y_diff, (target_pos - source_pos).abs());
                }
            }
        }

        max_y_diff
    }

    /// `addBendPoint(_:_:_:_:)`.
    fn add_bend_point(&mut self, lg: &mut LGraphArena, edge: LEdgeId, bend_point: KVector, add_junction_point: bool, curr_port: LPortId) {
        let anchor = lg.port_absolute_anchor(curr_port);
        let differs = !(anchor.x == bend_point.x && anchor.y == bend_point.y);
        if (lg.edge_is_in_layer_edge(edge) || differs) && !lg.edge_is_self_loop(edge) {
            if lg[edge].source == Some(curr_port) {
                lg[edge].bend_points.insert(0, bend_point);
            } else {
                lg[edge].bend_points.add(bend_point);
            }

            if add_junction_point && !self.created_junction_points.contains(&bend_point) {
                // `JUNCTION_POINTS` has no default: the typed read is the
                // stored chain (shared, mutated in place) or nothing.
                let junction_points: KVectorChainRef = match lg[edge].props.get_typed::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS) {
                    Some(existing) => existing,
                    None => {
                        let jp = kvector_chain_ref(Default::default());
                        lg[edge].props.set(&LayeredOptions::JUNCTION_POINTS, jp.clone());
                        jp
                    }
                };

                let jpoint = bend_point;
                junction_points.borrow_mut().add(jpoint);
                self.created_junction_points.insert(&jpoint);
            }
        }
    }

    /// `isInLayerDummy(_:)`.
    fn is_in_layer_dummy(lg: &LGraphArena, node: LNodeId) -> bool {
        if lg[node].node_type == NodeType::LONG_EDGE {
            for e in lg.node_connected_edges(node) {
                if lg.edge_is_in_layer_edge(e) {
                    return true;
                }
            }
        }
        false
    }
}

impl ILayoutProcessor for PolylineEdgeRouter {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Polyline edge routing", 1.0);

        let sloped_edge_zone_width: f64 = lg[layered_graph].props.get_typed(&LayeredOptions::EDGE_ROUTING_POLYLINE_SLOPED_EDGE_ZONE_WIDTH).unwrap_or(2.0);
        let node_spacing: f64 = lg[layered_graph].props.get_typed(&LayeredOptions::SPACING_NODE_NODE_BETWEEN_LAYERS).unwrap_or(20.0);
        let edge_spacing: f64 = lg[layered_graph].props.get_typed(&LayeredOptions::SPACING_EDGE_EDGE_BETWEEN_LAYERS).unwrap_or(10.0);
        let edge_space_fac = swift::min(1.0, edge_spacing / swift::max(node_spacing, 1.0));

        let mut xpos: f64 = 0.0;

        let layers = lg[layered_graph].layers.clone();

        // Determine horizontal spacing for west-side in-layer edges of first layer
        if !layers.is_empty() {
            let y_diff = Self::calculate_west_in_layer_edge_y_diff(lg, layers[0]);
            xpos = LAYER_SPACE_FAC * edge_space_fac * y_diff;
        }

        // Iterate over layers
        for layer_index in 0..layers.len() {
            let layer = layers[layer_index];
            let external_layer = lg[layer].nodes.iter().all(|&n| Self::is_external_west_or_east_port(lg, n));

            // Don't give node spacing to rightmost external port layer
            if external_layer && xpos > 0.0 {
                xpos -= node_spacing;
            }

            // Set horizontal coordinates for all nodes of the layer
            lg.place_nodes_horizontally(layer, xpos);

            // Track max vertical span of edges between this and next layer
            let mut max_vert_diff: f64 = 0.0;

            for node in lg[layer].nodes.clone() {
                let mut max_curr_output_y_diff: f64 = 0.0;
                for outgoing_edge in lg.node_outgoing_edges(node) {
                    let (Some(source_port), Some(target_port)) = (lg[outgoing_edge].source, lg[outgoing_edge].target) else { continue };

                    let source_pos = lg.port_absolute_anchor(source_port).y;
                    let target_pos = lg.port_absolute_anchor(target_port).y;

                    if Some(layer) == lg[target_port].owner.and_then(|n| lg[n].layer) && !lg.edge_is_self_loop(outgoing_edge) {
                        // In-layer edge: add extra bend point
                        Self::process_in_layer_edge(lg, outgoing_edge, xpos, LAYER_SPACE_FAC * edge_space_fac * (source_pos - target_pos).abs());

                        if lg[source_port].side == PortSide::WEST {
                            // West in-layer edges don't contribute to between-layer spacing
                            continue;
                        }
                    }

                    max_curr_output_y_diff = swift::max(max_curr_output_y_diff, (target_pos - source_pos).abs());
                }

                // Process bend points for certain node types
                match lg[node].node_type {
                    NodeType::NORMAL | NodeType::LABEL | NodeType::LONG_EDGE | NodeType::NORTH_SOUTH_PORT | NodeType::BREAKING_POINT => {
                        self.process_node(lg, node, xpos, sloped_edge_zone_width);
                    }
                    _ => {}
                }

                max_vert_diff = swift::max(max_vert_diff, max_curr_output_y_diff);
            }

            // Consider west-side in-layer edges of next layer
            if layer_index + 1 < layers.len() {
                let y_diff = Self::calculate_west_in_layer_edge_y_diff(lg, layers[layer_index + 1]);
                max_vert_diff = swift::max(max_vert_diff, y_diff);
            }

            // Determine where next layer should start
            let mut layer_spacing = LAYER_SPACE_FAC * edge_space_fac * max_vert_diff;
            if !external_layer && layer_index + 1 < layers.len() {
                layer_spacing += node_spacing;
            }

            xpos += lg[layer].size.x + layer_spacing;
        }

        self.created_junction_points.clear();

        // Set the graph's horizontal size
        lg[layered_graph].size.x = xpos;

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "PolylineEdgeRouter"
    }
}

impl ILayoutPhase for PolylineEdgeRouter {
    fn get_layout_processor_configuration(&self, lg: &LGraphArena, graph: LGraphId) -> Option<LayoutProcessorConfiguration> {
        let graph_properties = lg[graph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();

        let mut configuration = LayoutProcessorConfiguration::create_from(&Self::baseline_processor_configuration());

        if graph_properties.contains(GraphProperties::NORTH_SOUTH_PORTS) {
            configuration.add_all(&Self::north_south_port_processing_additions());
        }
        if graph_properties.contains(GraphProperties::SELF_LOOPS) {
            configuration.add_all(&Self::self_loop_processing_additions());
        }
        if graph_properties.contains(GraphProperties::CENTER_LABELS) {
            configuration.add_all(&Self::center_edge_label_processing_additions());
        }
        if graph_properties.contains(GraphProperties::END_LABELS) {
            configuration.add_all(&Self::end_edge_label_processing_additions());
        }

        Some(configuration)
    }
}
