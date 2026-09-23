//! Port of `alg/layered/p5edges/OrthogonalEdgeRouter.swift`.
//!
//! Phase 5: places the layers horizontally and routes the edges between each
//! pair of adjacent layers orthogonally.

use crate::org::eclipse::elk::alg::layered::intermediate::intermediate_processor_strategy::IntermediateProcessorStrategy;
use crate::org::eclipse::elk::alg::layered::layered_phases::LayeredPhases;
use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal::direction::routing_direction::RoutingDirection;
use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal::orthogonal_routing_generator::OrthogonalRoutingGenerator;
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::alg::layout_processor_configuration::LayoutProcessorConfiguration;
use crate::prelude::*;

#[derive(Default)]
pub struct OrthogonalEdgeRouter;

impl OrthogonalEdgeRouter {
    pub fn new() -> OrthogonalEdgeRouter {
        OrthogonalEdgeRouter
    }

    fn hyperedge_processing_additions() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_before(LayeredPhases::P4_NODE_PLACEMENT, IntermediateProcessorStrategy::HYPEREDGE_DUMMY_MERGER);
        c
    }

    fn inverted_port_processing_additions() -> LayoutProcessorConfiguration {
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

    fn hierarchical_port_processing_additions() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_before(LayeredPhases::P3_NODE_ORDERING, IntermediateProcessorStrategy::HIERARCHICAL_PORT_CONSTRAINT_PROCESSOR)
            .add_before(LayeredPhases::P4_NODE_PLACEMENT, IntermediateProcessorStrategy::HIERARCHICAL_PORT_DUMMY_SIZE_PROCESSOR)
            .add_after(LayeredPhases::P5_EDGE_ROUTING, IntermediateProcessorStrategy::HIERARCHICAL_PORT_ORTHOGONAL_EDGE_ROUTER);
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

    fn hypernode_processing_additions() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_after(LayeredPhases::P5_EDGE_ROUTING, IntermediateProcessorStrategy::HYPERNODE_PROCESSOR);
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

    /// `resolveDouble(_:_:)`: a property value as `Double` (from a `Double`,
    /// an `Int` or a numeric `String`), else 0.
    fn resolve_double(lg: &LGraphArena, graph: LGraphId, property: &Property) -> f64 {
        match lg[graph].props.get(property) {
            Some(PropValue::Double(d)) => d,
            Some(PropValue::Int(i)) => i as f64,
            Some(PropValue::Str(s)) => swift::parse_double(&s).unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// `allExternalWestOrEastPort(_:)`.
    fn all_external_west_or_east_port(lg: &LGraphArena, nodes: &[LNodeId]) -> bool {
        for &node in nodes {
            let ext_port_side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE);
            if !(lg[node].node_type == NodeType::EXTERNAL_PORT && (ext_port_side == Some(PortSide::WEST) || ext_port_side == Some(PortSide::EAST))) {
                return false;
            }
        }
        true
    }
}

impl ILayoutProcessor for OrthogonalEdgeRouter {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Orthogonal edge routing", 1.0);

        let node_node_spacing = Self::resolve_double(lg, layered_graph, &LayeredOptions::SPACING_NODE_NODE_BETWEEN_LAYERS);
        let edge_edge_spacing = Self::resolve_double(lg, layered_graph, &LayeredOptions::SPACING_EDGE_EDGE_BETWEEN_LAYERS);
        let edge_node_spacing = Self::resolve_double(lg, layered_graph, &LayeredOptions::SPACING_EDGE_NODE_BETWEEN_LAYERS);

        let mut routing_generator = OrthogonalRoutingGenerator::new(RoutingDirection::WEST_TO_EAST, edge_edge_spacing, Some("phase5"));

        let mut xpos: f64 = 0.0;
        let mut layer_index: usize = 0;
        let mut left_layer: Option<LayerId> = None;
        let mut right_layer: Option<LayerId>;
        let mut left_layer_nodes: Option<Vec<LNodeId>> = None;
        let mut right_layer_nodes: Option<Vec<LNodeId>>;
        let mut left_layer_index: i64 = -1;
        let mut right_layer_index: i64;

        loop {
            // Fetch the next layer, if any
            if layer_index < lg[layered_graph].layers.len() {
                let rl = lg[layered_graph].layers[layer_index];
                right_layer = Some(rl);
                right_layer_nodes = Some(lg[rl].nodes.clone());
                right_layer_index = layer_index as i64;
                layer_index += 1;
            } else {
                right_layer = None;
                right_layer_nodes = None;
                right_layer_index = layer_index as i64 - 1;
            }

            // Place the left layer's nodes
            if let Some(left) = left_layer {
                lg.place_nodes_horizontally(left, xpos);
                xpos += lg[left].size.x;
            }

            // Route edges between the two layers
            let start_pos = if left_layer.is_none() { xpos } else { xpos + edge_node_spacing };
            let slots_count = routing_generator.route_edges(
                monitor,
                lg,
                layered_graph,
                left_layer_nodes.as_deref(),
                left_layer_index,
                right_layer_nodes.as_deref(),
                start_pos,
            );

            let is_left_layer_external = left_layer.is_none() || Self::all_external_west_or_east_port(lg, left_layer_nodes.as_deref().unwrap_or(&[]));
            let is_right_layer_external = right_layer.is_none() || Self::all_external_west_or_east_port(lg, right_layer_nodes.as_deref().unwrap_or(&[]));

            if slots_count > 0 {
                let mut routing_width = (slots_count - 1) as f64 * edge_edge_spacing;

                if left_layer.is_some() {
                    routing_width += edge_node_spacing;
                }
                if right_layer.is_some() {
                    routing_width += edge_node_spacing;
                }

                if routing_width < node_node_spacing && !is_left_layer_external && !is_right_layer_external {
                    routing_width = node_node_spacing;
                }
                xpos += routing_width;
            } else if !is_left_layer_external && !is_right_layer_external {
                xpos += node_node_spacing;
            }

            left_layer = right_layer;
            left_layer_nodes = right_layer_nodes;
            left_layer_index = right_layer_index;

            if right_layer.is_none() {
                break;
            }
        }

        lg[layered_graph].size.x = xpos;

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "OrthogonalEdgeRouter"
    }
}

impl ILayoutPhase for OrthogonalEdgeRouter {
    fn get_layout_processor_configuration(&self, lg: &LGraphArena, graph: LGraphId) -> Option<LayoutProcessorConfiguration> {
        let graph_properties = lg[graph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();

        let mut configuration = LayoutProcessorConfiguration::create();

        if graph_properties.contains(GraphProperties::HYPEREDGES) {
            configuration.add_all(&Self::hyperedge_processing_additions());
            configuration.add_all(&Self::inverted_port_processing_additions());
        }

        if graph_properties.contains(GraphProperties::NON_FREE_PORTS) || lg[graph].props.get_as::<bool>(&LayeredOptions::FEEDBACK_EDGES).unwrap_or(false) {
            configuration.add_all(&Self::inverted_port_processing_additions());

            if graph_properties.contains(GraphProperties::NORTH_SOUTH_PORTS) {
                configuration.add_all(&Self::north_south_port_processing_additions());
            }
        }

        if graph_properties.contains(GraphProperties::EXTERNAL_PORTS) {
            configuration.add_all(&Self::hierarchical_port_processing_additions());
        }

        if graph_properties.contains(GraphProperties::SELF_LOOPS) {
            configuration.add_all(&Self::self_loop_processing_additions());
        }

        if graph_properties.contains(GraphProperties::HYPERNODES) {
            configuration.add_all(&Self::hypernode_processing_additions());
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
