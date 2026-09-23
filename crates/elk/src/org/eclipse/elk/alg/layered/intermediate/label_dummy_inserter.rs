//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_LabelDummyInserter.swift`.
//!
//! Replaces every edge that carries center labels by a label dummy node
//! (splitting the edge with `LongEdgeSplitter.splitEdge`) that represents the
//! labels during layout.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LGraphId, LLabelId, LNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::direction::Direction;
use crate::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::org::eclipse::elk::graph::properties::property::PropValue;
use crate::swift;

/// `LongEdgeSplitter.splitEdge(_:_:)`.
fn split_edge(lg: &mut LGraphArena, edge: LEdgeId, dummy_node: LNodeId) -> LEdgeId {
    crate::org::eclipse::elk::alg::layered::intermediate::long_edge_splitter::LongEdgeSplitter::split_edge(lg, edge, dummy_node)
}

#[derive(Default)]
pub struct LabelDummyInserter;

impl LabelDummyInserter {
    pub fn new() -> LabelDummyInserter {
        LabelDummyInserter
    }
}

impl ILayoutProcessor for LabelDummyInserter {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Label dummy insertions", 1.0);

        let mut new_dummy_nodes: Vec<LNodeId> = Vec::new();

        let edge_label_spacing = lg[layered_graph].props.get_as::<f64>(&LayeredOptions::SPACING_EDGE_LABEL).unwrap_or(0.0);
        let label_label_spacing = lg[layered_graph].props.get_as::<f64>(&LayeredOptions::SPACING_LABEL_LABEL).unwrap_or(0.0);
        let layout_direction = lg[layered_graph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);

        for node in lg[layered_graph].layerless_nodes.clone() {
            for edge in lg.node_outgoing_edges(node) {
                if edge_needs_to_be_processed(lg, edge) {
                    let thickness = retrieve_thickness(lg, edge);

                    let mut represented_labels: Vec<LLabelId> = Vec::new();
                    let dummy_node = create_label_dummy(lg, layered_graph, edge, thickness, &represented_labels);
                    new_dummy_nodes.push(dummy_node);

                    // `dummySize` is the dummy's own size vector.
                    let mut i = 0;
                    while i < lg[edge].labels.len() {
                        let label = lg[edge].labels[i];

                        if lg[label].props.get_as::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT) == Some(EdgeLabelPlacement::CENTER) {
                            let label_size = lg[label].size;
                            let dummy_size = &mut lg[dummy_node].size;
                            if layout_direction.is_vertical() {
                                dummy_size.x += label_size.x + label_label_spacing;
                                dummy_size.y = swift::max(dummy_size.y, label_size.y);
                            } else {
                                dummy_size.x = swift::max(dummy_size.x, label_size.x);
                                dummy_size.y += label_size.y + label_label_spacing;
                            }

                            represented_labels.push(label);
                            lg[edge].labels.remove(i);
                        } else {
                            i += 1;
                        }
                    }

                    // Update the property with the filled list
                    lg[dummy_node].props.set(&InternalProperties::REPRESENTED_LABELS, represented_labels);

                    let dummy_size = &mut lg[dummy_node].size;
                    if layout_direction.is_vertical() {
                        dummy_size.x -= label_label_spacing;
                        dummy_size.y += edge_label_spacing + thickness;
                    } else {
                        dummy_size.y += edge_label_spacing - label_label_spacing + thickness;
                    }
                }
            }
        }

        lg[layered_graph].layerless_nodes.extend(new_dummy_nodes);

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LabelDummyInserter"
    }
}

fn edge_needs_to_be_processed(lg: &LGraphArena, edge: LEdgeId) -> bool {
    // `edge.source?.node !== edge.target?.node` (two missing ends are equal).
    if lg.edge_source_node(edge) == lg.edge_target_node(edge) {
        return false;
    }
    lg[edge]
        .labels
        .iter()
        .any(|&label| lg[label].props.get_as::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT) == Some(EdgeLabelPlacement::CENTER))
}

fn retrieve_thickness(lg: &mut LGraphArena, edge: LEdgeId) -> f64 {
    let mut thickness = lg[edge].props.get_as::<f64>(&LayeredOptions::EDGE_THICKNESS).unwrap_or(0.0);
    if thickness < 0.0 {
        thickness = 0.0;
        lg[edge].props.set(&LayeredOptions::EDGE_THICKNESS, thickness);
    }
    thickness
}

fn create_label_dummy(lg: &mut LGraphArena, layered_graph: LGraphId, edge: LEdgeId, thickness: f64, represented_labels: &[LLabelId]) -> LNodeId {
    let dummy_node = lg.new_node(Some(layered_graph));
    lg[dummy_node].node_type = NodeType::LABEL;
    lg[dummy_node].props.set(&InternalProperties::ORIGIN, PropValue::LEdge(edge));
    lg[dummy_node].props.set(&InternalProperties::REPRESENTED_LABELS, represented_labels.to_vec());
    lg[dummy_node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
    let (Some(src), Some(tgt)) = (lg[edge].source, lg[edge].target) else { return dummy_node };
    lg[dummy_node].props.set(&InternalProperties::LONG_EDGE_SOURCE, PropValue::LPort(src));
    lg[dummy_node].props.set(&InternalProperties::LONG_EDGE_TARGET, PropValue::LPort(tgt));

    split_edge(lg, edge, dummy_node);

    let port_pos = (thickness / 2.0).floor();
    for dummy_port in lg[dummy_node].ports.clone() {
        lg[dummy_port].position.y = port_pos;
    }

    dummy_node
}
