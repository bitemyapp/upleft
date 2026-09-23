//! Port of `core/data/LayoutAlgorithmResolver.swift`.

use crate::bridge::elk::ElkError;
use crate::bridge::elk_graph_impl::{ElkElement, ElkGraph, ElkNodeId};
use crate::org::eclipse::elk::core::data::layout_meta_data_service::LayoutMetaDataService;
use crate::org::eclipse::elk::core::options::core_options as CoreOptions;
use crate::org::eclipse::elk::graph::properties::property::PropValue;

pub struct LayoutAlgorithmResolver;

impl LayoutAlgorithmResolver {
    pub fn visit(&self, graph: &mut ElkGraph, element: ElkElement) -> Result<(), ElkError> {
        if let ElkElement::Node(node) = element {
            let no_layout = graph[node].props.get_as::<bool>(&CoreOptions::NO_LAYOUT).unwrap_or(false);
            if !no_layout {
                self.resolve_algorithm(graph, node)?;
            }
        }
        Ok(())
    }

    fn resolve_algorithm(&self, graph: &mut ElkGraph, node: ElkNodeId) -> Result<(), ElkError> {
        let algorithm_id: Option<String> = graph[node].props.get_as(&CoreOptions::ALGORITHM);
        if Self::resolve_and_set_algorithm(graph, algorithm_id.as_deref(), node) {
            return Ok(());
        }
        if Self::must_resolve(graph, node) {
            if let Some(id) = algorithm_id.as_deref().filter(|id| !id.trim().is_empty()) {
                return Err(ElkError::Runtime(format!("Layout algorithm '{id}' not found")));
            }
            let default_id = "org.eclipse.elk.layered";
            if !Self::resolve_and_set_algorithm(graph, Some(default_id), node) {
                return Err(ElkError::Runtime(format!("Unable to load default layout algorithm {default_id}")));
            }
        }
        Ok(())
    }

    fn resolve_and_set_algorithm(graph: &mut ElkGraph, algorithm_id: Option<&str>, node: ElkNodeId) -> bool {
        let Some(id) = algorithm_id else { return false };
        match LayoutMetaDataService::get_algorithm_data_by_suffix(id) {
            Some(data) => {
                graph[node].props.set(&CoreOptions::RESOLVED_ALGORITHM, PropValue::object(data));
                true
            }
            None => false,
        }
    }

    fn must_resolve(graph: &ElkGraph, node: ElkNodeId) -> bool {
        let has_resolved = graph[node].props.has(&CoreOptions::RESOLVED_ALGORITHM);
        let has_children = !graph[node].children.is_empty();
        let inside_self_loops = graph[node].props.get_as::<bool>(&CoreOptions::INSIDE_SELF_LOOPS_ACTIVATE).unwrap_or(false);
        !has_resolved && (has_children || inside_self_loops)
    }
}
