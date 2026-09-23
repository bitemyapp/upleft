//! Port of `core/alg/ILayoutProcessor.swift` (`ILayoutProcessor`,
//! `AnyGraphProcessor`).

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

/// A layout processor (or phase) run on one layered graph.
pub trait ILayoutProcessor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, monitor: &mut dyn IElkProgressMonitor);

    /// `processor is IHierarchyAwareLayoutProcessor` — only
    /// `LayerSweepCrossingMinimizer` is.
    fn is_hierarchy_aware(&self) -> bool {
        false
    }

    /// The Swift type name (for traces).
    fn name(&self) -> &'static str;
}

/// A processor list shared by every graph that copied the `PROCESSORS`
/// property (components of one graph share processor instances in Swift).
pub type ProcessorList = std::rc::Rc<std::cell::RefCell<Vec<Box<dyn ILayoutProcessor>>>>;
