//! Port of `alg/layered/p4nodes/bk/BKAligner.swift`.
//!
//! Vertical alignment of the Brandes & Köpf node placer: groups nodes into
//! blocks (`root`/`align`) and computes the inner shifts within each block.

use super::bk_aligned_layout::{BKAlignedLayout, HDirection, VDirection};
use super::bk_node_placer::BKNodePlacer;
use super::neighborhood_information::{Neighbor, NeighborhoodInformation};
use crate::prelude::*;

pub struct BKAligner<'a> {
    pub layered_graph: LGraphId,
    pub ni: &'a NeighborhoodInformation,
}

#[inline]
fn nid(lg: &LGraphArena, n: LNodeId) -> usize {
    lg[n].id as usize
}

impl<'a> BKAligner<'a> {
    pub fn new(layered_graph: LGraphId, ni: &'a NeighborhoodInformation) -> BKAligner<'a> {
        BKAligner { layered_graph, ni }
    }

    /// `verticalAlignment(_:_:)`. `marked_edges` is the placer's
    /// `markedEdges` set as a membership table indexed by edge id.
    pub fn vertical_alignment(&self, lg: &LGraphArena, bal: &mut BKAlignedLayout, marked_edges: &[bool]) {
        let ni = self.ni;
        for &layer in &lg[self.layered_graph].layers {
            for &v in &lg[layer].nodes {
                bal.root[nid(lg, v)] = Some(v);
                bal.align[nid(lg, v)] = Some(v);
                bal.inner_shift[nid(lg, v)] = 0.0;
            }
        }

        let mut layers = lg[self.layered_graph].layers.clone();
        if bal.hdir == HDirection::LEFT {
            layers.reverse();
        }

        let is_marked = |e: LEdgeId| marked_edges.get(e.index()).copied().unwrap_or(false);

        for layer in layers {
            let mut r: i64 = -1;
            let mut nodes = lg[layer].nodes.clone();

            if bal.vdir == VDirection::UP {
                r = i64::MAX;
                nodes.reverse();
            }

            for v_i_k in nodes {
                let neighbors: &Vec<Neighbor> = if bal.hdir == HDirection::LEFT {
                    &ni.right_neighbors[nid(lg, v_i_k)]
                } else {
                    &ni.left_neighbors[nid(lg, v_i_k)]
                };

                if neighbors.is_empty() {
                    continue;
                }

                let d = neighbors.len();
                let low = ((d as f64 + 1.0) / 2.0).floor() as i64 - 1;
                let high = ((d as f64 + 1.0) / 2.0).ceil() as i64 - 1;

                if bal.vdir == VDirection::UP {
                    let mut m = high;
                    while m >= low {
                        if bal.align[nid(lg, v_i_k)] == Some(v_i_k) {
                            let (u_m, edge) = neighbors[m as usize];
                            if !is_marked(edge) && r > ni.node_index[nid(lg, u_m)] {
                                bal.align[nid(lg, u_m)] = Some(v_i_k);
                                bal.root[nid(lg, v_i_k)] = bal.root[nid(lg, u_m)];
                                if let Some(root) = bal.root[nid(lg, v_i_k)] {
                                    bal.align[nid(lg, v_i_k)] = Some(root);
                                    bal.od[nid(lg, root)] = bal.od[nid(lg, root)] && lg[v_i_k].node_type == NodeType::LONG_EDGE;
                                }
                                r = ni.node_index[nid(lg, u_m)];
                            }
                        }
                        m -= 1;
                    }
                } else {
                    for m in low..=high {
                        if bal.align[nid(lg, v_i_k)] == Some(v_i_k) {
                            let (um, edge) = neighbors[m as usize];
                            if !is_marked(edge) && r < ni.node_index[nid(lg, um)] {
                                bal.align[nid(lg, um)] = Some(v_i_k);
                                bal.root[nid(lg, v_i_k)] = bal.root[nid(lg, um)];
                                if let Some(root) = bal.root[nid(lg, v_i_k)] {
                                    bal.align[nid(lg, v_i_k)] = Some(root);
                                    bal.od[nid(lg, root)] = bal.od[nid(lg, root)] && lg[v_i_k].node_type == NodeType::LONG_EDGE;
                                }
                                r = ni.node_index[nid(lg, um)];
                            }
                        }
                    }
                }
            }
        }
    }

    /// `insideBlockShift(_:)`.
    pub fn inside_block_shift(&self, lg: &LGraphArena, bal: &mut BKAlignedLayout) {
        // Swift builds `BKNodePlacer.getBlocks(bal)` and skips roots without a
        // block. Every node's root is a key of that dictionary (it is keyed by
        // `root ?? node` over all nodes, and `root` here is a candidate's
        // root), so the check never skips anything and the dictionary is not
        // built.
        let mut seen_roots = vec![false; bal.root.len()];

        for &layer in &lg[self.layered_graph].layers {
            for &candidate in &lg[layer].nodes {
                let Some(root) = bal.root[nid(lg, candidate)] else { continue };
                if seen_roots[nid(lg, root)] {
                    continue;
                }
                seen_roots[nid(lg, root)] = true;

                let mut space_above = lg[root].margin.top;
                let mut space_below = lg[root].size.y + lg[root].margin.bottom;
                bal.inner_shift[nid(lg, root)] = 0.0;

                let mut current = root;
                while let Some(next) = bal.align[nid(lg, current)] {
                    if next == root {
                        break;
                    }
                    let Some(edge) = BKNodePlacer::get_edge(lg, current, next) else {
                        current = next;
                        continue;
                    };
                    let (Some(source), Some(target)) = (lg[edge].source, lg[edge].target) else {
                        current = next;
                        continue;
                    };

                    let port_pos_diff = if bal.hdir == HDirection::LEFT {
                        lg[target].position.y + lg[target].anchor.y - lg[source].position.y - lg[source].anchor.y
                    } else {
                        lg[source].position.y + lg[source].anchor.y - lg[target].position.y - lg[target].anchor.y
                    };

                    let next_inner_shift = bal.inner_shift[nid(lg, current)] + port_pos_diff;
                    bal.inner_shift[nid(lg, next)] = next_inner_shift;

                    space_above = swift::max(space_above, lg[next].margin.top - next_inner_shift);
                    space_below = swift::max(space_below, next_inner_shift + lg[next].size.y + lg[next].margin.bottom);

                    current = next;
                }

                current = root;
                loop {
                    bal.inner_shift[nid(lg, current)] += space_above;
                    let Some(next) = bal.align[nid(lg, current)] else { break };
                    current = next;
                    if current == root {
                        break;
                    }
                }

                bal.block_size[nid(lg, root)] = space_above + space_below;
            }
        }
    }
}
