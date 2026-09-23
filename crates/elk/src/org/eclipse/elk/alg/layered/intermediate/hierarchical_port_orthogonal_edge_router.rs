//! Port of `alg/layered/intermediate/HierarchicalPortOrthogonalEdgeRouter.swift`.
//!
//! This processor does the job of routing edges connected to hierarchical ports.
//!
//! Six steps:
//! 1. Restore N/S port dummies removed by HierarchicalPortConstraintProcessor and connect to proxy dummies.
//! 2. Calculate N/S dummy coordinates.
//! 3. Route edges via OrthogonalRoutingGenerator.
//! 4. Remove temporary proxy dummies, rerouting edges to original dummies with bend points.
//! 5. Fix E/W dummy x coordinates and adjust y if graph height changed.
//! 6. Correct slanted edge segments on E/W dummies.
//!
//! Runs after phase 5.

use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal::direction::routing_direction::RoutingDirection;
use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal::orthogonal_routing_generator::OrthogonalRoutingGenerator;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::k_vector::KVectorRef;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::alg::common::nodespacing::node_dimension_calculation::NodeDimensionCalculation;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LGraphAdapters;
use crate::prelude::*;

#[derive(Default)]
pub struct HierarchicalPortOrthogonalEdgeRouter {
    /// The amount of space necessary to accommodate northern external port edge routing.
    northern_ext_port_edge_routing_height: f64,
}

impl HierarchicalPortOrthogonalEdgeRouter {
    pub fn new() -> HierarchicalPortOrthogonalEdgeRouter {
        HierarchicalPortOrthogonalEdgeRouter::default()
    }

    // MARK: - STEP 1: RESTORE NORTH / SOUTH DUMMIES

    /// `restoreNorthSouthDummies(_:)`: restores hierarchical port dummy nodes
    /// and connects them to temporary proxy dummies.
    fn restore_north_south_dummies(lg: &mut LGraphArena, layered_graph: LGraphId) -> Vec<LNodeId> {
        let mut restored_dummies: Vec<LNodeId> = Vec::new();

        if !lg[layered_graph].props.has(&InternalProperties::EXT_PORT_REPLACED_DUMMIES) {
            return restored_dummies;
        }

        // Restore the original external port dummies
        if let Some(replaced_dummies) = lg[layered_graph].props.get_as::<Vec<LNodeId>>(&InternalProperties::EXT_PORT_REPLACED_DUMMIES) {
            for dummy in replaced_dummies {
                Self::restore_dummy(lg, dummy, layered_graph);
                restored_dummies.push(dummy);
            }
        }

        // Looking for hierarchical port dummies that replaced the restored ones
        for layer in lg[layered_graph].layers.clone() {
            for node in lg[layer].nodes.clone() {
                if lg[node].node_type != NodeType::EXTERNAL_PORT {
                    continue;
                }

                if let Some(replaced_dummy) = lg[node].props.get_as::<LNodeId>(&InternalProperties::EXT_PORT_REPLACED_DUMMY) {
                    // assert(replacedDummy.getType() == .externalPort): a no-op
                    Self::connect_node_to_dummy(lg, layered_graph, node, replaced_dummy);
                }
            }
        }

        // Assign the restored dummies to the graph's last layer
        if let Some(&last) = lg[layered_graph].layers.last() {
            for &dummy in &restored_dummies {
                lg.node_set_layer(dummy, Some(last));
            }
        }

        restored_dummies
    }

    /// `restoreDummy(_:_:)`: restores the given dummy by setting its port side properly.
    fn restore_dummy(lg: &mut LGraphArena, dummy: LNodeId, graph: LGraphId) {
        let port_side = lg[dummy].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE);
        let dummy_port = lg[dummy].ports[0];

        if port_side == Some(PortSide::NORTH) {
            lg.port_set_side(dummy_port, PortSide::SOUTH);
        } else if port_side == Some(PortSide::SOUTH) {
            lg.port_set_side(dummy_port, PortSide::NORTH);
        }

        // Since the dummy node was hidden from the algorithm, its port labels are not placed properly
        // and its margins are not set accordingly.
        let size_constraints = lg[graph].props.get_as::<SizeConstraint>(&LayeredOptions::NODE_SIZE_CONSTRAINTS).unwrap_or(SizeConstraint::empty());
        if size_constraints.contains(SizeConstraint::PORT_LABELS) {
            let port_label_spacing_horizontal = lg[dummy].props.get_as::<f64>(&LayeredOptions::SPACING_LABEL_PORT_HORIZONTAL).unwrap_or(0.0);
            let port_label_spacing_vertical = lg[dummy].props.get_as::<f64>(&LayeredOptions::SPACING_LABEL_PORT_VERTICAL).unwrap_or(0.0);
            let label_label_spacing = lg[dummy].props.get_as::<f64>(&LayeredOptions::SPACING_LABEL_LABEL).unwrap_or(0.0);

            let port_label_placement = lg[graph].props.get_as::<PortLabelPlacement>(&LayeredOptions::PORT_LABELS_PLACEMENT).unwrap_or(PortLabelPlacement::empty());
            if port_label_placement.contains(PortLabelPlacement::INSIDE) {
                let mut current_y = port_label_spacing_vertical;
                let x_center_relative_to_port = lg[dummy].size.x / 2.0 - lg[dummy_port].position.x;

                for label in lg[dummy_port].labels.clone() {
                    lg[label].position.y = current_y;
                    lg[label].position.x = x_center_relative_to_port - lg[label].size.x / 2.0;

                    current_y += lg[label].size.y + label_label_spacing;
                }
            } else if port_label_placement.contains(PortLabelPlacement::OUTSIDE) {
                for label in lg[dummy_port].labels.clone() {
                    lg[label].position.x = port_label_spacing_horizontal + lg[dummy].size.x - lg[dummy_port].position.x;
                }
            }

            // Calculate margins
            NodeDimensionCalculation::get_node_margin_calculator(LGraphAdapters::adapt_ns(graph, false))
                .process_node(lg, &LGraphAdapters::adapt_node(dummy, false));
        }
    }

    /// `connectNodeToDummy(_:_:_:)`: adds a port to the given node and connects
    /// that to the given dummy node.
    fn connect_node_to_dummy(lg: &mut LGraphArena, _layered_graph: LGraphId, node: LNodeId, dummy: LNodeId) {
        let out_port = lg.new_port();
        lg.port_set_node(out_port, Some(node));

        let ext_port_side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE).unwrap_or(PortSide::UNDEFINED);
        lg.port_set_side(out_port, ext_port_side);

        // Find the dummy node's port
        let in_port = lg[dummy].ports[0];

        // Connect the two nodes
        let edge = lg.new_edge();
        lg.edge_set_source(edge, Some(out_port));
        lg.edge_set_target(edge, Some(in_port));
    }

    // MARK: - STEP 2: SET NORTH / SOUTH DUMMY COORDINATES

    /// `setNorthSouthDummyCoordinates(_:_:)`.
    fn set_north_south_dummy_coordinates(lg: &mut LGraphArena, layered_graph: LGraphId, north_south_dummies: &[LNodeId]) {
        let constraints = lg[layered_graph].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
        let graph_size = lg[layered_graph].size;
        let graph_padding = lg[layered_graph].padding;
        let graph_width = graph_size.x + graph_padding.left + graph_padding.right;
        let north_y = 0.0 - graph_padding.top - lg[layered_graph].offset.y;
        let south_y = graph_size.y + graph_padding.top + graph_padding.bottom - lg[layered_graph].offset.y;

        let mut northern_dummies: Vec<LNodeId> = Vec::new();
        let mut southern_dummies: Vec<LNodeId> = Vec::new();

        for &dummy in north_south_dummies {
            // Set x coordinate
            match constraints {
                PortConstraints::FREE | PortConstraints::FIXED_SIDE | PortConstraints::FIXED_ORDER => {
                    Self::calculate_north_south_dummy_positions(lg, dummy);
                }
                PortConstraints::FIXED_RATIO => {
                    Self::apply_north_south_dummy_ratio(lg, dummy, graph_width);
                    lg.node_border_to_content_area_coordinates(dummy, true, false);
                }
                PortConstraints::FIXED_POS => {
                    Self::apply_north_south_dummy_position(lg, dummy);
                    lg.node_border_to_content_area_coordinates(dummy, true, false);
                    // Ensure that the graph is wide enough to hold the port
                    let needed = lg[dummy].position.x + lg[dummy].size.x / 2.0;
                    lg[layered_graph].size.x = swift::max(lg[layered_graph].size.x, needed);
                }
                _ => {}
            }

            // Set y coordinates and add the dummy to its respective list
            match lg[dummy].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
                Some(PortSide::NORTH) => {
                    lg[dummy].position.y = north_y;
                    northern_dummies.push(dummy);
                }
                Some(PortSide::SOUTH) => {
                    lg[dummy].position.y = south_y;
                    southern_dummies.push(dummy);
                }
                _ => {}
            }
        }

        // Check for correct ordering and unique positions
        match constraints {
            PortConstraints::FREE | PortConstraints::FIXED_SIDE => {
                Self::ensure_unique_positions(lg, &northern_dummies, layered_graph);
                Self::ensure_unique_positions(lg, &southern_dummies, layered_graph);
            }
            PortConstraints::FIXED_ORDER => {
                Self::restore_proper_order(lg, &northern_dummies, layered_graph);
                Self::restore_proper_order(lg, &southern_dummies, layered_graph);
            }
            _ => {}
        }
    }

    /// `calculateNorthSouthDummyPositions(_:)`: positions N/S dummies based on
    /// the connected port positions.
    fn calculate_north_south_dummy_positions(lg: &mut LGraphArena, dummy: LNodeId) {
        let dummy_in_port = lg[dummy].ports[0];

        if lg.port_degree(dummy_in_port) == 0 {
            lg[dummy].position.x = 0.0;
        } else {
            let mut pos_sum = 0.0;

            for connected_port in lg.port_connected_ports(dummy_in_port) {
                let node_x = lg[connected_port].owner.map_or(0.0, |n| lg[n].position.x);
                pos_sum += node_x + lg[connected_port].position.x + lg[connected_port].anchor.x;
            }

            // PORT_ANCHOR aliases the dummy port's position
            let offset = lg.node_port_anchor(dummy).map_or(0.0, |a| a.x);
            lg[dummy].position.x = pos_sum / lg.port_degree(dummy_in_port) as f64 - offset;
        }
    }

    /// `applyNorthSouthDummyRatio(_:_:)`.
    fn apply_north_south_dummy_ratio(lg: &mut LGraphArena, dummy: LNodeId, width: f64) {
        let offset = lg.node_port_anchor(dummy).map_or(0.0, |a| a.x);
        let ratio = lg[dummy].props.get_as::<f64>(&InternalProperties::PORT_RATIO_OR_POSITION).unwrap_or(0.0);
        lg[dummy].position.x = width * ratio - offset;
    }

    /// `applyNorthSouthDummyPosition(_:)`.
    fn apply_north_south_dummy_position(lg: &mut LGraphArena, dummy: LNodeId) {
        let offset = lg.node_port_anchor(dummy).map_or(0.0, |a| a.x);
        let pos = lg[dummy].props.get_as::<f64>(&InternalProperties::PORT_RATIO_OR_POSITION).unwrap_or(0.0);
        lg[dummy].position.x = pos - offset;
    }

    /// `ensureUniquePositions(_:_:)`.
    fn ensure_unique_positions(lg: &mut LGraphArena, dummies: &[LNodeId], graph: LGraphId) {
        if dummies.is_empty() {
            return;
        }

        let mut dummy_array = dummies.to_vec();
        {
            let lgr: &LGraphArena = lg;
            swift::sort_by(&mut dummy_array, |&a, &b| lgr[a].position.x < lgr[b].position.x);
        }

        Self::assign_ascending_coordinates(lg, &dummy_array, graph);
    }

    /// `restoreProperOrder(_:_:)`.
    fn restore_proper_order(lg: &mut LGraphArena, dummies: &[LNodeId], graph: LGraphId) {
        if dummies.is_empty() {
            return;
        }

        let mut dummy_array = dummies.to_vec();
        {
            let lgr: &LGraphArena = lg;
            swift::sort_by(&mut dummy_array, |&a, &b| {
                let pa = lgr[a].props.get_as::<f64>(&InternalProperties::PORT_RATIO_OR_POSITION).unwrap_or(0.0);
                let pb = lgr[b].props.get_as::<f64>(&InternalProperties::PORT_RATIO_OR_POSITION).unwrap_or(0.0);
                pa < pb
            });
        }

        Self::assign_ascending_coordinates(lg, &dummy_array, graph);
    }

    /// `assignAscendingCoordinates(_:_:)`: makes the x coordinates strictly ascending.
    fn assign_ascending_coordinates(lg: &mut LGraphArena, dummies: &[LNodeId], graph: LGraphId) {
        let spacing = lg[graph].props.get_as::<f64>(&LayeredOptions::SPACING_PORT_PORT).unwrap_or(0.0);

        let d0 = &lg[dummies[0]];
        let mut next_valid_coordinate = d0.position.x + d0.size.x + d0.margin.right + spacing;

        for &dummy in &dummies[1..] {
            let current_size = lg[dummy].size;
            let current_margin = lg[dummy].margin;

            // Ensure spacings are adhered to
            let delta = lg[dummy].position.x - current_margin.left - next_valid_coordinate;
            if delta < 0.0 {
                lg[dummy].position.x -= delta;
            }

            // Ensure the graph is large enough for this node
            let needed = lg[dummy].position.x + current_size.x;
            lg[graph].size.x = swift::max(lg[graph].size.x, needed);

            // Compute next valid coordinate
            next_valid_coordinate = lg[dummy].position.x + current_size.x + current_margin.right + spacing;
        }
    }

    // MARK: - STEP 3: EDGE ROUTING

    /// `routeEdges(_:_:_:)`: routes northern and southern hierarchical port
    /// edges and adjusts the graph's height and offsets accordingly.
    fn route_edges(&mut self, monitor: &mut dyn IElkProgressMonitor, lg: &mut LGraphArena, layered_graph: LGraphId, north_south_dummies: &[LNodeId]) {
        let mut northern_source_layer: Vec<LNodeId> = Vec::new();
        let mut northern_target_layer: Vec<LNodeId> = Vec::new();
        let mut southern_source_layer: Vec<LNodeId> = Vec::new();
        let mut southern_target_layer: Vec<LNodeId> = Vec::new();

        let node_spacing = lg[layered_graph].props.get_as::<f64>(&LayeredOptions::SPACING_NODE_NODE).unwrap_or(0.0);
        let edge_spacing = lg[layered_graph].props.get_as::<f64>(&LayeredOptions::SPACING_EDGE_EDGE).unwrap_or(0.0);

        // Assemble the N/S hierarchical port dummies and their connected nodes
        for &hierarchical_port_dummy in north_south_dummies {
            let port_side = lg[hierarchical_port_dummy].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE);

            if port_side == Some(PortSide::NORTH) {
                northern_target_layer.push(hierarchical_port_dummy);
                for edge in lg.node_incoming_edges(hierarchical_port_dummy) {
                    if let Some(source_node) = lg.edge_source_node(edge) {
                        if !northern_source_layer.contains(&source_node) {
                            northern_source_layer.push(source_node);
                        }
                    }
                }
            } else if port_side == Some(PortSide::SOUTH) {
                southern_target_layer.push(hierarchical_port_dummy);
                for edge in lg.node_incoming_edges(hierarchical_port_dummy) {
                    if let Some(source_node) = lg.edge_source_node(edge) {
                        if !southern_source_layer.contains(&source_node) {
                            southern_source_layer.push(source_node);
                        }
                    }
                }
            }
        }

        // Northern routing
        if !northern_source_layer.is_empty() {
            let mut routing_generator = OrthogonalRoutingGenerator::new(RoutingDirection::SOUTH_TO_NORTH, edge_spacing, Some("extnorth"));

            let start = -node_spacing - lg[layered_graph].offset.y;
            let slots = routing_generator.route_edges(monitor, lg, layered_graph, Some(&northern_source_layer), 0, Some(&northern_target_layer), start);

            if slots > 0 {
                self.northern_ext_port_edge_routing_height = node_spacing + (slots - 1) as f64 * edge_spacing;
                lg[layered_graph].offset.y += self.northern_ext_port_edge_routing_height;
                lg[layered_graph].size.y += self.northern_ext_port_edge_routing_height;
            }
        }

        // Southern routing
        if !southern_source_layer.is_empty() {
            let mut routing_generator = OrthogonalRoutingGenerator::new(RoutingDirection::NORTH_TO_SOUTH, edge_spacing, Some("extsouth"));

            let start = lg[layered_graph].size.y + node_spacing - lg[layered_graph].offset.y;
            let slots = routing_generator.route_edges(monitor, lg, layered_graph, Some(&southern_source_layer), 0, Some(&southern_target_layer), start);

            if slots > 0 {
                lg[layered_graph].size.y += node_spacing + (slots - 1) as f64 * edge_spacing;
            }
        }
    }

    // MARK: - STEP 4: REMOVE TEMPORARY DUMMIES

    /// `removeTemporaryNorthSouthDummies(_:)`: removes the temporary dummies,
    /// reconnecting their edges to the original dummies with bend points.
    fn remove_temporary_north_south_dummies(lg: &mut LGraphArena, layered_graph: LGraphId) {
        let mut nodes_to_remove: Vec<LNodeId> = Vec::new();

        for layer in lg[layered_graph].layers.clone() {
            for node in lg[layer].nodes.clone() {
                if lg[node].node_type != NodeType::EXTERNAL_PORT {
                    continue;
                }

                if !lg[node].props.has(&InternalProperties::EXT_PORT_REPLACED_DUMMY) {
                    continue;
                }

                // Find the three ports: in (WEST), out (EAST), and origin (N or S)
                let mut node_in_port: Option<LPortId> = None;
                let mut node_out_port: Option<LPortId> = None;
                let mut node_origin_port: Option<LPortId> = None;

                for &port in &lg[node].ports {
                    match lg[port].side {
                        PortSide::WEST => node_in_port = Some(port),
                        PortSide::EAST => node_out_port = Some(port),
                        _ => node_origin_port = Some(port),
                    }
                }

                let Some(origin_port) = node_origin_port else { continue };
                if lg[origin_port].outgoing_edges.is_empty() {
                    continue;
                }

                // Find the edge connecting this dummy to the original external port dummy
                let node_to_origin_edge = lg[origin_port].outgoing_edges[0];

                // Compute bend points for incoming edges
                let mut incoming_edge_bend_points = lg[node_to_origin_edge].bend_points.clone();

                let mut first_bend_point = lg[origin_port].position;
                first_bend_point.add(lg[node].position);
                incoming_edge_bend_points.insert(0, first_bend_point);

                // Compute bend points for outgoing edges
                let mut outgoing_edge_bend_points = lg[node_to_origin_edge].bend_points.reversed();

                let mut last_bend_point = lg[origin_port].position;
                last_bend_point.add(lg[node].position);
                outgoing_edge_bend_points.add(last_bend_point);

                // Retrieve the original hierarchical port dummy
                let Some(replaced_dummy) = lg[node].props.get_as::<LNodeId>(&InternalProperties::EXT_PORT_REPLACED_DUMMY) else { continue };
                let replaced_dummy_port = lg[replaced_dummy].ports[0];

                // Reroute all the input port's edges
                if let Some(in_port) = node_in_port {
                    for edge in lg[in_port].incoming_edges.clone() {
                        lg.edge_set_target(edge, Some(replaced_dummy_port));
                        let at = lg[edge].bend_points.size();
                        lg[edge].bend_points.add_all_as_copies(at, &incoming_edge_bend_points.to_array());
                    }
                }

                // Reroute all the output port's edges
                if let Some(out_port) = node_out_port {
                    for edge in lg[out_port].outgoing_edges.clone() {
                        lg.edge_set_source(edge, Some(replaced_dummy_port));
                        lg[edge].bend_points.add_all_as_copies(0, &outgoing_edge_bend_points.to_array());
                    }
                }

                // Remove connection between node and original hierarchical port dummy
                lg.edge_set_source(node_to_origin_edge, None);
                lg.edge_set_target(node_to_origin_edge, None);

                // Remember the temporary node for removal
                nodes_to_remove.push(node);
            }
        }

        // Remove nodes
        for node in nodes_to_remove {
            lg.node_set_layer(node, None);
        }
    }

    // MARK: - STEP 5: FIX DUMMY COORDINATES

    /// `fixCoordinates(_:)`.
    fn fix_coordinates(lg: &mut LGraphArena, layered_graph: LGraphId) {
        let constraints = lg[layered_graph].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);

        let layers = lg[layered_graph].layers.clone();
        Self::fix_coordinates_in_layer(lg, layers[0], constraints, layered_graph);
        Self::fix_coordinates_in_layer(lg, layers[layers.len() - 1], constraints, layered_graph);
    }

    /// `fixCoordinatesInLayer(_:_:_:)`.
    fn fix_coordinates_in_layer(lg: &mut LGraphArena, layer: LayerId, constraints: PortConstraints, graph: LGraphId) {
        let padding = lg[graph].padding;
        let offset = lg[graph].offset;
        let graph_actual_size = lg.graph_actual_size(graph);

        let mut new_actual_graph_height = graph_actual_size.y;

        // First iteration: fix EAST and WEST dummy nodes (may change graph height)
        for node in lg[layer].nodes.clone() {
            if lg[node].node_type != NodeType::EXTERNAL_PORT {
                continue;
            }

            let ext_port_side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE).unwrap_or(PortSide::UNDEFINED);
            let ext_port_size = lg[node].props.get_as::<KVectorRef>(&InternalProperties::EXT_PORT_SIZE).map_or(KVector::default(), |v| *v.borrow());

            // Set x coordinate
            match ext_port_side {
                PortSide::EAST => lg[node].position.x = lg[graph].size.x + padding.right - offset.x,
                PortSide::WEST => lg[node].position.x = -offset.x - padding.left,
                _ => {}
            }

            // Set y coordinate
            let mut required_actual_graph_height = 0.0;

            match ext_port_side {
                PortSide::EAST | PortSide::WEST => {
                    if constraints == PortConstraints::FIXED_RATIO {
                        let ratio = lg[node].props.get_as::<f64>(&InternalProperties::PORT_RATIO_OR_POSITION).unwrap_or(0.0);
                        let anchor_y = lg.node_port_anchor(node).map_or(0.0, |a| a.y);
                        lg[node].position.y = graph_actual_size.y * ratio - anchor_y;
                        required_actual_graph_height = lg[node].position.y + ext_port_size.y;
                        lg.node_border_to_content_area_coordinates(node, false, true);
                    } else if constraints == PortConstraints::FIXED_POS {
                        let pos_or_ratio = lg[node].props.get_as::<f64>(&InternalProperties::PORT_RATIO_OR_POSITION).unwrap_or(0.0);
                        let anchor_y = lg.node_port_anchor(node).map_or(0.0, |a| a.y);
                        lg[node].position.y = pos_or_ratio - anchor_y;
                        required_actual_graph_height = lg[node].position.y + ext_port_size.y;
                        lg.node_border_to_content_area_coordinates(node, false, true);
                    }
                }
                _ => {}
            }

            new_actual_graph_height = swift::max(new_actual_graph_height, required_actual_graph_height);
        }

        // Make the graph larger, if necessary
        lg[graph].size.y += new_actual_graph_height - graph_actual_size.y;

        // Second iteration: fix NORTH and SOUTH dummies now that the graph's height is fixed
        for node in lg[layer].nodes.clone() {
            if lg[node].node_type != NodeType::EXTERNAL_PORT {
                continue;
            }

            let ext_port_side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE).unwrap_or(PortSide::UNDEFINED);

            match ext_port_side {
                PortSide::NORTH => lg[node].position.y = -offset.y - padding.top,
                PortSide::SOUTH => lg[node].position.y = lg[graph].size.y + padding.bottom - offset.y,
                _ => {}
            }
        }
    }

    // MARK: - STEP 6: SLANTED EDGE SEGMENT CORRECTION

    /// `correctSlantedEdgeSegments(_:)`.
    fn correct_slanted_edge_segments(lg: &mut LGraphArena, layered_graph: LGraphId) {
        let layers = lg[layered_graph].layers.clone();
        Self::correct_slanted_edge_segments_in_layer(lg, layers[0]);
        Self::correct_slanted_edge_segments_in_layer(lg, layers[layers.len() - 1]);
    }

    /// `correctSlantedEdgeSegmentsInLayer(_:)`. Mutates the first/last bend
    /// point in place; bend points are never shared between chains here (every
    /// producer adds fresh vectors or copies), so editing the value is the same.
    fn correct_slanted_edge_segments_in_layer(lg: &mut LGraphArena, layer: LayerId) {
        for node in lg[layer].nodes.clone() {
            if lg[node].node_type != NodeType::EXTERNAL_PORT {
                continue;
            }

            let ext_port_side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE);

            if ext_port_side == Some(PortSide::EAST) || ext_port_side == Some(PortSide::WEST) {
                for edge in lg.node_connected_edges(node) {
                    if lg[edge].bend_points.is_empty() {
                        // TODO: The edge has no bend points yet, but may still be slanted.
                        continue;
                    }

                    // Correct slanted segment connected to the source port if it belongs to our node
                    let source_port = lg[edge].source.unwrap();
                    if lg[source_port].owner == Some(node) {
                        let y = lg.port_absolute_anchor(source_port).y;
                        lg[edge].bend_points.iter_mut().next().unwrap().y = y;
                    }

                    // Correct slanted segment connected to the target port if it belongs to our node
                    let target_port = lg[edge].target.unwrap();
                    if lg[target_port].owner == Some(node) {
                        let y = lg.port_absolute_anchor(target_port).y;
                        lg[edge].bend_points.iter_mut().last().unwrap().y = y;
                    }
                }
            }
        }
    }
}

impl ILayoutProcessor for HierarchicalPortOrthogonalEdgeRouter {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Orthogonally routing hierarchical port edges", 1.0);
        self.northern_ext_port_edge_routing_height = 0.0;

        // Step 1: Restore N/S port dummies
        let north_south_dummies = Self::restore_north_south_dummies(lg, layered_graph);

        // Step 2: Calculate N/S dummy coordinates
        Self::set_north_south_dummy_coordinates(lg, layered_graph, &north_south_dummies);

        // Step 3: Route edges
        self.route_edges(monitor, lg, layered_graph, &north_south_dummies);

        // Step 4: Remove temporary N/S dummies
        Self::remove_temporary_north_south_dummies(lg, layered_graph);

        // Step 5: Fix E/W dummy coordinates
        Self::fix_coordinates(lg, layered_graph);

        // Step 6: Correct slanted edge segments
        Self::correct_slanted_edge_segments(lg, layered_graph);

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "HierarchicalPortOrthogonalEdgeRouter"
    }
}
