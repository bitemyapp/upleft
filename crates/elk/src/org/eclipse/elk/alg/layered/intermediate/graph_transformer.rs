//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_GraphTransformer.swift`.
//!
//! Mirrors and transposes the layered graph so that the phases can work left
//! to right, and back. Note elk-swift's factory maps `DIRECTION_PREPROCESSOR`
//! to `TO_INPUT_DIRECTION` and `DIRECTION_POSTPROCESSOR` to `TO_INTERNAL_LTR`
//! (the reverse of Java ELK); that mapping lives in
//! `intermediate_processor_strategy.rs`.
//!
//! Aliasing: Swift transforms `KVector` objects in place. Every vector touched
//! here is reached once per node (positions, sizes, anchors, bend points,
//! junction points, label positions); none of them is shared with another
//! transformed location by the Swift code that runs before this processor
//! (`JUNCTION_POINTS` chains and bend points are always filled with fresh
//! vectors, `LongEdgeJoiner` copies bend points). The one known alias, an
//! external port dummy's `PORT_ANCHOR` property, *is* its port's `position`
//! (see `LGraphArena::node_port_anchor`), so mirroring the port position
//! moves the property too, as in Swift. Spacing and `POSITION` values stored
//! in properties are shared `Rc`s and are mutated in place, so graphs that
//! share one (components copy their parent's properties) see the change, as
//! in Swift.

use std::cell::RefCell;
use std::rc::Rc;

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::direction_congruency::DirectionCongruency;
use crate::org::eclipse::elk::alg::layered::options::edge_label_side_selection::EdgeLabelSideSelection;
use crate::org::eclipse::elk::alg::layered::options::in_layer_constraint::InLayerConstraint;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::k_vector::{KVector, KVectorRef};
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChainRef;
use crate::org::eclipse::elk::core::math::spacing::Spacing;
use crate::org::eclipse::elk::core::options::alignment::Alignment;
use crate::org::eclipse::elk::core::options::direction::Direction;
use crate::org::eclipse::elk::core::options::node_label_placement::NodeLabelPlacement;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;
use crate::org::eclipse::elk::graph::properties::property::PropValue;
use crate::swift;

/// `GraphTransformer.Mode` (declared in `IntermediateProcessorStrategy.swift`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    TO_INPUT_DIRECTION,
    TO_INTERNAL_LTR,
}

impl Mode {
    /// `Mode.defaults`.
    pub const fn defaults() -> Mode {
        Mode::TO_INPUT_DIRECTION
    }
}

pub struct GraphTransformer {
    pub mode: Mode,
}

impl GraphTransformer {
    pub fn new(mode: Mode) -> GraphTransformer {
        GraphTransformer { mode }
    }
}

impl ILayoutProcessor for GraphTransformer {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Graph transformation", 1.0);

        // Collect all nodes (layerless + in layers)
        let mut nodes: Vec<LNodeId> = lg[layered_graph].layerless_nodes.clone();
        for &layer in &lg[layered_graph].layers {
            nodes.extend_from_slice(&lg[layer].nodes);
        }

        // Default is READING_DIRECTION per ELK's Layered.melk definition
        let congruency = lg[layered_graph]
            .props
            .get_as::<DirectionCongruency>(&LayeredOptions::DIRECTION_CONGRUENCY)
            .unwrap_or(DirectionCongruency::READING_DIRECTION);
        let direction = lg[layered_graph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
        if congruency == DirectionCongruency::READING_DIRECTION {
            match direction {
                Direction::LEFT => mirror_all_x(lg, layered_graph, &nodes),
                Direction::DOWN => transpose_all(lg, layered_graph, &nodes),
                Direction::UP => {
                    if self.mode == Mode::TO_INTERNAL_LTR {
                        transpose_all(lg, layered_graph, &nodes);
                        mirror_all_y(lg, layered_graph, &nodes);
                    } else {
                        mirror_all_y(lg, layered_graph, &nodes);
                        transpose_all(lg, layered_graph, &nodes);
                    }
                }
                _ => {}
            }
        } else if self.mode == Mode::TO_INTERNAL_LTR {
            match direction {
                Direction::LEFT => {
                    mirror_all_x(lg, layered_graph, &nodes);
                    mirror_all_y(lg, layered_graph, &nodes);
                }
                Direction::DOWN => rotate90_clockwise(lg, layered_graph, &nodes),
                Direction::UP => rotate90_counter_clockwise(lg, layered_graph, &nodes),
                _ => {}
            }
        } else {
            match direction {
                Direction::LEFT => {
                    mirror_all_x(lg, layered_graph, &nodes);
                    mirror_all_y(lg, layered_graph, &nodes);
                }
                Direction::DOWN => rotate90_counter_clockwise(lg, layered_graph, &nodes),
                Direction::UP => rotate90_clockwise(lg, layered_graph, &nodes),
                _ => {}
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "GraphTransformer"
    }
}

// MARK: - Convenience

fn rotate90_clockwise(lg: &mut LGraphArena, graph: LGraphId, nodes: &[LNodeId]) {
    transpose_all(lg, graph, nodes);
    mirror_all_x(lg, graph, nodes);
}

fn rotate90_counter_clockwise(lg: &mut LGraphArena, graph: LGraphId, nodes: &[LNodeId]) {
    mirror_all_x(lg, graph, nodes);
    transpose_all(lg, graph, nodes);
}

/// Applies `f` to the graph's `NODE_LABELS_PADDING` value if it is a
/// `Spacing` (`as? Spacing` matches both `ElkPadding` and `ElkMargin`). The
/// value is a shared Swift object and is mutated in place.
fn with_node_labels_padding(lg: &mut LGraphArena, graph: LGraphId, f: fn(&mut Spacing)) {
    match lg[graph].props.get(&LayeredOptions::NODE_LABELS_PADDING) {
        Some(PropValue::ElkPadding(p)) => f(&mut p.borrow_mut().0),
        Some(PropValue::ElkMargin(m)) => f(&mut m.borrow_mut().0),
        _ => {}
    }
}

fn mirror_all_x(lg: &mut LGraphArena, graph: LGraphId, nodes: &[LNodeId]) {
    mirror_x(lg, nodes, graph);
    mirror_x_spacing(&mut lg[graph].padding.0);
    with_node_labels_padding(lg, graph, mirror_x_spacing);
}

fn mirror_all_y(lg: &mut LGraphArena, graph: LGraphId, nodes: &[LNodeId]) {
    mirror_y(lg, nodes, graph);
    mirror_y_spacing(&mut lg[graph].padding.0);
    with_node_labels_padding(lg, graph, mirror_y_spacing);
}

fn transpose_all(lg: &mut LGraphArena, graph: LGraphId, nodes: &[LNodeId]) {
    transpose_nodes(lg, nodes);
    transpose_edge_label_placement(lg, graph);
    transpose_vec(&mut lg[graph].offset);
    transpose_vec(&mut lg[graph].size);
    transpose_spacing(&mut lg[graph].padding.0);
    with_node_labels_padding(lg, graph, transpose_spacing);
}

/// `getAllProperties().keys.contains(POSITION.id)` then
/// `getProperty(POSITION) as? KVector`.
fn stored_position(props: &PropertyMap) -> Option<KVectorRef> {
    if props.has(&LayeredOptions::POSITION) {
        props.get_as::<KVectorRef>(&LayeredOptions::POSITION)
    } else {
        None
    }
}

// MARK: - Mirror Horizontally

fn mirror_x(lg: &mut LGraphArena, nodes: &[LNodeId], graph: LGraphId) {
    let mut offset: f64 = 0.0;

    if lg[graph].size.x == 0.0 {
        for &node in nodes {
            let n = &lg[node];
            offset = swift::max(offset, n.position.x + n.size.x + n.margin.right);
        }
    } else {
        offset = lg[graph].size.x - lg[graph].offset.x;
    }
    offset -= lg[graph].offset.x;

    for &node in nodes {
        let node_size_x = lg[node].size.x;
        mirror_x_vec(&mut lg[node].position, offset - node_size_x);
        mirror_x_spacing(&mut lg[node].padding.0);
        mirror_node_label_placement_x(&mut lg[node].props);

        if let Some(pos) = stored_position(&lg[node].props) {
            mirror_x_vec(&mut pos.borrow_mut(), offset - node_size_x);
        }

        // Mirror alignment
        let alignment = lg[node].props.get_as::<Alignment>(&LayeredOptions::ALIGNMENT);
        if alignment == Some(Alignment::LEFT) {
            lg[node].props.set(&LayeredOptions::ALIGNMENT, Alignment::RIGHT);
        } else if alignment == Some(Alignment::RIGHT) {
            lg[node].props.set(&LayeredOptions::ALIGNMENT, Alignment::LEFT);
        }

        let node_size = lg[node].size;
        for port in lg[node].ports.clone() {
            let port_size_x = lg[port].size.x;
            mirror_x_vec(&mut lg[port].position, node_size.x - port_size_x);
            mirror_x_vec(&mut lg[port].anchor, port_size_x);
            mirror_port_side_x(lg, port);
            reverse_index(lg, port);

            for edge in lg[port].outgoing_edges.clone() {
                for bend_point in lg[edge].bend_points.iter_mut() {
                    mirror_x_vec(bend_point, offset);
                }

                if let Some(junction_points) = lg[edge].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS) {
                    for jp in junction_points.borrow_mut().iter_mut() {
                        mirror_x_vec(jp, offset);
                    }
                }

                for label in lg[edge].labels.clone() {
                    let label_size_x = lg[label].size.x;
                    mirror_x_vec(&mut lg[label].position, offset - label_size_x);
                }
            }

            for label in lg[port].labels.clone() {
                let label_size_x = lg[label].size.x;
                mirror_x_vec(&mut lg[label].position, port_size_x - label_size_x);
            }
        }

        if lg[node].node_type == NodeType::EXTERNAL_PORT {
            mirror_external_port_side_x(&mut lg[node].props);
            mirror_layer_constraint_x(&mut lg[node].props);
        }

        for label in lg[node].labels.clone() {
            mirror_node_label_placement_x(&mut lg[label].props);
            let label_size_x = lg[label].size.x;
            mirror_x_vec(&mut lg[label].position, node_size.x - label_size_x);
        }
    }
}

#[inline]
fn mirror_x_vec(v: &mut KVector, offset: f64) {
    v.x = offset - v.x;
}

fn mirror_x_spacing(spacing: &mut Spacing) {
    let old_left = spacing.left;
    let old_right = spacing.right;
    spacing.left = old_right;
    spacing.right = old_left;
}

fn mirror_node_label_placement_x(props: &mut PropertyMap) {
    if !props.has(&LayeredOptions::NODE_LABELS_PLACEMENT) {
        return;
    }
    if let Some(mut old_placement) = props.get_as::<NodeLabelPlacement>(&LayeredOptions::NODE_LABELS_PLACEMENT) {
        if old_placement.contains(NodeLabelPlacement::H_LEFT) {
            old_placement.remove(NodeLabelPlacement::H_LEFT);
            old_placement.insert(NodeLabelPlacement::H_RIGHT);
        } else if old_placement.contains(NodeLabelPlacement::H_RIGHT) {
            old_placement.remove(NodeLabelPlacement::H_RIGHT);
            old_placement.insert(NodeLabelPlacement::H_LEFT);
        }
        props.set(&LayeredOptions::NODE_LABELS_PLACEMENT, old_placement);
    }
}

fn mirror_port_side_x(lg: &mut LGraphArena, port: LPortId) {
    let side = get_mirrored_port_side_x(lg[port].side);
    lg.port_set_side(port, side);
}

fn mirror_external_port_side_x(props: &mut PropertyMap) {
    if let Some(side) = props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
        props.set(&InternalProperties::EXT_PORT_SIDE, get_mirrored_port_side_x(side));
    }
}

fn get_mirrored_port_side_x(side: PortSide) -> PortSide {
    match side {
        PortSide::EAST => PortSide::WEST,
        PortSide::WEST => PortSide::EAST,
        _ => side,
    }
}

fn mirror_layer_constraint_x(props: &mut PropertyMap) {
    let constraint = props.get_as::<LayerConstraint>(&LayeredOptions::LAYERING_LAYER_CONSTRAINT).unwrap_or(LayerConstraint::NONE);
    match constraint {
        LayerConstraint::FIRST => props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::LAST),
        LayerConstraint::FIRST_SEPARATE => props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::LAST_SEPARATE),
        LayerConstraint::LAST => props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::FIRST),
        LayerConstraint::LAST_SEPARATE => props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::FIRST_SEPARATE),
        _ => {}
    }
}

// MARK: - Mirror Vertically

fn mirror_y(lg: &mut LGraphArena, nodes: &[LNodeId], graph: LGraphId) {
    let mut offset: f64 = 0.0;
    if lg[graph].size.y == 0.0 {
        for &node in nodes {
            let n = &lg[node];
            offset = swift::max(offset, n.position.y + n.size.y + n.margin.bottom);
        }
    } else {
        offset = lg[graph].size.y - lg[graph].offset.y;
    }
    offset -= lg[graph].offset.y;

    for &node in nodes {
        let node_size_y = lg[node].size.y;
        mirror_y_vec(&mut lg[node].position, offset - node_size_y);
        mirror_y_spacing(&mut lg[node].padding.0);
        mirror_node_label_placement_y(&mut lg[node].props);

        if let Some(pos) = stored_position(&lg[node].props) {
            mirror_y_vec(&mut pos.borrow_mut(), offset - node_size_y);
        }

        let alignment = lg[node].props.get_as::<Alignment>(&LayeredOptions::ALIGNMENT);
        if alignment == Some(Alignment::TOP) {
            lg[node].props.set(&LayeredOptions::ALIGNMENT, Alignment::BOTTOM);
        } else if alignment == Some(Alignment::BOTTOM) {
            lg[node].props.set(&LayeredOptions::ALIGNMENT, Alignment::TOP);
        }

        let node_size = lg[node].size;
        for port in lg[node].ports.clone() {
            let port_size_y = lg[port].size.y;
            mirror_y_vec(&mut lg[port].position, node_size.y - port_size_y);
            mirror_y_vec(&mut lg[port].anchor, port_size_y);
            mirror_port_side_y(lg, port);
            reverse_index(lg, port);

            for edge in lg[port].outgoing_edges.clone() {
                for bend_point in lg[edge].bend_points.iter_mut() {
                    mirror_y_vec(bend_point, offset);
                }

                if let Some(junction_points) = lg[edge].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS) {
                    for jp in junction_points.borrow_mut().iter_mut() {
                        mirror_y_vec(jp, offset);
                    }
                }

                for label in lg[edge].labels.clone() {
                    let label_size_y = lg[label].size.y;
                    mirror_y_vec(&mut lg[label].position, offset - label_size_y);
                }
            }

            for label in lg[port].labels.clone() {
                let label_size_y = lg[label].size.y;
                mirror_y_vec(&mut lg[label].position, port_size_y - label_size_y);
            }
        }

        if lg[node].node_type == NodeType::EXTERNAL_PORT {
            mirror_external_port_side_y(&mut lg[node].props);
            mirror_in_layer_constraint_y(&mut lg[node].props);
        }

        for label in lg[node].labels.clone() {
            mirror_node_label_placement_y(&mut lg[label].props);
            let label_size_y = lg[label].size.y;
            mirror_y_vec(&mut lg[label].position, node_size.y - label_size_y);
        }
    }
}

#[inline]
fn mirror_y_vec(v: &mut KVector, offset: f64) {
    v.y = offset - v.y;
}

fn mirror_y_spacing(spacing: &mut Spacing) {
    let old_top = spacing.top;
    let old_bottom = spacing.bottom;
    spacing.top = old_bottom;
    spacing.bottom = old_top;
}

fn mirror_node_label_placement_y(props: &mut PropertyMap) {
    if !props.has(&LayeredOptions::NODE_LABELS_PLACEMENT) {
        return;
    }
    if let Some(mut old_placement) = props.get_as::<NodeLabelPlacement>(&LayeredOptions::NODE_LABELS_PLACEMENT) {
        if old_placement.contains(NodeLabelPlacement::V_TOP) {
            old_placement.remove(NodeLabelPlacement::V_TOP);
            old_placement.insert(NodeLabelPlacement::V_BOTTOM);
        } else if old_placement.contains(NodeLabelPlacement::V_BOTTOM) {
            old_placement.remove(NodeLabelPlacement::V_BOTTOM);
            old_placement.insert(NodeLabelPlacement::V_TOP);
        }
        props.set(&LayeredOptions::NODE_LABELS_PLACEMENT, old_placement);
    }
}

fn mirror_port_side_y(lg: &mut LGraphArena, port: LPortId) {
    let side = get_mirrored_port_side_y(lg[port].side);
    lg.port_set_side(port, side);
}

fn mirror_external_port_side_y(props: &mut PropertyMap) {
    if let Some(side) = props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
        props.set(&InternalProperties::EXT_PORT_SIDE, get_mirrored_port_side_y(side));
    }
}

fn get_mirrored_port_side_y(side: PortSide) -> PortSide {
    match side {
        PortSide::NORTH => PortSide::SOUTH,
        PortSide::SOUTH => PortSide::NORTH,
        _ => side,
    }
}

fn mirror_in_layer_constraint_y(props: &mut PropertyMap) {
    let constraint = props.get_as::<InLayerConstraint>(&InternalProperties::IN_LAYER_CONSTRAINT).unwrap_or(InLayerConstraint::NONE);
    match constraint {
        InLayerConstraint::TOP => props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::BOTTOM),
        InLayerConstraint::BOTTOM => props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::TOP),
        _ => {}
    }
}

// MARK: - Transpose

fn transpose_nodes(lg: &mut LGraphArena, nodes: &[LNodeId]) {
    for &node in nodes {
        transpose_vec(&mut lg[node].position);
        transpose_vec(&mut lg[node].size);
        transpose_spacing(&mut lg[node].padding.0);
        transpose_node_label_placement(&mut lg[node].props);
        transpose_properties(&mut lg[node].props);

        for port in lg[node].ports.clone() {
            transpose_vec(&mut lg[port].position);
            transpose_vec(&mut lg[port].anchor);
            transpose_vec(&mut lg[port].size);
            transpose_port_side(lg, port);
            reverse_index(lg, port);

            for edge in lg[port].outgoing_edges.clone() {
                for bend_point in lg[edge].bend_points.iter_mut() {
                    transpose_vec(bend_point);
                }

                if let Some(junction_points) = lg[edge].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS) {
                    for jp in junction_points.borrow_mut().iter_mut() {
                        transpose_vec(jp);
                    }
                }

                for label in lg[edge].labels.clone() {
                    transpose_vec(&mut lg[label].position);
                    transpose_vec(&mut lg[label].size);
                }
            }

            for label in lg[port].labels.clone() {
                transpose_vec(&mut lg[label].position);
                transpose_vec(&mut lg[label].size);
            }
        }

        if lg[node].node_type == NodeType::EXTERNAL_PORT {
            transpose_external_port_side(&mut lg[node].props);
            transpose_layer_constraint(&mut lg[node].props);
        }

        for label in lg[node].labels.clone() {
            transpose_node_label_placement(&mut lg[label].props);
            transpose_vec(&mut lg[label].size);
            transpose_vec(&mut lg[label].position);
        }
    }
}

#[inline]
fn transpose_vec(v: &mut KVector) {
    let temp = v.x;
    v.x = v.y;
    v.y = temp;
}

fn transpose_spacing(spacing: &mut Spacing) {
    let old_top = spacing.top;
    let old_bottom = spacing.bottom;
    let old_left = spacing.left;
    let old_right = spacing.right;

    spacing.top = old_left;
    spacing.bottom = old_right;
    spacing.left = old_top;
    spacing.right = old_bottom;
}

fn transpose_node_label_placement(props: &mut PropertyMap) {
    if !props.has(&LayeredOptions::NODE_LABELS_PLACEMENT) {
        return;
    }
    let Some(old_placement) = props.get_as::<NodeLabelPlacement>(&LayeredOptions::NODE_LABELS_PLACEMENT) else { return };
    if old_placement.is_empty() {
        return;
    }

    let mut new_placement = NodeLabelPlacement::empty();

    // Inside or outside
    if old_placement.contains(NodeLabelPlacement::INSIDE) {
        new_placement.insert(NodeLabelPlacement::INSIDE);
    } else {
        new_placement.insert(NodeLabelPlacement::OUTSIDE);
    }

    // Horizontal priority
    if !old_placement.contains(NodeLabelPlacement::H_PRIORITY) {
        new_placement.insert(NodeLabelPlacement::H_PRIORITY);
    }

    // Horizontal alignment -> vertical
    if old_placement.contains(NodeLabelPlacement::H_LEFT) {
        new_placement.insert(NodeLabelPlacement::V_TOP);
    } else if old_placement.contains(NodeLabelPlacement::H_CENTER) {
        new_placement.insert(NodeLabelPlacement::V_CENTER);
    } else if old_placement.contains(NodeLabelPlacement::H_RIGHT) {
        new_placement.insert(NodeLabelPlacement::V_BOTTOM);
    }

    // Vertical alignment -> horizontal
    if old_placement.contains(NodeLabelPlacement::V_TOP) {
        new_placement.insert(NodeLabelPlacement::H_LEFT);
    } else if old_placement.contains(NodeLabelPlacement::V_CENTER) {
        new_placement.insert(NodeLabelPlacement::H_CENTER);
    } else if old_placement.contains(NodeLabelPlacement::V_BOTTOM) {
        new_placement.insert(NodeLabelPlacement::H_RIGHT);
    }

    props.set(&LayeredOptions::NODE_LABELS_PLACEMENT, new_placement);
}

fn transpose_port_side(lg: &mut LGraphArena, port: LPortId) {
    let side = transposed_port_side(lg[port].side);
    lg.port_set_side(port, side);
}

/// `transposedPortSide(_:)`.
pub fn transposed_port_side(side: PortSide) -> PortSide {
    match side {
        PortSide::NORTH => PortSide::WEST,
        PortSide::WEST => PortSide::NORTH,
        PortSide::SOUTH => PortSide::EAST,
        PortSide::EAST => PortSide::SOUTH,
        _ => PortSide::UNDEFINED,
    }
}

fn transpose_edge_label_placement(lg: &mut LGraphArena, graph: LGraphId) {
    if let Some(old_side) = lg[graph].props.get_as::<EdgeLabelSideSelection>(&LayeredOptions::EDGE_LABELS_SIDE_SELECTION) {
        lg[graph].props.set(&LayeredOptions::EDGE_LABELS_SIDE_SELECTION, old_side.transpose());
    }
}

fn transpose_external_port_side(props: &mut PropertyMap) {
    if let Some(side) = props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
        props.set(&InternalProperties::EXT_PORT_SIDE, transposed_port_side(side));
    }
}

fn transpose_layer_constraint(props: &mut PropertyMap) {
    let layer_constraint = props.get_as::<LayerConstraint>(&LayeredOptions::LAYERING_LAYER_CONSTRAINT).unwrap_or(LayerConstraint::NONE);
    let in_layer_constraint = props.get_as::<InLayerConstraint>(&InternalProperties::IN_LAYER_CONSTRAINT).unwrap_or(InLayerConstraint::NONE);

    if layer_constraint == LayerConstraint::FIRST_SEPARATE {
        props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::NONE);
        props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::TOP);
    } else if layer_constraint == LayerConstraint::LAST_SEPARATE {
        props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::NONE);
        props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::BOTTOM);
    } else if in_layer_constraint == InLayerConstraint::TOP {
        props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::FIRST_SEPARATE);
        props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::NONE);
    } else if in_layer_constraint == InLayerConstraint::BOTTOM {
        props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::LAST_SEPARATE);
        props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::NONE);
    }
}

fn transpose_properties(props: &mut PropertyMap) {
    // Transpose MIN_HEIGHT and MIN_WIDTH (NODE_SIZE_MINIMUM): a new vector
    // replaces the stored one (the old object is left untouched, as in Swift).
    if let Some(min_size) = props.get_as::<KVectorRef>(&LayeredOptions::NODE_SIZE_MINIMUM) {
        let m = *min_size.borrow();
        props.set(&LayeredOptions::NODE_SIZE_MINIMUM, PropValue::KVector(Rc::new(RefCell::new(KVector::new(m.y, m.x)))));
    }

    // Transpose ALIGNMENT
    match props.get_as::<Alignment>(&LayeredOptions::ALIGNMENT) {
        Some(Alignment::LEFT) => props.set(&LayeredOptions::ALIGNMENT, Alignment::TOP),
        Some(Alignment::RIGHT) => props.set(&LayeredOptions::ALIGNMENT, Alignment::BOTTOM),
        Some(Alignment::TOP) => props.set(&LayeredOptions::ALIGNMENT, Alignment::LEFT),
        Some(Alignment::BOTTOM) => props.set(&LayeredOptions::ALIGNMENT, Alignment::RIGHT),
        _ => {}
    }

    // POSITION
    if let Some(pos) = stored_position(props) {
        let mut pos = pos.borrow_mut();
        let tmp = pos.x;
        pos.x = pos.y;
        pos.y = tmp;
    }
}

fn reverse_index(lg: &mut LGraphArena, port: LPortId) {
    if let Some(index) = lg[port].props.get_as::<i64>(&LayeredOptions::PORT_INDEX) {
        lg[port].props.set(&LayeredOptions::PORT_INDEX, -index);
    }
}
