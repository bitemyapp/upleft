//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/compound/org_eclipse_elk_alg_layered_compound_CompoundGraphPostprocessor.swift`.
//!
//! Postprocess a compound graph by restoring cross-hierarchy edges that have
//! previously been split by the `CompoundGraphPreprocessor`.
//!
//! Aliasing: `lastPoint` refers to vectors that were also appended to the
//! original edge's bend points; it is only read, so copies are equivalent.
//! `TARGET_OFFSET` receives a vector that is not touched afterwards. The
//! original edge's `JUNCTION_POINTS` chain is a shared `Rc` and is cleared
//! and refilled in place, as in Swift.

use std::cell::RefCell;
use std::rc::Rc;

use super::cross_hierarchy_edge::{CrossHierarchyEdge, CrossHierarchyMap};
use super::cross_hierarchy_edge_comparator::CrossHierarchyEdgeComparator;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LGraphId};
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::math::k_vector_chain::{KVectorChain, KVectorChainRef};
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
use crate::org::eclipse::elk::graph::properties::property::PropValue;
use crate::swift;

/// `OrthogonalRoutingGenerator.TOLERANCE` (group C's constant, `1e-3`).
const ORTHOGONAL_ROUTING_GENERATOR_TOLERANCE: f64 = 1e-3;

#[derive(Default)]
pub struct CompoundGraphPostprocessor;

impl CompoundGraphPostprocessor {
    pub fn new() -> CompoundGraphPostprocessor {
        CompoundGraphPostprocessor
    }

    /// `hasJunctionPointsPredicate`: whether a cross hierarchy edge has
    /// junction points.
    pub fn has_junction_points_predicate(lg: &LGraphArena, ch_edge: &CrossHierarchyEdge) -> bool {
        let jps = lg[ch_edge.get_edge()].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS);
        !jps.is_none_or(|j| j.borrow().is_empty())
    }
}

impl ILayoutProcessor for CompoundGraphPostprocessor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Compound graph postprocessor", 1.0);

        // whether bend points should be added whenever crossing a hierarchy boundary
        let add_unnecessary_bendpoints = lg[graph].props.get_as::<bool>(&LayeredOptions::UNNECESSARY_BENDPOINTS).unwrap_or(false);

        // restore the cross-hierarchy map that was built by the preprocessor
        let Some(cross_hierarchy_map) = lg[graph].props.get_object::<CrossHierarchyMap>(&InternalProperties::CROSS_HIERARCHY_MAP) else {
            monitor.done();
            return;
        };

        // remember all dummy edges we encounter; these need to be removed at
        // the end (a Swift `Set`, only used to disconnect them: any order)
        let mut dummy_edges: Vec<LEdgeId> = Vec::new();

        // iterate over all original edges
        // NONDETERMINISTIC IN SWIFT: dictionary iteration; see `CrossHierarchyMap`.
        for (orig_edge, cross_hierarchy_edges_list) in cross_hierarchy_map.iter() {
            // find all cross-hierarchy edges the original edge was split into, and sort them from source to target
            let mut cross_hierarchy_edges = cross_hierarchy_edges_list.to_vec();
            let comparator = CrossHierarchyEdgeComparator::new(graph);
            swift::sort_by(&mut cross_hierarchy_edges, |a, b| comparator.compare(lg, a, b) == std::cmp::Ordering::Less);

            // find the original source and target ports for the original edge
            let source_port = cross_hierarchy_edges[0].get_actual_source(lg);
            let target_port = cross_hierarchy_edges[cross_hierarchy_edges.len() - 1].get_actual_target(lg);

            let Some(reference_node) = lg[source_port].owner else { continue };
            let reference_graph: LGraphId;
            let target_node = lg[target_port].owner;
            match (target_node, lg[reference_node].nested_graph) {
                (Some(target_node), Some(nested_graph)) if lg.is_descendant(Some(target_node), Some(reference_node)) => {
                    reference_graph = nested_graph;
                }
                _ => {
                    if let Some(node_graph) = lg[reference_node].graph {
                        reference_graph = node_graph;
                    } else {
                        continue;
                    }
                }
            }

            // check whether there are any junction points
            let junction_points = clear_junction_points(lg, orig_edge, &cross_hierarchy_edges);

            // reset bend points (we have computed new ones anyway)
            lg[orig_edge].bend_points.clear();

            // apply the computed layouts to the cross-hierarchy edge
            let mut last_point: Option<KVector> = None;
            for ch_edge in &cross_hierarchy_edges {
                // transform all coordinates from the graph of the dummy edge to the reference graph
                let mut offset = KVector::default();
                lg.change_coord_system(&mut offset, ch_edge.get_graph(), reference_graph);

                let ledge = ch_edge.get_edge();
                let mut bend_points = KVectorChain::new();
                bend_points.add_all_as_copies(0, &lg[ledge].bend_points.to_array());
                bend_points.offset(offset);

                // Note: if an NPE occurs here, that means ELK Layered has replaced the original edge
                let (Some(ledge_source), Some(ledge_target)) = (lg[ledge].source, lg[ledge].target) else { continue };
                let mut source_point = lg.port_absolute_anchor(ledge_source);
                let mut target_point = lg.port_absolute_anchor(ledge_target);
                source_point.add(offset);
                target_point.add(offset);

                if let Some(last) = last_point {
                    let next_point = if bend_points.is_empty() {
                        target_point
                    } else if let Some(first) = bend_points.get_first() {
                        first
                    } else {
                        target_point
                    };

                    let x_diff_enough = (last.x - next_point.x).abs() > ORTHOGONAL_ROUTING_GENERATOR_TOLERANCE;
                    let y_diff_enough = (last.y - next_point.y).abs() > ORTHOGONAL_ROUTING_GENERATOR_TOLERANCE;

                    if (!add_unnecessary_bendpoints && x_diff_enough && y_diff_enough) || (add_unnecessary_bendpoints && (x_diff_enough || y_diff_enough)) {
                        lg[orig_edge].bend_points.add(source_point);
                    }
                }

                lg[orig_edge].bend_points.add_all(&bend_points.elements);

                if bend_points.is_empty() {
                    last_point = Some(source_point);
                } else {
                    last_point = bend_points.get_last();
                }

                // copy junction points
                if let Some(jp) = &junction_points {
                    copy_junction_points(lg, ledge, jp, offset);
                }

                // add offset to target port with a special property
                if ch_edge.get_actual_target(lg) == target_port {
                    let tp_graph = lg[target_port].owner.and_then(|n| lg[n].graph);
                    match tp_graph {
                        Some(tp_graph) if tp_graph != ch_edge.get_graph() => {
                            let mut new_offset = KVector::default();
                            lg.change_coord_system(&mut new_offset, tp_graph, reference_graph);
                            lg[orig_edge].props.set(&InternalProperties::TARGET_OFFSET, PropValue::kvector(new_offset));
                        }
                        _ => {
                            lg[orig_edge].props.set(&InternalProperties::TARGET_OFFSET, PropValue::kvector(offset));
                        }
                    }
                }

                // copy labels back to the original edge
                copy_labels_back(lg, ledge, orig_edge, reference_graph);

                // remember the dummy edge for later removal
                if !dummy_edges.contains(&ledge) {
                    dummy_edges.push(ledge);
                }
            }

            // restore the original source port and target port
            lg.edge_set_source(orig_edge, Some(source_port));
            lg.edge_set_target(orig_edge, Some(target_port));
        }

        // remove the dummy edges from the graph
        for dummy_edge in dummy_edges {
            lg.edge_set_source(dummy_edge, None);
            lg.edge_set_target(dummy_edge, None);
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "CompoundGraphPostprocessor"
    }
}

/// Clears an original edge's list of junction points and returns them.
pub fn clear_junction_points(lg: &mut LGraphArena, orig_edge: LEdgeId, cross_hierarchy_edges: &[CrossHierarchyEdge]) -> Option<KVectorChainRef> {
    let mut junction_points = lg[orig_edge].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS);

    let has_junction_points = cross_hierarchy_edges.iter().any(|che| CompoundGraphPostprocessor::has_junction_points_predicate(lg, che));
    if has_junction_points {
        match &junction_points {
            None => {
                let new_jps: KVectorChainRef = Rc::new(RefCell::new(KVectorChain::new()));
                junction_points = Some(new_jps.clone());
                lg[orig_edge].props.set(&LayeredOptions::JUNCTION_POINTS, PropValue::KVectorChain(new_jps));
            }
            Some(jp) => jp.borrow_mut().clear(),
        }
    } else if junction_points.is_some() {
        lg[orig_edge].props.remove(&LayeredOptions::JUNCTION_POINTS);
    }

    junction_points
}

/// Copies the junction points of the source to the target, adding the given offset.
pub fn copy_junction_points(lg: &LGraphArena, source: LEdgeId, target: &KVectorChainRef, offset: KVector) {
    let Some(ledge_jps) = lg[source].props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS) else { return };

    let mut jp_copies = KVectorChain::new();
    jp_copies.add_all_as_copies(0, &ledge_jps.borrow().to_array());
    jp_copies.offset(offset);

    target.borrow_mut().add_all(&jp_copies.elements);
}

/// Copies the labels from the given hierarchy segment back to the original
/// hierarchical edge.
pub fn copy_labels_back(lg: &mut LGraphArena, hierarchy_segment: LEdgeId, orig_edge: LEdgeId, reference_graph: LGraphId) {
    // Collect matching labels first to avoid mutation during iteration
    let to_move: Vec<_> = lg[hierarchy_segment]
        .labels
        .iter()
        .copied()
        .filter(|&l| lg[l].props.get_as::<LEdgeId>(&InternalProperties::ORIGINAL_LABEL_EDGE) == Some(orig_edge))
        .collect();
    for curr_label in to_move {
        if let Some(seg_graph) = lg.edge_source_node(hierarchy_segment).and_then(|n| lg[n].graph) {
            let mut position = lg[curr_label].position;
            lg.change_coord_system(&mut position, seg_graph, reference_graph);
            lg[curr_label].position = position;
        }

        lg[hierarchy_segment].labels.retain(|&l| l != curr_label);
        lg[orig_edge].labels.push(curr_label);
    }
}
