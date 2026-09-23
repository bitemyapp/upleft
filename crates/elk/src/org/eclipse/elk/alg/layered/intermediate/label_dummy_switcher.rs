//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_LabelDummySwitcher.swift`.
//!
//! Moves every label dummy to the layer its center-label placement strategy
//! asks for, by swapping it with a long-edge dummy of the same long edge.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LLabelId, LNodeId, LayerId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::center_edge_label_placement_strategy::CenterEdgeLabelPlacementStrategy;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::alignment::Alignment;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::org::eclipse::elk::graph::properties::keys;
use crate::org::eclipse::elk::graph::properties::property::Property;
use crate::swift;

/// `LabelDummySwitcher.INCLUDE_LABEL`.
pub static INCLUDE_LABEL: Property = Property::new(keys::EDGELABELCENTEREDNESSANALYSIS_INCLUDELABEL);

type Strategy = CenterEdgeLabelPlacementStrategy;

/// Swift's `lo...hi` closed range, which traps when `lo > hi`.
fn closed_range(lo: i64, hi: i64) -> std::ops::RangeInclusive<i64> {
    assert!(lo <= hi, "Range requires lowerBound <= upperBound");
    lo..=hi
}

#[derive(Default)]
pub struct LabelDummySwitcher {
    layer_widths: Vec<f64>,
}

impl LabelDummySwitcher {
    pub fn new() -> LabelDummySwitcher {
        LabelDummySwitcher::default()
    }
}

impl ILayoutProcessor for LabelDummySwitcher {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Label dummy switching", 1.0);

        let default_placement_strategy = lg[layered_graph]
            .props
            .get_as::<Strategy>(&LayeredOptions::EDGE_LABELS_CENTER_LABEL_PLACEMENT_STRATEGY)
            .unwrap_or(Strategy::MEDIAN_LAYER);

        assign_ids_to_layers(lg, layered_graph);

        let mut label_dummy_infos = gather_label_dummy_infos(lg, layered_graph, default_placement_strategy);

        self.layer_widths = vec![0.0; lg[layered_graph].layers.len()];

        for strategy in Strategy::ALL {
            if strategy.uses_label_size_information() && !label_dummy_infos[strategy.ordinal()].is_empty() {
                self.calculate_layer_widths(lg, layered_graph);
                break;
            }
        }

        for strategy in Strategy::ALL {
            if !strategy.uses_label_size_information() {
                let infos = std::mem::take(&mut label_dummy_infos[strategy.ordinal()]);
                self.process_strategy(lg, &infos);
            }
        }

        for strategy in Strategy::ALL {
            if strategy.uses_label_size_information() {
                let infos = std::mem::take(&mut label_dummy_infos[strategy.ordinal()]);
                self.process_strategy(lg, &infos);
            }
        }

        self.layer_widths = Vec::new();
        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LabelDummySwitcher"
    }
}

fn assign_ids_to_layers(lg: &mut LGraphArena, layered_graph: LGraphId) {
    for (index, layer) in lg[layered_graph].layers.clone().into_iter().enumerate() {
        lg[layer].id = index as i32;
    }
}

fn layer_id(lg: &LGraphArena, layer: LayerId) -> i64 {
    lg[layer].id as i64
}

fn gather_label_dummy_infos(lg: &LGraphArena, layered_graph: LGraphId, default_placement_strategy: Strategy) -> [Vec<LabelDummyInfo>; 6] {
    let mut infos: [Vec<LabelDummyInfo>; 6] = Default::default();

    for &layer in &lg[layered_graph].layers {
        for &node in &lg[layer].nodes {
            if lg[node].node_type == NodeType::LABEL {
                let info = LabelDummyInfo::new(lg, node, default_placement_strategy);
                infos[info.placement_strategy.ordinal()].push(info);
            }
        }
    }

    infos
}

impl LabelDummySwitcher {
    fn calculate_layer_widths(&mut self, lg: &LGraphArena, layered_graph: LGraphId) {
        for &layer in &lg[layered_graph].layers {
            self.layer_widths[layer_id(lg, layer) as usize] = lg.find_max_non_dummy_node_width(layer, false);
        }
    }

    fn process_strategy(&mut self, lg: &mut LGraphArena, label_dummy_infos: &[LabelDummyInfo]) {
        if label_dummy_infos.is_empty() {
            return;
        }

        if label_dummy_infos[0].placement_strategy == Strategy::SPACE_EFFICIENT_LAYER {
            self.compute_space_efficient_assignment(lg, label_dummy_infos);
        } else {
            for info in label_dummy_infos {
                match info.placement_strategy {
                    Strategy::CENTER_LAYER => {
                        let target = self.find_center_layer_target_id(lg, info);
                        self.assign_layer(lg, info, target);
                    }
                    Strategy::MEDIAN_LAYER => self.assign_layer(lg, info, find_median_layer_target_id(info)),
                    Strategy::WIDEST_LAYER => {
                        let target = self.find_widest_layer_target_id(info);
                        self.assign_layer(lg, info, target);
                    }
                    Strategy::HEAD_LAYER => {
                        set_end_layer_node_alignment(lg, info);
                        let target = find_end_layer_target_id(lg, info, true);
                        self.assign_layer(lg, info, target);
                    }
                    Strategy::TAIL_LAYER => {
                        set_end_layer_node_alignment(lg, info);
                        let target = find_end_layer_target_id(lg, info, false);
                        self.assign_layer(lg, info, target);
                    }
                    Strategy::SPACE_EFFICIENT_LAYER => {}
                }
                update_long_edge_source_label_dummy_info(lg, info);
            }
        }
    }

    // MARK: - Widest Layer

    fn find_widest_layer_target_id(&self, info: &LabelDummyInfo) -> i64 {
        let mut widest_layer_index = info.leftmost_layer_id;
        for index in closed_range(widest_layer_index + 1, info.rightmost_layer_id) {
            if self.layer_widths[index as usize] > self.layer_widths[widest_layer_index as usize] {
                widest_layer_index = index;
            }
        }
        widest_layer_index
    }

    // MARK: - Center Layer

    fn find_center_layer_target_id(&self, lg: &LGraphArena, info: &LabelDummyInfo) -> i64 {
        let sums = self.compute_layer_width_sums(lg, info);
        let threshold = sums[sums.len().wrapping_sub(1)] / 2.0;
        for (i, &sum) in sums.iter().enumerate() {
            if sum >= threshold {
                return info.leftmost_layer_id + i as i64;
            }
        }
        info.leftmost_layer_id + info.left_long_edge_dummies.len() as i64
    }

    fn compute_layer_width_sums(&self, lg: &LGraphArena, info: &LabelDummyInfo) -> Vec<f64> {
        let Some(lgraph) = lg.node_graph(info.label_dummy) else { return Vec::new() };
        let edge_node_spacing = lg[lgraph].props.get_as::<f64>(&LayeredOptions::SPACING_EDGE_NODE_BETWEEN_LAYERS).unwrap_or(0.0) * 2.0;
        let node_node_spacing = lg[lgraph].props.get_as::<f64>(&LayeredOptions::SPACING_NODE_NODE_BETWEEN_LAYERS).unwrap_or(0.0);
        let min_space_between_layers = swift::max(edge_node_spacing, node_node_spacing);

        let mut sums = vec![0.0; info.total_dummy_count() as usize];
        let mut current_width_sum = -min_space_between_layers;
        let mut idx = 0;

        for &left_dummy in &info.left_long_edge_dummies {
            let Some(left_dummy_layer) = lg[left_dummy].layer else { continue };
            current_width_sum += self.layer_widths[layer_id(lg, left_dummy_layer) as usize] + min_space_between_layers;
            sums[idx] = current_width_sum;
            idx += 1;
        }

        if let Some(label_dummy_layer) = lg[info.label_dummy].layer {
            current_width_sum += self.layer_widths[layer_id(lg, label_dummy_layer) as usize] + min_space_between_layers;
            sums[idx] = current_width_sum;
            idx += 1;
        }

        for &right_dummy in &info.right_long_edge_dummies {
            let Some(right_dummy_layer) = lg[right_dummy].layer else { continue };
            current_width_sum += self.layer_widths[layer_id(lg, right_dummy_layer) as usize] + min_space_between_layers;
            sums[idx] = current_width_sum;
            idx += 1;
        }

        sums
    }

    // MARK: - Space Efficient

    fn compute_space_efficient_assignment(&mut self, lg: &mut LGraphArena, label_dummy_infos: &[LabelDummyInfo]) {
        let non_trivial_labels = self.perform_trivial_assignments(lg, label_dummy_infos);
        if non_trivial_labels.is_empty() {
            return;
        }

        let sorted = swift::sorted_by(non_trivial_labels, |a, b| lg[a.label_dummy].size.x > lg[b.label_dummy].size.x);
        for label_index in 0..sorted.len() {
            let target = self.find_potentially_widest_layer(lg, &sorted, label_index);
            self.assign_layer(lg, &sorted[label_index], target);
        }
    }

    fn perform_trivial_assignments(&mut self, lg: &mut LGraphArena, label_dummy_infos: &[LabelDummyInfo]) -> Vec<LabelDummyInfo> {
        let mut remaining = Vec::new();
        for info in label_dummy_infos {
            if info.leftmost_layer_id == info.rightmost_layer_id {
                self.assign_layer(lg, info, info.leftmost_layer_id);
            } else if !self.assign_to_wider_layer(lg, info) {
                remaining.push(info.clone());
            }
        }
        remaining
    }

    fn assign_to_wider_layer(&mut self, lg: &mut LGraphArena, info: &LabelDummyInfo) -> bool {
        let dummy_width = lg[info.label_dummy].size.x;
        let Some(graph) = lg.node_graph(info.label_dummy) else { return false };
        let range = closed_range(info.leftmost_layer_id, info.rightmost_layer_id);
        let valid_layers: Vec<LayerId> = lg[graph].layers[*range.start() as usize..=*range.end() as usize].to_vec();
        for layer in valid_layers {
            if lg[layer].size.x >= dummy_width {
                let id = layer_id(lg, layer);
                self.assign_layer(lg, info, id);
                return true;
            }
        }
        false
    }

    fn find_potentially_widest_layer(&self, lg: &LGraphArena, label_dummy_infos: &[LabelDummyInfo], label_index: usize) -> i64 {
        let info = &label_dummy_infos[label_index];
        let label_dummy_width = lg[info.label_dummy].size.x;

        let mut widest_layer_index = info.leftmost_layer_id;
        let mut widest_layer_width: f64 = 0.0;

        for layer in closed_range(info.leftmost_layer_id, info.rightmost_layer_id) {
            if label_dummy_width <= self.layer_widths[layer as usize] {
                return layer;
            }

            let mut potential_width = self.layer_widths[layer as usize];

            for curr_info in &label_dummy_infos[label_index + 1..] {
                if curr_info.leftmost_layer_id <= layer && curr_info.rightmost_layer_id >= layer {
                    potential_width = swift::max(potential_width, lg[curr_info.label_dummy].size.x);
                    break;
                }
            }

            if potential_width > widest_layer_width {
                widest_layer_index = layer;
                widest_layer_width = potential_width;
            }
        }

        widest_layer_index
    }

    // MARK: - Swapping Utilities

    fn assign_layer(&mut self, lg: &mut LGraphArena, info: &LabelDummyInfo, target_layer_index: i64) {
        if target_layer_index != info.leftmost_layer_id + info.left_long_edge_dummies.len() as i64 {
            let other = info.ith_dummy_node(target_layer_index - info.leftmost_layer_id);
            swap_nodes(lg, info.label_dummy, other);
        }

        let Some(new_layer) = lg[info.label_dummy].layer else { return };
        let new_layer_id = layer_id(lg, new_layer) as usize;
        self.layer_widths[new_layer_id] = swift::max(self.layer_widths[new_layer_id], lg[info.label_dummy].size.x);

        if let Some(represented_labels) = lg[info.label_dummy].props.get_as::<Vec<LLabelId>>(&InternalProperties::REPRESENTED_LABELS) {
            for label in represented_labels {
                lg[label].props.set(&INCLUDE_LABEL, true);
            }
        }
    }
}

// MARK: - Median Layer

fn find_median_layer_target_id(info: &LabelDummyInfo) -> i64 {
    let layers = info.total_dummy_count();
    let lower_median = (layers - 1) / 2;
    info.leftmost_layer_id + lower_median
}

// MARK: - End Layer

fn find_end_layer_target_id(lg: &LGraphArena, info: &LabelDummyInfo, head_layer: bool) -> i64 {
    let reversed = is_part_of_reversed_edge(lg, info);
    if (head_layer && !reversed) || (!head_layer && reversed) {
        info.rightmost_layer_id
    } else {
        info.leftmost_layer_id
    }
}

fn set_end_layer_node_alignment(lg: &mut LGraphArena, info: &LabelDummyInfo) {
    let is_head_label = info.placement_strategy == Strategy::HEAD_LAYER;
    let is_reversed = is_part_of_reversed_edge(lg, info);

    if (is_head_label && !is_reversed) || (!is_head_label && is_reversed) {
        lg[info.label_dummy].props.set(&LayeredOptions::ALIGNMENT, Alignment::RIGHT);
    } else {
        lg[info.label_dummy].props.set(&LayeredOptions::ALIGNMENT, Alignment::LEFT);
    }
}

fn is_part_of_reversed_edge(lg: &LGraphArena, info: &LabelDummyInfo) -> bool {
    let incoming = lg.node_incoming_edges(info.label_dummy).first().copied();
    let outgoing = lg.node_outgoing_edges(info.label_dummy).first().copied();

    let in_reversed = incoming.and_then(|e| lg[e].props.get_as::<bool>(&InternalProperties::REVERSED)).unwrap_or(false);
    let out_reversed = outgoing.and_then(|e| lg[e].props.get_as::<bool>(&InternalProperties::REVERSED)).unwrap_or(false);
    in_reversed || out_reversed
}

fn swap_nodes(lg: &mut LGraphArena, label_dummy: LNodeId, long_edge_dummy: LNodeId) {
    let (Some(layer1), Some(layer2)) = (lg[label_dummy].layer, lg[long_edge_dummy].layer) else { return };

    let dummy1_layer_position = lg[layer1].nodes.iter().position(|&n| n == label_dummy).unwrap_or(0);
    let dummy2_layer_position = lg[layer2].nodes.iter().position(|&n| n == long_edge_dummy).unwrap_or(0);

    let (Some(&input_port1), Some(&output_port1), Some(&input_port2), Some(&output_port2)) = (
        lg.node_ports_of_type(label_dummy, PortType::INPUT).first(),
        lg.node_ports_of_type(label_dummy, PortType::OUTPUT).first(),
        lg.node_ports_of_type(long_edge_dummy, PortType::INPUT).first(),
        lg.node_ports_of_type(long_edge_dummy, PortType::OUTPUT).first(),
    ) else {
        return;
    };

    let incoming_edges1 = lg[input_port1].incoming_edges.clone();
    let outgoing_edges1 = lg[output_port1].outgoing_edges.clone();
    let incoming_edges2 = lg[input_port2].incoming_edges.clone();
    let outgoing_edges2 = lg[output_port2].outgoing_edges.clone();

    lg.node_set_layer_at(label_dummy, dummy2_layer_position, layer2);
    for edge in incoming_edges2 {
        lg.edge_set_target(edge, Some(input_port1));
    }
    for edge in outgoing_edges2 {
        lg.edge_set_source(edge, Some(output_port1));
    }

    lg.node_set_layer_at(long_edge_dummy, dummy1_layer_position, layer1);
    for edge in incoming_edges1 {
        lg.edge_set_target(edge, Some(input_port2));
    }
    for edge in outgoing_edges1 {
        lg.edge_set_source(edge, Some(output_port2));
    }
}

fn update_long_edge_source_label_dummy_info(lg: &mut LGraphArena, info: &LabelDummyInfo) {
    do_update_long_edge_label_dummy_info(lg, info.label_dummy, previous_long_edge_node, true);
}

/// The `nextElement` closure of `updateLongEdgeSourceLabelDummyInfo`: the
/// source node of the node's first incoming edge.
fn previous_long_edge_node(lg: &LGraphArena, node: LNodeId) -> Option<LNodeId> {
    let edge = *lg.node_incoming_edges(node).first()?;
    lg.edge_source_node(edge)
}

fn do_update_long_edge_label_dummy_info(
    lg: &mut LGraphArena,
    label_dummy: LNodeId,
    next_element: fn(&LGraphArena, LNodeId) -> Option<LNodeId>,
    value: bool,
) {
    let Some(mut long_edge_dummy) = next_element(lg, label_dummy) else { return };
    while lg[long_edge_dummy].node_type == NodeType::LONG_EDGE {
        lg[long_edge_dummy].props.set(&InternalProperties::LONG_EDGE_BEFORE_LABEL_DUMMY, value);
        let Some(next) = next_element(lg, long_edge_dummy) else { return };
        long_edge_dummy = next;
    }
}

// MARK: - LabelDummyInfo

/// `LabelDummyInfo` (a Swift class; instances are only read after creation,
/// so a clone is indistinguishable from a reference).
#[derive(Clone, Debug)]
struct LabelDummyInfo {
    label_dummy: LNodeId,
    placement_strategy: Strategy,
    left_long_edge_dummies: Vec<LNodeId>,
    right_long_edge_dummies: Vec<LNodeId>,
    leftmost_layer_id: i64,
    rightmost_layer_id: i64,
}

impl LabelDummyInfo {
    fn new(lg: &LGraphArena, label_dummy: LNodeId, default_placement_strategy: Strategy) -> LabelDummyInfo {
        let mut info = LabelDummyInfo {
            label_dummy,
            placement_strategy: default_placement_strategy,
            left_long_edge_dummies: Vec::new(),
            right_long_edge_dummies: Vec::new(),
            leftmost_layer_id: 0,
            rightmost_layer_id: 0,
        };

        info.gather_left_long_edge_dummies(lg);
        info.gather_right_long_edge_dummies(lg);

        let layer_id_of = |n: LNodeId| lg[n].layer.map(|l| layer_id(lg, l));
        let label_dummy_layer_id = layer_id_of(label_dummy).unwrap_or(0);
        info.leftmost_layer_id = if info.left_long_edge_dummies.is_empty() {
            label_dummy_layer_id
        } else {
            layer_id_of(info.left_long_edge_dummies[0]).unwrap_or(label_dummy_layer_id)
        };
        info.rightmost_layer_id = if info.right_long_edge_dummies.is_empty() {
            label_dummy_layer_id
        } else {
            info.right_long_edge_dummies.last().and_then(|&n| layer_id_of(n)).unwrap_or(label_dummy_layer_id)
        };

        if let Some(represented_labels) = lg[label_dummy].props.get_as::<Vec<LLabelId>>(&InternalProperties::REPRESENTED_LABELS) {
            for label in represented_labels {
                if lg[label].props.has(&LayeredOptions::EDGE_LABELS_CENTER_LABEL_PLACEMENT_STRATEGY) {
                    if let Some(strategy) = lg[label].props.get_as::<Strategy>(&LayeredOptions::EDGE_LABELS_CENTER_LABEL_PLACEMENT_STRATEGY) {
                        info.placement_strategy = strategy;
                        break;
                    }
                }
            }
        }

        info
    }

    fn gather_left_long_edge_dummies(&mut self, lg: &LGraphArena) {
        let mut source = self.label_dummy;
        loop {
            let Some(&edge) = lg.node_incoming_edges(source).first() else { break };
            let Some(source_node) = lg.edge_source_node(edge) else { break };
            source = source_node;
            if lg[source].node_type == NodeType::LONG_EDGE {
                self.left_long_edge_dummies.push(source);
            }
            if lg[source].node_type != NodeType::LONG_EDGE {
                break;
            }
        }
        self.left_long_edge_dummies.reverse();
    }

    fn gather_right_long_edge_dummies(&mut self, lg: &LGraphArena) {
        let mut target = self.label_dummy;
        loop {
            let Some(&edge) = lg.node_outgoing_edges(target).first() else { break };
            let Some(target_node) = lg.edge_target_node(edge) else { break };
            target = target_node;
            if lg[target].node_type == NodeType::LONG_EDGE {
                self.right_long_edge_dummies.push(target);
            }
            if lg[target].node_type != NodeType::LONG_EDGE {
                break;
            }
        }
    }

    fn total_dummy_count(&self) -> i64 {
        self.rightmost_layer_id - self.leftmost_layer_id + 1
    }

    fn ith_dummy_node(&self, i: i64) -> LNodeId {
        let left = self.left_long_edge_dummies.len() as i64;
        if i < left {
            self.left_long_edge_dummies[i as usize]
        } else if i == left {
            self.label_dummy
        } else {
            self.right_long_edge_dummies[(i - left - 1) as usize]
        }
    }
}
