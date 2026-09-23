//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/compound/org_eclipse_elk_alg_layered_compound_CompoundGraphPreprocessor.swift`.
//!
//! Preprocess a compound graph by splitting cross-hierarchy edges. The result
//! is stored in `InternalProperties.CROSS_HIERARCHY_MAP`, which is attached to
//! the top-level graph.
//!
//! Instance state: `ElkLayered` keeps one preprocessor for every layout it
//! runs. `crossHierarchyMap` and `dummyNodeMap` are reset by every run.
//! `portToNodeEntries` is never reset in Swift: it keeps growing across
//! layouts, and every run re-applies all earlier runs' entries — to ports
//! and nodes of earlier runs' layered graphs, whose layouts were already
//! applied, so that has no effect on any output. Those ids would dangle
//! here (every layout has its own arena), so the port starts each run with
//! an empty list; see `process`.

use std::collections::HashMap;
use std::rc::Rc;

use super::cross_hierarchy_edge::{CrossHierarchyEdge, CrossHierarchyMap};
use super::cross_hierarchy_edge_comparator::CrossHierarchyEdgeComparator;
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LGraphId, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::options::graph_properties::GraphProperties;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::options::direction::Direction;
use crate::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;
use crate::org::eclipse::elk::graph::properties::property::PropValue;
use crate::swift;

/// `ElkUtil.computeInsidePart(_:_:_:_:_:)` (the label overload). The
/// coordinator owns `ElkUtil`; this private copy of the five-line function
/// can be replaced by `ElkUtil::compute_inside_part` at merge.
fn compute_inside_part(label_pos: KVector, label_size: KVector, port_size: KVector, _label_spacing: f64, port_side: PortSide) -> f64 {
    match port_side {
        PortSide::EAST | PortSide::WEST => {
            let inside_end = swift::min(label_pos.x + label_size.x, port_size.x);
            let inside_start = swift::max(label_pos.x, 0.0);
            swift::max(0.0, inside_end - inside_start)
        }
        PortSide::NORTH | PortSide::SOUTH => {
            let inside_end = swift::min(label_pos.y + label_size.y, port_size.y);
            let inside_start = swift::max(label_pos.y, 0.0);
            swift::max(0.0, inside_end - inside_start)
        }
        _ => 0.0,
    }
}

/// An `ExternalPort` reference (index into the run's external port list).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ExtPortId(u32);

/// `CompoundGraphPreprocessor.ExternalPort` (a class; shared between lists
/// and mutated through `origEdges`, so it lives in an arena).
#[derive(Clone, Debug)]
struct ExternalPort {
    orig_edges: Vec<LEdgeId>,
    new_edge: LEdgeId,
    dummy_node: LNodeId,
    dummy_port: LPortId,
    port_type: PortType,
    exported: bool,
}

#[derive(Default)]
pub struct CompoundGraphPreprocessor {
    /// Map of original edges to generated cross-hierarchy edges.
    cross_hierarchy_map: CrossHierarchyMap,
    /// Map of ports to their assigned dummy nodes in the nested graphs.
    dummy_node_map: HashMap<LPortId, LNodeId>,
    /// Tracks actual port→node pairs for `setSidesOfPortsToSidesOfDummyNodes`.
    port_to_node_entries: Vec<(LPortId, LNodeId)>,
    /// The `ExternalPort` objects of the current run.
    external_ports: Vec<ExternalPort>,
}

impl CompoundGraphPreprocessor {
    pub fn new() -> CompoundGraphPreprocessor {
        CompoundGraphPreprocessor::default()
    }

    fn get_dummy_node(&self, port: LPortId) -> Option<LNodeId> {
        self.dummy_node_map.get(&port).copied()
    }

    fn set_dummy_node_tracked(&mut self, node: LNodeId, port: LPortId) {
        self.dummy_node_map.insert(port, node);
        self.port_to_node_entries.push((port, node));
    }

    fn ext(&self, id: ExtPortId) -> &ExternalPort {
        &self.external_ports[id.0 as usize]
    }

    fn new_external_port(&mut self, orig_edge: LEdgeId, new_edge: LEdgeId, dummy_node: LNodeId, dummy_port: LPortId, port_type: PortType, exported: bool) -> ExtPortId {
        let id = ExtPortId(self.external_ports.len() as u32);
        self.external_ports.push(ExternalPort { orig_edges: vec![orig_edge], new_edge, dummy_node, dummy_port, port_type, exported });
        id
    }
}

impl ILayoutProcessor for CompoundGraphPreprocessor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Compound graph preprocessor", 1.0);

        self.cross_hierarchy_map = CrossHierarchyMap::new();
        // Not in Swift, where `portToNodeEntries` is never cleared (see the
        // module comment): earlier runs' entries only touch earlier runs'
        // (finished) layered graphs, so dropping them changes no output.
        self.port_to_node_entries.clear();
        self.external_ports.clear();

        // Create new dummy edges at hierarchy bounds and move the labels around accordingly
        let _ = self.transform_hierarchy_edges(lg, graph, None);
        self.move_labels_and_remove_original_edges(lg, graph);

        self.set_sides_of_ports_to_sides_of_dummy_nodes(lg);

        // Attach cross hierarchy map to the graph and cleanup
        let map = std::mem::take(&mut self.cross_hierarchy_map);
        lg[graph].props.set(&InternalProperties::CROSS_HIERARCHY_MAP, PropValue::object(Rc::new(map)));
        self.dummy_node_map.clear();

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "CompoundGraphPreprocessor"
    }
}

/// `graph.getProperty(GRAPH_PROPERTIES) as? Set<GraphProperties> ?? []`.
fn graph_properties(lg: &LGraphArena, graph: LGraphId) -> EnumSet<GraphProperties> {
    lg[graph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default()
}

fn insert_graph_properties(lg: &mut LGraphArena, graph: LGraphId, add: &[GraphProperties]) {
    let mut graph_props = graph_properties(lg, graph);
    for &p in add {
        graph_props.insert(p);
    }
    lg[graph].props.set(&InternalProperties::GRAPH_PROPERTIES, graph_props);
}

/// `LGraphUtil.createExternalPortDummy(port, ...)` with an `LPort` as the
/// property holder: its property map is lent out for the call.
#[allow(clippy::too_many_arguments)]
fn create_external_port_dummy_for_port(
    lg: &mut LGraphArena,
    port: LPortId,
    port_constraints: PortConstraints,
    port_side: PortSide,
    net_flow: i64,
    port_node_size: KVector,
    port_position: KVector,
    port_size: KVector,
    layout_direction: Direction,
    layered_graph: LGraphId,
) -> LNodeId {
    let mut holder = std::mem::take(&mut lg[port].props);
    let dummy = lg.create_external_port_dummy(
        &mut holder,
        port_constraints,
        port_side,
        net_flow,
        port_node_size,
        port_position,
        port_size,
        layout_direction,
        layered_graph,
    );
    lg[port].props = holder;
    dummy
}

impl CompoundGraphPreprocessor {
    /// Ensures that for each dummy node the external port and vice versa is set.
    fn set_sides_of_ports_to_sides_of_dummy_nodes(&mut self, lg: &mut LGraphArena) {
        for &(port, dummy_node) in &self.port_to_node_entries {
            lg[dummy_node].props.set(&InternalProperties::ORIGIN, PropValue::LPort(port));
            lg[port].props.set(&InternalProperties::PORT_DUMMY, PropValue::LNode(dummy_node));
            lg[port].props.set(&InternalProperties::INSIDE_CONNECTIONS, true);
            if let Some(ext_port_side) = lg[dummy_node].props.get_typed::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
                lg.port_set_side(port, ext_port_side);
            }
            if let Some(owner_node) = lg[port].owner {
                lg[owner_node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_SIDE);
                if let Some(owner_graph) = lg[owner_node].graph {
                    insert_graph_properties(lg, owner_graph, &[GraphProperties::NON_FREE_PORTS]);
                }
            }
        }
    }

    // MARK: - Hierarchy Edge Transformation

    /// Recursively transform cross-hierarchy edges into sequences of dummy
    /// ports and dummy edges.
    fn transform_hierarchy_edges(&mut self, lg: &mut LGraphArena, graph: LGraphId, parent_node: Option<LNodeId>) -> Vec<ExtPortId> {
        // Process all children and recurse down to gather their external ports
        let mut contained_external_ports: Vec<ExtPortId> = Vec::new();

        for node in lg[graph].layerless_nodes.clone() {
            if let Some(nested_graph) = lg[node].nested_graph {
                // Recursively process the child graph
                let child_ports = self.transform_hierarchy_edges(lg, nested_graph, Some(node));
                contained_external_ports.extend(child_ports);

                // Process inside self loops
                self.process_inside_self_loops(lg, nested_graph, node);

                // Make sure that all hierarchical ports have had dummy nodes created for them
                let nested_graph_props = graph_properties(lg, nested_graph);
                if nested_graph_props.contains(GraphProperties::EXTERNAL_PORTS) {
                    let port_constraints = lg[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
                    let port_labels_placement = lg[node].props.get_as::<PortLabelPlacement>(&LayeredOptions::PORT_LABELS_PLACEMENT);
                    let inside_port_labels = port_labels_placement.is_some_and(|plp| plp.contains(PortLabelPlacement::INSIDE));

                    for port in lg[node].ports.clone() {
                        // Make sure that every port has a dummy node created for it
                        let mut dummy_node = self.get_dummy_node(port);
                        if dummy_node.is_none() {
                            let net_flow = -(lg.port_net_flow(port) as i64);
                            let side = lg[port].side;
                            let size = lg[port].size;
                            let direction = lg[nested_graph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
                            let dn = create_external_port_dummy_for_port(
                                lg,
                                port,
                                port_constraints,
                                side,
                                net_flow,
                                KVector::default(),
                                KVector::default(),
                                size,
                                direction,
                                nested_graph,
                            );
                            lg[dn].props.set(&InternalProperties::ORIGIN, PropValue::LPort(port));
                            self.set_dummy_node_tracked(dn, port);
                            lg[nested_graph].layerless_nodes.push(dn);
                            dummy_node = Some(dn);
                        }

                        let Some(resolved_dummy_node) = dummy_node else { continue };
                        let dummy_node_port = lg[resolved_dummy_node].ports[0];

                        for ext_port_label in lg[port].labels.clone() {
                            let dummy_port_label = lg.new_label("");
                            lg[dummy_port_label].size.x = lg[ext_port_label].size.x;
                            lg[dummy_port_label].size.y = lg[ext_port_label].size.y;
                            lg[dummy_node_port].labels.push(dummy_port_label);

                            if !inside_port_labels {
                                let side = lg[port].side;
                                let mut inside_part: f64 = 0.0;
                                if let Some(plp) = port_labels_placement {
                                    if PortLabelPlacement::is_fixed(plp) {
                                        inside_part = compute_inside_part(lg[ext_port_label].position, lg[ext_port_label].size, lg[port].size, 0.0, side);
                                    }
                                }
                                if port_constraints == PortConstraints::FREE || side == PortSide::EAST || side == PortSide::WEST {
                                    lg[dummy_port_label].size.x = inside_part;
                                } else {
                                    lg[dummy_port_label].size.y = inside_part;
                                }
                            }
                        }
                    }
                }
            }
        }

        // This will be the list of external ports we will export
        let mut exported_external_ports: Vec<ExtPortId> = Vec::new();

        // Process the cross-hierarchy edges connected to the inside of the child nodes
        self.process_inner_hierarchical_edge_segments(lg, graph, parent_node, &contained_external_ports, &mut exported_external_ports);

        // Process the cross-hierarchy edges connected to the outside of the parent node
        if let Some(parent_node) = parent_node {
            self.process_outer_hierarchical_edge_segments(lg, graph, parent_node, &mut exported_external_ports);
        }

        exported_external_ports
    }

    // MARK: - Move Labels and Remove Original Edges

    fn move_labels_and_remove_original_edges(&mut self, lg: &mut LGraphArena, graph: LGraphId) {
        // NONDETERMINISTIC IN SWIFT: dictionary iteration; see `CrossHierarchyMap`.
        let entries: Vec<(LEdgeId, Vec<CrossHierarchyEdge>)> = self.cross_hierarchy_map.iter().map(|(e, s)| (e, s.to_vec())).collect();
        for (orig_edge, segments) in entries {
            // If the original edge had any labels, move them to the newly introduced edge segments
            if !lg[orig_edge].labels.is_empty() {
                let comp = CrossHierarchyEdgeComparator::new(graph);
                let sorted_segments = swift::sorted_by(segments, |a, b| comp.compare(lg, a, b) == std::cmp::Ordering::Less);

                // Iterate over labels and move them
                let mut labels_to_remove: Vec<usize> = Vec::new();
                for (label_idx, curr_label) in lg[orig_edge].labels.clone().into_iter().enumerate() {
                    let mut target_dummy_edge_index: i64 = -1;
                    let placement = lg[curr_label].props.get_typed::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT);

                    match placement {
                        Some(EdgeLabelPlacement::HEAD) => target_dummy_edge_index = sorted_segments.len() as i64 - 1,
                        Some(EdgeLabelPlacement::CENTER) => target_dummy_edge_index = get_shallowest_edge_segment(&sorted_segments),
                        Some(EdgeLabelPlacement::TAIL) => target_dummy_edge_index = 0,
                        None => {}
                    }

                    if target_dummy_edge_index != -1 {
                        let target_segment = sorted_segments[target_dummy_edge_index as usize];
                        let target_edge = target_segment.get_edge();
                        lg[target_edge].labels.push(curr_label);

                        if let Some(seg_graph) = lg.edge_source_node(target_edge).and_then(|n| lg[n].graph) {
                            insert_graph_properties(lg, seg_graph, &[GraphProperties::END_LABELS, GraphProperties::CENTER_LABELS]);
                        }

                        labels_to_remove.push(label_idx);
                        lg[curr_label].props.set(&InternalProperties::ORIGINAL_LABEL_EDGE, PropValue::LEdge(orig_edge));
                    }
                }

                // Remove labels in reverse order
                for &idx in labels_to_remove.iter().rev() {
                    lg[orig_edge].labels.remove(idx);
                }
            }

            // Remove original edge
            lg.edge_set_source(orig_edge, None);
            lg.edge_set_target(orig_edge, None);
        }
    }

    // MARK: - Inner Hierarchical Edge Segment Processing

    fn process_inner_hierarchical_edge_segments(
        &mut self,
        lg: &mut LGraphArena,
        graph: LGraphId,
        parent_node: Option<LNodeId>,
        contained_external_ports: &[ExtPortId],
        exported_external_ports: &mut Vec<ExtPortId>,
    ) {
        let mut created_external_ports: Vec<ExtPortId> = Vec::new();

        for &external_port in contained_external_ports {
            let mut current_external_port: Option<ExtPortId> = None;
            let ext = self.ext(external_port).clone();

            if ext.port_type == PortType::OUTPUT {
                for out_edge in ext.orig_edges.clone() {
                    let Some(out_target) = lg[out_edge].target else { continue };
                    let Some(target_node) = lg[out_target].owner else { continue };
                    if lg[target_node].graph == Some(graph) {
                        self.connect_child(lg, graph, external_port, out_edge, ext.dummy_port, out_target);
                    } else if parent_node.is_none() || lg.is_descendant(Some(target_node), parent_node) {
                        // Case 2: edge connects two direct children
                        self.connect_siblings(lg, graph, external_port, contained_external_ports, out_edge);
                    } else {
                        let Some(pn) = parent_node else { continue };
                        let new_external_port =
                            self.introduce_hierarchical_edge_segment(lg, graph, pn, out_edge, ext.dummy_port, PortType::OUTPUT, current_external_port);
                        if Some(new_external_port) != current_external_port {
                            created_external_ports.push(new_external_port);
                        }
                        if self.ext(new_external_port).exported {
                            current_external_port = Some(new_external_port);
                        }
                    }
                }
            } else {
                for in_edge in ext.orig_edges.clone() {
                    let Some(in_source) = lg[in_edge].source else { continue };
                    let Some(source_node) = lg[in_source].owner else { continue };
                    if lg[source_node].graph == Some(graph) {
                        self.connect_child(lg, graph, external_port, in_edge, in_source, ext.dummy_port);
                    } else if parent_node.is_none() || lg.is_descendant(Some(source_node), parent_node) {
                        // Case 2: handled by output port code above
                        continue;
                    } else {
                        let Some(pn) = parent_node else { continue };
                        let new_external_port =
                            self.introduce_hierarchical_edge_segment(lg, graph, pn, in_edge, ext.dummy_port, PortType::INPUT, current_external_port);
                        if Some(new_external_port) != current_external_port {
                            created_external_ports.push(new_external_port);
                        }
                        if self.ext(new_external_port).exported {
                            current_external_port = Some(new_external_port);
                        }
                    }
                }
            }
        }

        // Add dummy nodes and exported external ports
        self.add_created_external_ports(lg, graph, &created_external_ports, exported_external_ports);
    }

    fn add_created_external_ports(&self, lg: &mut LGraphArena, graph: LGraphId, created_external_ports: &[ExtPortId], exported_external_ports: &mut Vec<ExtPortId>) {
        for &external_port in created_external_ports {
            let dummy_node = self.ext(external_port).dummy_node;
            if !lg[graph].layerless_nodes.contains(&dummy_node) {
                lg[graph].layerless_nodes.push(dummy_node);
            }
            if self.ext(external_port).exported {
                exported_external_ports.push(external_port);
            }
        }
    }

    /// Connects an external port with a child node of the given graph.
    fn connect_child(&mut self, lg: &mut LGraphArena, graph: LGraphId, external_port: ExtPortId, orig_edge: LEdgeId, source_port: LPortId, target_port: LPortId) {
        let dummy_edge = create_dummy_edge(lg, graph, orig_edge);
        lg.edge_set_source(dummy_edge, Some(source_port));
        lg.edge_set_target(dummy_edge, Some(target_port));

        let port_type = self.ext(external_port).port_type;
        self.cross_hierarchy_map.append(orig_edge, CrossHierarchyEdge::new(dummy_edge, graph, port_type));
    }

    /// Connects external ports of two child nodes of the given graph.
    fn connect_siblings(&mut self, lg: &mut LGraphArena, graph: LGraphId, external_output_port: ExtPortId, contained_external_ports: &[ExtPortId], orig_edge: LEdgeId) {
        // Find the opposite external port
        let mut target_external_port: Option<ExtPortId> = None;
        for &external_port2 in contained_external_ports {
            if external_port2 != external_output_port && self.ext(external_port2).orig_edges.contains(&orig_edge) {
                target_external_port = Some(external_port2);
                break;
            }
        }
        let Some(target_ext_port) = target_external_port else { return };
        // assert(targetExtPort.type == .INPUT) (a no-op in release builds)

        let dummy_edge = create_dummy_edge(lg, graph, orig_edge);
        lg.edge_set_source(dummy_edge, Some(self.ext(external_output_port).dummy_port));
        lg.edge_set_target(dummy_edge, Some(self.ext(target_ext_port).dummy_port));

        let port_type = self.ext(external_output_port).port_type;
        self.cross_hierarchy_map.append(orig_edge, CrossHierarchyEdge::new(dummy_edge, graph, port_type));
    }

    // MARK: - Outer Hierarchical Edge Segment Processing

    fn process_outer_hierarchical_edge_segments(&mut self, lg: &mut LGraphArena, graph: LGraphId, parent_node: LNodeId, exported_external_ports: &mut Vec<ExtPortId>) {
        let mut created_external_ports: Vec<ExtPortId> = Vec::new();

        for child_node in lg[graph].layerless_nodes.clone() {
            for child_port in lg[child_node].ports.clone() {
                // Outgoing edges
                let mut current_external_output_port: Option<ExtPortId> = None;
                for out_edge in lg[child_port].outgoing_edges.clone() {
                    let (Some(out_target), Some(out_source)) = (lg[out_edge].target, lg[out_edge].source) else { continue };
                    if !lg.is_descendant(lg[out_target].owner, Some(parent_node)) {
                        let new_external_port =
                            self.introduce_hierarchical_edge_segment(lg, graph, parent_node, out_edge, out_source, PortType::OUTPUT, current_external_output_port);
                        if Some(new_external_port) != current_external_output_port {
                            created_external_ports.push(new_external_port);
                        }
                        if self.ext(new_external_port).exported {
                            current_external_output_port = Some(new_external_port);
                        }
                    }
                }

                // Incoming edges
                let mut current_external_input_port: Option<ExtPortId> = None;
                for in_edge in lg[child_port].incoming_edges.clone() {
                    let (Some(in_source), Some(in_target)) = (lg[in_edge].source, lg[in_edge].target) else { continue };
                    if !lg.is_descendant(lg[in_source].owner, Some(parent_node)) {
                        let new_external_port =
                            self.introduce_hierarchical_edge_segment(lg, graph, parent_node, in_edge, in_target, PortType::INPUT, current_external_input_port);
                        if Some(new_external_port) != current_external_input_port {
                            created_external_ports.push(new_external_port);
                        }
                        if self.ext(new_external_port).exported {
                            current_external_input_port = Some(new_external_port);
                        }
                    }
                }
            }
        }

        // Add dummy nodes and exported external ports
        self.add_created_external_ports(lg, graph, &created_external_ports, exported_external_ports);
    }

    // MARK: - Inside Self Loop Processing

    fn process_inside_self_loops(&mut self, lg: &mut LGraphArena, nested_graph: LGraphId, node: LNodeId) {
        let activate = lg[node].props.get_typed::<bool>(&LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE).unwrap_or(false);
        if !activate {
            return;
        }

        for lport in lg[node].ports.clone() {
            let out_edges = lg[lport].outgoing_edges.clone();

            for out_edge in out_edges {
                let is_self_loop = lg.edge_target_node(out_edge) == Some(node);
                let is_inside_self_loop = is_self_loop && lg[out_edge].props.get_typed::<bool>(&LayeredOptions::INSIDE_SELF_LOOPS_YO).unwrap_or(false);

                if is_inside_self_loop {
                    let Some(source_port) = lg[out_edge].source else { continue };
                    let mut source_ext_port_dummy = self.get_dummy_node(source_port);
                    if source_ext_port_dummy.is_none() {
                        let side = lg[source_port].side;
                        let size = lg[source_port].size;
                        let direction = lg[nested_graph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
                        let new_dummy = create_external_port_dummy_for_port(
                            lg,
                            source_port,
                            PortConstraints::FREE,
                            side,
                            -1,
                            KVector::default(),
                            KVector::default(),
                            size,
                            direction,
                            nested_graph,
                        );
                        lg[new_dummy].props.set(&InternalProperties::ORIGIN, PropValue::LPort(source_port));
                        self.set_dummy_node_tracked(new_dummy, source_port);
                        lg[nested_graph].layerless_nodes.push(new_dummy);
                        source_ext_port_dummy = Some(new_dummy);
                    }

                    let Some(target_port) = lg[out_edge].target else { continue };
                    let mut target_ext_port_dummy = self.get_dummy_node(target_port);
                    if target_ext_port_dummy.is_none() {
                        let side = lg[target_port].side;
                        let size = lg[target_port].size;
                        let direction = lg[nested_graph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
                        let new_dummy = create_external_port_dummy_for_port(
                            lg,
                            target_port,
                            PortConstraints::FREE,
                            side,
                            1,
                            KVector::default(),
                            KVector::default(),
                            size,
                            direction,
                            nested_graph,
                        );
                        lg[new_dummy].props.set(&InternalProperties::ORIGIN, PropValue::LPort(target_port));
                        self.set_dummy_node_tracked(new_dummy, target_port);
                        lg[nested_graph].layerless_nodes.push(new_dummy);
                        target_ext_port_dummy = Some(new_dummy);
                    }

                    let (Some(src_dummy), Some(tgt_dummy)) = (source_ext_port_dummy, target_ext_port_dummy) else { continue };
                    let dummy_edge = create_dummy_edge(lg, nested_graph, out_edge);
                    let src_port = lg[src_dummy].ports[0];
                    let tgt_port = lg[tgt_dummy].ports[0];
                    lg.edge_set_source(dummy_edge, Some(src_port));
                    lg.edge_set_target(dummy_edge, Some(tgt_port));

                    self.cross_hierarchy_map.append(out_edge, CrossHierarchyEdge::new(dummy_edge, nested_graph, PortType::OUTPUT));

                    insert_graph_properties(lg, nested_graph, &[GraphProperties::EXTERNAL_PORTS]);
                }
            }
        }
    }

    // MARK: - General Hierarchical Edge Segment Processing

    /// Does the actual work of creating a new hierarchical edge segment.
    #[allow(clippy::too_many_arguments)]
    fn introduce_hierarchical_edge_segment(
        &mut self,
        lg: &mut LGraphArena,
        graph: LGraphId,
        parent_node: LNodeId,
        orig_edge: LEdgeId,
        opposite_port: LPortId,
        port_type: PortType,
        default_external_port: Option<ExtPortId>,
    ) -> ExtPortId {
        // Check if external ports are to be merged
        let merge_external_ports = lg[graph].props.get_typed::<bool>(&LayeredOptions::MERGE_HIERARCHY_EDGES).unwrap_or(false);

        // Check if the edge connects to the parent node
        let mut parent_end_port: Option<LPortId> = None;
        if port_type == PortType::INPUT && lg.edge_source_node(orig_edge) == Some(parent_node) {
            parent_end_port = lg[orig_edge].source;
        } else if port_type == PortType::OUTPUT && lg.edge_target_node(orig_edge) == Some(parent_node) {
            parent_end_port = lg[orig_edge].target;
        }

        let mut external_port = default_external_port;
        if external_port.is_none() || !merge_external_ports || parent_end_port.is_some() {
            // Create a dummy node that will represent the external port
            let mut external_port_side = PortSide::UNDEFINED;
            if let Some(parent_end_port) = parent_end_port {
                external_port_side = lg[parent_end_port].side;
            } else {
                let pc = lg[parent_node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
                if pc.is_side_fixed() {
                    external_port_side = if port_type == PortType::INPUT { PortSide::WEST } else { PortSide::EAST };
                }
            }
            let dummy_node = self.create_external_port_dummy(lg, graph, parent_node, port_type, external_port_side, orig_edge);

            // Create a dummy edge to be connected to the port
            let Some(parent_graph) = lg[parent_node].graph else {
                return match external_port {
                    Some(ep) => ep,
                    None => {
                        let new_edge = lg.new_edge();
                        let dummy_port = lg.new_port();
                        self.new_external_port(orig_edge, new_edge, dummy_node, dummy_port, port_type, false)
                    }
                };
            };
            let dummy_edge = create_dummy_edge(lg, parent_graph, orig_edge);

            let dummy_node_port = lg[dummy_node].ports[0];
            if port_type == PortType::INPUT {
                lg.edge_set_source(dummy_edge, Some(dummy_node_port));
                lg.edge_set_target(dummy_edge, Some(opposite_port));
            } else {
                lg.edge_set_source(dummy_edge, Some(opposite_port));
                lg.edge_set_target(dummy_edge, Some(dummy_node_port));
            }

            // Create the external port (exported if not connecting just to the parent node)
            let dummy_port = match lg[dummy_node].props.get_as::<LPortId>(&InternalProperties::ORIGIN) {
                Some(p) => p,
                None => lg.new_port(),
            };
            external_port = Some(self.new_external_port(orig_edge, dummy_edge, dummy_node, dummy_port, port_type, parent_end_port.is_none()));
        } else if let Some(ep) = external_port {
            self.external_ports[ep.0 as usize].orig_edges.push(orig_edge);

            let new_edge = self.ext(ep).new_edge;
            let existing_thickness = lg[new_edge].props.get_typed::<f64>(&LayeredOptions::EDGE_THICKNESS).unwrap_or(0.0);
            let orig_thickness = lg[orig_edge].props.get_typed::<f64>(&LayeredOptions::EDGE_THICKNESS).unwrap_or(0.0);
            let thickness = swift::max(existing_thickness, orig_thickness);
            lg[new_edge].props.set(&LayeredOptions::EDGE_THICKNESS, thickness);
        }

        let result = match external_port {
            Some(ep) => ep,
            None => {
                let new_edge = lg.new_edge();
                let dummy_node = lg.new_node(None);
                let dummy_port = lg.new_port();
                self.new_external_port(orig_edge, new_edge, dummy_node, dummy_port, port_type, false)
            }
        };
        let result_new_edge = self.ext(result).new_edge;
        self.cross_hierarchy_map.append(orig_edge, CrossHierarchyEdge::new(result_new_edge, graph, port_type));

        result
    }

    /// Retrieves a dummy node to be used to represent a new external port of
    /// the parent node.
    fn create_external_port_dummy(&mut self, lg: &mut LGraphArena, graph: LGraphId, parent_node: LNodeId, port_type: PortType, port_side: PortSide, edge: LEdgeId) -> LNodeId {
        let dummy_node: LNodeId;

        let outside_port = if port_type == PortType::INPUT { lg[edge].source } else { lg[edge].target };
        let layout_direction = lg.get_direction(graph);

        match outside_port {
            Some(outside_port) if lg[outside_port].owner == Some(parent_node) => {
                if let Some(existing) = self.get_dummy_node(outside_port) {
                    dummy_node = existing;
                } else {
                    let pc = lg[parent_node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
                    let net_flow = calculate_net_flow(lg, outside_port);
                    let position = lg[outside_port].position;
                    let size = lg[outside_port].size;
                    dummy_node = create_external_port_dummy_for_port(lg, outside_port, pc, port_side, net_flow, KVector::default(), position, size, layout_direction, graph);
                    lg[dummy_node].props.set(&InternalProperties::ORIGIN, PropValue::LPort(outside_port));
                    self.set_dummy_node_tracked(dummy_node, outside_port);
                }
            }
            _ => {
                let pc = lg[parent_node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
                let mut holder = create_external_port_properties(lg, graph);
                dummy_node = lg.create_external_port_dummy(
                    &mut holder,
                    pc,
                    port_side,
                    if port_type == PortType::INPUT { -1 } else { 1 },
                    KVector::default(),
                    KVector::default(),
                    KVector::new(0.0, 0.0),
                    layout_direction,
                    graph,
                );
                let dummy_port = create_port_for_dummy(lg, dummy_node, parent_node, port_type);
                lg[dummy_node].props.set(&InternalProperties::ORIGIN, PropValue::LPort(dummy_port));
                self.set_dummy_node_tracked(dummy_node, dummy_port);
            }
        }

        insert_graph_properties(lg, graph, &[GraphProperties::EXTERNAL_PORTS]);

        let graph_pc = lg[graph].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
        if graph_pc.is_side_fixed() {
            lg[graph].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_SIDE);
        } else {
            lg[graph].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FREE);
        }

        dummy_node
    }
}

/// Determines the index of the shallowest edge segment.
fn get_shallowest_edge_segment(edge_segments: &[CrossHierarchyEdge]) -> i64 {
    let mut result: i64 = -1;
    let mut index: i64 = 0;

    for cross_hierarchy_edge in edge_segments {
        if cross_hierarchy_edge.get_type() == PortType::INPUT {
            result = if index == 0 { 0 } else { index - 1 };
            break;
        } else if index == edge_segments.len() as i64 - 1 {
            result = index;
        }
        index += 1;
    }

    result
}

/// Creates and initializes a new dummy edge for the given original
/// hierarchy-crossing edge.
fn create_dummy_edge(lg: &mut LGraphArena, _graph: LGraphId, orig_edge: LEdgeId) -> LEdgeId {
    let dummy_edge = lg.new_edge();
    let props = lg[orig_edge].props.clone();
    lg[dummy_edge].props.copy_properties(&props);
    lg[dummy_edge].props.remove(&LayeredOptions::JUNCTION_POINTS);
    dummy_edge
}

/// Count how many edges want the port to be an output port of the parent and
/// how many want it to be an input port.
fn calculate_net_flow(lg: &LGraphArena, port: LPortId) -> i64 {
    let Some(node) = lg[port].owner else { return 0 };
    let inside_self_loops_enabled = lg[node].props.get_typed::<bool>(&LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE).unwrap_or(false);

    let mut output_port_vote = 0;
    let mut input_port_vote = 0;

    for &outgoing_edge in &lg[port].outgoing_edges {
        let is_self_loop = lg.edge_is_self_loop(outgoing_edge);
        let is_inside_self_loop =
            is_self_loop && inside_self_loops_enabled && lg[outgoing_edge].props.get_typed::<bool>(&LayeredOptions::INSIDE_SELF_LOOPS_YO).unwrap_or(false);
        let Some(target_node) = lg.edge_target_node(outgoing_edge) else { continue };

        if is_self_loop && is_inside_self_loop {
            input_port_vote += 1;
        } else if is_self_loop && !is_inside_self_loop {
            output_port_vote += 1;
        } else if lg[target_node].graph.and_then(|g| lg[g].parent_node) == Some(node) {
            input_port_vote += 1;
        } else {
            output_port_vote += 1;
        }
    }

    for &incoming_edge in &lg[port].incoming_edges {
        let is_self_loop = lg.edge_is_self_loop(incoming_edge);
        let is_inside_self_loop =
            is_self_loop && inside_self_loops_enabled && lg[incoming_edge].props.get_typed::<bool>(&LayeredOptions::INSIDE_SELF_LOOPS_YO).unwrap_or(false);
        let Some(source_node) = lg.edge_source_node(incoming_edge) else { continue };

        if is_self_loop && is_inside_self_loop {
            output_port_vote += 1;
        } else if is_self_loop && !is_inside_self_loop {
            input_port_vote += 1;
        } else if lg[source_node].graph.and_then(|g| lg[g].parent_node) == Some(node) {
            output_port_vote += 1;
        } else {
            input_port_vote += 1;
        }
    }

    output_port_vote - input_port_vote
}

/// Create suitable port properties for dummy external ports.
fn create_external_port_properties(lg: &LGraphArena, graph: LGraphId) -> PropertyMap {
    let mut property_holder = PropertyMap::new();
    let offset = lg[graph].props.get_typed::<f64>(&LayeredOptions::SPACING_EDGE_EDGE).unwrap_or(10.0) / 2.0;
    property_holder.set(&LayeredOptions::PORT_BORDER_OFFSET, offset);
    property_holder
}

/// Create a port for an existing external port dummy node.
fn create_port_for_dummy(lg: &mut LGraphArena, dummy_node: LNodeId, parent_node: LNodeId, port_type: PortType) -> LPortId {
    let Some(graph) = lg[parent_node].graph else { return lg.new_port() };
    let layout_direction = lg.get_direction(graph);
    let port = lg.new_port();
    lg.port_set_node(port, Some(parent_node));
    match port_type {
        PortType::INPUT => lg.port_set_side(port, PortSide::from_direction(layout_direction).opposed()),
        PortType::OUTPUT => lg.port_set_side(port, PortSide::from_direction(layout_direction)),
        _ => {}
    }
    let border_offset = lg[dummy_node].props.get_typed::<f64>(&LayeredOptions::PORT_BORDER_OFFSET).unwrap_or(0.0);
    lg[port].props.set(&LayeredOptions::PORT_BORDER_OFFSET, border_offset);
    port
}
