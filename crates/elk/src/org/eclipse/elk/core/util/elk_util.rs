//! Port of `core/util/ElkUtil.swift` (the parts elk-swift's layout path uses).

use std::collections::VecDeque;

use crate::bridge::elk_graph_impl::{ElkElement, ElkGraph, ElkNodeId};
use crate::org::eclipse::elk::core::options::core_options as CoreOptions;

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
}
