//! Port of `alg/layered/components/AbstractGraphPlacer.swift`.
//!
//! Takes a list of laid-out component graphs and combines them into a single
//! graph. The helpers move or offset whole graphs (nodes, bend points,
//! junction points and edge labels of outgoing edges).

use std::cell::RefCell;
use std::rc::Rc;

use crate::prelude::*;

pub trait AbstractGraphPlacer {
    /// Computes a placement for the given graphs and combines them into
    /// `target`.
    fn combine(&mut self, lg: &mut LGraphArena, components: &[LGraphId], target: LGraphId);
}

/// Adds `offset` to every point of a chain (`KVectorChain.offset(_:)`).
fn offset_chain(chain: &mut KVectorChain, offset: KVector) {
    for v in chain.iter_mut() {
        v.add(offset);
    }
}

/// Offsets one node's position and its outgoing edges' bend points,
/// junction points and labels.
fn offset_node(lg: &mut LGraphArena, node: LNodeId, graph_offset: KVector) {
    lg[node].position.add(graph_offset);
    for pi in 0..lg[node].ports.len() {
        let port = lg[node].ports[pi];
        for ei in 0..lg[port].outgoing_edges.len() {
            let edge = lg[port].outgoing_edges[ei];
            offset_chain(&mut lg[edge].bend_points, graph_offset);
            // `let junctionPoints: KVectorChain = edge.getProperty(...)` is the
            // stored chain object itself, so the offset is visible through
            // every holder of it.
            if let Some(junction_points) = lg[edge].props.get_typed::<Rc<RefCell<KVectorChain>>>(&LayeredOptions::JUNCTION_POINTS) {
                offset_chain(&mut junction_points.borrow_mut(), graph_offset);
            }
            for li in 0..lg[edge].labels.len() {
                let label = lg[edge].labels[li];
                lg[label].position.add(graph_offset);
            }
        }
    }
}

/// `moveGraphs(_:_:_:_:)`.
pub fn move_graphs(lg: &mut LGraphArena, dest_graph: LGraphId, source_graphs: &[LGraphId], offsetx: f64, offsety: f64) {
    for &source_graph in source_graphs {
        move_graph(lg, dest_graph, source_graph, offsetx, offsety);
    }
}

/// `moveGraph(_:_:_:_:)`: offsets the source graph's nodes by its offset plus
/// the given one and appends them to the destination graph.
pub fn move_graph(lg: &mut LGraphArena, dest_graph: LGraphId, source_graph: LGraphId, offsetx: f64, offsety: f64) {
    let mut graph_offset = lg[source_graph].offset;
    graph_offset.add(KVector::new(offsetx, offsety));

    for node in lg[source_graph].layerless_nodes.clone() {
        offset_node(lg, node, graph_offset);
        lg[dest_graph].layerless_nodes.push(node);
        lg[node].graph = Some(dest_graph);
    }
}

/// `offsetGraphs(_:_:_:)`.
pub fn offset_graphs(lg: &mut LGraphArena, graphs: &[LGraphId], offsetx: f64, offsety: f64) {
    for &graph in graphs {
        offset_graph(lg, graph, offsetx, offsety);
    }
}

/// `offsetGraph(_:_:_:)`: offsets a graph's contents in place.
pub fn offset_graph(lg: &mut LGraphArena, graph: LGraphId, offsetx: f64, offsety: f64) {
    let graph_offset = KVector::new(offsetx, offsety);
    for ni in 0..lg[graph].layerless_nodes.len() {
        let node = lg[graph].layerless_nodes[ni];
        offset_node(lg, node, graph_offset);
    }
}
