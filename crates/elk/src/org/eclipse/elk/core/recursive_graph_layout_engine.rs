//! Port of `core/RecursiveGraphLayoutEngine.swift`.
//!
//! Top-down layout (`org.eclipse.elk.topdownLayout`) is not ported: nothing
//! beautiful-mermaid builds enables it.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::bridge::elk::ElkError;
use crate::bridge::elk_graph_impl::{ElkEdgeId, ElkGraph, ElkNodeId};
use crate::org::eclipse::elk::alg::layered::layered_layout_provider;
use crate::org::eclipse::elk::core::data::deprecated_layout_option_replacer::DeprecatedLayoutOptionReplacer;
use crate::org::eclipse::elk::core::data::layout_algorithm_data::LayoutAlgorithmData;
use crate::org::eclipse::elk::core::data::layout_algorithm_resolver::LayoutAlgorithmResolver;
use crate::org::eclipse::elk::core::options::core_options as CoreOptions;
use crate::org::eclipse::elk::core::options::hierarchy_handling::HierarchyHandling;
use crate::org::eclipse::elk::core::util::elk_util::ElkUtil;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::org::eclipse::elk::graph::properties::graph_feature::GraphFeature;

#[derive(Default)]
pub struct RecursiveGraphLayoutEngine;

impl RecursiveGraphLayoutEngine {
    pub fn new() -> RecursiveGraphLayoutEngine {
        RecursiveGraphLayoutEngine
    }

    pub fn layout(&mut self, graph: &mut ElkGraph, layout_graph: ElkNodeId, monitor: &mut dyn IElkProgressMonitor) -> Result<(), ElkError> {
        let node_count = self.count_nodes_recursively(graph, layout_graph, true);
        monitor.begin("Recursive Graph Layout", node_count as f32);

        let replacer = DeprecatedLayoutOptionReplacer;
        for element in ElkUtil::visitor_order(graph, layout_graph) {
            replacer.visit(graph, element);
        }

        if !graph[layout_graph].props.has(&CoreOptions::RESOLVED_ALGORITHM) {
            let resolver = LayoutAlgorithmResolver;
            for element in ElkUtil::visitor_order(graph, layout_graph) {
                resolver.visit(graph, element)?;
            }
        }

        self.layout_recursively(graph, layout_graph, monitor)?;
        monitor.done();
        Ok(())
    }

    pub fn layout_recursively(&mut self, graph: &mut ElkGraph, layout_node: ElkNodeId, monitor: &mut dyn IElkProgressMonitor) -> Result<Vec<ElkEdgeId>, ElkError> {
        if monitor.is_canceled() {
            return Ok(Vec::new());
        }
        let no_layout: bool = graph[layout_node].props.get_typed(&CoreOptions::NO_LAYOUT).unwrap_or(false);
        if no_layout {
            return Ok(Vec::new());
        }

        let has_children = !graph[layout_node].children.is_empty();
        let inside_self_loops = self.gather_inside_self_loops(graph, layout_node);
        let has_inside_self_loops = !inside_self_loops.is_empty();

        if !(has_children || has_inside_self_loops) {
            return Ok(Vec::new());
        }

        // `guard let algorithmData ... else { assertionFailure; return [] }`
        let Some(algorithm_data) = graph[layout_node].props.get_object::<LayoutAlgorithmData>(&CoreOptions::RESOLVED_ALGORITHM) else {
            return Ok(Vec::new());
        };
        let supports_inside_self_loops = algorithm_data.supports_feature(GraphFeature::INSIDE_SELF_LOOPS);

        self.evaluate_hierarchy_handling_inheritance(graph, layout_node);

        if !has_children && has_inside_self_loops && !supports_inside_self_loops {
            return Ok(Vec::new());
        }

        let mut children_inside_self_loops: Vec<ElkEdgeId> = Vec::new();

        let hierarchy_handling: HierarchyHandling =
            graph[layout_node].props.get_typed(&CoreOptions::HIERARCHY_HANDLING).unwrap_or(HierarchyHandling::SEPARATE_CHILDREN);
        if hierarchy_handling == HierarchyHandling::INCLUDE_CHILDREN
            && (algorithm_data.supports_feature(GraphFeature::COMPOUND) || algorithm_data.supports_feature(GraphFeature::CLUSTERS))
        {
            let topdown: bool = graph[layout_node].props.get_typed(&CoreOptions::TOPDOWN_LAYOUT).unwrap_or(false);
            if topdown {
                return Err(ElkError::Runtime("Topdown layout cannot be used together with hierarchy handling.".into()));
            }
            self.count_nodes_with_hierarchy(graph, layout_node);

            let mut node_queue: VecDeque<ElkNodeId> = graph[layout_node].children.iter().copied().collect();
            while let Some(node) = node_queue.pop_front() {
                self.evaluate_hierarchy_handling_inheritance(graph, node);
                let node_hh: HierarchyHandling =
                    graph[node].props.get_typed(&CoreOptions::HIERARCHY_HANDLING).unwrap_or(HierarchyHandling::SEPARATE_CHILDREN);
                let stop_hierarchy = node_hh == HierarchyHandling::SEPARATE_CHILDREN;
                let has_alg = graph[node].props.has(&CoreOptions::ALGORITHM);
                let alg_match = Self::node_resolved_algorithm_equals(graph, node, &algorithm_data);
                if stop_hierarchy || (has_alg && !alg_match) {
                    let child_loops = self.layout_recursively(graph, node, monitor)?;
                    children_inside_self_loops.extend(child_loops);
                    graph[node].props.set(&CoreOptions::HIERARCHY_HANDLING, HierarchyHandling::SEPARATE_CHILDREN);
                    ElkUtil::apply_configured_node_scaling(graph, node);
                } else {
                    node_queue.extend(graph[node].children.iter().copied());
                }
            }
        } else {
            let topdown: bool = graph[layout_node].props.get_typed(&CoreOptions::TOPDOWN_LAYOUT).unwrap_or(false);
            if topdown {
                unimplemented!("top-down layout is not reachable from beautiful-mermaid and is not ported");
            }
            for child in graph[layout_node].children.clone() {
                let child_loops = self.layout_recursively(graph, child, monitor)?;
                children_inside_self_loops.extend(child_loops);
                ElkUtil::apply_configured_node_scaling(graph, child);
            }
        }

        if monitor.is_canceled() {
            return Ok(Vec::new());
        }

        for &self_loop in &children_inside_self_loops {
            graph[self_loop].props.set(&CoreOptions::NO_LAYOUT, true);
        }

        let topdown: bool = graph[layout_node].props.get_typed(&CoreOptions::TOPDOWN_LAYOUT).unwrap_or(false);
        if !topdown {
            let node_count = graph[layout_node].children.len();
            if let Some(mut sub) = monitor.sub_task(node_count as f32) {
                self.execute_algorithm(graph, layout_node, &algorithm_data, sub.as_mut());
            }
        }

        self.post_process_inside_self_loops(graph, &children_inside_self_loops);

        if has_inside_self_loops && supports_inside_self_loops {
            Ok(inside_self_loops)
        } else {
            Ok(Vec::new())
        }
    }

    /// `executeAlgorithm`: a provider that throws is dropped (`dispose()`)
    /// and the error swallowed; otherwise it goes back to the pool.
    pub fn execute_algorithm(&mut self, graph: &mut ElkGraph, layout_node: ElkNodeId, _algorithm_data: &Rc<LayoutAlgorithmData>, monitor: &mut dyn IElkProgressMonitor) {
        let mut provider = layered_layout_provider::fetch();
        match provider.layout(graph, layout_node, monitor) {
            Ok(()) => layered_layout_provider::release(provider),
            Err(_) => drop(provider),
        }
    }

    pub fn count_nodes_recursively(&self, graph: &ElkGraph, layout_node: ElkNodeId, count_ancestors: bool) -> usize {
        let mut count = graph[layout_node].children.len();
        for &child in &graph[layout_node].children {
            if !graph[child].children.is_empty() {
                count += self.count_nodes_recursively(graph, child, false);
            }
        }
        if count_ancestors {
            let mut parent = graph[layout_node].parent;
            while let Some(p) = parent {
                count += graph[p].children.len();
                parent = graph[p].parent;
            }
        }
        count
    }

    pub fn evaluate_hierarchy_handling_inheritance(&self, graph: &mut ElkGraph, layout_node: ElkNodeId) {
        let hh: HierarchyHandling = graph[layout_node].props.get_typed(&CoreOptions::HIERARCHY_HANDLING).unwrap_or(HierarchyHandling::INHERIT);
        if hh == HierarchyHandling::INHERIT {
            match graph[layout_node].parent {
                None => graph[layout_node].props.set(&CoreOptions::HIERARCHY_HANDLING, HierarchyHandling::SEPARATE_CHILDREN),
                Some(parent) => {
                    let parent_handling: HierarchyHandling =
                        graph[parent].props.get_typed(&CoreOptions::HIERARCHY_HANDLING).unwrap_or(HierarchyHandling::SEPARATE_CHILDREN);
                    graph[layout_node].props.set(&CoreOptions::HIERARCHY_HANDLING, parent_handling);
                }
            }
        }
    }

    pub fn count_nodes_with_hierarchy(&self, graph: &ElkGraph, parent_node: ElkNodeId) -> usize {
        let mut count = graph[parent_node].children.len();
        for &child in &graph[parent_node].children {
            let child_hh: HierarchyHandling = graph[child].props.get_typed(&CoreOptions::HIERARCHY_HANDLING).unwrap_or(HierarchyHandling::INHERIT);
            if child_hh != HierarchyHandling::SEPARATE_CHILDREN {
                let parent_data = graph[parent_node].props.get_object::<LayoutAlgorithmData>(&CoreOptions::RESOLVED_ALGORITHM);
                let child_data = graph[child].props.get_object::<LayoutAlgorithmData>(&CoreOptions::RESOLVED_ALGORITHM);
                if let (Some(pd), Some(cd)) = (parent_data, child_data) {
                    if pd.id == cd.id && !graph[child].children.is_empty() {
                        count += self.count_nodes_with_hierarchy(graph, child);
                    }
                }
            }
        }
        count
    }

    fn node_resolved_algorithm_equals(graph: &ElkGraph, node: ElkNodeId, algorithm_data: &LayoutAlgorithmData) -> bool {
        match graph[node].props.get_object::<LayoutAlgorithmData>(&CoreOptions::RESOLVED_ALGORITHM) {
            Some(d) => d.id == algorithm_data.id,
            None => false,
        }
    }

    pub fn gather_inside_self_loops(&self, graph: &ElkGraph, node: ElkNodeId) -> Vec<ElkEdgeId> {
        let activate: bool = graph[node].props.get_typed(&CoreOptions::INSIDE_SELF_LOOPS_ACTIVATE).unwrap_or(false);
        if !activate {
            return Vec::new();
        }
        let mut loops = Vec::new();
        for edge in graph.all_outgoing_edges(node) {
            let yo: bool = graph[edge].props.get_typed(&CoreOptions::INSIDE_SELF_LOOPS_YO).unwrap_or(false);
            if graph.edge_is_selfloop(edge) && yo {
                loops.push(edge);
            }
        }
        loops
    }

    pub fn post_process_inside_self_loops(&self, graph: &mut ElkGraph, inside_self_loops: &[ElkEdgeId]) {
        for &self_loop in inside_self_loops {
            let node = graph.connectable_shape_to_node(graph[self_loop].sources[0]);
            let x_offset = graph[node].x;
            let y_offset = graph[node].y;
            let section = graph[self_loop].sections[0];
            let s = &mut graph[section];
            s.start_x += x_offset;
            s.start_y += y_offset;
            s.end_x += x_offset;
            s.end_y += y_offset;
            for bend in &mut s.bend_points {
                bend.x += x_offset;
                bend.y += y_offset;
            }
            if let Some(jp) = graph[self_loop].props.get_typed::<crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChainRef>(&CoreOptions::JUNCTION_POINTS) {
                jp.borrow_mut().offset_xy(x_offset, y_offset);
            }
        }
    }
}
