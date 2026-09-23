//! Port of `alg/layered/p4nodes/bk/BKNodePlacer.swift`.
//!
//! The Brandes & Köpf node placer (phase 4): computes four aligned layouts
//! (or one, with a fixed alignment), compacts them, and picks either their
//! balanced combination or the narrowest one that respects the node order.

use std::collections::HashMap;

use super::bk_aligned_layout::{BKAlignedLayout, HDirection, VDirection};
use super::bk_aligner::BKAligner;
use super::bk_compactor::BKCompactor;
use super::i_compactor::ICompactor;
use super::neighborhood_information::NeighborhoodInformation;
use crate::org::eclipse::elk::alg::layered::intermediate::intermediate_processor_strategy::IntermediateProcessorStrategy;
use crate::org::eclipse::elk::alg::layered::layered_phases::LayeredPhases;
use crate::org::eclipse::elk::alg::layered::options::fixed_alignment::FixedAlignment;
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::alg::layout_processor_configuration::LayoutProcessorConfiguration;
use crate::prelude::*;

pub mod option_keys {
    pub const NODE_PLACEMENT_BK_FIXED_ALIGNMENT: &str = "org.eclipse.elk.layered.nodePlacement.bk.fixedAlignment";
    pub const NODE_PLACEMENT_FAVOR_STRAIGHT_EDGES_FULL: &str = "org.eclipse.elk.layered.nodePlacement.favorStraightEdges";
    pub const NODE_PLACEMENT_FAVOR_STRAIGHT_EDGES_SHORT: &str = "nodePlacement.favorStraightEdges";
}

pub const MIN_LAYERS_FOR_CONFLICTS: usize = 3;

#[derive(Default)]
pub struct BKNodePlacer {
    pub l_graph: Option<LGraphId>,
    /// `markedEdges`, a membership table indexed by edge id.
    pub marked_edges: Vec<bool>,
    pub ni: Option<NeighborhoodInformation>,
    pub produce_balanced_layout: bool,
}

#[inline]
fn nid(lg: &LGraphArena, n: LNodeId) -> usize {
    lg[n].id as usize
}

impl BKNodePlacer {
    pub fn new() -> BKNodePlacer {
        BKNodePlacer::default()
    }

    /// `HIERARCHY_PROCESSING_ADDITIONS`.
    pub fn hierarchy_processing_additions() -> LayoutProcessorConfiguration {
        let mut c = LayoutProcessorConfiguration::create();
        c.add_before(LayeredPhases::P5_EDGE_ROUTING, IntermediateProcessorStrategy::HIERARCHICAL_PORT_POSITION_PROCESSOR);
        c
    }

    /// `markConflicts(_:)`.
    pub fn mark_conflicts(&mut self, lg: &LGraphArena, layered_graph: LGraphId) {
        let number_of_layers = lg[layered_graph].layers.len();
        if number_of_layers < MIN_LAYERS_FOR_CONFLICTS {
            return;
        }

        let layers = &lg[layered_graph].layers;
        let layer_size: Vec<usize> = layers.iter().map(|&l| lg[l].nodes.len()).collect();

        for i in 1..number_of_layers - 1 {
            let current_layer = layers[i + 1];
            let mut k_0: i64 = 0;
            let mut l: usize = 0;

            for l_1 in 0..layer_size[i + 1] {
                let v_l_i = lg[current_layer].nodes[l_1];

                if l_1 == layer_size[i + 1] - 1 || self.incident_to_inner_segment(lg, v_l_i, i + 1, i) {
                    let mut k_1 = layer_size[i] as i64 - 1;
                    if self.incident_to_inner_segment(lg, v_l_i, i + 1, i) {
                        if let Some(ni) = &self.ni {
                            if let Some(&(left_neighbor, _)) = ni.left_neighbors[nid(lg, v_l_i)].first() {
                                k_1 = ni.node_index[nid(lg, left_neighbor)];
                            }
                        }
                    }

                    while l <= l_1 {
                        let v_l = lg[current_layer].nodes[l];

                        if !self.incident_to_inner_segment(lg, v_l, i + 1, i) {
                            if let Some(ni) = &self.ni {
                                for &(upper, edge) in &ni.left_neighbors[nid(lg, v_l)] {
                                    let k = ni.node_index[nid(lg, upper)];

                                    if k < k_0 || k > k_1 {
                                        if edge.index() >= self.marked_edges.len() {
                                            self.marked_edges.resize(edge.index() + 1, false);
                                        }
                                        self.marked_edges[edge.index()] = true;
                                    }
                                }
                            }
                        }

                        l += 1;
                    }

                    k_0 = k_1;
                }
            }
        }
    }

    /// `createBalancedLayout(_:_:)`.
    pub fn create_balanced_layout(&self, lg: &mut LGraphArena, layouts: &[BKAlignedLayout], node_count: usize) -> BKAlignedLayout {
        let Some(l_graph) = self.l_graph else {
            let empty = lg.new_graph();
            return BKAlignedLayout::new(lg, empty, 0, VDirection::DOWN, HDirection::RIGHT);
        };

        let no_of_layouts = layouts.len();
        let mut balanced = BKAlignedLayout::new(lg, l_graph, node_count, VDirection::DOWN, HDirection::RIGHT);

        if no_of_layouts == 0 {
            return balanced;
        }

        let mut width = vec![0.0; no_of_layouts];
        let mut min_vals = vec![f64::MAX; no_of_layouts];
        let mut max_vals = vec![-f64::MAX; no_of_layouts];
        let mut min_width_layout = 0;

        for i in 0..no_of_layouts {
            let bal = &layouts[i];
            width[i] = bal.layout_size(lg);
            if width[min_width_layout] > width[i] {
                min_width_layout = i;
            }

            for &layer in &lg[l_graph].layers {
                for &n in &lg[layer].nodes {
                    let node_pos_y = bal.y[nid(lg, n)] + bal.inner_shift[nid(lg, n)];
                    min_vals[i] = swift::min(min_vals[i], node_pos_y);
                    max_vals[i] = swift::max(max_vals[i], node_pos_y + lg[n].size.y);
                }
            }
        }

        let mut shift = vec![0.0; no_of_layouts];
        for i in 0..no_of_layouts {
            if layouts[i].vdir == VDirection::DOWN {
                shift[i] = min_vals[min_width_layout] - min_vals[i];
            } else {
                shift[i] = max_vals[min_width_layout] - max_vals[i];
            }
        }

        let mut calculated_ys = vec![0.0; no_of_layouts];
        for &layer in &lg[l_graph].layers {
            for &node in &lg[layer].nodes {
                for i in 0..no_of_layouts {
                    calculated_ys[i] = layouts[i].y[nid(lg, node)] + layouts[i].inner_shift[nid(lg, node)] + shift[i];
                }

                swift::sort(&mut calculated_ys);
                if no_of_layouts >= 4 {
                    balanced.y[nid(lg, node)] = (calculated_ys[1] + calculated_ys[2]) / 2.0;
                } else {
                    balanced.y[nid(lg, node)] = calculated_ys[no_of_layouts / 2];
                }
                balanced.inner_shift[nid(lg, node)] = 0.0;
            }
        }

        balanced
    }

    /// `incidentToInnerSegment(_:_:_:)`.
    pub fn incident_to_inner_segment(&self, lg: &LGraphArena, node: LNodeId, layer1: usize, layer2: usize) -> bool {
        if lg[node].node_type == NodeType::LONG_EDGE {
            for &port in &lg[node].ports {
                for &edge in &lg[port].incoming_edges {
                    let Some(source_node) = lg.edge_source_node(edge) else { continue };
                    if lg[source_node].node_type != NodeType::LONG_EDGE {
                        continue;
                    }
                    let (Some(source_layer), Some(node_layer), Some(ni)) = (lg[source_node].layer, lg[node].layer, self.ni.as_ref()) else { continue };

                    if ni.layer_index[lg[source_layer].id as usize] == layer2 as i64 && ni.layer_index[lg[node_layer].id as usize] == layer1 as i64 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// `getEdge(_:_:)`: the first edge connecting `source` with `target`.
    pub fn get_edge(lg: &LGraphArena, source: LNodeId, target: LNodeId) -> Option<LEdgeId> {
        // `source.getConnectedEdges()`: per port, incoming then outgoing.
        for &port in &lg[source].ports {
            for &edge in lg[port].incoming_edges.iter().chain(lg[port].outgoing_edges.iter()) {
                if lg.edge_target_node(edge) == Some(target) || lg.edge_source_node(edge) == Some(target) {
                    return Some(edge);
                }
            }
        }
        None
    }

    /// `getBlocks(_:)`: root → nodes of its block. Not used by the pipeline
    /// (see `BKAligner::inside_block_shift`); the Swift dictionary has no
    /// order, the result here is keyed by root.
    pub fn get_blocks(lg: &LGraphArena, bal: &BKAlignedLayout) -> HashMap<LNodeId, Vec<LNodeId>> {
        let mut blocks: HashMap<LNodeId, Vec<LNodeId>> = HashMap::new();
        for &layer in &lg[bal.layered_graph].layers {
            for &node in &lg[layer].nodes {
                let root = bal.root[nid(lg, node)].unwrap_or(node);
                blocks.entry(root).or_default().push(node);
            }
        }
        blocks
    }

    /// `getClasses(_:_:)`: sink → roots of its class (in `root` array order).
    /// Not used by the pipeline.
    pub fn get_classes(lg: &LGraphArena, bal: &BKAlignedLayout) -> HashMap<LNodeId, Vec<LNodeId>> {
        let mut classes: HashMap<LNodeId, Vec<LNodeId>> = HashMap::new();
        let mut roots_seen = std::collections::HashSet::new();
        for &root in bal.root.iter().flatten() {
            if !roots_seen.insert(root) {
                continue;
            }
            let Some(sink) = bal.sink[nid(lg, root)] else { continue };
            classes.entry(sink).or_default().push(root);
        }
        classes
    }

    /// `checkOrderConstraint(_:_:_:)`.
    pub fn check_order_constraint(&self, lg: &LGraphArena, layered_graph: LGraphId, bal: &BKAlignedLayout) -> bool {
        let mut feasible = true;

        for &layer in &lg[layered_graph].layers {
            let mut pos = -f64::INFINITY;

            for &node in &lg[layer].nodes {
                let n = &lg[node];
                let top = bal.y[nid(lg, node)] + bal.inner_shift[nid(lg, node)] - n.margin.top;
                let bottom = bal.y[nid(lg, node)] + bal.inner_shift[nid(lg, node)] + n.size.y + n.margin.bottom;

                if top > pos && bottom > pos {
                    pos = bal.y[nid(lg, node)] + bal.inner_shift[nid(lg, node)] + n.size.y + n.margin.bottom;
                } else {
                    feasible = false;
                    break;
                }
            }

            if !feasible {
                break;
            }
        }

        feasible
    }

    /// `getFixedAlignment(_:)` (string-key read: the stored value only).
    pub fn get_fixed_alignment(&self, lg: &LGraphArena, graph: LGraphId) -> FixedAlignment {
        lg[graph]
            .props
            .get_by_id(option_keys::NODE_PLACEMENT_BK_FIXED_ALIGNMENT)
            .and_then(|v| v.cast::<FixedAlignment>())
            .unwrap_or(FixedAlignment::NONE)
    }

    /// `getFavorStraightEdges(_:)` (string-key reads).
    pub fn get_favor_straight_edges(&self, lg: &LGraphArena, graph: LGraphId) -> bool {
        if let Some(value) = lg[graph].props.get_by_id(option_keys::NODE_PLACEMENT_FAVOR_STRAIGHT_EDGES_FULL).and_then(|v| v.cast::<bool>()) {
            return value;
        }
        lg[graph].props.get_by_id(option_keys::NODE_PLACEMENT_FAVOR_STRAIGHT_EDGES_SHORT).and_then(|v| v.cast::<bool>()).unwrap_or(false)
    }
}

impl ILayoutProcessor for BKNodePlacer {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Brandes & Koepf node placement", 1.0);

        self.l_graph = Some(layered_graph);
        self.ni = Some(NeighborhoodInformation::build_for(lg, layered_graph));
        self.marked_edges.clear();
        self.marked_edges.resize(lg.edges.len(), false);

        let align = self.get_fixed_alignment(lg, layered_graph);
        let favor_straight_edges = self.get_favor_straight_edges(lg, layered_graph);
        self.produce_balanced_layout = (align == FixedAlignment::NONE && !favor_straight_edges) || align == FixedAlignment::BALANCED;

        self.mark_conflicts(lg, layered_graph);

        let node_count = self.ni.as_ref().map_or(0, |ni| ni.node_count);
        let mut layouts: Vec<BKAlignedLayout> = Vec::new();
        match align {
            FixedAlignment::LEFTDOWN => layouts.push(BKAlignedLayout::new(lg, layered_graph, node_count, VDirection::DOWN, HDirection::LEFT)),
            FixedAlignment::LEFTUP => layouts.push(BKAlignedLayout::new(lg, layered_graph, node_count, VDirection::UP, HDirection::LEFT)),
            FixedAlignment::RIGHTDOWN => layouts.push(BKAlignedLayout::new(lg, layered_graph, node_count, VDirection::DOWN, HDirection::RIGHT)),
            FixedAlignment::RIGHTUP => layouts.push(BKAlignedLayout::new(lg, layered_graph, node_count, VDirection::UP, HDirection::RIGHT)),
            FixedAlignment::NONE | FixedAlignment::BALANCED => {
                layouts.push(BKAlignedLayout::new(lg, layered_graph, node_count, VDirection::DOWN, HDirection::RIGHT));
                layouts.push(BKAlignedLayout::new(lg, layered_graph, node_count, VDirection::UP, HDirection::RIGHT));
                layouts.push(BKAlignedLayout::new(lg, layered_graph, node_count, VDirection::DOWN, HDirection::LEFT));
                layouts.push(BKAlignedLayout::new(lg, layered_graph, node_count, VDirection::UP, HDirection::LEFT));
            }
        }

        let Some(ni) = self.ni.take() else {
            monitor.done();
            return;
        };

        {
            let aligner = BKAligner::new(layered_graph, &ni);
            for bal in layouts.iter_mut() {
                aligner.vertical_alignment(lg, bal, &self.marked_edges);
                aligner.inside_block_shift(lg, bal);
            }
        }

        {
            let mut compactor = BKCompactor::new(lg, layered_graph, &ni);
            for bal in layouts.iter_mut() {
                compactor.horizontal_compaction(lg, bal);
            }
        }

        // `chosenLayout`: an index into `layouts`, or the balanced layout.
        let mut balanced: Option<BKAlignedLayout> = None;
        let mut chosen: Option<usize> = None;
        let mut chose_balanced = false;

        if self.produce_balanced_layout {
            let b = self.create_balanced_layout(lg, &layouts, ni.node_count);
            let passes_constraint = self.check_order_constraint(lg, layered_graph, &b);
            if passes_constraint {
                chose_balanced = true;
            }
            balanced = Some(b);
        }

        if !chose_balanced {
            for (i, bal) in layouts.iter().enumerate() {
                let passes = self.check_order_constraint(lg, layered_graph, bal);
                if passes {
                    let chosen_size = chosen.map_or(f64::INFINITY, |c| layouts[c].layout_size(lg));
                    if chosen.is_none() || chosen_size > bal.layout_size(lg) {
                        chosen = Some(i);
                    }
                }
            }
        }

        if !chose_balanced && chosen.is_none() && !layouts.is_empty() {
            chosen = Some(0);
        }

        let chosen_layout: Option<&BKAlignedLayout> = if chose_balanced { balanced.as_ref() } else { chosen.map(|c| &layouts[c]) };
        if let Some(chosen_layout) = chosen_layout {
            for li in 0..lg[layered_graph].layers.len() {
                let layer = lg[layered_graph].layers[li];
                for k in 0..lg[layer].nodes.len() {
                    let node = lg[layer].nodes[k];
                    let id = nid(lg, node);
                    lg[node].position.y = chosen_layout.y[id] + chosen_layout.inner_shift[id];
                }
            }
        }

        for bal in layouts.iter_mut() {
            bal.cleanup();
        }
        let mut ni = ni;
        ni.cleanup();
        self.ni = Some(ni);
        self.marked_edges.clear();

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "BKNodePlacer"
    }
}

impl ILayoutPhase for BKNodePlacer {
    fn get_layout_processor_configuration(&self, lg: &LGraphArena, graph: LGraphId) -> Option<LayoutProcessorConfiguration> {
        let graph_properties = lg[graph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
        if graph_properties.contains(GraphProperties::EXTERNAL_PORTS) {
            return Some(Self::hierarchy_processing_additions());
        }
        None
    }
}
