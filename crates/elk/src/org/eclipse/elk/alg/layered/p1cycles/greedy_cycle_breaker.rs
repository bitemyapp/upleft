//! Port of `alg/layered/p1cycles/GreedyCycleBreaker.swift`.
//!
//! Eades, Lin and Smyth's greedy heuristic: sinks go to the right, sources to
//! the left, and otherwise a node with maximal outflow (a random one among
//! equals, drawn from the graph's shared `RANDOM`) to the left; edges pointing
//! leftwards are reversed.
//!
//! elk-swift only reports progress to monitors conforming to a private
//! protocol nothing conforms to, so this phase never calls `begin`/`done`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::org::eclipse::elk::alg::layered::graph_configurator::Random;
use crate::org::eclipse::elk::alg::layered::intermediate::intermediate_processor_strategy::IntermediateProcessorStrategy;
use crate::org::eclipse::elk::alg::layered::layered_phases::LayeredPhases;
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::alg::layout_processor_configuration::LayoutProcessorConfiguration;
use crate::prelude::*;

#[derive(Default)]
pub struct GreedyCycleBreaker {
    pub indeg: Option<Vec<i64>>,
    pub outdeg: Option<Vec<i64>>,
    pub mark: Option<Vec<i64>>,
    pub sources: VecDeque<LNodeId>,
    pub sinks: VecDeque<LNodeId>,
    layered_graph: Option<LGraphId>,
}

impl GreedyCycleBreaker {
    pub fn new() -> GreedyCycleBreaker {
        GreedyCycleBreaker::default()
    }

    /// `INTERMEDIATE_PROCESSING_CONFIGURATION`.
    fn intermediate_processing_configuration() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_after(LayeredPhases::P5_EDGE_ROUTING, IntermediateProcessorStrategy::REVERSED_EDGE_RESTORER);
        c
    }

    /// `chooseNodeWithMaxOutflow(_:)`: a node drawn with the graph's random
    /// generator.
    pub fn choose_node_with_max_outflow(&self, lg: &LGraphArena, nodes: &[LNodeId]) -> LNodeId {
        let first = nodes[0];
        if let Some(g) = self.layered_graph {
            if let Some(rng) = lg[g].props.get_as::<Rc<RefCell<Random>>>(&InternalProperties::RANDOM) {
                let i = rng.borrow_mut().next_int_bounded(nodes.len() as i64);
                return nodes[i as usize];
            }
        }
        // NONDETERMINISTIC IN SWIFT: without a `RANDOM` property Swift uses
        // `Int.random(in:)`. Unreachable (the configurator always sets
        // `RANDOM`, and components copy it); the port takes the first node.
        first
    }

    pub fn dispose(&mut self) {
        self.indeg = None;
        self.outdeg = None;
        self.mark = None;
        self.sources = VecDeque::new();
        self.sinks = VecDeque::new();
    }

    /// `updateNeighbors(_:)`: removes `node`'s edges from its unmarked
    /// neighbours' degrees and queues neighbours that became sources or sinks.
    pub fn update_neighbors(&mut self, lg: &LGraphArena, node: LNodeId) {
        let (Some(indeg), Some(outdeg), Some(mark)) = (self.indeg.as_mut(), self.outdeg.as_mut(), self.mark.as_ref()) else { return };
        for &port in &lg[node].ports {
            let p = &lg[port];
            for &edge in p.incoming_edges.iter().chain(p.outgoing_edges.iter()) {
                let connected_port = if lg[edge].source == Some(port) { lg[edge].target } else { lg[edge].source };
                let Some(endpoint_port) = connected_port else { continue };
                let Some(endpoint) = lg[endpoint_port].owner else { continue };

                if node == endpoint {
                    continue;
                }

                let index = lg[endpoint].id;
                // Safety: skip nodes from a different graph (e.g. cross-hierarchy edges)
                if !(index >= 0 && (index as usize) < mark.len() && mark[index as usize] == 0) {
                    continue;
                }
                let index = index as usize;

                let weight = Self::edge_weight(lg, edge);
                if lg[edge].target == Some(endpoint_port) {
                    indeg[index] -= weight;
                    if indeg[index] <= 0 && outdeg[index] > 0 {
                        self.sources.push_back(endpoint);
                    }
                } else {
                    outdeg[index] -= weight;
                    if outdeg[index] <= 0 && indeg[index] > 0 {
                        self.sinks.push_back(endpoint);
                    }
                }
            }
        }
    }

    /// `edgeWeight(_:)`: `max(priority, 0) + 1`.
    pub fn edge_weight(lg: &LGraphArena, edge: LEdgeId) -> i64 {
        let priority = lg[edge].props.get_as::<i64>(&LayeredOptions::PRIORITY_DIRECTION).unwrap_or(0);
        swift::max(priority, 0) + 1
    }
}

impl ILayoutProcessor for GreedyCycleBreaker {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {
        self.layered_graph = Some(layered_graph);

        let nodes = lg[layered_graph].layerless_nodes.clone();
        let mut unprocessed_node_count = nodes.len() as i64;

        let mut indeg = vec![0i64; nodes.len()];
        let mut outdeg = vec![0i64; nodes.len()];
        self.mark = Some(vec![0i64; nodes.len()]);

        for (index, &node) in nodes.iter().enumerate() {
            lg[node].id = index as i32;

            for &port in &lg[node].ports {
                for &edge in &lg[port].incoming_edges {
                    if lg.edge_source_node(edge) == Some(node) {
                        continue;
                    }
                    indeg[index] += Self::edge_weight(lg, edge);
                }
                for &edge in &lg[port].outgoing_edges {
                    if lg.edge_target_node(edge) == Some(node) {
                        continue;
                    }
                    outdeg[index] += Self::edge_weight(lg, edge);
                }
            }

            if outdeg[index] == 0 {
                self.sinks.push_back(node);
            } else if indeg[index] == 0 {
                self.sources.push_back(node);
            }
        }
        self.indeg = Some(indeg);
        self.outdeg = Some(outdeg);

        let mut next_right: i64 = -1;
        let mut next_left: i64 = 1;
        let mut max_nodes: Vec<LNodeId> = Vec::new();

        while unprocessed_node_count > 0 {
            while let Some(sink) = self.sinks.pop_front() {
                self.mark.as_mut().unwrap()[lg[sink].id as usize] = next_right;
                next_right -= 1;
                self.update_neighbors(lg, sink);
                unprocessed_node_count -= 1;
            }

            while let Some(source) = self.sources.pop_front() {
                self.mark.as_mut().unwrap()[lg[source].id as usize] = next_left;
                next_left += 1;
                self.update_neighbors(lg, source);
                unprocessed_node_count -= 1;
            }

            if unprocessed_node_count > 0 {
                let mut max_outflow = i64::MIN;
                max_nodes.clear();

                {
                    let (mark, outdeg, indeg) = (self.mark.as_ref().unwrap(), self.outdeg.as_ref().unwrap(), self.indeg.as_ref().unwrap());
                    for &node in &nodes {
                        let id = lg[node].id as usize;
                        if mark[id] != 0 {
                            continue;
                        }
                        let outflow = outdeg[id] - indeg[id];
                        if outflow >= max_outflow {
                            if outflow > max_outflow {
                                max_nodes.clear();
                                max_outflow = outflow;
                            }
                            max_nodes.push(node);
                        }
                    }
                }

                if !max_nodes.is_empty() {
                    let max_node = self.choose_node_with_max_outflow(lg, &max_nodes);
                    self.mark.as_mut().unwrap()[lg[max_node].id as usize] = next_left;
                    next_left += 1;
                    self.update_neighbors(lg, max_node);
                    unprocessed_node_count -= 1;
                } else {
                    break;
                }
            }
        }

        let shift_base = nodes.len() as i64 + 1;
        let mark = self.mark.as_mut().unwrap();
        for value in mark.iter_mut() {
            if *value < 0 {
                *value += shift_base;
            }
        }

        for &node in &nodes {
            let ports = lg[node].ports.clone();
            for port in ports {
                let outgoing_edges = lg[port].outgoing_edges.clone();
                for edge in outgoing_edges {
                    let Some(target_node) = lg.edge_target_node(edge) else { continue };
                    let target_ix = lg[target_node].id;
                    let mark = self.mark.as_ref().unwrap();
                    // Out-of-range indices trap in Swift as well.
                    let source_mark = mark[lg[node].id as usize];
                    let target_mark = mark[usize::try_from(target_ix).expect("Index out of range")];
                    if source_mark > target_mark {
                        lg.edge_reverse(edge, layered_graph, true);
                        lg[layered_graph].props.set(&InternalProperties::CYCLIC, true);
                    }
                }
            }
        }

        self.dispose();
    }

    fn name(&self) -> &'static str {
        "GreedyCycleBreaker"
    }
}

impl ILayoutPhase for GreedyCycleBreaker {
    fn get_layout_processor_configuration(&self, _lg: &LGraphArena, _graph: LGraphId) -> Option<LayoutProcessorConfiguration> {
        Some(Self::intermediate_processing_configuration())
    }
}
