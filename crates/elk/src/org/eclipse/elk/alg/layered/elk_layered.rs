//! Port of `alg/layered/ElkLayered.swift`.
//!
//! The layout-test API (`prepareLayoutTest`, `runLayoutTestUntil`, …) is only
//! used by elk-swift's own tests and is not ported.

use std::rc::Rc;

use super::components::components_processor::ComponentsProcessor;
use super::compound::compound_graph_postprocessor::CompoundGraphPostprocessor;
use super::compound::compound_graph_preprocessor::CompoundGraphPreprocessor;
use super::graph::l_graph::{LGraphArena, LGraphId};
use super::graph::l_node::NodeType;
use super::graph_configurator::GraphConfigurator;
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::{
    crossing_minimization_strategy::CrossingMinimizationStrategy, graph_properties::GraphProperties,
    greedy_switch_type::GreedySwitchType,
};
use crate::org::eclipse::elk::core::alg::i_layout_processor::{ILayoutProcessor, ProcessorList};
use crate::org::eclipse::elk::core::math::k_vector::{KVector, KVectorRef};
use crate::org::eclipse::elk::core::options::content_alignment::ContentAlignment;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::org::eclipse::elk::core::util::basic_progress_monitor::BasicProgressMonitor;
use crate::org::eclipse::elk::core::util::elk_util::ElkUtil;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::swift;

#[derive(Default)]
pub struct ElkLayered {
    pub graph_configurator: GraphConfigurator,
    pub components_processor: ComponentsProcessor,
    pub compound_graph_preprocessor: CompoundGraphPreprocessor,
    pub compound_graph_postprocessor: CompoundGraphPostprocessor,
}

/// Optional per-processor state dumps for debugging conformance
/// (`UPLEFT_ELK_TRACE=1`, written to stderr).
fn trace(lg: &LGraphArena, graph: LGraphId, what: &str) {
    thread_local! {
        static ON: bool = std::env::var_os("UPLEFT_ELK_TRACE").is_some();
    }
    if !ON.with(|on| *on) {
        return;
    }
    let mut s = format!("== {what}\n");
    for &n in &lg[graph].layerless_nodes {
        let node = &lg[n];
        s += &format!(
            " ll {} {},{} {},{} ports:{}\n",
            lg.node_designation(n),
            swift::describe_double(node.position.x),
            swift::describe_double(node.position.y),
            swift::describe_double(node.size.x),
            swift::describe_double(node.size.y),
            node.ports.len()
        );
    }
    for (i, &layer) in lg[graph].layers.iter().enumerate() {
        s += &format!(" L{i}:");
        for &n in &lg[layer].nodes {
            let node = &lg[n];
            let label = node.labels.first().map_or(String::new(), |&l| lg[l].text.clone());
            s += &format!(
                " {}{}@{},{}",
                &node.node_type.raw_value()[..2],
                label,
                swift::describe_double(node.position.x),
                swift::describe_double(node.position.y)
            );
        }
        s += "\n";
    }
    eprint!("{s}");
}

impl ElkLayered {
    pub fn new() -> ElkLayered {
        ElkLayered::default()
    }

    /// `doLayout(_:_:)`.
    pub fn do_layout(&mut self, lg: &mut LGraphArena, lgraph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Layered layout", 1.0);
        self.graph_configurator.prepare_graph_for_layout(lg, lgraph);
        let components = self.components_processor.split(lg, lgraph);
        if components.len() == 1 {
            self.layout(lg, components[0], monitor);
        } else {
            let comp_work = 1.0f32 / components.len() as f32;
            for &comp in &components {
                if monitor.is_canceled() {
                    return;
                }
                if let Some(mut sub) = monitor.sub_task(comp_work) {
                    self.layout(lg, comp, sub.as_mut());
                }
            }
        }
        self.components_processor.combine(lg, &components, lgraph);
        self.resize_graph(lg, lgraph);
        monitor.done();
    }

    /// `doCompoundLayout(_:_:)`.
    pub fn do_compound_layout(&mut self, lg: &mut LGraphArena, lgraph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Layered layout", 2.0);
        if let Some(mut sub) = monitor.sub_task(1.0) {
            self.compound_graph_preprocessor.process(lg, lgraph, sub.as_mut());
        }
        if let Some(mut sub) = monitor.sub_task(1.0) {
            self.hierarchical_layout(lg, lgraph, sub.as_mut());
        }
        if let Some(mut sub) = monitor.sub_task(1.0) {
            self.compound_graph_postprocessor.process(lg, lgraph, sub.as_mut());
        }
        monitor.done();
    }

    fn processors_of(lg: &LGraphArena, graph: LGraphId) -> Option<ProcessorList> {
        lg[graph].props.get_object::<std::cell::RefCell<Vec<Box<dyn ILayoutProcessor>>>>(&InternalProperties::PROCESSORS)
    }

    /// `hierarchicalLayout(_:_:)`: runs every graph's processors in lockstep.
    pub fn hierarchical_layout(&mut self, lg: &mut LGraphArena, lgraph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        let graphs = self.collect_all_graphs_bottom_up(lg, lgraph);
        self.review_and_correct_hierarchical_processors(lg, lgraph, &graphs);

        let mut work = 0usize;
        let mut graphs_and_algorithms: Vec<(LGraphId, Option<ProcessorList>, usize)> = Vec::new();
        for &g in &graphs {
            self.graph_configurator.prepare_graph_for_layout(lg, g);
            let processors = Self::processors_of(lg, g);
            work += processors.as_ref().map_or(0, |p| p.borrow().len());
            graphs_and_algorithms.push((g, processors, 0));
        }

        monitor.begin("Recursive hierarchical layout", work as f32);

        let Some(root_idx) = graphs_and_algorithms.iter().rposition(|(g, _, _)| lg[*g].parent_node.is_none()) else {
            monitor.done();
            return;
        };
        let count = |entry: &(LGraphId, Option<ProcessorList>, usize)| entry.1.as_ref().map_or(0, |p| p.borrow().len());

        while graphs_and_algorithms[root_idx].2 < count(&graphs_and_algorithms[root_idx]) {
            for i in 0..graphs_and_algorithms.len() {
                let (graph, processors, mut idx) = graphs_and_algorithms[i].clone();
                let n = count(&graphs_and_algorithms[i]);
                while idx < n {
                    let processors = processors.as_ref().unwrap();
                    let hierarchy_aware = processors.borrow()[idx].is_hierarchy_aware();
                    if !hierarchy_aware {
                        if let Some(mut sub) = monitor.sub_task(1.0) {
                            processors.borrow_mut()[idx].process(lg, graph, sub.as_mut());
                        }
                        trace(lg, graph, processors.borrow()[idx].name());
                        idx += 1;
                    } else if lg[graph].parent_node.is_none() {
                        if let Some(mut sub) = monitor.sub_task(1.0) {
                            processors.borrow_mut()[idx].process(lg, graph, sub.as_mut());
                        }
                        trace(lg, graph, processors.borrow()[idx].name());
                        idx += 1;
                        break;
                    } else {
                        idx += 1;
                        break;
                    }
                }
                graphs_and_algorithms[i].2 = idx;
            }
        }
        monitor.done();
    }

    /// `collectAllGraphsBottomUp(_:)`: innermost graphs first, root last.
    pub fn collect_all_graphs_bottom_up(&self, lg: &LGraphArena, root: LGraphId) -> Vec<LGraphId> {
        let mut collected = vec![root];
        let mut stack = vec![root];
        while let Some(next) = stack.pop() {
            for &node in &lg[next].layerless_nodes {
                if let Some(nested) = lg[node].nested_graph {
                    collected.push(nested);
                    stack.push(nested);
                }
            }
        }
        collected.reverse();
        collected
    }

    /// `reviewAndCorrectHierarchicalProcessors(_:_:)`.
    pub fn review_and_correct_hierarchical_processors(&self, lg: &mut LGraphArena, root: LGraphId, graphs: &[LGraphId]) {
        let parent_cms = lg[root].props.get_as::<CrossingMinimizationStrategy>(&LayeredOptions::CROSSING_MINIMIZATION_STRATEGY);
        for &child in graphs {
            let child_cms = lg[child].props.get_as::<CrossingMinimizationStrategy>(&LayeredOptions::CROSSING_MINIMIZATION_STRATEGY);
            if child_cms != parent_cms {
                // assertionFailure (a no-op in release builds), then return.
                return;
            }
        }
        let root_type = lg[root].props.get_as::<GreedySwitchType>(&LayeredOptions::CROSSING_MINIMIZATION_GREEDY_SWITCH_HIERARCHICAL_TYPE);
        for &g in graphs {
            lg[g].props.set_opt(&LayeredOptions::CROSSING_MINIMIZATION_GREEDY_SWITCH_HIERARCHICAL_TYPE, root_type.map(Into::into));
        }
    }

    /// `layout(_:_:)`: runs one graph's processors, then moves every node back
    /// into the layerless list.
    pub fn layout(&mut self, lg: &mut LGraphArena, lgraph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        let monitor_was_already_running = monitor.is_running();
        if !monitor_was_already_running {
            monitor.begin("Component Layout", 1.0);
        }
        let Some(algorithm) = Self::processors_of(lg, lgraph) else { return };
        let monitor_progress = 1.0f32 / algorithm.borrow().len() as f32;
        let n = algorithm.borrow().len();
        for i in 0..n {
            if monitor.is_canceled() {
                return;
            }
            if let Some(mut sub) = monitor.sub_task(monitor_progress) {
                algorithm.borrow_mut()[i].process(lg, lgraph, sub.as_mut());
            }
            trace(lg, lgraph, algorithm.borrow()[i].name());
        }
        for layer in lg[lgraph].layers.clone() {
            for node in lg[layer].nodes.clone() {
                lg[lgraph].layerless_nodes.push(node);
                lg[node].layer = None;
            }
        }
        lg[lgraph].layers.clear();
        if !monitor_was_already_running {
            monitor.done();
        }
    }

    /// `resizeGraph(_:)`.
    pub fn resize_graph(&mut self, lg: &mut LGraphArena, lgraph: LGraphId) {
        let size_constraint = lg[lgraph].props.get_as::<SizeConstraint>(&LayeredOptions::NODE_SIZE_CONSTRAINTS).unwrap_or_default();
        let size_options = lg[lgraph].props.get_as::<SizeOptions>(&LayeredOptions::NODE_SIZE_OPTIONS).unwrap_or_default();

        let calculated_size = lg.graph_actual_size(lgraph);
        let mut adjusted_size = calculated_size;

        if size_constraint.contains(SizeConstraint::MINIMUM_SIZE) {
            // `as? KVector ?? KVector()`: the stored vector itself (mutated
            // below, as in Swift) or a fresh one.
            let min_size: KVectorRef = lg[lgraph]
                .props
                .get_as::<KVectorRef>(&LayeredOptions::NODE_SIZE_MINIMUM)
                .unwrap_or_else(|| crate::org::eclipse::elk::core::math::k_vector::kvector_ref(KVector::default()));
            if size_options.contains(SizeOptions::DEFAULT_MINIMUM_SIZE) {
                let mut m = min_size.borrow_mut();
                if m.x <= 0.0 {
                    m.x = ElkUtil::DEFAULT_MIN_WIDTH;
                }
                if m.y <= 0.0 {
                    m.y = ElkUtil::DEFAULT_MIN_HEIGHT;
                }
            }
            let m = *min_size.borrow();
            adjusted_size.x = swift::max(calculated_size.x, m.x);
            adjusted_size.y = swift::max(calculated_size.y, m.y);
        }

        let fixed_graph_size = lg[lgraph].props.get_as::<bool>(&LayeredOptions::NODE_SIZE_FIXED_GRAPH_SIZE).unwrap_or(false);
        if !fixed_graph_size {
            self.resize_graph_no_really_i_mean_it(lg, lgraph, calculated_size, adjusted_size);
        }
    }

    /// `resizeGraphNoReallyIMeanIt(_:_:_:)`.
    pub fn resize_graph_no_really_i_mean_it(&mut self, lg: &mut LGraphArena, lgraph: LGraphId, old_size: KVector, new_size: KVector) {
        let content_alignment = lg[lgraph].props.get_as::<ContentAlignment>(&LayeredOptions::CONTENT_ALIGNMENT).unwrap_or_default();

        if new_size.x > old_size.x {
            if content_alignment.contains(ContentAlignment::H_CENTER) {
                lg[lgraph].offset.x += (new_size.x - old_size.x) / 2.0;
            } else if content_alignment.contains(ContentAlignment::H_RIGHT) {
                lg[lgraph].offset.x += new_size.x - old_size.x;
            }
        }
        if new_size.y > old_size.y {
            if content_alignment.contains(ContentAlignment::V_CENTER) {
                lg[lgraph].offset.y += (new_size.y - old_size.y) / 2.0;
            } else if content_alignment.contains(ContentAlignment::V_BOTTOM) {
                lg[lgraph].offset.y += new_size.y - old_size.y;
            }
        }

        let graph_properties = lg[lgraph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
        if graph_properties.contains(GraphProperties::EXTERNAL_PORTS) && (new_size.x > old_size.x || new_size.y > old_size.y) {
            for node in lg[lgraph].layerless_nodes.clone() {
                if lg[node].node_type == NodeType::EXTERNAL_PORT {
                    let ext_port_side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE);
                    if ext_port_side == Some(PortSide::EAST) {
                        lg[node].position.x += new_size.x - old_size.x;
                    } else if ext_port_side == Some(PortSide::SOUTH) {
                        lg[node].position.y += new_size.y - old_size.y;
                    }
                }
            }
        }

        let padding = lg[lgraph].padding;
        lg[lgraph].size.x = new_size.x - padding.left - padding.right;
        lg[lgraph].size.y = new_size.y - padding.top - padding.bottom;
    }
}

#[allow(dead_code)]
fn _unused(_: Rc<()>, _: BasicProgressMonitor) {}
