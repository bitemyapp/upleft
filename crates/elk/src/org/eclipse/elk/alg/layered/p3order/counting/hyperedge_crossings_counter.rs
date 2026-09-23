//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/counting/org_eclipse_elk_alg_layered_p3order_counting_HyperedgeCrossingsCounter.swift`.
//!
//! Counts crossings between two layers where ports may have several edges,
//! treating the edges of each connected port group as one hyperedge.
//! `portPos` is this counter's own array (a Swift value copy).

use std::collections::HashMap;

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::swift;

#[derive(Clone, Debug)]
pub struct HyperedgeCrossingsCounter {
    pub in_layer_edge_count: Vec<i64>,
    pub has_north_south_ports: Vec<bool>,
    pub port_pos: Vec<i64>,
}

/// `Hyperedge` (a class in Swift; identified here by its index in the
/// per-call arena, which is also its creation order).
#[derive(Default)]
struct Hyperedge {
    edges: Vec<LEdgeId>,
    ports: Vec<LPortId>,
    upper_left: i64,
    lower_left: i64,
    upper_right: i64,
    lower_right: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CornerType {
    UPPER,
    LOWER,
}

#[derive(Clone, Copy)]
struct HyperedgeCorner {
    hyperedge: usize,
    position: i64,
    opposite_position: i64,
    corner_type: CornerType,
}

/// `compareCorners(_:_:)`.
///
/// NONDETERMINISTIC IN SWIFT: two corners at the same position and opposite
/// position of different hyperedges are ordered by `ObjectIdentifier(...)
/// .hashValue`, which depends on the per-process hash seed and heap
/// addresses. The port orders them by hyperedge creation order. The crossing
/// count does not depend on this order: corners that tie on both positions
/// belong either to hyperedges spanning the same interval (their UPPER or
/// LOWER corners are interchangeable) or to single-point hyperedges whose
/// UPPER/LOWER pairs stay together.
fn compare_corners(a: &HyperedgeCorner, b: &HyperedgeCorner) -> bool {
    if a.position != b.position {
        return a.position < b.position;
    }
    if a.opposite_position != b.opposite_position {
        return a.opposite_position < b.opposite_position;
    }
    if a.hyperedge != b.hyperedge {
        return a.hyperedge < b.hyperedge;
    }
    if a.corner_type == CornerType::UPPER && b.corner_type == CornerType::LOWER {
        return true;
    }
    false
}

impl HyperedgeCrossingsCounter {
    pub fn new(in_layer_edge_count: Vec<i64>, has_north_south_ports: Vec<bool>, port_pos: Vec<i64>) -> HyperedgeCrossingsCounter {
        HyperedgeCrossingsCounter { in_layer_edge_count, has_north_south_ports, port_pos }
    }

    #[inline]
    fn set_pos(&mut self, lg: &LGraphArena, port: LPortId, value: i64) {
        let pid = lg[port].id;
        if pid >= 0 && (pid as usize) < self.port_pos.len() {
            self.port_pos[pid as usize] = value;
        }
    }

    /// `countCrossings(_:_:)`.
    pub fn count_crossings(&mut self, lg: &LGraphArena, left_layer: &[LNodeId], right_layer: &[LNodeId]) -> i64 {
        if left_layer.is_empty() || right_layer.is_empty() {
            return 0;
        }

        // Assign index values to the ports of the left layer
        let mut source_count: i64 = 0;
        for &node in left_layer {
            // Assign index values in the order north - east - south - west
            let node_layer = lg[node].layer;
            for &port in &lg[node].ports {
                let mut port_edges = 0;
                for &edge in &lg[port].outgoing_edges {
                    if node_layer != lg.edge_target_node(edge).and_then(|n| lg[n].layer) {
                        port_edges += 1;
                    }
                }
                if port_edges > 0 {
                    self.set_pos(lg, port, source_count);
                    source_count += 1;
                }
            }
        }

        // Assign index values to the ports of the right layer
        let mut target_count: i64 = 0;
        for &node in right_layer {
            let node_layer = lg[node].layer;
            // Determine how many input ports there are on the north side
            let mut north_input_ports = 0;
            for &port in &lg[node].ports {
                if lg[port].side == PortSide::NORTH {
                    for &edge in &lg[port].incoming_edges {
                        if node_layer != lg.edge_source_node(edge).and_then(|n| lg[n].layer) {
                            north_input_ports += 1;
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
            // Assign index values in the order north - west - south - east
            let mut other_input_ports = 0;
            for &port in lg[node].ports.iter().rev() {
                let mut port_edges = 0;
                for &edge in &lg[port].incoming_edges {
                    if node_layer != lg.edge_source_node(edge).and_then(|n| lg[n].layer) {
                        port_edges += 1;
                    }
                }
                if port_edges > 0 {
                    if lg[port].side == PortSide::NORTH {
                        self.set_pos(lg, port, target_count);
                        target_count += 1;
                    } else {
                        self.set_pos(lg, port, target_count + north_input_ports + other_input_ports);
                        other_input_ports += 1;
                    }
                }
            }
            target_count += other_input_ports;
        }

        // Gather hyperedges. `hyperedgeSet` keeps insertion order like a
        // LinkedHashSet.
        let mut arena: Vec<Hyperedge> = Vec::new();
        let mut port2_hyperedge_map: HashMap<LPortId, usize> = HashMap::new();
        let mut hyperedge_set: Vec<usize> = Vec::new();

        for &node in left_layer {
            let node_layer = lg[node].layer;
            for &source_port in &lg[node].ports {
                for &edge in &lg[source_port].outgoing_edges {
                    let Some(target_port) = lg[edge].target else { continue };
                    if node_layer == lg[target_port].owner.and_then(|n| lg[n].layer) {
                        continue;
                    }

                    let source_he = port2_hyperedge_map.get(&source_port).copied();
                    let target_he = port2_hyperedge_map.get(&target_port).copied();

                    match (source_he, target_he) {
                        (None, None) => {
                            let he = arena.len();
                            arena.push(Hyperedge::default());
                            hyperedge_set.push(he);
                            arena[he].edges.push(edge);
                            arena[he].ports.push(source_port);
                            port2_hyperedge_map.insert(source_port, he);
                            arena[he].ports.push(target_port);
                            port2_hyperedge_map.insert(target_port, he);
                        }
                        (None, Some(target_he)) => {
                            arena[target_he].edges.push(edge);
                            arena[target_he].ports.push(source_port);
                            port2_hyperedge_map.insert(source_port, target_he);
                        }
                        (Some(source_he), None) => {
                            arena[source_he].edges.push(edge);
                            arena[source_he].ports.push(target_port);
                            port2_hyperedge_map.insert(target_port, source_he);
                        }
                        (Some(source_he), Some(target_he)) if source_he == target_he => {
                            arena[source_he].edges.push(edge);
                        }
                        (Some(source_he), Some(target_he)) => {
                            arena[source_he].edges.push(edge);
                            for i in 0..arena[target_he].ports.len() {
                                let p = arena[target_he].ports[i];
                                port2_hyperedge_map.insert(p, source_he);
                            }
                            let target_edges = arena[target_he].edges.clone();
                            arena[source_he].edges.extend(target_edges);
                            let target_ports = arena[target_he].ports.clone();
                            arena[source_he].ports.extend(target_ports);
                            hyperedge_set.retain(|&h| h != target_he);
                        }
                    }
                }
            }
        }

        // Determine top and bottom positions for each hyperedge
        let left_layer_ref = lg[left_layer[0]].layer;
        let right_layer_ref = lg[right_layer[0]].layer;
        for &he in &hyperedge_set {
            let h = &mut arena[he];
            h.upper_left = source_count;
            h.upper_right = target_count;
            for &port in &h.ports {
                let pid = lg[port].id;
                if pid < 0 || pid as usize >= self.port_pos.len() {
                    continue;
                }
                let pos = self.port_pos[pid as usize];
                let port_layer = lg[port].owner.and_then(|n| lg[n].layer);
                if port_layer == left_layer_ref {
                    if pos < h.upper_left {
                        h.upper_left = pos;
                    }
                    if pos > h.lower_left {
                        h.lower_left = pos;
                    }
                } else if port_layer == right_layer_ref {
                    if pos < h.upper_right {
                        h.upper_right = pos;
                    }
                    if pos > h.lower_right {
                        h.lower_right = pos;
                    }
                }
            }
        }

        // Determine the sequence of edge target positions sorted by source and target index.
        // NONDETERMINISTIC IN SWIFT: `Hyperedge.<` breaks ties on (upperLeft,
        // upperRight) by `ObjectIdentifier(...).hashValue`; the port uses
        // creation order. Tied hyperedges contribute the same `upperRight`
        // to `southSequence`, so the count is unaffected.
        let hyperedges = swift::sorted_by(hyperedge_set.iter().copied(), |&a, &b| {
            let (l, r) = (&arena[a], &arena[b]);
            if l.upper_left != r.upper_left {
                return l.upper_left < r.upper_left;
            }
            if l.upper_right != r.upper_right {
                return l.upper_right < r.upper_right;
            }
            a < b
        });
        let mut south_sequence: Vec<i64> = vec![0; hyperedges.len()];
        let mut compress_deltas: Vec<i64> = vec![0; (target_count + 1) as usize];
        for i in 0..hyperedges.len() {
            south_sequence[i] = arena[hyperedges[i]].upper_right;
            if south_sequence[i] >= 0 && (south_sequence[i] as usize) < compress_deltas.len() {
                compress_deltas[south_sequence[i] as usize] = 1;
            }
        }
        let mut delta = 0;
        for i in 0..compress_deltas.len() {
            if compress_deltas[i] == 1 {
                compress_deltas[i] = delta;
            } else {
                delta -= 1;
            }
        }
        let mut q: i64 = 0;
        for i in 0..south_sequence.len() {
            let idx = south_sequence[i];
            if idx >= 0 && (idx as usize) < compress_deltas.len() {
                south_sequence[i] += compress_deltas[idx as usize];
            }
            q = swift::max(q, south_sequence[i] + 1);
        }

        // Build the accumulator tree
        let mut first_index: i64 = 1;
        while first_index < q {
            first_index *= 2;
        }
        let tree_size = 2 * first_index - 1;
        first_index -= 1;
        let mut tree: Vec<i64> = vec![0; tree_size as usize];

        // Count the straight-line crossings of the topmost edges
        let mut crossings: i64 = 0;
        for k in 0..south_sequence.len() {
            let mut index = south_sequence[k] + first_index;
            if index < 0 || index as usize >= tree.len() {
                continue;
            }
            tree[index as usize] += 1;
            while index > 0 {
                if index % 2 > 0 {
                    crossings += tree[(index + 1) as usize];
                }
                index = (index - 1) / 2;
                tree[index as usize] += 1;
            }
        }

        // Create corners for the left side
        let mut left_corners: Vec<HyperedgeCorner> = Vec::with_capacity(hyperedges.len() * 2);
        for &he in &hyperedges {
            let h = &arena[he];
            left_corners.push(HyperedgeCorner { hyperedge: he, position: h.upper_left, opposite_position: h.lower_left, corner_type: CornerType::UPPER });
            left_corners.push(HyperedgeCorner { hyperedge: he, position: h.lower_left, opposite_position: h.upper_left, corner_type: CornerType::LOWER });
        }
        swift::sort_by(&mut left_corners, compare_corners);

        // Count crossings caused by overlapping hyperedge areas on the left side
        let mut open_hyperedges: i64 = 0;
        for corner in &left_corners {
            match corner.corner_type {
                CornerType::UPPER => open_hyperedges += 1,
                CornerType::LOWER => {
                    open_hyperedges -= 1;
                    crossings += open_hyperedges;
                }
            }
        }

        // Create corners for the right side
        let mut right_corners: Vec<HyperedgeCorner> = Vec::with_capacity(hyperedges.len() * 2);
        for &he in &hyperedges {
            let h = &arena[he];
            right_corners.push(HyperedgeCorner { hyperedge: he, position: h.upper_right, opposite_position: h.lower_right, corner_type: CornerType::UPPER });
            right_corners.push(HyperedgeCorner { hyperedge: he, position: h.lower_right, opposite_position: h.upper_right, corner_type: CornerType::LOWER });
        }
        swift::sort_by(&mut right_corners, compare_corners);

        // Count crossings caused by overlapping hyperedge areas on the right side
        open_hyperedges = 0;
        for corner in &right_corners {
            match corner.corner_type {
                CornerType::UPPER => open_hyperedges += 1,
                CornerType::LOWER => {
                    open_hyperedges -= 1;
                    crossings += open_hyperedges;
                }
            }
        }

        crossings
    }
}
