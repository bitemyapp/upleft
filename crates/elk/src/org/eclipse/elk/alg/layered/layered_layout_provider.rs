//! Port of `alg/layered/LayeredLayoutProvider.swift`, and of the provider
//! pool (`LayoutAlgorithmData.providerPool`, an `InstancePool`).
//!
//! elk-swift keeps one process-wide pool per algorithm: `fetch` takes the
//! oldest released provider or creates one, `release` appends. A provider (and
//! the `ElkLayered` instance inside it) is therefore reused by every later
//! layout on that thread of control. The port pools per thread.

use std::cell::RefCell;

use super::elk_layered::{ElkLayered, TestExecutionState};
use super::graph::l_graph::LGraphArena;
use super::graph::transform::elk_graph_transformer::ElkGraphTransformer;
use crate::bridge::elk::ElkError;
use crate::bridge::elk_graph_impl::{ElkGraph, ElkNodeId};
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::options::hierarchy_handling::HierarchyHandling;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct LayeredLayoutProvider {
    pub elk_layered: ElkLayered,
}

impl LayeredLayoutProvider {
    pub fn new() -> LayeredLayoutProvider {
        LayeredLayoutProvider::default()
    }

    pub fn layout(&mut self, graph: &mut ElkGraph, elkgraph: ElkNodeId, monitor: &mut dyn IElkProgressMonitor) -> Result<(), ElkError> {
        let mut transformer = ElkGraphTransformer::new();
        let Some((mut lg, layered_graph)) = transformer.import_graph(graph, elkgraph)? else { return Ok(()) };

        let hier_handling: Option<HierarchyHandling> = graph[elkgraph].props.get_as(&LayeredOptions::HIERARCHY_HANDLING);
        if hier_handling == Some(HierarchyHandling::INCLUDE_CHILDREN) {
            self.elk_layered.do_compound_layout(&mut lg, layered_graph, monitor);
        } else {
            self.elk_layered.do_layout(&mut lg, layered_graph, monitor);
        }

        if !monitor.is_canceled() {
            transformer.apply_layout(graph, &mut lg, layered_graph);
        }
        Ok(())
    }
}

impl LayeredLayoutProvider {
    /// `startLayoutTest(_:)`: imports the graph and prepares a stepped
    /// layout test (an empty graph if the import yields none).
    pub fn start_layout_test(&mut self, graph: &mut ElkGraph, elkgraph: ElkNodeId) -> Result<(LGraphArena, TestExecutionState), ElkError> {
        let mut transformer = ElkGraphTransformer::new();
        let (mut lg, layered_graph) = match transformer.import_graph(graph, elkgraph)? {
            Some(imported) => imported,
            None => {
                let mut lg = LGraphArena::new();
                let empty = lg.new_graph();
                (lg, empty)
            }
        };
        let state = self.elk_layered.prepare_layout_test(&mut lg, layered_graph);
        Ok((lg, state))
    }

    /// `getLayoutAlgorithm()`.
    pub fn get_layout_algorithm(&mut self) -> &mut ElkLayered {
        &mut self.elk_layered
    }
}

thread_local! {
    static POOL: RefCell<Vec<LayeredLayoutProvider>> = const { RefCell::new(Vec::new()) };
}

/// `InstancePool.fetch()`.
pub fn fetch() -> LayeredLayoutProvider {
    POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        if pool.is_empty() { LayeredLayoutProvider::new() } else { pool.remove(0) }
    })
}

/// `InstancePool.release(_:)`.
pub fn release(provider: LayeredLayoutProvider) {
    POOL.with(|pool| pool.borrow_mut().push(provider));
}
