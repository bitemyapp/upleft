//! Port of `alg/layered/graph/transform/ElkGraphImporter.swift`: ELK graph →
//! layered graph.
//!
//! `ElkGraphAdapters.adapt(...)` returns `nil` in elk-swift, so the node label
//! padding and minimum graph size computations guarded by it never run; the
//! statements before those guards do.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::bridge::elk::ElkError;
use crate::bridge::elk_graph_impl::{ElkEdgeId, ElkGraph, ElkLabelId, ElkNodeId, ElkPortId, ElkShape};
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::components::component_ordering_strategy::ComponentOrderingStrategy;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LGraphId, LLabelId, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::layered_spacings::LayeredSpacings;
use crate::org::eclipse::elk::alg::layered::options::{
    crossing_minimization_strategy::CrossingMinimizationStrategy, cycle_breaking_strategy::CycleBreakingStrategy,
    graph_properties::GraphProperties, layering_strategy::LayeringStrategy,
    node_placement_strategy::NodePlacementStrategy, node_promotion_strategy::NodePromotionStrategy,
    ordering_strategy::OrderingStrategy, port_type::PortType,
};
use crate::org::eclipse::elk::alg::layered::graph_configurator::LabelManagementOptions;
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::org::eclipse::elk::core::math::k_vector::{KVector, KVectorRef};
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChain;
use crate::org::eclipse::elk::core::options::core_options as CoreOptions;
use crate::org::eclipse::elk::core::options::{
    direction::Direction, edge_label_placement::EdgeLabelPlacement, hierarchy_handling::HierarchyHandling,
    port_constraints::PortConstraints, port_label_placement::PortLabelPlacement, port_side::PortSide,
    size_constraint::SizeConstraint,
};
use crate::org::eclipse::elk::core::util::elk_util::ElkUtil;
use crate::org::eclipse::elk::graph::properties::property::PropValue;

/// What `nodeAndPortMap` maps an ELK node or port to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mapped {
    Node(LNodeId),
    Port(LPortId),
}

#[derive(Default)]
pub struct ElkGraphImporter {
    node_and_port_map: HashMap<ElkShape, Mapped>,
}

fn bool_prop(props: &crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap, p: &crate::org::eclipse::elk::graph::properties::property::Property) -> bool {
    props.get_as::<bool>(p).unwrap_or(false)
}

impl ElkGraphImporter {
    pub fn new() -> ElkGraphImporter {
        ElkGraphImporter::default()
    }

    fn mapped_node(&self, n: ElkNodeId) -> Option<LNodeId> {
        match self.node_and_port_map.get(&ElkShape::Node(n)) {
            Some(Mapped::Node(l)) => Some(*l),
            _ => None,
        }
    }

    /// `importGraph(_:)`.
    pub fn import_graph(&mut self, graph: &mut ElkGraph, lg: &mut LGraphArena, elkgraph: ElkNodeId) -> Result<LGraphId, ElkError> {
        let top_level_graph = self.create_lgraph(graph, lg, elkgraph);

        for elkport in graph[elkgraph].ports.clone() {
            self.ensure_defined_port_side(graph, lg, top_level_graph, elkport);
        }

        let mut graph_properties = lg[top_level_graph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
        self.check_external_ports(graph, elkgraph, &mut graph_properties);

        if graph_properties.contains(GraphProperties::EXTERNAL_PORTS) {
            for elkport in graph[elkgraph].ports.clone() {
                self.transform_external_port(graph, lg, elkgraph, top_level_graph, elkport);
            }
        }

        if self.should_calculate_minimum_graph_size(graph, elkgraph) {
            self.calculate_minimum_graph_size(graph, lg, elkgraph, top_level_graph);
        }

        if bool_prop(&lg[top_level_graph].props, &LayeredOptions::PARTITIONING_ACTIVATE) {
            graph_properties.insert(GraphProperties::PARTITIONS);
        }
        lg[top_level_graph].props.set(&InternalProperties::GRAPH_PROPERTIES, graph_properties);

        if lg[top_level_graph].props.has(&LayeredOptions::SPACING_BASE_VALUE) {
            let base_value = lg[top_level_graph].props.get_as::<f64>(&LayeredOptions::SPACING_BASE_VALUE).unwrap_or(0.0);
            LayeredSpacings::with_base_value(base_value).apply(lg, top_level_graph);
        }

        if graph[elkgraph].props.get_as::<HierarchyHandling>(&LayeredOptions::HIERARCHY_HANDLING) == Some(HierarchyHandling::INCLUDE_CHILDREN) {
            self.import_hierarchical_graph(graph, lg, elkgraph, top_level_graph)?;
        } else {
            self.import_flat_graph(graph, lg, elkgraph, top_level_graph)?;
        }
        Ok(top_level_graph)
    }

    /// `ensureDefinedPortSide(_:_:)`.
    pub fn ensure_defined_port_side(&mut self, graph: &mut ElkGraph, lg: &LGraphArena, lgraph: LGraphId, elkport: ElkPortId) {
        let mut layout_direction = lg[lgraph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
        if layout_direction == Direction::UNDEFINED {
            layout_direction = Direction::RIGHT;
        }
        let mut port_side = graph[elkport].props.get_as::<PortSide>(&LayeredOptions::PORT_SIDE).unwrap_or(PortSide::UNDEFINED);
        let port_constraints = lg[lgraph].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED);
        if !port_constraints.is_side_fixed() {
            let net_flow = self.calculate_net_flow(graph, elkport);
            if net_flow > 0 {
                port_side = PortSide::from_direction(layout_direction);
            } else {
                port_side = PortSide::from_direction(layout_direction).opposed();
            }
        } else if port_side == PortSide::UNDEFINED {
            port_side = ElkUtil::calc_port_side(graph, elkport, layout_direction);
            if port_side == PortSide::UNDEFINED {
                port_side = PortSide::from_direction(layout_direction);
            }
        }
        graph[elkport].props.set(&LayeredOptions::PORT_SIDE, port_side);
    }

    fn should_calculate_minimum_graph_size(&self, graph: &ElkGraph, elkgraph: ElkNodeId) -> bool {
        !graph[elkgraph].props.get_as::<SizeConstraint>(&LayeredOptions::NODE_SIZE_CONSTRAINTS).unwrap_or_default().is_empty()
    }

    /// `calculateMinimumGraphSize(_:_:)`: everything after the adapter guard
    /// is dead in elk-swift.
    fn calculate_minimum_graph_size(&mut self, graph: &mut ElkGraph, _lg: &mut LGraphArena, elkgraph: ElkNodeId, _lgraph: LGraphId) {
        if graph[elkgraph].parent.is_none() {
            return;
        }
        if graph[elkgraph].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS) == Some(PortConstraints::UNDEFINED) {
            graph[elkgraph].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FREE);
        }
        // `ElkGraphAdapters.adapt(parent)` is nil: return.
    }

    /// `importFlatGraph(_:_:)`.
    fn import_flat_graph(&mut self, graph: &mut ElkGraph, lg: &mut LGraphArena, elkgraph: ElkNodeId, lgraph: LGraphId) -> Result<(), ElkError> {
        let mut index: i64 = 0;
        let mut cb_group_model_orders: HashSet<i64> = HashSet::new();

        for child in graph[elkgraph].children.clone() {
            if !bool_prop(&graph[child].props, &LayeredOptions::NO_LAYOUT) {
                if self.needs_model_order(graph, child) {
                    graph[child].props.set(&InternalProperties::MODEL_ORDER, index);
                    index += 1;
                    if graph[child].props.has(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CYCLE_BREAKING_ID) {
                        cb_group_model_orders.insert(graph[child].props.get_as::<i64>(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CYCLE_BREAKING_ID).unwrap_or(0));
                    }
                }
                self.transform_node(graph, lg, child, lgraph);
            }
        }

        lg[lgraph].props.set(&InternalProperties::MAX_MODEL_ORDER_NODES, index);
        lg[lgraph].props.set(&InternalProperties::CB_NUM_MODEL_ORDER_GROUPS, cb_group_model_orders.len() as i64);

        index = 0;
        for elkedge in graph[elkgraph].contained_edges.clone() {
            if self.needs_model_order_based_on_parent(graph, elkgraph) {
                graph[elkedge].props.set(&InternalProperties::MODEL_ORDER, index);
                index += 1;
            }
            let source = graph.connectable_shape_to_node(graph[elkedge].sources[0]);
            let target = graph.connectable_shape_to_node(graph[elkedge].targets[0]);

            let enable_inside_self_loops = bool_prop(&graph[source].props, &LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE);
            let is_to_be_laid_out = !bool_prop(&graph[elkedge].props, &LayeredOptions::NO_LAYOUT);
            let is_inside_self_loop =
                enable_inside_self_loops && graph.edge_is_selfloop(elkedge) && bool_prop(&graph[elkedge].props, &LayeredOptions::INSIDE_SELF_LOOPS_YO);
            let connects_siblings = graph[source].parent == Some(elkgraph) && graph[target].parent == Some(elkgraph);
            let connects_to_graph = (graph[source].parent == Some(elkgraph) && target == elkgraph)
                != (graph[target].parent == Some(elkgraph) && source == elkgraph);

            if is_to_be_laid_out && !is_inside_self_loop && (connects_to_graph || connects_siblings) {
                self.transform_edge(graph, lg, elkedge, elkgraph, lgraph)?;
            }
        }

        if let Some(parent) = graph[elkgraph].parent {
            for elkedge in graph[parent].contained_edges.clone() {
                let source = graph.connectable_shape_to_node(graph[elkedge].sources[0]);
                if source == elkgraph && graph.edge_is_selfloop(elkedge) {
                    let is_inside_self_loop = bool_prop(&graph[source].props, &LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE)
                        && bool_prop(&graph[elkedge].props, &LayeredOptions::INSIDE_SELF_LOOPS_YO);
                    if is_inside_self_loop {
                        self.transform_edge(graph, lg, elkedge, elkgraph, lgraph)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn uses_elk_layered(graph: &ElkGraph, node: ElkNodeId) -> bool {
        !graph[node].props.has(&CoreOptions::ALGORITHM)
            || LayeredOptions::ALGORITHM_ID.ends_with(graph[node].props.get_as::<String>(&CoreOptions::ALGORITHM).unwrap_or_default().as_str())
    }

    /// `importHierarchicalGraph(_:_:)`.
    fn import_hierarchical_graph(&mut self, graph: &mut ElkGraph, lg: &mut LGraphArena, elkgraph: ElkNodeId, lgraph: LGraphId) -> Result<(), ElkError> {
        let mut queue: VecDeque<ElkNodeId> = VecDeque::new();
        let parent_graph_direction = lg[lgraph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);

        let mut index: i64 = 0;
        let mut cb_group_model_orders: HashSet<i64> = HashSet::new();

        queue.extend(graph[elkgraph].children.iter().copied());
        while let Some(elknode) = queue.pop_front() {
            if self.needs_model_order(graph, elknode) {
                graph[elknode].props.set(&InternalProperties::MODEL_ORDER, index);
                index += 1;
                if graph[elknode].props.has(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CYCLE_BREAKING_ID) {
                    cb_group_model_orders.insert(graph[elknode].props.get_as::<i64>(&LayeredOptions::CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CYCLE_BREAKING_ID).unwrap_or(0));
                }
            }

            let is_node_to_be_laid_out = !bool_prop(&graph[elknode].props, &LayeredOptions::NO_LAYOUT);
            if is_node_to_be_laid_out {
                let has_children = !graph[elknode].children.is_empty();
                let has_inside_self_loops = self.has_inside_self_loops(graph, elknode);
                let has_hierarchy_handling_enabled =
                    graph[elknode].props.get_as::<HierarchyHandling>(&LayeredOptions::HIERARCHY_HANDLING) == Some(HierarchyHandling::INCLUDE_CHILDREN);
                let uses_elk_layered = Self::uses_elk_layered(graph, elknode);

                let mut nested_graph: Option<LGraphId> = None;
                if uses_elk_layered && has_hierarchy_handling_enabled && (has_children || has_inside_self_loops) {
                    let ng = self.create_lgraph(graph, lg, elknode);
                    nested_graph = Some(ng);
                    // elk-swift: only inherit the parent's direction without an
                    // explicit one of its own.
                    let nested_dir = lg[ng].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
                    if nested_dir == Direction::UNDEFINED {
                        lg[ng].props.set(&LayeredOptions::DIRECTION, parent_graph_direction);
                    }
                    if lg[ng].props.has(&LayeredOptions::SPACING_BASE_VALUE) {
                        let base_value = lg[ng].props.get_as::<f64>(&LayeredOptions::SPACING_BASE_VALUE).unwrap_or(0.0);
                        LayeredSpacings::with_base_value(base_value).apply(lg, ng);
                    }
                    if self.should_calculate_minimum_graph_size(graph, elknode) {
                        for elkport in graph[elknode].ports.clone() {
                            self.ensure_defined_port_side(graph, lg, ng, elkport);
                        }
                        self.calculate_minimum_graph_size(graph, lg, elknode, ng);
                    }
                }

                let mut parent_lgraph = lgraph;
                if let Some(parent) = graph[elknode].parent {
                    if let Some(parent_lnode) = self.mapped_node(parent) {
                        if let Some(parent_nested) = lg[parent_lnode].nested_graph {
                            parent_lgraph = parent_nested;
                        }
                    }
                }

                let lnode = self.transform_node(graph, lg, elknode, parent_lgraph);

                if let Some(ng) = nested_graph {
                    lg[lnode].nested_graph = Some(ng);
                    lg[ng].parent_node = Some(lnode);
                    queue.extend(graph[elknode].children.iter().copied());
                }
            }
        }

        lg[lgraph].props.set(&InternalProperties::MAX_MODEL_ORDER_NODES, index);
        lg[lgraph].props.set(&InternalProperties::CB_NUM_MODEL_ORDER_GROUPS, cb_group_model_orders.len() as i64);

        index = 0;
        queue.push_back(elkgraph);
        while let Some(elk_graph_node) = queue.pop_front() {
            for elkedge in graph[elk_graph_node].contained_edges.clone() {
                Self::check_edge_validity(graph, elkedge)?;

                if self.needs_model_order_based_on_parent(graph, elkgraph) {
                    graph[elkedge].props.set(&InternalProperties::MODEL_ORDER, index);
                    index += 1;
                }

                let source_node = graph.connectable_shape_to_node(graph[elkedge].sources[0]);
                let target_node = graph.connectable_shape_to_node(graph[elkedge].targets[0]);

                if bool_prop(&graph[elkedge].props, &LayeredOptions::NO_LAYOUT)
                    || bool_prop(&graph[source_node].props, &LayeredOptions::NO_LAYOUT)
                    || bool_prop(&graph[target_node].props, &LayeredOptions::NO_LAYOUT)
                {
                    continue;
                }

                let is_inside_self_loop = graph.edge_is_selfloop(elkedge)
                    && bool_prop(&graph[source_node].props, &LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE)
                    && bool_prop(&graph[elkedge].props, &LayeredOptions::INSIDE_SELF_LOOPS_YO);

                let mut parent_elk_graph = elk_graph_node;
                if is_inside_self_loop || graph.is_descendant(target_node, source_node) {
                    parent_elk_graph = source_node;
                } else if graph.is_descendant(source_node, target_node) {
                    parent_elk_graph = target_node;
                }

                let mut parent_lgraph = lgraph;
                if let Some(parent_lnode) = self.mapped_node(parent_elk_graph) {
                    if let Some(parent_nested) = lg[parent_lnode].nested_graph {
                        parent_lgraph = parent_nested;
                    }
                }

                let ledge = self.transform_edge(graph, lg, elkedge, parent_elk_graph, parent_lgraph)?;
                let origin = self.find_coordinate_system_origin(graph, lg, elkedge, elkgraph, lgraph);
                lg[ledge].props.set_opt(&InternalProperties::COORDINATE_SYSTEM_ORIGIN, origin.map(PropValue::LGraph));
            }

            let enabled = graph[elk_graph_node].props.get_as::<HierarchyHandling>(&LayeredOptions::HIERARCHY_HANDLING) == Some(HierarchyHandling::INCLUDE_CHILDREN);
            if enabled {
                for child in graph[elk_graph_node].children.clone() {
                    let uses_elk_layered = Self::uses_elk_layered(graph, child);
                    let part_of_same_layout_run =
                        graph[child].props.get_as::<HierarchyHandling>(&LayeredOptions::HIERARCHY_HANDLING) == Some(HierarchyHandling::INCLUDE_CHILDREN);
                    if uses_elk_layered && part_of_same_layout_run {
                        queue.push_back(child);
                    }
                }
            }
        }
        Ok(())
    }

    /// `needsModelOrder(_:)`.
    fn needs_model_order(&self, graph: &ElkGraph, child: ElkNodeId) -> bool {
        let Some(elkgraph) = graph[child].parent else { return false };
        self.needs_model_order_based_on_parent(graph, elkgraph) && !bool_prop(&graph[child].props, &LayeredOptions::CONSIDER_MODEL_ORDER_NO_MODEL_ORDER)
    }

    /// `needsModelOrderBasedOnParent(_:)`.
    fn needs_model_order_based_on_parent(&self, graph: &ElkGraph, elkgraph: ElkNodeId) -> bool {
        let props = &graph[elkgraph].props;
        let cb = props.get_as::<CycleBreakingStrategy>(&LayeredOptions::CYCLE_BREAKING_STRATEGY).unwrap_or(CycleBreakingStrategy::GREEDY);
        let model_order_cycle_breaking = matches!(
            cb,
            CycleBreakingStrategy::MODEL_ORDER
                | CycleBreakingStrategy::BFS_NODE_ORDER
                | CycleBreakingStrategy::DFS_NODE_ORDER
                | CycleBreakingStrategy::GREEDY_MODEL_ORDER
                | CycleBreakingStrategy::SCC_CONNECTIVITY
                | CycleBreakingStrategy::SCC_NODE_TYPE
        );
        let layering = props.get_as::<LayeringStrategy>(&LayeredOptions::LAYERING_STRATEGY).unwrap_or(LayeringStrategy::NETWORK_SIMPLEX);
        let promotion = props.get_as::<NodePromotionStrategy>(&LayeredOptions::LAYERING_NODE_PROMOTION_STRATEGY).unwrap_or(NodePromotionStrategy::NONE);
        let model_order_layering = layering == LayeringStrategy::BF_MODEL_ORDER
            || layering == LayeringStrategy::DF_MODEL_ORDER
            || promotion == NodePromotionStrategy::MODEL_ORDER_LEFT_TO_RIGHT
            || promotion == NodePromotionStrategy::MODEL_ORDER_RIGHT_TO_LEFT;
        let model_order_crossing_minimization = props.get_as::<OrderingStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_STRATEGY).unwrap_or(OrderingStrategy::NONE) != OrderingStrategy::NONE
            || bool_prop(props, &LayeredOptions::CROSSING_MINIMIZATION_FORCE_NODE_MODEL_ORDER)
            || props.get_as::<ComponentOrderingStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_COMPONENTS).unwrap_or(ComponentOrderingStrategy::NONE) != ComponentOrderingStrategy::NONE
            || props.get_as::<i64>(&LayeredOptions::CONSIDER_MODEL_ORDER_CROSSING_COUNTER_NODE_INFLUENCE).unwrap_or(0) != 0
            || props.get_as::<i64>(&LayeredOptions::CONSIDER_MODEL_ORDER_CROSSING_COUNTER_PORT_INFLUENCE).unwrap_or(0) != 0;
        model_order_cycle_breaking || model_order_layering || model_order_crossing_minimization
    }

    fn has_inside_self_loops(&self, graph: &ElkGraph, elknode: ElkNodeId) -> bool {
        if bool_prop(&graph[elknode].props, &LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE) {
            for edge in graph.all_outgoing_edges(elknode) {
                if graph.edge_is_selfloop(edge) && bool_prop(&graph[edge].props, &LayeredOptions::INSIDE_SELF_LOOPS_YO) {
                    return true;
                }
            }
        }
        false
    }

    /// `findCoordinateSystemOrigin(_:_:_:)`.
    fn find_coordinate_system_origin(&self, graph: &ElkGraph, lg: &LGraphArena, elkedge: ElkEdgeId, top_level_elk_graph: ElkNodeId, top_level_lgraph: LGraphId) -> Option<LGraphId> {
        let source = graph.connectable_shape_to_node(graph[elkedge].sources[0]);
        let target = graph.connectable_shape_to_node(graph[elkedge].targets[0]);
        if graph[source].parent == graph[target].parent {
            return None;
        }
        if graph.is_descendant(target, source) {
            return None;
        }
        let origin = graph[elkedge].containing_node?;
        if origin == top_level_elk_graph {
            Some(top_level_lgraph)
        } else {
            self.mapped_node(origin).and_then(|lnode| lg[lnode].nested_graph)
        }
    }

    /// `createLGraph(_:)`.
    fn create_lgraph(&mut self, graph: &ElkGraph, lg: &mut LGraphArena, elkgraph: ElkNodeId) -> LGraphId {
        let lgraph = lg.new_graph();
        lg[lgraph].props.copy_properties(&graph[elkgraph].props);

        if lg[lgraph].props.get_as::<Direction>(&LayeredOptions::DIRECTION) == Some(Direction::UNDEFINED) {
            let d = lg.get_direction(lgraph);
            lg[lgraph].props.set(&LayeredOptions::DIRECTION, d);
        }

        if lg[lgraph].props.get(&LabelManagementOptions::LABEL_MANAGER).is_none() {
            // The root container's label manager (never set): `setProperty(_, nil)`.
            let mut root = elkgraph;
            while let Some(p) = graph[root].parent {
                root = p;
            }
            let manager = graph[root].props.get(&LabelManagementOptions::LABEL_MANAGER);
            lg[lgraph].props.set_opt(&LabelManagementOptions::LABEL_MANAGER, manager);
        }

        lg[lgraph].props.set(&InternalProperties::ORIGIN, PropValue::ElkNode(elkgraph));
        lg[lgraph].props.set(&InternalProperties::GRAPH_PROPERTIES, EnumSet::<GraphProperties>::new());

        let node_padding: ElkPadding = lg[lgraph]
            .props
            .get_as::<std::rc::Rc<std::cell::RefCell<ElkPadding>>>(&LayeredOptions::PADDING)
            .map(|p| *p.borrow())
            .unwrap_or_default();
        lg[lgraph].padding.add(&node_padding);
        // The inside-node-label padding block is guarded by
        // `ElkGraphAdapters.adapt(...)`, which is nil.
        lgraph
    }

    /// `checkExternalPorts(_:_:)`.
    fn check_external_ports(&self, graph: &ElkGraph, elkgraph: ElkNodeId, graph_properties: &mut EnumSet<GraphProperties>) {
        let enable_self_loops = bool_prop(&graph[elkgraph].props, &LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE);
        let port_label_placement = graph[elkgraph].props.get_as::<PortLabelPlacement>(&LayeredOptions::PORT_LABELS_PLACEMENT).unwrap_or_default();

        let mut has_external_ports = false;
        let mut has_hyperedges = false;

        for &elkport in &graph[elkgraph].ports {
            let mut external_port_edges = 0;
            let incident: Vec<ElkEdgeId> = graph[elkport].incoming_edges.iter().chain(graph[elkport].outgoing_edges.iter()).copied().collect();
            for elkedge in incident {
                let is_inside_self_loop = enable_self_loops && graph.edge_is_selfloop(elkedge) && bool_prop(&graph[elkedge].props, &LayeredOptions::INSIDE_SELF_LOOPS_YO);
                let connects_to_child = if graph[elkedge].sources.contains(&ElkShape::Port(elkport)) {
                    Some(elkgraph) == graph[graph.connectable_shape_to_node(graph[elkedge].targets[0])].parent
                } else {
                    Some(elkgraph) == graph[graph.connectable_shape_to_node(graph[elkedge].sources[0])].parent
                };
                if is_inside_self_loop || connects_to_child {
                    external_port_edges += 1;
                    if external_port_edges > 1 {
                        break;
                    }
                }
            }
            if external_port_edges > 0 {
                has_external_ports = true;
            } else if port_label_placement.contains(PortLabelPlacement::INSIDE) && !graph[elkport].labels.is_empty() {
                has_external_ports = true;
            }
            if external_port_edges > 1 {
                has_hyperedges = true;
            }
        }
        if has_external_ports {
            graph_properties.insert(GraphProperties::EXTERNAL_PORTS);
        }
        if has_hyperedges {
            graph_properties.insert(GraphProperties::HYPEREDGES);
        }
    }

    /// `transformExternalPort(_:_:_:)`.
    fn transform_external_port(&mut self, graph: &mut ElkGraph, lg: &mut LGraphArena, elkgraph: ElkNodeId, lgraph: LGraphId, elkport: ElkPortId) {
        let p = &graph[elkport];
        let elkport_position = KVector::new(p.x + p.width / 2.0, p.y + p.height / 2.0);
        let net_flow = self.calculate_net_flow(graph, elkport);
        let port_constraints = graph[elkgraph].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED);

        let mut port_side = graph[elkport].props.get_as::<PortSide>(&LayeredOptions::PORT_SIDE).unwrap_or(PortSide::UNDEFINED);
        if port_side == PortSide::UNDEFINED {
            let direction = lg[lgraph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::RIGHT);
            port_side = ElkUtil::calc_port_side(graph, elkport, direction);
            if port_side == PortSide::UNDEFINED {
                port_side = PortSide::from_direction(direction);
            }
            graph[elkport].props.set(&LayeredOptions::PORT_SIDE, port_side);
        }

        if !graph[elkport].props.has(&LayeredOptions::PORT_BORDER_OFFSET) {
            let port_offset = if graph[elkport].x == 0.0 && graph[elkport].y == 0.0 { 0.0 } else { ElkUtil::calc_port_offset(graph, elkport, port_side) };
            graph[elkport].props.set(&LayeredOptions::PORT_BORDER_OFFSET, port_offset);
        }

        let graph_size = KVector::new(graph[elkgraph].width, graph[elkgraph].height);
        let port_size = KVector::new(graph[elkport].width, graph[elkport].height);
        let direction = lg[lgraph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::RIGHT);
        let dummy = lg.create_external_port_dummy(
            &mut graph[elkport].props,
            port_constraints,
            port_side,
            net_flow,
            graph_size,
            elkport_position,
            port_size,
            direction,
            lgraph,
        );
        lg[dummy].props.set(&InternalProperties::ORIGIN, PropValue::ElkPort(elkport));

        let dummy_port = lg[dummy].ports[0];
        lg[dummy_port].connected_to_external_nodes = self.is_connected_to_external_nodes(graph, elkport);
        lg[dummy].props.set(&LayeredOptions::PORT_LABELS_PLACEMENT, PortLabelPlacement::OUTSIDE);

        let graph_port_label_placement = graph[elkgraph].props.get_as::<PortLabelPlacement>(&LayeredOptions::PORT_LABELS_PLACEMENT);
        let inside_port_labels = graph_port_label_placement.unwrap_or_default().contains(PortLabelPlacement::INSIDE);

        for elklabel in graph[elkport].labels.clone() {
            if !bool_prop(&graph[elklabel].props, &LayeredOptions::NO_LAYOUT) && !graph[elklabel].text.is_empty() {
                let llabel = self.transform_label(graph, lg, elklabel);
                lg[dummy_port].labels.push(llabel);
                if !inside_port_labels {
                    let mut inside_part = 0.0;
                    if PortLabelPlacement::is_fixed(graph_port_label_placement.unwrap_or_default()) {
                        let l = &graph[elklabel];
                        inside_part = ElkUtil::compute_inside_part(
                            KVector::new(l.x, l.y),
                            KVector::new(l.width, l.height),
                            KVector::new(graph[elkport].width, graph[elkport].height),
                            0.0,
                            port_side,
                        );
                    }
                    match port_side {
                        PortSide::EAST | PortSide::WEST => lg[llabel].size.x = inside_part,
                        PortSide::NORTH | PortSide::SOUTH => lg[llabel].size.y = inside_part,
                        _ => {}
                    }
                }
            }
        }

        if let Some(parent) = graph[elkgraph].parent {
            let props = &graph[parent].props;
            let h = props.get(&LayeredOptions::SPACING_LABEL_PORT_HORIZONTAL);
            let v = props.get(&LayeredOptions::SPACING_LABEL_PORT_VERTICAL);
            let ll = props.get(&LayeredOptions::SPACING_LABEL_LABEL);
            lg[dummy].props.set_opt(&LayeredOptions::SPACING_LABEL_PORT_HORIZONTAL, h);
            lg[dummy].props.set_opt(&LayeredOptions::SPACING_LABEL_PORT_VERTICAL, v);
            lg[dummy].props.set_opt(&LayeredOptions::SPACING_LABEL_LABEL, ll);
        }

        lg[lgraph].layerless_nodes.push(dummy);
        self.node_and_port_map.insert(ElkShape::Port(elkport), Mapped::Node(dummy));
    }

    /// `calculateNetFlow(_:)`.
    fn calculate_net_flow(&self, graph: &ElkGraph, elkport: ElkPortId) -> i64 {
        let Some(elkgraph) = graph[elkport].parent else { return 0 };
        let inside_self_loops_enabled = bool_prop(&graph[elkgraph].props, &LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE);
        let mut output_port_vote = 0;
        let mut input_port_vote = 0;

        for &outgoing in &graph[elkport].outgoing_edges {
            let is_self_loop = graph.edge_is_selfloop(outgoing);
            let is_inside_self_loop = is_self_loop && inside_self_loops_enabled && bool_prop(&graph[outgoing].props, &LayeredOptions::INSIDE_SELF_LOOPS_YO);
            let target_node = graph.connectable_shape_to_node(graph[outgoing].targets[0]);
            if is_self_loop && is_inside_self_loop {
                input_port_vote += 1;
            } else if is_self_loop && !is_inside_self_loop {
                output_port_vote += 1;
            } else if graph[target_node].parent == Some(elkgraph) || target_node == elkgraph {
                input_port_vote += 1;
            } else {
                output_port_vote += 1;
            }
        }

        for &incoming in &graph[elkport].incoming_edges {
            let is_self_loop = graph.edge_is_selfloop(incoming);
            let is_inside_self_loop = is_self_loop && inside_self_loops_enabled && bool_prop(&graph[incoming].props, &LayeredOptions::INSIDE_SELF_LOOPS_YO);
            let source_node = graph.connectable_shape_to_node(graph[incoming].sources[0]);
            if is_self_loop && is_inside_self_loop {
                output_port_vote += 1;
            } else if is_self_loop && !is_inside_self_loop {
                input_port_vote += 1;
            } else if graph[source_node].parent == Some(elkgraph) || source_node == elkgraph {
                output_port_vote += 1;
            } else {
                input_port_vote += 1;
            }
        }
        output_port_vote - input_port_vote
    }

    /// `isConnectedToExternalNodes(_:)`.
    fn is_connected_to_external_nodes(&self, graph: &ElkGraph, elkport: ElkPortId) -> bool {
        let Some(parent) = graph[elkport].parent else { return false };
        for &out_edge in &graph[elkport].outgoing_edges {
            let target_node = graph.connectable_shape_to_node(graph[out_edge].targets[0]);
            if !graph.is_descendant(target_node, parent) {
                return true;
            }
        }
        for &in_edge in &graph[elkport].incoming_edges {
            let source_node = graph.connectable_shape_to_node(graph[in_edge].sources[0]);
            if !graph.is_descendant(source_node, parent) {
                return true;
            }
        }
        false
    }

    /// `transformNode(_:_:)`.
    fn transform_node(&mut self, graph: &mut ElkGraph, lg: &mut LGraphArena, elknode: ElkNodeId, lgraph: LGraphId) -> LNodeId {
        let lnode = lg.new_node(Some(lgraph));
        lg[lnode].props.copy_properties(&graph[elknode].props);
        lg[lnode].props.set(&InternalProperties::ORIGIN, PropValue::ElkNode(elknode));

        lg[lnode].size.x = graph[elknode].width;
        lg[lnode].size.y = graph[elknode].height;
        lg[lnode].position.x = graph[elknode].x;
        lg[lnode].position.y = graph[elknode].y;

        lg[lgraph].layerless_nodes.push(lnode);
        self.node_and_port_map.insert(ElkShape::Node(elknode), Mapped::Node(lnode));

        if !graph[elknode].children.is_empty() || bool_prop(&graph[elknode].props, &LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE) {
            lg[lnode].props.set(&InternalProperties::COMPOUND_NODE, true);
        }

        let mut graph_properties = lg[lgraph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();

        let port_constraints = lg[lnode].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED);
        if port_constraints == PortConstraints::UNDEFINED {
            lg[lnode].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FREE);
        } else if port_constraints != PortConstraints::FREE {
            graph_properties.insert(GraphProperties::NON_FREE_PORTS);
        }

        let mut port_model_order: i64 = 0;
        let direction = {
            let d = lg[lgraph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
            if d == Direction::UNDEFINED { Direction::RIGHT } else { d }
        };

        for elkport in graph[elknode].ports.clone() {
            if self.needs_model_order(graph, elknode) {
                graph[elkport].props.set(&InternalProperties::MODEL_ORDER, port_model_order);
                port_model_order += 1;
            }
            if !bool_prop(&graph[elkport].props, &LayeredOptions::NO_LAYOUT) {
                // `portConstraints` is the value read before UNDEFINED was
                // replaced by FREE.
                self.transform_port(graph, lg, elkport, lnode, &mut graph_properties, direction, port_constraints);
            }
        }

        for elklabel in graph[elknode].labels.clone() {
            if !bool_prop(&graph[elklabel].props, &LayeredOptions::NO_LAYOUT) && !graph[elklabel].text.is_empty() {
                let llabel = self.transform_label(graph, lg, elklabel);
                lg[lnode].labels.push(llabel);
            }
        }

        if bool_prop(&lg[lnode].props, &LayeredOptions::COMMENT_BOX) {
            graph_properties.insert(GraphProperties::COMMENTS);
        }
        if bool_prop(&lg[lnode].props, &LayeredOptions::HYPERNODE) {
            graph_properties.insert(GraphProperties::HYPERNODES);
            graph_properties.insert(GraphProperties::HYPEREDGES);
            lg[lnode].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FREE);
        }

        lg[lgraph].props.set(&InternalProperties::GRAPH_PROPERTIES, graph_properties);
        lnode
    }

    /// `transformPort(_:_:_:_:_:)`.
    #[allow(clippy::too_many_arguments)]
    fn transform_port(
        &mut self,
        graph: &ElkGraph,
        lg: &mut LGraphArena,
        elkport: ElkPortId,
        parent_lnode: LNodeId,
        graph_properties: &mut EnumSet<GraphProperties>,
        layout_direction: Direction,
        port_constraints: PortConstraints,
    ) -> LPortId {
        let lport = lg.new_port();
        lg[lport].props.copy_properties(&graph[elkport].props);
        lg[lport].side = graph[elkport].props.get_as::<PortSide>(&LayeredOptions::PORT_SIDE).unwrap_or(PortSide::UNDEFINED);
        lg[lport].props.set(&InternalProperties::ORIGIN, PropValue::ElkPort(elkport));
        lg.port_set_node(lport, Some(parent_lnode));

        lg[lport].size.x = graph[elkport].width;
        lg[lport].size.y = graph[elkport].height;
        lg[lport].position.x = graph[elkport].x;
        lg[lport].position.y = graph[elkport].y;

        self.node_and_port_map.insert(ElkShape::Port(elkport), Mapped::Port(lport));

        let mut connections_to_descendants = false;
        if let Some(port_parent) = graph[elkport].parent {
            connections_to_descendants = graph[elkport]
                .outgoing_edges
                .iter()
                .flat_map(|&e| graph[e].targets.iter().copied())
                .map(|s| graph.connectable_shape_to_node(s))
                .any(|n| graph.is_descendant(n, port_parent));
            if !connections_to_descendants {
                connections_to_descendants = graph[elkport]
                    .incoming_edges
                    .iter()
                    .flat_map(|&e| graph[e].sources.iter().copied())
                    .map(|s| graph.connectable_shape_to_node(s))
                    .any(|n| graph.is_descendant(n, port_parent));
            }
        }
        if !connections_to_descendants {
            connections_to_descendants = graph[elkport]
                .outgoing_edges
                .iter()
                .any(|&e| graph.edge_is_selfloop(e) && bool_prop(&graph[e].props, &LayeredOptions::INSIDE_SELF_LOOPS_YO));
        }
        lg[lport].props.set(&InternalProperties::INSIDE_CONNECTIONS, connections_to_descendants);

        let anchor = graph[elkport].props.get_as::<KVectorRef>(&LayeredOptions::PORT_ANCHOR).map(|a| *a.borrow());
        lg.initialize_port(lport, port_constraints, layout_direction, anchor);

        for elklabel in graph[elkport].labels.clone() {
            if !bool_prop(&graph[elklabel].props, &LayeredOptions::NO_LAYOUT) && !graph[elklabel].text.is_empty() {
                let llabel = self.transform_label(graph, lg, elklabel);
                lg[lport].labels.push(llabel);
            }
        }

        let side = lg[lport].side;
        match layout_direction {
            Direction::LEFT | Direction::RIGHT => {
                if side == PortSide::NORTH || side == PortSide::SOUTH {
                    graph_properties.insert(GraphProperties::NORTH_SOUTH_PORTS);
                }
            }
            Direction::UP | Direction::DOWN => {
                if side == PortSide::EAST || side == PortSide::WEST {
                    graph_properties.insert(GraphProperties::NORTH_SOUTH_PORTS);
                }
            }
            _ => {}
        }
        lport
    }

    /// `transformEdge(_:_:_:)`.
    fn transform_edge(&mut self, graph: &mut ElkGraph, lg: &mut LGraphArena, elkedge: ElkEdgeId, elkparent: ElkNodeId, lgraph: LGraphId) -> Result<LEdgeId, ElkError> {
        Self::check_edge_validity(graph, elkedge)?;

        let elk_source_shape = graph[elkedge].sources[0];
        let elk_target_shape = graph[elkedge].targets[0];
        let elk_source_node = graph.connectable_shape_to_node(elk_source_shape);
        let elk_target_node = graph.connectable_shape_to_node(elk_target_shape);
        let edge_section = graph[elkedge].sections.first().copied();

        let mut source_lnode = self.mapped_node(elk_source_node);
        let mut target_lnode = self.mapped_node(elk_target_node);
        let mut source_lport: Option<LPortId> = None;
        let mut target_lport: Option<LPortId> = None;

        if let ElkShape::Port(_) = elk_source_shape {
            match self.node_and_port_map.get(&elk_source_shape) {
                Some(Mapped::Port(p)) => source_lport = Some(*p),
                Some(Mapped::Node(n)) => {
                    source_lnode = Some(*n);
                    source_lport = Some(lg[*n].ports[0]);
                }
                None => {}
            }
        }
        if let ElkShape::Port(_) = elk_target_shape {
            match self.node_and_port_map.get(&elk_target_shape) {
                Some(Mapped::Port(p)) => target_lport = Some(*p),
                Some(Mapped::Node(n)) => {
                    target_lnode = Some(*n);
                    target_lport = Some(lg[*n].ports[0]);
                }
                None => {}
            }
        }

        let (Some(source_lnode), Some(target_lnode)) = (source_lnode, target_lnode) else {
            // assertionFailure, then `return LEdge()`.
            return Ok(lg.new_edge());
        };

        let ledge = lg.new_edge();
        lg[ledge].props.copy_properties(&graph[elkedge].props);
        lg[ledge].props.set(&InternalProperties::ORIGIN, PropValue::ElkEdge(elkedge));
        lg[ledge].props.set_opt(&LayeredOptions::JUNCTION_POINTS, None);

        let mut graph_properties = lg[lgraph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
        if source_lnode == target_lnode {
            graph_properties.insert(GraphProperties::SELF_LOOPS);
        }

        if source_lport.is_none() {
            let mut port_type = PortType::OUTPUT;
            let mut source_point: Option<KVector> = None;
            let side_fixed = lg[source_lnode].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED).is_side_fixed();
            if let (Some(section), true) = (edge_section, side_fixed) {
                let mut point = KVector::new(graph[section].start_x, graph[section].start_y);
                ElkUtil::to_absolute(graph, &mut point, graph[elkedge].containing_node);
                ElkUtil::to_relative(graph, &mut point, Some(elkparent));
                if graph.is_descendant(elk_target_node, elk_source_node) {
                    port_type = PortType::INPUT;
                    point.add(lg[source_lnode].position);
                }
                source_point = Some(point);
            }
            source_lport = Some(lg.create_port(source_lnode, source_point, port_type, lgraph));
        }

        if target_lport.is_none() {
            let port_type = PortType::INPUT;
            let mut target_point: Option<KVector> = None;
            let side_fixed = lg[target_lnode].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED).is_side_fixed();
            if let (Some(section), true) = (edge_section, side_fixed) {
                let mut point = KVector::new(graph[section].end_x, graph[section].end_y);
                ElkUtil::to_absolute(graph, &mut point, graph[elkedge].containing_node);
                ElkUtil::to_relative(graph, &mut point, Some(elkparent));
                target_point = Some(point);
            }
            let port_graph = lg[target_lnode].graph.unwrap_or(lgraph);
            target_lport = Some(lg.create_port(target_lnode, target_point, port_type, port_graph));
        }

        lg.edge_set_source(ledge, source_lport);
        lg.edge_set_target(ledge, target_lport);

        if let (Some(sp), Some(tp)) = (source_lport, target_lport) {
            if lg[sp].incoming_edges.len() > 1 || lg[sp].outgoing_edges.len() > 1 || lg[tp].incoming_edges.len() > 1 || lg[tp].outgoing_edges.len() > 1 {
                graph_properties.insert(GraphProperties::HYPEREDGES);
            }
        }

        for elklabel in graph[elkedge].labels.clone() {
            if !bool_prop(&graph[elklabel].props, &LayeredOptions::NO_LAYOUT) && !graph[elklabel].text.is_empty() {
                let llabel = self.transform_label(graph, lg, elklabel);
                lg[ledge].labels.push(llabel);
                let placement = lg[llabel].props.get_as::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT).unwrap_or(EdgeLabelPlacement::CENTER);
                match placement {
                    EdgeLabelPlacement::HEAD | EdgeLabelPlacement::TAIL => {
                        graph_properties.insert(GraphProperties::END_LABELS);
                    }
                    EdgeLabelPlacement::CENTER => {
                        graph_properties.insert(GraphProperties::CENTER_LABELS);
                        lg[llabel].props.set(&LayeredOptions::EDGE_LABELS_PLACEMENT, EdgeLabelPlacement::CENTER);
                    }
                }
            }
        }

        lg[lgraph].props.set(&InternalProperties::GRAPH_PROPERTIES, graph_properties);

        let cross_min = lg[lgraph].props.get_as::<CrossingMinimizationStrategy>(&LayeredOptions::CROSSING_MINIMIZATION_STRATEGY).unwrap_or(CrossingMinimizationStrategy::LAYER_SWEEP);
        let node_place = lg[lgraph].props.get_as::<NodePlacementStrategy>(&LayeredOptions::NODE_PLACEMENT_STRATEGY).unwrap_or(NodePlacementStrategy::BRANDES_KOEPF);
        let bend_points_required = cross_min == CrossingMinimizationStrategy::INTERACTIVE || node_place == NodePlacementStrategy::INTERACTIVE;
        if let Some(section) = edge_section {
            if !graph[section].bend_points.is_empty() && bend_points_required {
                let original = ElkUtil::create_vector_chain(graph, section);
                let mut imported = KVectorChain::new();
                for point in original.iter() {
                    imported.add(KVector::new(point.x, point.y));
                }
                lg[ledge].props.set(&InternalProperties::ORIGINAL_BENDPOINTS, PropValue::kvector_chain(imported));
            }
        }
        Ok(ledge)
    }

    /// `checkEdgeValidity(_:)`.
    fn check_edge_validity(graph: &ElkGraph, edge: ElkEdgeId) -> Result<(), ElkError> {
        if graph[edge].sources.is_empty() {
            Err(ElkError::Runtime("Edges must have a source.".into()))
        } else if graph[edge].targets.is_empty() {
            Err(ElkError::Runtime("Edges must have a target.".into()))
        } else if graph.edge_is_hyperedge(edge) {
            Err(ElkError::Runtime("Hyperedges are not supported.".into()))
        } else {
            Ok(())
        }
    }

    /// `transformLabel(_:)`.
    fn transform_label(&mut self, graph: &ElkGraph, lg: &mut LGraphArena, elklabel: ElkLabelId) -> LLabelId {
        let new_label = lg.new_label(&graph[elklabel].text);
        lg[new_label].props.copy_properties(&graph[elklabel].props);
        lg[new_label].props.set(&InternalProperties::ORIGIN, PropValue::ElkLabel(elklabel));
        lg[new_label].size.x = graph[elklabel].width;
        lg[new_label].size.y = graph[elklabel].height;
        lg[new_label].position.x = graph[elklabel].x;
        lg[new_label].position.y = graph[elklabel].y;
        new_label
    }
}

#[allow(dead_code)]
fn _unused(_: SizeConstraint) {}
