//! Port of `alg/layered/p2layers/NetworkSimplexLayerer.swift`.
//!
//! Layers each connected component with the network simplex algorithm, the
//! largest component first; later components are balanced against the node
//! counts of the layers the earlier ones filled.

use std::collections::HashMap;

use crate::org::eclipse::elk::alg::common::networksimplex::n_edge::NEdge;
use crate::org::eclipse::elk::alg::common::networksimplex::n_graph::NGraph;
use crate::org::eclipse::elk::alg::common::networksimplex::n_node::{NNode, NNodeId};
use crate::org::eclipse::elk::alg::common::networksimplex::network_simplex::NetworkSimplex;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_element::LElement;
use crate::org::eclipse::elk::alg::layered::intermediate::intermediate_processor_strategy::IntermediateProcessorStrategy;
use crate::org::eclipse::elk::alg::layered::layered_phases::LayeredPhases;
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::alg::layout_processor_configuration::LayoutProcessorConfiguration;
use crate::prelude::*;

#[derive(Default)]
pub struct NetworkSimplexLayerer {
    component_nodes: Vec<LNodeId>,
    node_visited: Vec<bool>,
}

impl NetworkSimplexLayerer {
    const ITER_LIMIT_FACTOR: i64 = 4;

    pub fn new() -> NetworkSimplexLayerer {
        NetworkSimplexLayerer::default()
    }

    /// `BASELINE_PROCESSING_CONFIGURATION`.
    fn baseline_processing_configuration() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_before(LayeredPhases::P1_CYCLE_BREAKING, IntermediateProcessorStrategy::EDGE_AND_LAYER_CONSTRAINT_EDGE_REVERSER)
            .add_before(LayeredPhases::P2_LAYERING, IntermediateProcessorStrategy::LAYER_CONSTRAINT_PREPROCESSOR)
            .add_before(LayeredPhases::P3_NODE_ORDERING, IntermediateProcessorStrategy::LAYER_CONSTRAINT_POSTPROCESSOR);
        c
    }

    /// `connectedComponents(_:)`: the components in DFS order, except that a
    /// component larger than the current first one is put in front.
    fn connected_components(&mut self, lg: &mut LGraphArena, the_nodes: &[LNodeId]) -> Vec<Vec<LNodeId>> {
        if self.node_visited.len() < the_nodes.len() {
            self.node_visited = vec![false; the_nodes.len()];
        } else {
            for v in self.node_visited.iter_mut() {
                *v = false;
            }
        }
        self.component_nodes = Vec::new();

        let mut counter = 0;
        for &node in the_nodes {
            lg[node].id = counter;
            counter += 1;
        }

        let mut components: Vec<Vec<LNodeId>> = Vec::new();
        for &node in the_nodes {
            if !self.node_visited[lg[node].id as usize] {
                self.connected_components_dfs(lg, node);
                let component = std::mem::take(&mut self.component_nodes);
                // Connected component with the most nodes should be layered first
                if components.is_empty() || components[0].len() < component.len() {
                    components.insert(0, component);
                } else {
                    components.push(component);
                }
            }
        }
        components
    }

    /// `connectedComponentsDFS(_:)`, with an explicit stack visiting ports,
    /// then each port's incoming and outgoing edges, in the Swift order.
    fn connected_components_dfs(&mut self, lg: &LGraphArena, start: LNodeId) {
        self.node_visited[lg[start].id as usize] = true;
        self.component_nodes.push(start);
        // (node, port index, connected-edge index within the port)
        let mut stack: Vec<(LNodeId, usize, usize)> = vec![(start, 0, 0)];
        while let Some(top) = stack.last_mut() {
            let (node, pi, ei) = *top;
            let ports = &lg[node].ports;
            if pi >= ports.len() {
                stack.pop();
                continue;
            }
            let port = ports[pi];
            let p = &lg[port];
            let edge_count = p.incoming_edges.len() + p.outgoing_edges.len();
            if ei >= edge_count {
                top.1 += 1;
                top.2 = 0;
                continue;
            }
            top.2 += 1;
            let edge = if ei < p.incoming_edges.len() { p.incoming_edges[ei] } else { p.outgoing_edges[ei - p.incoming_edges.len()] };
            let Some(opposite) = lg[Self::get_opposite(lg, port, edge)].owner else { continue };
            // Safety: skip nodes from a different graph (e.g. cross-hierarchy edges)
            let id = lg[opposite].id;
            if !(id >= 0 && (id as usize) < self.node_visited.len()) {
                continue;
            }
            if !self.node_visited[id as usize] {
                self.node_visited[id as usize] = true;
                self.component_nodes.push(opposite);
                stack.push((opposite, 0, 0));
            }
        }
    }

    /// `initialize(_:)`: the network simplex graph of one component.
    fn initialize(lg: &LGraphArena, the_nodes: &[LNodeId]) -> NGraph {
        let mut node_map: HashMap<LNodeId, NNodeId> = HashMap::with_capacity(the_nodes.len());

        let mut graph = NGraph::new();
        for &l_node in the_nodes {
            let n_node = NNode::of().origin(LElement::Node(l_node)).create(&mut graph);
            node_map.insert(l_node, n_node);
        }

        for &l_node in the_nodes {
            for &port in &lg[l_node].ports {
                for &l_edge in &lg[port].outgoing_edges {
                    if lg.edge_is_self_loop(l_edge) {
                        continue;
                    }

                    let priority = lg[l_edge].props.get_as::<i64>(&LayeredOptions::PRIORITY_SHORTNESS).unwrap_or(0);
                    let weight = (1 * swift::max(1, priority)) as f64;

                    let (Some(source_node), Some(target_node)) = (lg.edge_source_node(l_edge), lg.edge_target_node(l_edge)) else { continue };
                    let (Some(&n_source), Some(&n_target)) = (node_map.get(&source_node), node_map.get(&target_node)) else { continue };

                    // `NEdge.of(lEdge)` ignores its origin in elk-swift.
                    NEdge::of_origin(Some(LElement::Edge(l_edge))).weight(weight).delta(1).source(n_source).target(n_target).create(&mut graph);
                }
            }
        }
        graph
    }

    fn dispose(&mut self) {
        self.component_nodes = Vec::new();
        self.node_visited = Vec::new();
    }

    /// `getOpposite(_:_:)`.
    fn get_opposite(lg: &LGraphArena, port: LPortId, edge: LEdgeId) -> LPortId {
        if lg[edge].source == Some(port) {
            lg[edge].target.unwrap_or(port)
        } else {
            lg[edge].source.unwrap_or(port)
        }
    }
}

impl ILayoutProcessor for NetworkSimplexLayerer {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Network simplex layering", 1.0);

        let thoroughness = lg[graph].props.get_as::<i64>(&LayeredOptions::THOROUGHNESS).unwrap_or(7) * Self::ITER_LIMIT_FACTOR;

        let the_nodes = lg[graph].layerless_nodes.clone();
        if the_nodes.len() < 1 {
            monitor.done();
            return;
        }

        // Layer graph, each connected component separately
        let connected_comps = self.connected_components(lg, &the_nodes);
        let mut previous_layering_node_counts: Option<Vec<i64>> = None;

        for conn_comp in &connected_comps {
            let iter_limit = thoroughness * ((conn_comp.len() as f64).sqrt() as i64);

            let mut n_graph = Self::initialize(lg, conn_comp);

            {
                let mut ns = NetworkSimplex::for_graph(&mut n_graph)
                    .with_iteration_limit(iter_limit)
                    .with_previous_layering(previous_layering_node_counts.clone())
                    .with_balancing(true);
                if let Some(mut sub_monitor) = monitor.sub_task(1.0) {
                    ns.execute(sub_monitor.as_mut());
                } else {
                    ns.execute_default();
                }
            }

            // The layers are stored in the NNode's layer field
            for &n_node in &n_graph.nodes {
                let layer_index = n_graph[n_node].layer;
                while (lg[graph].layers.len() as i64) <= layer_index {
                    let layer = lg.new_layer(graph);
                    lg[graph].layers.push(layer);
                }
                let Some(LElement::Node(l_node)) = n_graph[n_node].origin else { continue };
                let layer = lg[graph].layers[layer_index as usize];
                lg.node_set_layer(l_node, Some(layer));
            }

            if connected_comps.len() > 1 {
                let counts: Vec<i64> = lg[graph].layers.iter().map(|&l| lg[l].nodes.len() as i64).collect();
                previous_layering_node_counts = Some(counts);
            }
        }

        // Empty the list of unlayered nodes
        lg[graph].layerless_nodes.clear();

        self.dispose();
        monitor.done();
    }

    fn name(&self) -> &'static str {
        "NetworkSimplexLayerer"
    }
}

impl ILayoutPhase for NetworkSimplexLayerer {
    fn get_layout_processor_configuration(&self, _lg: &LGraphArena, _graph: LGraphId) -> Option<LayoutProcessorConfiguration> {
        Some(Self::baseline_processing_configuration())
    }
}
