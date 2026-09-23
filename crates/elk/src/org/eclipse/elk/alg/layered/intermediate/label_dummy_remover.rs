//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_LabelDummyRemover.swift`.
//!
//! Places the labels represented by label dummies and joins the edges the
//! dummies split (`LongEdgeJoiner.joinAt`).

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LGraphId, LLabelId, LNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::options::direction::Direction;
use crate::org::eclipse::elk::core::options::edge_routing::EdgeRouting;
use crate::org::eclipse::elk::core::options::label_side::LabelSide;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

/// `LongEdgeJoiner.joinAt(_:_:)`.
fn join_at(lg: &mut LGraphArena, long_edge_dummy: LNodeId, add_unnecessary_bendpoints: bool) {
    crate::org::eclipse::elk::alg::layered::intermediate::long_edge_joiner::LongEdgeJoiner::join_at(lg, long_edge_dummy, add_unnecessary_bendpoints)
}

#[derive(Default)]
pub struct LabelDummyRemover;

impl LabelDummyRemover {
    pub fn new() -> LabelDummyRemover {
        LabelDummyRemover
    }
}

impl ILayoutProcessor for LabelDummyRemover {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Label dummy removal", 1.0);

        let edge_label_spacing = lg[layered_graph].props.get_as::<f64>(&LayeredOptions::SPACING_EDGE_LABEL).unwrap_or(0.0);
        let label_label_spacing = lg[layered_graph].props.get_as::<f64>(&LayeredOptions::SPACING_LABEL_LABEL).unwrap_or(0.0);
        let layout_direction = lg[layered_graph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);

        for layer in lg[layered_graph].layers.clone() {
            let mut i = 0;
            while i < lg[layer].nodes.len() {
                let node = lg[layer].nodes[i];

                if lg[node].node_type == NodeType::LABEL {
                    let Some(origin_edge) = lg[node].props.get_as::<LEdgeId>(&InternalProperties::ORIGIN) else {
                        i += 1;
                        continue;
                    };
                    let thickness = lg[origin_edge].props.get_as::<f64>(&LayeredOptions::EDGE_THICKNESS).unwrap_or(0.0);
                    let labels_below_edge = lg[node].props.get_as::<LabelSide>(&InternalProperties::LABEL_SIDE) == Some(LabelSide::BELOW);

                    let mut curr_label_pos = lg[node].position;

                    if labels_below_edge {
                        curr_label_pos.y += thickness + edge_label_spacing;
                    }

                    let inline = lg.node_is_inline_edge_label(node);
                    let node_size = lg[node].size;
                    let label_space = KVector::new(node_size.x, node_size.y + if inline { 0.0 } else { -thickness - edge_label_spacing });

                    let represented_labels: Vec<LLabelId> = lg[node].props.get_as::<Vec<LLabelId>>(&InternalProperties::REPRESENTED_LABELS).unwrap_or_default();

                    if layout_direction.is_vertical() {
                        place_labels_for_vertical_layout(
                            lg,
                            &represented_labels,
                            &mut curr_label_pos,
                            label_label_spacing,
                            label_space,
                            labels_below_edge,
                            layout_direction,
                        );
                    } else {
                        place_labels_for_horizontal_layout(lg, &represented_labels, &mut curr_label_pos, label_label_spacing, label_space);
                    }

                    lg[origin_edge].labels.extend_from_slice(&represented_labels);

                    let edge_routing = lg[layered_graph].props.get_as::<EdgeRouting>(&LayeredOptions::EDGE_ROUTING);
                    join_at(lg, node, edge_routing == Some(EdgeRouting::POLYLINE));

                    // Swift removes the node from the layer's array directly;
                    // the node keeps its `layer` reference.
                    lg[layer].nodes.remove(i);
                } else {
                    i += 1;
                }
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LabelDummyRemover"
    }
}

fn place_labels_for_horizontal_layout(lg: &mut LGraphArena, labels: &[LLabelId], label_pos: &mut KVector, label_spacing: f64, label_space: KVector) {
    for &label in labels {
        let l = &mut lg[label];
        l.position.x = label_pos.x + (label_space.x - l.size.x) / 2.0;
        l.position.y = label_pos.y;
        label_pos.y += l.size.y + label_spacing;
    }
}

fn place_labels_for_vertical_layout(
    lg: &mut LGraphArena,
    labels: &[LLabelId],
    label_pos: &mut KVector,
    label_spacing: f64,
    label_space: KVector,
    left_aligned: bool,
    layout_direction: Direction,
) {
    let inline = labels.iter().all(|&l| lg[l].props.get_as::<bool>(&LayeredOptions::EDGE_LABELS_INLINE).unwrap_or(false));

    let mut effective_labels = labels.to_vec();
    if layout_direction == Direction::UP {
        effective_labels.reverse();
    }

    for label in effective_labels {
        let l = &mut lg[label];
        l.position.x = label_pos.x;

        if inline {
            l.position.y = label_pos.y + (label_space.y - l.size.y) / 2.0;
        } else if left_aligned {
            l.position.y = label_pos.y;
        } else {
            l.position.y = label_pos.y + label_space.y - l.size.y;
        }

        label_pos.x += l.size.x + label_spacing;
    }
}
