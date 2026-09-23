//! Port of `alg/layered/graph/transform/ElkGraphLayoutTransferrer.swift`:
//! writes the layered graph's layout back into the ELK graph.
//!
//! Only the mutating `apply(_:)` path is used (`ElkGraphTransformer.applyLayout`);
//! `applyLayoutNonMutating` is not ported.

use crate::bridge::elk_graph_impl::{ElkGraph, ElkShape};
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LGraphId, LNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_padding::LPadding;
use crate::org::eclipse::elk::alg::layered::options::graph_properties::GraphProperties;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::node_flexibility::NodeFlexibility;
use crate::org::eclipse::elk::alg::layered::options::node_placement_strategy::NodePlacementStrategy;
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::org::eclipse::elk::core::math::k_vector::{KVector, KVectorRef};
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChainRef;
use crate::org::eclipse::elk::core::options::{
    edge_routing::EdgeRouting, node_label_placement::NodeLabelPlacement, port_constraints::PortConstraints,
    port_label_placement::PortLabelPlacement, size_constraint::SizeConstraint, size_options::SizeOptions,
};
use crate::org::eclipse::elk::core::util::elk_util::ElkUtil;
use crate::org::eclipse::elk::graph::properties::property::PropValue;

#[derive(Default)]
pub struct ElkGraphLayoutTransferrer;

impl ElkGraphLayoutTransferrer {
    pub fn new() -> ElkGraphLayoutTransferrer {
        ElkGraphLayoutTransferrer
    }

    /// `apply(_:)`.
    pub fn apply(&mut self, graph: &mut ElkGraph, lg: &mut LGraphArena, lgraph: LGraphId) {
        let Some(PropValue::ElkNode(parent_elk_node)) = lg[lgraph].props.get(&InternalProperties::ORIGIN) else { return };
        let parent_lnode = lg[lgraph].parent_node;

        let mut offset = lg[lgraph].offset;
        let l_padding = lg[lgraph].padding;
        offset.x += l_padding.left;
        offset.y += l_padding.top;

        let size_options = graph[parent_elk_node].props.get_as::<SizeOptions>(&LayeredOptions::NODE_SIZE_OPTIONS).unwrap_or_default();
        if size_options.contains(SizeOptions::COMPUTE_PADDING) {
            if let Some(padding) = graph[parent_elk_node].props.get_as::<std::rc::Rc<std::cell::RefCell<ElkPadding>>>(&LayeredOptions::PADDING) {
                let mut p = padding.borrow_mut();
                p.bottom = l_padding.bottom;
                p.top = l_padding.top;
                p.left = l_padding.left;
                p.right = l_padding.right;
            }
        }

        let mut edge_list: Vec<LEdgeId> = Vec::new();
        for lnode in lg[lgraph].layerless_nodes.clone() {
            if Self::represents_node(lg, lnode) {
                self.apply_node_layout(graph, lg, lnode, offset);
            } else if Self::represents_external_port(lg, lnode) && parent_lnode.is_none() {
                if let Some(PropValue::ElkPort(elkport)) = lg[lnode].props.get(&InternalProperties::ORIGIN) {
                    let (w, h) = (graph[elkport].width, graph[elkport].height);
                    let port_position = lg.get_external_port_position(lgraph, lnode, w, h);
                    graph[elkport].x = port_position.x;
                    graph[elkport].y = port_position.y;
                }
            }
            for &port in &lg[lnode].ports {
                for &edge in &lg[port].outgoing_edges {
                    let target_node = lg[edge].target.and_then(|t| lg[t].owner);
                    if !lg.is_descendant(target_node, Some(lnode)) {
                        edge_list.push(edge);
                    }
                }
            }
        }

        if let Some(parent_lnode) = parent_lnode {
            for &port in &lg[parent_lnode].ports {
                for &edge in &lg[port].outgoing_edges {
                    let target_node = lg[edge].target.and_then(|t| lg[t].owner);
                    if lg.is_descendant(target_node, Some(parent_lnode)) {
                        edge_list.push(edge);
                    }
                }
            }
        }

        let routing = graph[parent_elk_node].props.get_as::<EdgeRouting>(&LayeredOptions::EDGE_ROUTING).unwrap_or(EdgeRouting::UNDEFINED);
        for ledge in edge_list {
            self.apply_edge_layout(graph, lg, ledge, routing, offset, l_padding);
        }

        self.apply_parent_node_layout(graph, lg, lgraph);

        for lnode in lg[lgraph].layerless_nodes.clone() {
            if let Some(nested) = lg[lnode].nested_graph {
                self.apply(graph, lg, nested);
            }
        }
    }

    /// `applyNodeLayout(_:offset:)`.
    fn apply_node_layout(&mut self, graph: &mut ElkGraph, lg: &LGraphArena, lnode: LNodeId, offset: KVector) {
        let Some(PropValue::ElkNode(elknode)) = lg[lnode].props.get(&InternalProperties::ORIGIN) else { return };

        let node_id = lg[lnode].props.get(&LayeredOptions::CROSSING_MINIMIZATION_POSITION_ID);
        let layer_id = lg[lnode].props.get(&LayeredOptions::LAYERING_LAYER_ID);
        graph[elknode].props.set_opt(&LayeredOptions::CROSSING_MINIMIZATION_POSITION_ID, node_id);
        graph[elknode].props.set_opt(&LayeredOptions::LAYERING_LAYER_ID, layer_id);

        let position = lg[lnode].position;
        graph[elknode].x = position.x + offset.x;
        graph[elknode].y = position.y + offset.y;

        let size_constraints = graph[elknode].props.get_as::<SizeConstraint>(&LayeredOptions::NODE_SIZE_CONSTRAINTS).unwrap_or_default();
        let has_nested_graph = lg[lnode].nested_graph.is_some();
        let node_placement_strategy = lg.node_graph(lnode).and_then(|g| lg[g].props.get_as::<NodePlacementStrategy>(&LayeredOptions::NODE_PLACEMENT_STRATEGY));
        let node_flexibility = NodeFlexibility::get_node_flexibility(lg, lnode);
        let flexible = node_flexibility.is_flexible_size_where_space_permits();
        if !size_constraints.is_empty() || has_nested_graph || (node_placement_strategy == Some(NodePlacementStrategy::NETWORK_SIMPLEX) && flexible) {
            let size = lg[lnode].size;
            graph[elknode].width = size.x;
            graph[elknode].height = size.y;
        }

        for &lport in &lg[lnode].ports {
            if let Some(PropValue::ElkPort(origin)) = lg[lport].props.get(&InternalProperties::ORIGIN) {
                let position = lg[lport].position;
                graph[origin].x = position.x;
                graph[origin].y = position.y;
                graph[origin].props.set(&LayeredOptions::PORT_SIDE, lg[lport].side);
            }
        }

        let node_has_label_placement = !lg[lnode].props.get_as::<NodeLabelPlacement>(&LayeredOptions::NODE_LABELS_PLACEMENT).unwrap_or_default().is_empty();
        for &llabel in &lg[lnode].labels {
            if node_has_label_placement || !lg[llabel].props.get_as::<NodeLabelPlacement>(&LayeredOptions::NODE_LABELS_PLACEMENT).unwrap_or_default().is_empty() {
                if let Some(PropValue::ElkLabel(elklabel)) = lg[llabel].props.get(&InternalProperties::ORIGIN) {
                    let size = lg[llabel].size;
                    let position = lg[llabel].position;
                    let l = &mut graph[elklabel];
                    l.width = size.x;
                    l.height = size.y;
                    l.x = position.x;
                    l.y = position.y;
                }
            }
        }

        let port_label_placement = lg[lnode].props.get_as::<PortLabelPlacement>(&LayeredOptions::PORT_LABELS_PLACEMENT).unwrap_or_default();
        if !PortLabelPlacement::is_fixed(port_label_placement) {
            for &lport in &lg[lnode].ports {
                for &llabel in &lg[lport].labels {
                    if let Some(PropValue::ElkLabel(elklabel)) = lg[llabel].props.get(&InternalProperties::ORIGIN) {
                        let size = lg[llabel].size;
                        let position = lg[llabel].position;
                        let l = &mut graph[elklabel];
                        l.width = size.x;
                        l.height = size.y;
                        l.x = position.x;
                        l.y = position.y;
                    }
                }
            }
        }
    }

    /// `applyEdgeLayout(_:routing:offset:additionalPadding:)`. The Swift
    /// mutates the layered edge's own bend point chain and junction points;
    /// the layered graph is discarded afterwards, so the port works on copies.
    fn apply_edge_layout(&mut self, graph: &mut ElkGraph, lg: &mut LGraphArena, ledge: LEdgeId, routing: EdgeRouting, offset: KVector, _additional_padding: LPadding) {
        let Some(PropValue::ElkEdge(elkedge)) = lg[ledge].props.get(&InternalProperties::ORIGIN) else { return };

        let mut edge_offset = offset;
        edge_offset.add(self.calculate_hierarchical_offset(lg, ledge));

        let source_port = lg[ledge].source;
        let target_port = lg[ledge].target;
        let source_node = source_port.and_then(|p| lg[p].owner);
        let target_node = target_port.and_then(|p| lg[p].owner);

        let source_point = if lg.is_descendant(target_node, source_node) {
            let mut p = source_port.map_or(KVector::default(), |p| lg[p].position);
            p.add(source_port.map_or(KVector::default(), |p| lg[p].anchor));
            p.sub(offset);
            p
        } else {
            source_port.map_or(KVector::default(), |p| lg.port_absolute_anchor(p))
        };

        let mut bend_points = lg[ledge].bend_points.clone();
        bend_points.add_first(source_point);
        let mut target_point = target_port.map_or(KVector::default(), |p| lg.port_absolute_anchor(p));
        if let Some(target_offset) = lg[ledge].props.get_as::<KVectorRef>(&InternalProperties::TARGET_OFFSET) {
            target_point.add(*target_offset.borrow());
        }
        bend_points.add_last(target_point);
        bend_points.offset(edge_offset);
        lg[ledge].bend_points = bend_points.clone();

        let section = graph.first_edge_section(elkedge, true, true);
        let (src_shape, tgt_shape): (ElkShape, ElkShape) = (graph[elkedge].sources[0], graph[elkedge].targets[0]);
        graph[section].incoming_shape = Some(src_shape);
        graph[section].outgoing_shape = Some(tgt_shape);
        ElkUtil::apply_vector_chain(graph, &bend_points, section);

        for &llabel in &lg[ledge].labels {
            if let Some(PropValue::ElkLabel(elklabel)) = lg[llabel].props.get(&InternalProperties::ORIGIN) {
                let size = lg[llabel].size;
                let position = lg[llabel].position;
                let l = &mut graph[elklabel];
                l.width = size.x;
                l.height = size.y;
                l.x = position.x + edge_offset.x;
                l.y = position.y + edge_offset.y;
            }
        }

        // `var junctionPointsCopy = junctionPoints` is the same object: it is
        // offset in place and then shared with the ELK edge.
        if let Some(junction_points) = lg[ledge].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS) {
            junction_points.borrow_mut().offset(edge_offset);
            graph[elkedge].props.set(&LayeredOptions::JUNCTION_POINTS, junction_points);
        } else {
            graph[elkedge].props.set_opt(&LayeredOptions::JUNCTION_POINTS, None);
        }

        if routing == EdgeRouting::SPLINES {
            graph[elkedge].props.set(&LayeredOptions::EDGE_ROUTING, EdgeRouting::SPLINES);
        } else {
            graph[elkedge].props.set_opt(&LayeredOptions::EDGE_ROUTING, None);
        }
    }

    /// `calculateHierarchicalOffset(_:)`.
    fn calculate_hierarchical_offset(&self, lg: &LGraphArena, ledge: LEdgeId) -> KVector {
        let Some(PropValue::LGraph(target_coordinate_system)) = lg[ledge].props.get(&InternalProperties::COORDINATE_SYSTEM_ORIGIN) else {
            return KVector::default();
        };
        let mut result = KVector::new(0.0, 0.0);
        let mut current_graph = lg[ledge].source.and_then(|p| lg[p].owner).and_then(|n| lg[n].graph);
        while let Some(cg) = current_graph {
            if cg == target_coordinate_system {
                break;
            }
            let Some(representing_node) = lg[cg].parent_node else { break };
            current_graph = lg[representing_node].graph;
            result.add(lg[representing_node].position);
            if let Some(g) = current_graph {
                result.add(lg[g].offset);
                result.add_xy(lg[g].padding.left, lg[g].padding.top);
            }
        }
        result
    }

    /// `applyParentNodeLayout(_:)`.
    fn apply_parent_node_layout(&mut self, graph: &mut ElkGraph, lg: &LGraphArena, lgraph: LGraphId) {
        let Some(PropValue::ElkNode(elknode)) = lg[lgraph].props.get(&InternalProperties::ORIGIN) else { return };
        let size_constraints = graph[elknode].props.get_as::<SizeConstraint>(&LayeredOptions::NODE_SIZE_CONSTRAINTS).unwrap_or_default();
        let included_port_labels = size_constraints.contains(SizeConstraint::PORT_LABELS);

        if lg[lgraph].parent_node.is_none() {
            let graph_props = lg[lgraph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
            let actual = lg.graph_actual_size(lgraph);
            if graph_props.contains(GraphProperties::EXTERNAL_PORTS) {
                graph[elknode].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
                ElkUtil::resize_node(graph, elknode, actual.x, actual.y, false, true);
            } else if !graph[elknode].props.get_as::<bool>(&LayeredOptions::NODE_SIZE_FIXED_GRAPH_SIZE).unwrap_or(false) {
                ElkUtil::resize_node(graph, elknode, actual.x, actual.y, true, true);
            }
        }

        if included_port_labels {
            graph[elknode].props.set(&LayeredOptions::NODE_SIZE_CONSTRAINTS, SizeConstraint::PORT_LABELS);
        } else {
            graph[elknode].props.set(&LayeredOptions::NODE_SIZE_CONSTRAINTS, SizeConstraint::fixed());
        }
    }

    /// `representsNode(_:)`.
    pub fn represents_node(lg: &LGraphArena, lnode: LNodeId) -> bool {
        matches!(lg[lnode].props.get_stored(&InternalProperties::ORIGIN), Some(PropValue::ElkNode(_)))
    }

    /// `representsExternalPort(_:)`.
    pub fn represents_external_port(lg: &LGraphArena, lnode: LNodeId) -> bool {
        matches!(lg[lnode].props.get_stored(&InternalProperties::ORIGIN), Some(PropValue::ElkPort(_)))
    }
}
