//! Port of `alg/layered/graph/LGraphUtil.swift`.

use super::l_graph::{LGraphArena, LGraphId, LNodeId, LPortId, LayerId};
use super::l_node::NodeType;
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::options::edge_constraint::EdgeConstraint;
use crate::org::eclipse::elk::alg::layered::options::graph_properties::GraphProperties;
use crate::org::eclipse::elk::alg::layered::options::in_layer_constraint::InLayerConstraint;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
use crate::org::eclipse::elk::core::math::k_vector::{KVector, KVectorRef};
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChainRef;
use crate::org::eclipse::elk::core::options::alignment::Alignment;
use crate::org::eclipse::elk::core::options::core_options as CoreOptions;
use crate::org::eclipse::elk::core::options::direction::Direction;
use crate::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;
use crate::org::eclipse::elk::graph::properties::property::{PropValue, Property};
use crate::swift;

/// `LGraphUtil.centerPoint(_:boundary:side:)`.
pub fn center_point(point: &mut KVector, boundary: KVector, side: PortSide) {
    match side {
        PortSide::NORTH => {
            point.x = boundary.x / 2.0;
            point.y = 0.0;
        }
        PortSide::EAST => {
            point.x = boundary.x;
            point.y = boundary.y / 2.0;
        }
        PortSide::SOUTH => {
            point.x = boundary.x / 2.0;
            point.y = boundary.y;
        }
        PortSide::WEST => {
            point.x = 0.0;
            point.y = boundary.y / 2.0;
        }
        _ => {}
    }
}

impl LGraphArena {
    /// `LGraphUtil.resizeNode(_:newSize:movePorts:moveLabels:)`.
    pub fn resize_node(&mut self, node: LNodeId, new_size: KVector, move_ports: bool, move_labels: bool) {
        let old_size = self[node].size;
        let width_ratio = new_size.x / old_size.x;
        let height_ratio = new_size.y / old_size.y;
        let width_diff = new_size.x - old_size.x;
        let height_diff = new_size.y - old_size.y;

        if move_ports {
            let fixed_ports = self[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS) == Some(PortConstraints::FIXED_POS);
            for port in self[node].ports.clone() {
                let p = &mut self[port];
                match p.side {
                    PortSide::NORTH => {
                        if !fixed_ports {
                            p.position.x *= width_ratio;
                        }
                    }
                    PortSide::EAST => {
                        p.position.x += width_diff;
                        if !fixed_ports {
                            p.position.y *= height_ratio;
                        }
                    }
                    PortSide::SOUTH => {
                        if !fixed_ports {
                            p.position.x *= width_ratio;
                        }
                        p.position.y += height_diff;
                    }
                    PortSide::WEST => {
                        if !fixed_ports {
                            p.position.y *= height_ratio;
                        }
                    }
                    _ => {}
                }
            }
        }

        if move_labels {
            for label in self[node].labels.clone() {
                let l = &mut self[label];
                let midx = l.position.x + l.size.x / 2.0;
                let midy = l.position.y + l.size.y / 2.0;
                let width_percent = midx / old_size.x;
                let height_percent = midy / old_size.y;
                if width_percent + height_percent >= 1.0 {
                    if width_percent - height_percent > 0.0 && midy >= 0.0 {
                        l.position.x += width_diff;
                        l.position.y += height_diff * height_percent;
                    } else if width_percent - height_percent < 0.0 && midx >= 0.0 {
                        l.position.x += width_diff * width_percent;
                        l.position.y += height_diff;
                    }
                }
            }
        }

        self[node].size.x = new_size.x;
        self[node].size.y = new_size.y;
        self[node].props.set(&LayeredOptions::NODE_SIZE_CONSTRAINTS, SizeConstraint::fixed());
    }

    /// `LGraphUtil.offsetGraphs(_:offsetx:offsety:)`.
    pub fn offset_graphs(&mut self, graphs: &[LGraphId], offsetx: f64, offsety: f64) {
        for &g in graphs {
            self.offset_graph(g, offsetx, offsety);
        }
    }

    /// `LGraphUtil.offsetGraph(_:offsetx:offsety:)`.
    pub fn offset_graph(&mut self, graph: LGraphId, offsetx: f64, offsety: f64) {
        let graph_offset = KVector::new(offsetx, offsety);
        for node in self[graph].layerless_nodes.clone() {
            self[node].position.add(graph_offset);
            for port in self[node].ports.clone() {
                for edge in self[port].outgoing_edges.clone() {
                    self[edge].bend_points.offset(graph_offset);
                    if let Some(jp) = self[edge].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS) {
                        jp.borrow_mut().offset(graph_offset);
                    }
                    for label in self[edge].labels.clone() {
                        self[label].position.add(graph_offset);
                    }
                }
            }
        }
    }

    /// `LGraphUtil.placeNodesHorizontally(_:xoffset:)`.
    pub fn place_nodes_horizontally(&mut self, layer: LayerId, xoffset: f64) {
        let mut max_left_margin = 0.0;
        let mut max_right_margin = 0.0;
        for &node in &self[layer].nodes {
            max_left_margin = swift::max(max_left_margin, self[node].margin.left);
            max_right_margin = swift::max(max_right_margin, self[node].margin.right);
        }
        for node in self[layer].nodes.clone() {
            let alignment = self[node].props.get_as::<Alignment>(&LayeredOptions::ALIGNMENT);
            let ratio = match alignment {
                Some(Alignment::LEFT) => 0.0,
                Some(Alignment::RIGHT) => 1.0,
                Some(Alignment::CENTER) => 0.5,
                _ => {
                    let mut inports = 0;
                    let mut outports = 0;
                    for &port in &self[node].ports {
                        if !self[port].incoming_edges.is_empty() {
                            inports += 1;
                        }
                        if !self[port].outgoing_edges.is_empty() {
                            outports += 1;
                        }
                    }
                    if inports + outports == 0 { 0.5 } else { outports as f64 / (inports + outports) as f64 }
                }
            };
            let size = self[layer].size;
            let node_size = self[node].size.x;
            let mut xpos = (size.x - node_size) * ratio;
            if ratio > 0.5 {
                xpos -= max_right_margin * 2.0 * (ratio - 0.5);
            } else if ratio < 0.5 {
                xpos += max_left_margin * 2.0 * (0.5 - ratio);
            }
            let left_margin = self[node].margin.left;
            if xpos < left_margin {
                xpos = left_margin;
            }
            let right_margin = self[node].margin.right;
            if xpos > size.x - right_margin - node_size {
                xpos = size.x - right_margin - node_size;
            }
            self[node].position.x = xoffset + xpos;
        }
    }

    /// `LGraphUtil.findMaxNonDummyNodeWidth(_:respectNodeMargins:)`.
    pub fn find_max_non_dummy_node_width(&self, layer: LayerId, respect_node_margins: bool) -> f64 {
        let graph = self[layer].owner;
        if self[graph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::RIGHT).is_vertical() {
            return 0.0;
        }
        let mut max_width = 0.0;
        for &node in &self[layer].nodes {
            if self[node].node_type == NodeType::NORMAL {
                let mut width = self[node].size.x;
                if respect_node_margins {
                    width += self[node].margin.left + self[node].margin.right;
                }
                max_width = swift::max(max_width, width);
            }
        }
        max_width
    }

    /// `LGraphUtil.computeGraphProperties(_:)`.
    pub fn compute_graph_properties(&mut self, layered_graph: LGraphId) {
        let mut props: EnumSet<GraphProperties> = EnumSet::new();
        let direction = self.get_direction(layered_graph);
        for node in self[layered_graph].layerless_nodes.clone() {
            if self[node].props.get_as::<bool>(&LayeredOptions::COMMENT_BOX).unwrap_or(false) {
                props.insert(GraphProperties::COMMENTS);
            } else if self[node].props.get_as::<bool>(&LayeredOptions::HYPERNODE).unwrap_or(false) {
                props.insert(GraphProperties::HYPERNODES);
                props.insert(GraphProperties::HYPEREDGES);
            } else if self[node].node_type == NodeType::EXTERNAL_PORT {
                props.insert(GraphProperties::EXTERNAL_PORTS);
            }
            let port_constraints = self[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED);
            if port_constraints == PortConstraints::UNDEFINED {
                self[node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FREE);
            } else if port_constraints != PortConstraints::FREE {
                props.insert(GraphProperties::NON_FREE_PORTS);
            }
            for &port in &self[node].ports {
                if self[port].incoming_edges.len() + self[port].outgoing_edges.len() > 1 {
                    props.insert(GraphProperties::HYPEREDGES);
                }
                let port_side = self[port].side;
                match direction {
                    Direction::UP | Direction::DOWN => {
                        if port_side == PortSide::EAST || port_side == PortSide::WEST {
                            props.insert(GraphProperties::NORTH_SOUTH_PORTS);
                        }
                    }
                    _ => {
                        if port_side == PortSide::NORTH || port_side == PortSide::SOUTH {
                            props.insert(GraphProperties::NORTH_SOUTH_PORTS);
                        }
                    }
                }
                for &edge in &self[port].outgoing_edges {
                    if self[edge].target.and_then(|t| self[t].owner) == Some(node) {
                        props.insert(GraphProperties::SELF_LOOPS);
                    }
                    for &label in &self[edge].labels {
                        match self[label].props.get_as::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT) {
                            Some(EdgeLabelPlacement::CENTER) => {
                                props.insert(GraphProperties::CENTER_LABELS);
                            }
                            Some(EdgeLabelPlacement::HEAD | EdgeLabelPlacement::TAIL) => {
                                props.insert(GraphProperties::END_LABELS);
                            }
                            None => {}
                        }
                    }
                }
            }
        }
        self[layered_graph].props.set(&InternalProperties::GRAPH_PROPERTIES, props);
    }

    /// `LGraphUtil.createPort(_:_:_:_:)`.
    pub fn create_port(&mut self, node: LNodeId, end_point: Option<KVector>, port_type: PortType, layered_graph: LGraphId) -> LPortId {
        let direction = self.get_direction(layered_graph);
        let merge_ports = self[layered_graph].props.get_as::<bool>(&LayeredOptions::MERGE_EDGES).unwrap_or(false);
        let hypernode = self[node].props.get_as::<bool>(&LayeredOptions::HYPERNODE).unwrap_or(false);
        let side_fixed = self[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED).is_side_fixed();
        if (merge_ports || hypernode) && !side_fixed {
            let default_side = PortSide::from_direction(direction);
            let side = if port_type == PortType::OUTPUT { default_side } else { default_side.opposed() };
            return self.provide_collector_port(layered_graph, node, port_type, side);
        }
        let port = self.new_port();
        self.port_set_node(port, Some(node));
        if let Some(end_point) = end_point {
            let node_pos = self[node].position;
            let node_size = self[node].size;
            let pos = &mut self[port].position;
            pos.x = end_point.x - node_pos.x;
            pos.y = end_point.y - node_pos.y;
            pos.bound(0.0, 0.0, node_size.x, node_size.y);
            self[port].side = self.calc_port_side(port, direction);
        } else {
            let default_side = PortSide::from_direction(direction);
            self[port].side = if port_type == PortType::OUTPUT { default_side } else { default_side.opposed() };
        }
        let mut graph_properties = self[layered_graph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();
        let port_side = self[port].side;
        let mut needs_update = false;
        match direction {
            Direction::LEFT | Direction::RIGHT => {
                if port_side == PortSide::NORTH || port_side == PortSide::SOUTH {
                    graph_properties.insert(GraphProperties::NORTH_SOUTH_PORTS);
                    needs_update = true;
                }
            }
            Direction::UP | Direction::DOWN => {
                if port_side == PortSide::EAST || port_side == PortSide::WEST {
                    graph_properties.insert(GraphProperties::NORTH_SOUTH_PORTS);
                    needs_update = true;
                }
            }
            _ => {}
        }
        if needs_update {
            self[layered_graph].props.set(&InternalProperties::GRAPH_PROPERTIES, graph_properties);
        }
        port
    }

    /// `LGraphUtil.calcPortSide(_:direction:)`.
    pub fn calc_port_side(&self, port: LPortId, direction: Direction) -> PortSide {
        let Some(node) = self[port].owner else { return PortSide::UNDEFINED };
        let node_width = self[node].size.x;
        let node_height = self[node].size.y;
        if node_width <= 0.0 && node_height <= 0.0 {
            return PortSide::UNDEFINED;
        }
        let xpos = self[port].position.x;
        let ypos = self[port].position.y;
        let width = self[port].size.x;
        let height = self[port].size.y;
        match direction {
            Direction::LEFT | Direction::RIGHT => {
                if xpos < 0.0 {
                    return PortSide::WEST;
                } else if xpos + width > node_width {
                    return PortSide::EAST;
                }
            }
            Direction::UP | Direction::DOWN => {
                if ypos < 0.0 {
                    return PortSide::NORTH;
                } else if ypos + height > node_height {
                    return PortSide::SOUTH;
                }
            }
            _ => {}
        }
        let width_percent = (xpos + width / 2.0) / node_width;
        let height_percent = (ypos + height / 2.0) / node_height;
        if width_percent + height_percent <= 1.0 && width_percent - height_percent <= 0.0 {
            PortSide::WEST
        } else if width_percent + height_percent >= 1.0 && width_percent - height_percent >= 0.0 {
            PortSide::EAST
        } else if height_percent < 0.5 {
            PortSide::NORTH
        } else {
            PortSide::SOUTH
        }
    }

    /// `LGraphUtil.calcPortOffset(_:side:)`.
    pub fn calc_port_offset(&self, port: LPortId, side: PortSide) -> f64 {
        let Some(node) = self[port].owner else { return 0.0 };
        let p = &self[port];
        match side {
            PortSide::NORTH => -(p.position.y + p.size.y),
            PortSide::EAST => p.position.x - self[node].size.x,
            PortSide::SOUTH => p.position.y - self[node].size.y,
            PortSide::WEST => -(p.position.x + p.size.x),
            _ => 0.0,
        }
    }

    /// `LGraphUtil.provideCollectorPort(_:node:type:side:)`.
    pub fn provide_collector_port(&mut self, _layered_graph: LGraphId, node: LNodeId, port_type: PortType, side: PortSide) -> LPortId {
        let port = match port_type {
            PortType::INPUT => {
                for &inport in &self[node].ports {
                    if self[inport].props.get_as::<bool>(&InternalProperties::INPUT_COLLECT).unwrap_or(false) {
                        return inport;
                    }
                }
                let p = self.new_port();
                self[p].props.set(&InternalProperties::INPUT_COLLECT, true);
                p
            }
            PortType::OUTPUT => {
                for &outport in &self[node].ports {
                    if self[outport].props.get_as::<bool>(&InternalProperties::OUTPUT_COLLECT).unwrap_or(false) {
                        return outport;
                    }
                }
                let p = self.new_port();
                self[p].props.set(&InternalProperties::OUTPUT_COLLECT, true);
                p
            }
            PortType::UNDEFINED => self.new_port(),
        };
        self.port_set_node(port, Some(node));
        self[port].side = side;
        let boundary = self[node].size;
        center_point(&mut self[port].position, boundary, side);
        port
    }

    /// `LGraphUtil.initializePort(_:_:_:_:)`.
    pub fn initialize_port(&mut self, port: LPortId, port_constraints: PortConstraints, direction: Direction, anchor_pos: Option<KVector>) {
        let mut port_side = self[port].side;
        if port_side == PortSide::UNDEFINED && port_constraints.is_side_fixed() {
            port_side = self.calc_port_side(port, direction);
            self[port].side = port_side;
            let p = &self[port];
            if !p.props.has(&LayeredOptions::PORT_BORDER_OFFSET) && port_side != PortSide::UNDEFINED && (p.position.x != 0.0 || p.position.y != 0.0) {
                let offset = self.calc_port_offset(port, port_side);
                self[port].props.set(&LayeredOptions::PORT_BORDER_OFFSET, offset);
            }
        }
        if port_constraints.is_ratio_fixed() {
            let mut ratio = 0.0;
            match port_side {
                PortSide::NORTH | PortSide::SOUTH => {
                    let node_width = self[port].owner.map_or(0.0, |n| self[n].size.x);
                    if node_width > 0.0 {
                        ratio = self[port].position.x / node_width;
                    }
                }
                PortSide::EAST | PortSide::WEST => {
                    let node_height = self[port].owner.map_or(0.0, |n| self[n].size.y);
                    if node_height > 0.0 {
                        ratio = self[port].position.y / node_height;
                    }
                }
                _ => {}
            }
            self[port].props.set(&InternalProperties::PORT_RATIO_OR_POSITION, ratio);
        }
        let port_size = self[port].size;
        if let Some(anchor_pos) = anchor_pos {
            self[port].anchor.x = anchor_pos.x;
            self[port].anchor.y = anchor_pos.y;
            self[port].explicitly_supplied_port_anchor = true;
        } else if port_constraints.is_side_fixed() && port_side != PortSide::UNDEFINED {
            let anchor = &mut self[port].anchor;
            match port_side {
                PortSide::NORTH => anchor.x = port_size.x / 2.0,
                PortSide::EAST => {
                    anchor.x = port_size.x;
                    anchor.y = port_size.y / 2.0;
                }
                PortSide::SOUTH => {
                    anchor.x = port_size.x / 2.0;
                    anchor.y = port_size.y;
                }
                PortSide::WEST => anchor.y = port_size.y / 2.0,
                _ => {}
            }
        } else {
            self[port].anchor.x = port_size.x / 2.0;
            self[port].anchor.y = port_size.y / 2.0;
        }
    }

    /// `LGraphUtil.createExternalPortDummy(...)`. `property_holder` is the
    /// element the dummy represents (an ELK port or an `LPort`).
    ///
    /// Swift makes the dummy port's `position` and the dummy's `PORT_ANCHOR`
    /// property the same `KVector` object. The port records that in
    /// [`super::l_node::LNodeData::port_anchor_alias`]; read the property
    /// through [`LGraphArena::node_port_anchor`].
    #[allow(clippy::too_many_arguments)]
    pub fn create_external_port_dummy(
        &mut self,
        property_holder: &mut PropertyMap,
        port_constraints: PortConstraints,
        port_side: PortSide,
        net_flow: i64,
        port_node_size: KVector,
        port_position: KVector,
        port_size: KVector,
        layout_direction: Direction,
        layered_graph: LGraphId,
    ) -> LNodeId {
        let mut final_side = port_side;
        let dummy = self.new_node(Some(layered_graph));
        self[dummy].node_type = NodeType::EXTERNAL_PORT;
        self[dummy].props.set(&InternalProperties::EXT_PORT_SIZE, PropValue::kvector(port_size));
        self[dummy].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
        let port_border_offset = property_holder.get_as::<f64>(&LayeredOptions::PORT_BORDER_OFFSET).unwrap_or(0.0);
        self[dummy].props.set(&LayeredOptions::PORT_BORDER_OFFSET, port_border_offset);

        let dummy_port = self.new_port();
        self.port_set_node(dummy_port, Some(dummy));

        if !port_constraints.is_side_fixed() {
            let resolved = if layout_direction == Direction::UNDEFINED { Direction::RIGHT } else { layout_direction };
            final_side = if net_flow >= 0 { PortSide::from_direction(resolved) } else { PortSide::from_direction(resolved).opposed() };
            property_holder.set(&LayeredOptions::PORT_SIDE, final_side);
        }

        // `anchor` is a Swift reference: either the holder's own PORT_ANCHOR
        // object (mutated below) or a fresh vector.
        let explicit_anchor = property_holder.has(&LayeredOptions::PORT_ANCHOR);
        let anchor: KVectorRef = if explicit_anchor {
            property_holder
                .get_as::<KVectorRef>(&LayeredOptions::PORT_ANCHOR)
                .unwrap_or_else(|| crate::org::eclipse::elk::core::math::k_vector::kvector_ref(KVector::default()))
        } else {
            crate::org::eclipse::elk::core::math::k_vector::kvector_ref(KVector::new(port_size.x / 2.0, port_size.y / 2.0))
        };

        match final_side {
            PortSide::WEST => {
                self[dummy].props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::FIRST_SEPARATE);
                self[dummy].props.set(&InternalProperties::EDGE_CONSTRAINT, EdgeConstraint::OUTGOING_ONLY);
                self[dummy].size.y = port_size.y;
                if port_border_offset < 0.0 {
                    self[dummy].size.x = -port_border_offset;
                }
                self[dummy_port].side = PortSide::EAST;
                let mut a = anchor.borrow_mut();
                if !explicit_anchor {
                    a.x = port_size.x;
                }
                a.x -= port_size.x;
            }
            PortSide::EAST => {
                self[dummy].props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::LAST_SEPARATE);
                self[dummy].props.set(&InternalProperties::EDGE_CONSTRAINT, EdgeConstraint::INCOMING_ONLY);
                self[dummy].size.y = port_size.y;
                if port_border_offset < 0.0 {
                    self[dummy].size.x = -port_border_offset;
                }
                self[dummy_port].side = PortSide::WEST;
                if !explicit_anchor {
                    anchor.borrow_mut().x = 0.0;
                }
            }
            PortSide::NORTH => {
                self[dummy].props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::TOP);
                self[dummy].size.x = port_size.x;
                if port_border_offset < 0.0 {
                    self[dummy].size.y = -port_border_offset;
                }
                self[dummy_port].side = PortSide::SOUTH;
                let mut a = anchor.borrow_mut();
                if !explicit_anchor {
                    a.y = port_size.y;
                }
                a.y -= port_size.y;
            }
            PortSide::SOUTH => {
                self[dummy].props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::BOTTOM);
                self[dummy].size.x = port_size.x;
                if port_border_offset < 0.0 {
                    self[dummy].size.y = -port_border_offset;
                }
                self[dummy_port].side = PortSide::NORTH;
                if !explicit_anchor {
                    anchor.borrow_mut().y = 0.0;
                }
            }
            _ => {}
        }

        // `dummyPort.position = anchor`; `dummy.setProperty(PORT_ANCHOR, anchor)`.
        self[dummy_port].position = *anchor.borrow();
        self[dummy].port_anchor_alias = Some(dummy_port);
        self[dummy].props.set(&LayeredOptions::PORT_ANCHOR, anchor);

        if port_constraints.is_order_fixed() {
            let mut information_about_it = 0.0;
            if port_constraints == PortConstraints::FIXED_ORDER && property_holder.has(&LayeredOptions::PORT_INDEX) {
                let index = property_holder.get_as::<i64>(&LayeredOptions::PORT_INDEX).unwrap_or(0);
                match final_side {
                    PortSide::NORTH | PortSide::EAST => information_about_it = index as f64,
                    PortSide::SOUTH | PortSide::WEST => information_about_it = -1.0 * index as f64,
                    _ => {}
                }
            } else {
                match final_side {
                    PortSide::WEST | PortSide::EAST => {
                        information_about_it = port_position.y;
                        if port_constraints.is_ratio_fixed() {
                            information_about_it /= port_node_size.y;
                        }
                    }
                    PortSide::NORTH | PortSide::SOUTH => {
                        information_about_it = port_position.x;
                        if port_constraints.is_ratio_fixed() {
                            information_about_it /= port_node_size.x;
                        }
                    }
                    _ => {}
                }
            }
            self[dummy].props.set(&InternalProperties::PORT_RATIO_OR_POSITION, information_about_it);
        }
        self[dummy].props.set(&InternalProperties::EXT_PORT_SIDE, final_side);
        dummy
    }

    /// Reads `node.getProperty(LayeredOptions.PORT_ANCHOR) as? KVector`,
    /// honouring the Swift aliasing of an external port dummy's anchor
    /// property with its port's position.
    pub fn node_port_anchor(&self, node: LNodeId) -> Option<KVector> {
        if let Some(port) = self[node].port_anchor_alias {
            if self[node].props.get_as::<KVectorRef>(&LayeredOptions::PORT_ANCHOR).is_some() {
                return Some(self[port].position);
            }
        }
        self[node].props.get_as::<KVectorRef>(&LayeredOptions::PORT_ANCHOR).map(|a| *a.borrow())
    }

    /// `LGraphUtil.getExternalPortPosition(_:_:_:_:)`.
    pub fn get_external_port_position(&mut self, graph: LGraphId, port_dummy: LNodeId, port_width: f64, port_height: f64) -> KVector {
        let mut port_position = self[port_dummy].position;
        port_position.x += self[port_dummy].size.x / 2.0;
        port_position.y += self[port_dummy].size.y / 2.0;
        let port_offset = self[port_dummy].props.get_as::<f64>(&LayeredOptions::PORT_BORDER_OFFSET).unwrap_or(0.0);
        let graph_size = self[graph].size;
        let padding = self[graph].padding;
        let graph_offset = self[graph].offset;
        match self[port_dummy].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
            Some(PortSide::NORTH) => {
                port_position.x += padding.left + graph_offset.x - (port_width / 2.0);
                port_position.y = -port_height - port_offset;
                self[port_dummy].position.y = -(padding.top + port_offset + graph_offset.y);
            }
            Some(PortSide::EAST) => {
                port_position.x = graph_size.x + padding.left + padding.right + port_offset;
                port_position.y += padding.top + graph_offset.y - (port_height / 2.0);
                self[port_dummy].position.x = graph_size.x + padding.right + port_offset - graph_offset.x;
            }
            Some(PortSide::SOUTH) => {
                port_position.x += padding.left + graph_offset.x - (port_width / 2.0);
                port_position.y = graph_size.y + padding.top + padding.bottom + port_offset;
                self[port_dummy].position.y = graph_size.y + padding.bottom + port_offset - graph_offset.y;
            }
            Some(PortSide::WEST) => {
                port_position.x = -port_width - port_offset;
                port_position.y += padding.top + graph_offset.y - (port_height / 2.0);
                self[port_dummy].position.x = -(padding.left + port_offset + graph_offset.x);
            }
            _ => {}
        }
        port_position
    }

    /// `LGraphUtil.isDescendant(_:_:)`.
    pub fn is_descendant(&self, child: Option<LNodeId>, parent: Option<LNodeId>) -> bool {
        let (Some(child), Some(parent)) = (child, parent) else { return false };
        let mut next = self.node_graph(child).and_then(|g| self[g].parent_node);
        while let Some(n) = next {
            if n == parent {
                return true;
            }
            next = self.node_graph(n).and_then(|g| self[g].parent_node);
        }
        false
    }

    /// `LGraphUtil.changeCoordSystem(_:oldGraph:newGraph:)`.
    pub fn change_coord_system(&self, point: &mut KVector, old_graph: LGraphId, new_graph: LGraphId) {
        if old_graph == new_graph {
            return;
        }
        let mut graph = old_graph;
        loop {
            point.add(self[graph].offset);
            let node = self[graph].parent_node;
            if let Some(n) = node {
                let padding = self[graph].padding;
                point.add_xy(padding.left, padding.top);
                point.add(self[n].position);
                if let Some(g) = self.node_graph(n) {
                    graph = g;
                }
            } else {
                break;
            }
        }
        graph = new_graph;
        loop {
            point.sub(self[graph].offset);
            let node = self[graph].parent_node;
            if let Some(n) = node {
                let padding = self[graph].padding;
                point.sub_xy(padding.left, padding.top);
                point.sub(self[n].position);
                if let Some(g) = self.node_graph(n) {
                    graph = g;
                }
            } else {
                break;
            }
        }
    }

    /// `LGraphUtil.getIndividualOrInherited<T>(_:property:)` as all callers use
    /// it: a `Double` (falling back to `0.0`).
    pub fn get_individual_or_inherited(&self, node: LNodeId, property: &Property) -> f64 {
        let mut result: Option<f64> = None;
        if self[node].props.has(&CoreOptions::SPACING_INDIVIDUAL) {
            if let Some(individual) = self[node].props.get_object::<PropertyMap>(&CoreOptions::SPACING_INDIVIDUAL) {
                if individual.has(property) {
                    result = individual.get_as::<f64>(property);
                }
            }
        }
        if result.is_none() {
            if let Some(graph) = self.node_graph(node) {
                result = self[graph].props.get_as::<f64>(property);
            }
        }
        result.unwrap_or(0.0)
    }

    /// `LGraphUtil.getDirection(_:)`.
    pub fn get_direction(&self, graph: LGraphId) -> Direction {
        let direction = self[graph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
        if direction == Direction::UNDEFINED {
            let aspect_ratio = self[graph].props.get_as::<f64>(&LayeredOptions::ASPECT_RATIO).unwrap_or(1.0);
            return if aspect_ratio >= 1.0 { Direction::RIGHT } else { Direction::DOWN };
        }
        direction
    }

    /// `LGraphUtil.getMinimalModelOrder(_:)`.
    pub fn get_minimal_model_order(&self, graph: LGraphId) -> i64 {
        let mut order = i64::MAX;
        for &node in &self[graph].layerless_nodes {
            if self[node].props.has(&InternalProperties::MODEL_ORDER) {
                let node_order = self[node].props.get_as::<i64>(&InternalProperties::MODEL_ORDER).unwrap_or(i64::MAX);
                order = order.min(node_order);
            }
        }
        order
    }
}
