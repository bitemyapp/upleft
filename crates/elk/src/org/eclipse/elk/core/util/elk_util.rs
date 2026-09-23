//! Port of `core/util/ElkUtil.swift` (the parts elk-swift's layout path uses).

use std::collections::VecDeque;

use crate::bridge::elk_graph_impl::{ElkBendPoint, ElkEdgeSectionId, ElkElement, ElkGraph, ElkNodeId, ElkPortId};
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChain;
use crate::org::eclipse::elk::core::options::core_options as CoreOptions;
use crate::org::eclipse::elk::core::options::direction::Direction;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::org::eclipse::elk::core::math::k_vector::KVectorRef;
use crate::swift;

pub struct ElkUtil;

impl ElkUtil {
    pub const DEFAULT_MIN_WIDTH: f64 = 20.0;
    pub const DEFAULT_MIN_HEIGHT: f64 = 20.0;

    /// The element order of `applyVisitors`
    /// (`ElkGraphUtil.propertiesSkippingIteratorFor`): breadth first; a node
    /// enqueues its children, ports, labels, then contained edges; a port its
    /// labels; an edge its labels (its sections are not graph elements).
    pub fn visitor_order(graph: &ElkGraph, root: ElkNodeId) -> Vec<ElkElement> {
        let mut out = Vec::new();
        let mut queue = VecDeque::from([ElkElement::Node(root)]);
        while let Some(element) = queue.pop_front() {
            match element {
                ElkElement::Node(n) => {
                    queue.extend(graph[n].children.iter().map(|&c| ElkElement::Node(c)));
                    queue.extend(graph[n].ports.iter().map(|&p| ElkElement::Port(p)));
                    queue.extend(graph[n].labels.iter().map(|&l| ElkElement::Label(l)));
                    queue.extend(graph[n].contained_edges.iter().map(|&e| ElkElement::Edge(e)));
                }
                ElkElement::Port(p) => queue.extend(graph[p].labels.iter().map(|&l| ElkElement::Label(l))),
                ElkElement::Edge(e) => queue.extend(graph[e].labels.iter().map(|&l| ElkElement::Label(l))),
                ElkElement::Label(_) => {}
            }
            out.push(element);
        }
        out
    }

    /// `applyConfiguredNodeScaling(_:)`.
    pub fn apply_configured_node_scaling(graph: &mut ElkGraph, node: ElkNodeId) {
        let scaling_factor: f64 = graph[node].props.get_typed(&CoreOptions::SCALE_FACTOR).unwrap_or(1.0);
        if scaling_factor == 1.0 {
            return;
        }
        let n = &mut graph[node];
        n.width = scaling_factor * n.width;
        n.height = scaling_factor * n.height;
        let ports = graph[node].ports.clone();
        for &port in &ports {
            for label in graph[port].labels.clone() {
                let l = &mut graph[label];
                l.x = scaling_factor * l.x;
                l.y = scaling_factor * l.y;
                l.width = scaling_factor * l.width;
                l.height = scaling_factor * l.height;
                if let Some(anchor) = graph[label].props.get_typed::<crate::org::eclipse::elk::core::math::k_vector::KVectorRef>(&CoreOptions::PORT_ANCHOR) {
                    let mut a = anchor.borrow_mut();
                    a.x *= scaling_factor;
                    a.y *= scaling_factor;
                }
            }
        }
        let labels = graph[node].labels.clone();
        for &label in &labels {
            let l = &mut graph[label];
            l.x = scaling_factor * l.x;
            l.y = scaling_factor * l.y;
            l.width = scaling_factor * l.width;
            l.height = scaling_factor * l.height;
            if let Some(anchor) = graph[label].props.get_typed::<crate::org::eclipse::elk::core::math::k_vector::KVectorRef>(&CoreOptions::PORT_ANCHOR) {
                let mut a = anchor.borrow_mut();
                a.x *= scaling_factor;
                a.y *= scaling_factor;
            }
        }
        for &port in &ports {
            let p = &mut graph[port];
            p.x = scaling_factor * p.x;
            p.y = scaling_factor * p.y;
            p.width = scaling_factor * p.width;
            p.height = scaling_factor * p.height;
            if let Some(anchor) = graph[port].props.get_typed::<crate::org::eclipse::elk::core::math::k_vector::KVectorRef>(&CoreOptions::PORT_ANCHOR) {
                let mut a = anchor.borrow_mut();
                a.x *= scaling_factor;
                a.y *= scaling_factor;
            }
        }
    }

    /// `calcPortSide(_:direction:)` for an ELK port.
    pub fn calc_port_side(graph: &ElkGraph, port: ElkPortId, direction: Direction) -> PortSide {
        let Some(node) = graph[port].parent else { return PortSide::UNDEFINED };
        let node_width = graph[node].width;
        let node_height = graph[node].height;
        if node_width <= 0.0 && node_height <= 0.0 {
            return PortSide::UNDEFINED;
        }
        let p = &graph[port];
        let xpos = p.x;
        let ypos = p.y;
        match direction {
            Direction::LEFT | Direction::RIGHT => {
                if xpos < 0.0 {
                    return PortSide::WEST;
                } else if xpos + p.width > node_width {
                    return PortSide::EAST;
                }
            }
            Direction::UP | Direction::DOWN => {
                if ypos < 0.0 {
                    return PortSide::NORTH;
                } else if ypos + p.height > node_height {
                    return PortSide::SOUTH;
                }
            }
            _ => {}
        }
        let width_percent = (xpos + p.width / 2.0) / node_width;
        let height_percent = (ypos + p.height / 2.0) / node_height;
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

    /// `calcPortOffset(_:side:)` for an ELK port.
    pub fn calc_port_offset(graph: &ElkGraph, port: ElkPortId, side: PortSide) -> f64 {
        let Some(node) = graph[port].parent else { return 0.0 };
        let p = &graph[port];
        match side {
            PortSide::NORTH => -(p.y + p.height),
            PortSide::EAST => p.x - graph[node].width,
            PortSide::SOUTH => p.y - graph[node].height,
            PortSide::WEST => -(p.x + p.width),
            _ => 0.0,
        }
    }

    /// `resizeNode(_:newWidth:newHeight:movePorts:moveLabels:)`.
    pub fn resize_node(graph: &mut ElkGraph, node: ElkNodeId, new_width: f64, new_height: f64, move_ports: bool, move_labels: bool) -> KVector {
        let old_size = KVector::new(graph[node].width, graph[node].height);
        let mut new_size = Self::effective_min_size_constraint_for(graph, node);
        new_size.x = swift::max(new_size.x, new_width);
        new_size.y = swift::max(new_size.y, new_height);
        let width_ratio = new_size.x / old_size.x;
        let height_ratio = new_size.y / old_size.y;
        let width_diff = new_size.x - old_size.x;
        let height_diff = new_size.y - old_size.y;

        if move_ports {
            let direction: Direction = match graph[node].parent {
                Some(parent) => graph[parent].props.get_typed(&CoreOptions::DIRECTION).unwrap_or(Direction::UNDEFINED),
                None => graph[node].props.get_typed(&CoreOptions::DIRECTION).unwrap_or(Direction::UNDEFINED),
            };
            let fixed_ports = graph[node].props.get_typed::<PortConstraints>(&CoreOptions::PORT_CONSTRAINTS) == Some(PortConstraints::FIXED_POS);
            for port in graph[node].ports.clone() {
                let mut port_side: PortSide = graph[port].props.get_typed(&CoreOptions::PORT_SIDE).unwrap_or(PortSide::UNDEFINED);
                if port_side == PortSide::UNDEFINED {
                    port_side = Self::calc_port_side(graph, port, direction);
                    graph[port].props.set(&CoreOptions::PORT_SIDE, port_side);
                }
                let p = &mut graph[port];
                match port_side {
                    PortSide::NORTH => {
                        if !fixed_ports {
                            p.x = p.x * width_ratio;
                        }
                    }
                    PortSide::EAST => {
                        p.x = p.x + width_diff;
                        if !fixed_ports {
                            p.y = p.y * height_ratio;
                        }
                    }
                    PortSide::SOUTH => {
                        if !fixed_ports {
                            p.x = p.x * width_ratio;
                        }
                        p.y = p.y + height_diff;
                    }
                    PortSide::WEST => {
                        if !fixed_ports {
                            p.y = p.y * height_ratio;
                        }
                    }
                    _ => {}
                }
            }
        }

        graph[node].width = new_size.x;
        graph[node].height = new_size.y;

        if move_labels {
            for label in graph[node].labels.clone() {
                let l = &mut graph[label];
                let midx = l.x + l.width / 2.0;
                let midy = l.y + l.height / 2.0;
                let width_percent = midx / old_size.x;
                let height_percent = midy / old_size.y;
                if width_percent + height_percent >= 1.0 {
                    if width_percent - height_percent > 0.0 && midy >= 0.0 {
                        l.x = l.x + width_diff;
                        l.y = l.y + height_diff * height_percent;
                    } else if width_percent - height_percent < 0.0 && midx >= 0.0 {
                        l.x = l.x + width_diff * width_percent;
                        l.y = l.y + height_diff;
                    }
                }
            }
        }

        graph[node].props.set(&CoreOptions::NODE_SIZE_CONSTRAINTS, SizeConstraint::empty());
        KVector::new(width_ratio, height_ratio)
    }

    /// `effectiveMinSizeConstraintFor(_:)`.
    pub fn effective_min_size_constraint_for(graph: &ElkGraph, node: ElkNodeId) -> KVector {
        let size_constraint: SizeConstraint = graph[node].props.get_typed(&CoreOptions::NODE_SIZE_CONSTRAINTS).unwrap_or_default();
        if size_constraint.contains(SizeConstraint::MINIMUM_SIZE) {
            let size_options: SizeOptions = graph[node].props.get_typed(&CoreOptions::NODE_SIZE_OPTIONS).unwrap_or_default();
            let min_size_vec: KVector = graph[node]
                .props
                .get_typed::<KVectorRef>(&CoreOptions::NODE_SIZE_MINIMUM)
                .map(|v| *v.borrow())
                .unwrap_or_default();
            let mut min_size = KVector::new(min_size_vec.x, min_size_vec.y);
            if size_options.contains(SizeOptions::DEFAULT_MINIMUM_SIZE) {
                if min_size.x <= 0.0 {
                    min_size.x = Self::DEFAULT_MIN_WIDTH;
                }
                if min_size.y <= 0.0 {
                    min_size.y = Self::DEFAULT_MIN_HEIGHT;
                }
            }
            min_size
        } else {
            KVector::default()
        }
    }

    /// `toAbsolute(_:parent:)`: mutates `point` (a Swift reference) and
    /// returns it.
    pub fn to_absolute(graph: &ElkGraph, point: &mut KVector, parent: Option<ElkNodeId>) -> KVector {
        let mut node = parent;
        while let Some(current) = node {
            point.add_xy(graph[current].x, graph[current].y);
            node = graph[current].parent;
        }
        *point
    }

    /// `toRelative(_:parent:)`: mutates `point` and returns it.
    pub fn to_relative(graph: &ElkGraph, point: &mut KVector, parent: Option<ElkNodeId>) -> KVector {
        let mut node = parent;
        while let Some(current) = node {
            point.add_xy(-graph[current].x, -graph[current].y);
            node = graph[current].parent;
        }
        *point
    }

    /// `createVectorChain(_:)`.
    pub fn create_vector_chain(graph: &ElkGraph, section: ElkEdgeSectionId) -> KVectorChain {
        let s = &graph[section];
        let mut chain = KVectorChain::new();
        chain.add(KVector::new(s.start_x, s.start_y));
        for bp in &s.bend_points {
            chain.add(KVector::new(bp.x, bp.y));
        }
        chain.add(KVector::new(s.end_x, s.end_y));
        chain
    }

    /// `applyVectorChain(_:section:)`.
    pub fn apply_vector_chain(graph: &mut ElkGraph, vector_chain: &KVectorChain, section: ElkEdgeSectionId) {
        if vector_chain.size() < 2 {
            return;
        }
        let s = &mut graph[section];
        let first = vector_chain.get_first().unwrap();
        s.start_x = first.x;
        s.start_y = first.y;
        let old_count = s.bend_points.len();
        let mut old_index = 0;
        let mut new_point_index = 1;
        while new_point_index < vector_chain.size() - 1 {
            let next_point = vector_chain.get(new_point_index);
            new_point_index += 1;
            if old_index < old_count {
                s.bend_points[old_index] = ElkBendPoint { x: next_point.x, y: next_point.y };
                old_index += 1;
            } else {
                s.bend_points.push(ElkBendPoint { x: next_point.x, y: next_point.y });
            }
        }
        while s.bend_points.len() > vector_chain.size() - 2 {
            s.bend_points.pop();
        }
        let last = vector_chain.get_last().unwrap();
        s.end_x = last.x;
        s.end_y = last.y;
    }

    /// `computeInsidePart(_:_:_:_:_:)` (the position-based overload).
    pub fn compute_inside_part(label_pos: KVector, label_size: KVector, port_size: KVector, _label_spacing: f64, port_side: PortSide) -> f64 {
        match port_side {
            PortSide::EAST | PortSide::WEST => {
                let inside_end = swift::min(label_pos.x + label_size.x, port_size.x);
                let inside_start = swift::max(label_pos.x, 0.0);
                swift::max(0.0, inside_end - inside_start)
            }
            PortSide::NORTH | PortSide::SOUTH => {
                let inside_end = swift::min(label_pos.y + label_size.y, port_size.y);
                let inside_start = swift::max(label_pos.y, 0.0);
                swift::max(0.0, inside_end - inside_start)
            }
            _ => 0.0,
        }
    }
}
