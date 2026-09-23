//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_HierarchicalNodeResizingProcessor.swift`.
//!
//! Resizes a nested graph and applies its size (and its external ports'
//! positions) to the parent node. elk-swift adds a transposition when the
//! child and parent directions differ in whether `GraphTransformer`
//! transposes them.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::graph_properties::GraphProperties;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::k_vector::{kvector_ref, KVector, KVectorRef};
use crate::org::eclipse::elk::core::options::content_alignment::ContentAlignment;
use crate::org::eclipse::elk::core::options::direction::Direction;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::org::eclipse::elk::core::util::elk_util::ElkUtil;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::swift;

#[derive(Default)]
pub struct HierarchicalNodeResizingProcessor;

impl HierarchicalNodeResizingProcessor {
    pub fn new() -> HierarchicalNodeResizingProcessor {
        HierarchicalNodeResizingProcessor
    }
}

impl ILayoutProcessor for HierarchicalNodeResizingProcessor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, progress_monitor: &mut dyn IElkProgressMonitor) {
        progress_monitor.begin("Resize child graph to fit parent.", 1.0);

        // Move all layer nodes to layerless
        for layer in lg[graph].layers.clone() {
            let nodes = std::mem::take(&mut lg[layer].nodes);
            lg[graph].layerless_nodes.extend(nodes);
        }
        for node in lg[graph].layerless_nodes.clone() {
            lg.node_set_layer(node, None);
        }
        lg[graph].layers.clear();

        resize_graph(lg, graph);

        if is_nested(lg, graph) {
            if let Some(parent_node) = lg[graph].parent_node {
                graph_layout_to_node(lg, parent_node, graph);
            }
        }

        progress_monitor.done();
    }

    fn name(&self) -> &'static str {
        "HierarchicalNodeResizingProcessor"
    }
}

fn graph_layout_to_node(lg: &mut LGraphArena, node: LNodeId, lgraph: LGraphId) {
    // Check if child and parent graphs have different transposition behavior.
    // DOWN/UP directions transpose x↔y in GraphTransformer; RIGHT/LEFT do not.
    // When they differ, sizes and port positions from the child graph must be
    // transposed to match the parent graph's internal coordinate system.
    let child_dir = lg[lgraph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
    let parent_dir = lg
        .node_graph(node)
        .and_then(|g| lg[g].props.get_as::<Direction>(&LayeredOptions::DIRECTION))
        .unwrap_or(Direction::UNDEFINED);
    let needs_transpose = direction_transposes(child_dir) != direction_transposes(parent_dir);

    // Process external ports
    for child_node in lg[lgraph].layerless_nodes.clone() {
        if let Some(port) = lg[child_node].props.get_as::<LPortId>(&InternalProperties::ORIGIN) {
            let port_size = lg[port].size;
            let port_position = lg.get_external_port_position(lgraph, child_node, port_size.x, port_size.y);
            if needs_transpose {
                lg[port].position.x = port_position.y;
                lg[port].position.y = port_position.x;
            } else {
                lg[port].position.x = port_position.x;
                lg[port].position.y = port_position.y;
            }
            // Keep the port side from the child graph's direction — an LR subgraph
            // should have entry on WEST (left) and exit on EAST (right), not transposed
            // to NORTH/SOUTH which would force top/bottom entry.
            if let Some(side) = lg[child_node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
                lg.port_set_side(port, side);
            }
        }
    }

    // Setup the parent node
    let mut actual_graph_size = lg.graph_actual_size(lgraph);
    if needs_transpose {
        let temp = actual_graph_size.x;
        actual_graph_size.x = actual_graph_size.y;
        actual_graph_size.y = temp;
    }
    let graph_properties = lg[lgraph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();

    if graph_properties.contains(GraphProperties::EXTERNAL_PORTS) {
        lg[node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
        if let Some(parent_graph) = lg.node_graph(node) {
            if let Some(mut parent_graph_props) = lg[parent_graph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES) {
                parent_graph_props.insert(GraphProperties::NON_FREE_PORTS);
                lg[parent_graph].props.set(&InternalProperties::GRAPH_PROPERTIES, parent_graph_props);
            }
        }
        lg.resize_node(node, actual_graph_size, false, true);
    } else {
        lg.resize_node(node, actual_graph_size, true, true);
    }
}

/// Whether the given direction uses coordinate transposition in GraphTransformer.
/// DOWN and UP transpose x↔y; RIGHT and LEFT do not.
fn direction_transposes(dir: Direction) -> bool {
    dir == Direction::DOWN || dir == Direction::UP
}

fn is_nested(lg: &LGraphArena, graph: LGraphId) -> bool {
    lg[graph].parent_node.is_some()
}

fn resize_graph(lg: &mut LGraphArena, lgraph: LGraphId) {
    let size_constraint = lg[lgraph].props.get_as::<SizeConstraint>(&LayeredOptions::NODE_SIZE_CONSTRAINTS).unwrap_or_default();
    let size_options = lg[lgraph].props.get_as::<SizeOptions>(&LayeredOptions::NODE_SIZE_OPTIONS).unwrap_or_default();

    let calculated_size = lg.graph_actual_size(lgraph);
    let mut adjusted_size = calculated_size;

    if size_constraint.contains(SizeConstraint::MINIMUM_SIZE) {
        // `as? KVector ?? KVector()`: the stored vector itself (mutated below,
        // as in Swift) or a fresh one.
        let min_size: KVectorRef = lg[lgraph].props.get_as::<KVectorRef>(&LayeredOptions::NODE_SIZE_MINIMUM).unwrap_or_else(|| kvector_ref(KVector::default()));

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

    resize_graph_no_really_i_mean_it(lg, lgraph, calculated_size, adjusted_size);
}

fn resize_graph_no_really_i_mean_it(lg: &mut LGraphArena, lgraph: LGraphId, old_size: KVector, new_size: KVector) {
    let content_alignment = lg[lgraph].props.get_as::<ContentAlignment>(&LayeredOptions::CONTENT_ALIGNMENT).unwrap_or_default();

    // horizontal alignment
    if new_size.x > old_size.x {
        if content_alignment.contains(ContentAlignment::H_CENTER) {
            lg[lgraph].offset.x += (new_size.x - old_size.x) / 2.0;
        } else if content_alignment.contains(ContentAlignment::H_RIGHT) {
            lg[lgraph].offset.x += new_size.x - old_size.x;
        }
    }

    // vertical alignment
    if new_size.y > old_size.y {
        if content_alignment.contains(ContentAlignment::V_CENTER) {
            lg[lgraph].offset.y += (new_size.y - old_size.y) / 2.0;
        } else if content_alignment.contains(ContentAlignment::V_BOTTOM) {
            lg[lgraph].offset.y += new_size.y - old_size.y;
        }
    }

    // correct the position of eastern and southern hierarchical ports
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

    // Actually apply the new size
    let padding = lg[lgraph].padding;
    lg[lgraph].size.x = new_size.x - padding.left - padding.right;
    lg[lgraph].size.y = new_size.y - padding.top - padding.bottom;
}
