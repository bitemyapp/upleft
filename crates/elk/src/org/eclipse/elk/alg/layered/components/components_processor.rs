//! Port of `alg/layered/components/ComponentsProcessor.swift`.
//!
//! Splits a graph into its connected components before layout and packs the
//! laid-out components with a graph placer afterwards.
//!
//! Each component is a new `LGraph` with a copy of the parent graph's
//! properties. Class-typed values are shared, as in Swift: all components use
//! the same `RANDOM` generator, `PROCESSORS` list and `SPACINGS`.

use super::abstract_graph_placer::AbstractGraphPlacer;
use super::component_group_graph_placer::ComponentGroupGraphPlacer;
use super::component_group_model_order_graph_placer::ComponentGroupModelOrderGraphPlacer;
use super::component_ordering_strategy::ComponentOrderingStrategy;
use super::model_order_row_graph_placer::ModelOrderRowGraphPlacer;
use super::simple_row_graph_placer::SimpleRowGraphPlacer;
use crate::prelude::*;

/// Which cached placer `graphPlacer` refers to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GraphPlacerKind {
    #[default]
    SimpleRow,
    ModelOrderRow,
    ComponentGroup,
    ComponentGroupModelOrder,
}

#[derive(Default)]
pub struct ComponentsProcessor {
    pub component_group_graph_placer: ComponentGroupGraphPlacer,
    pub component_group_model_order_graph_placer: ComponentGroupModelOrderGraphPlacer,
    pub model_order_row_graph_placer: ModelOrderRowGraphPlacer,
    pub simple_row_graph_placer: SimpleRowGraphPlacer,
    /// `graphPlacer`: the placer `combine` uses.
    pub graph_placer: GraphPlacerKind,
}

/// The DFS accumulator (`Pair<Set<PortSide>, [LNode]>`, shared by reference
/// through the Swift recursion).
struct ComponentData {
    ext_port_sides: EnumSet<PortSide>,
    nodes: Vec<LNodeId>,
}

impl ComponentsProcessor {
    pub fn new() -> ComponentsProcessor {
        ComponentsProcessor::default()
    }

    /// `split(_:)`.
    pub fn split(&mut self, lg: &mut LGraphArena, graph: LGraphId) -> Vec<LGraphId> {
        let mut result: Vec<LGraphId> = Vec::new();

        // Default to the simple graph placer
        self.graph_placer = GraphPlacerKind::SimpleRow;

        // Whether separate components processing is requested
        let separate = lg[graph].props.get_typed::<bool>(&LayeredOptions::SEPARATE_CONNECTED_COMPONENTS).unwrap_or(true);

        // Whether the graph contains external ports
        let graph_properties = lg[graph].props.get_typed::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
        let ext_ports = graph_properties.contains(GraphProperties::EXTERNAL_PORTS);

        // The graph's external port constraints
        let ext_port_constraints = lg[graph].props.get_typed::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED);
        let compatible_port_constraints = !ext_port_constraints.is_order_fixed();

        if separate && (compatible_port_constraints || !ext_ports) {
            // Set id of all nodes to 0
            for i in 0..lg[graph].layerless_nodes.len() {
                let node = lg[graph].layerless_nodes[i];
                lg[node].id = 0;
            }

            // Perform DFS starting on each node, collecting connected components
            for node in lg[graph].layerless_nodes.clone() {
                let Some(component_data) = Self::dfs(lg, node) else { continue };

                let new_graph = lg.new_graph();
                let props = lg[graph].props.clone();
                lg[new_graph].props.copy_properties(&props);
                lg[new_graph].props.set(&InternalProperties::EXT_PORT_CONNECTIONS, component_data.ext_port_sides);
                lg[new_graph].padding = lg[graph].padding;

                // Remove minimum size on separated graphs
                lg[new_graph].props.remove(&LayeredOptions::NODE_SIZE_MINIMUM);

                for n in component_data.nodes {
                    lg[new_graph].layerless_nodes.push(n);
                    lg[n].graph = Some(new_graph);
                }

                result.push(new_graph);
            }

            if ext_ports {
                let consider_model_order = lg[graph].props.get_typed::<ComponentOrderingStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_COMPONENTS);
                self.graph_placer = match consider_model_order {
                    Some(ComponentOrderingStrategy::GROUP_MODEL_ORDER) => GraphPlacerKind::ComponentGroupModelOrder,
                    Some(ComponentOrderingStrategy::MODEL_ORDER) => GraphPlacerKind::ModelOrderRow,
                    _ => GraphPlacerKind::ComponentGroup,
                };
            }
        } else {
            result = vec![graph];
        }

        // Sort by model order if needed
        let consider_model_order = lg[graph].props.get_typed::<ComponentOrderingStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_COMPONENTS);
        if consider_model_order.is_some() && consider_model_order != Some(ComponentOrderingStrategy::NONE) {
            swift::sort_by(&mut result, |&g1, &g2| lg.get_minimal_model_order(g1) < lg.get_minimal_model_order(g2));
        }

        result
    }

    /// `dfs(_:data:)` from a start node: collects the start's connected
    /// component (in the preorder of the Swift recursion, which visits each
    /// port's predecessor ports, then its successor ports) and the external
    /// port sides it connects to. `None` if the node was already visited.
    fn dfs(lg: &mut LGraphArena, start: LNodeId) -> Option<ComponentData> {
        if lg[start].id != 0 {
            return None;
        }
        let mut data = ComponentData { ext_port_sides: EnumSet::new(), nodes: Vec::new() };
        Self::visit(lg, start, &mut data);

        // (node, port index, connected-port index within the port)
        let mut stack: Vec<(LNodeId, usize, usize)> = vec![(start, 0, 0)];
        while let Some(top) = stack.last_mut() {
            let (node, pi, ci) = *top;
            let Some(&port1) = lg[node].ports.get(pi) else {
                stack.pop();
                continue;
            };
            let p = &lg[port1];
            let (n_in, n_out) = (p.incoming_edges.len(), p.outgoing_edges.len());
            if ci >= n_in + n_out {
                top.1 += 1;
                top.2 = 0;
                continue;
            }
            top.2 += 1;
            // `getConnectedPorts()`: predecessor ports, then successor ports
            // (edges without that end are skipped).
            let port2 = if ci < n_in { lg[p.incoming_edges[ci]].source } else { lg[p.outgoing_edges[ci - n_in]].target };
            let Some(port2) = port2 else { continue };
            let Some(connected_node) = lg[port2].owner else { continue };
            if lg[connected_node].id == 0 {
                Self::visit(lg, connected_node, &mut data);
                stack.push((connected_node, 0, 0));
            }
        }
        Some(data)
    }

    fn visit(lg: &mut LGraphArena, node: LNodeId, data: &mut ComponentData) {
        // Mark the node as visited
        lg[node].id = 1;
        data.nodes.push(node);
        // Check if this node is an external port dummy and, if so, add its side
        if lg[node].node_type == NodeType::EXTERNAL_PORT {
            if let Some(ext_port_side) = lg[node].props.get_typed::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
                data.ext_port_sides.insert(ext_port_side);
            }
        }
    }

    /// `combine(_:target:)`.
    pub fn combine(&mut self, lg: &mut LGraphArena, components: &[LGraphId], target: LGraphId) {
        match self.graph_placer {
            GraphPlacerKind::SimpleRow => self.simple_row_graph_placer.combine(lg, components, target),
            GraphPlacerKind::ModelOrderRow => self.model_order_row_graph_placer.combine(lg, components, target),
            GraphPlacerKind::ComponentGroup => self.component_group_graph_placer.combine(lg, components, target),
            GraphPlacerKind::ComponentGroupModelOrder => self.component_group_model_order_graph_placer.combine(lg, components, target),
        }
    }
}
