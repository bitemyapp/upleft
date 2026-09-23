//! Port of `alg/layered/p4nodes/bk/BKCompactor.swift`.
//!
//! Horizontal compaction of the Brandes & Köpf node placer: places blocks as
//! close to each other as possible, then shifts the classes of blocks.

use std::rc::Rc;

use super::bk_aligned_layout::{BKAlignedLayout, HDirection, VDirection};
use super::i_compactor::ICompactor;
use super::neighborhood_information::NeighborhoodInformation;
use super::threshold_strategy::ThresholdStrategy;
use crate::org::eclipse::elk::alg::layered::options::edge_straightening_strategy::EdgeStraighteningStrategy;
use crate::org::eclipse::elk::alg::layered::options::spacings::Spacings;
use crate::prelude::*;

/// `ClassNode`: a class (sink) in the class graph. Edges point at other class
/// nodes by index into [`BKCompactor::class_nodes`].
#[derive(Clone, Debug, Default)]
pub struct ClassNode {
    pub class_shift: Option<f64>,
    pub node: Option<LNodeId>,
    pub outgoing: Vec<ClassEdge>,
    pub indegree: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct ClassEdge {
    pub separation: f64,
    pub target: usize,
}

pub struct BKCompactor<'a> {
    pub layered_graph: LGraphId,
    pub thresh_strategy: Option<ThresholdStrategy>,
    pub ni: &'a NeighborhoodInformation,
    pub use_simple_threshold: bool,
    pub spacings: Rc<Spacings>,
    /// `sinkNodes`' values, in insertion order.
    pub class_nodes: Vec<ClassNode>,
    /// `sinkNodes`' keys: class node index by sink node `id`.
    sink_node_index: Vec<Option<usize>>,
    /// `placedBlockRoots`, indexed by node `id`.
    placed_block_roots: Vec<bool>,
}

#[inline]
fn nid(lg: &LGraphArena, n: LNodeId) -> usize {
    lg[n].id as usize
}

impl<'a> BKCompactor<'a> {
    pub fn new(lg: &mut LGraphArena, layered_graph: LGraphId, ni: &'a NeighborhoodInformation) -> BKCompactor<'a> {
        let spacings = match lg[layered_graph].props.get_object::<Spacings>(&InternalProperties::SPACINGS) {
            Some(s) => s,
            None => {
                let empty = lg.new_graph();
                Rc::new(Spacings::new(lg, empty))
            }
        };

        // The string-key overload: the stored value only.
        let straightening = lg[layered_graph]
            .props
            .get_by_id("org.eclipse.elk.layered.nodePlacement.bk.edgeStraightening")
            .and_then(|v| v.cast::<EdgeStraighteningStrategy>())
            .unwrap_or(EdgeStraighteningStrategy::NONE);
        BKCompactor {
            layered_graph,
            thresh_strategy: None,
            ni,
            use_simple_threshold: straightening == EdgeStraighteningStrategy::IMPROVE_STRAIGHTNESS,
            spacings,
            class_nodes: Vec::new(),
            sink_node_index: Vec::new(),
            placed_block_roots: Vec::new(),
        }
    }

    /// `placeBlock(_:_:)`.
    pub fn place_block(&mut self, lg: &LGraphArena, root: LNodeId, bal: &mut BKAlignedLayout) {
        if self.placed_block_roots[nid(lg, root)] {
            return;
        }

        let mut is_initial_assignment = true;
        bal.y[nid(lg, root)] = 0.0;

        let mut current_node = root;
        let mut thresh = if bal.vdir == VDirection::DOWN { -f64::INFINITY } else { f64::INFINITY };

        loop {
            let Some(layer) = lg[current_node].layer else { break };
            let current_index_in_layer = self.ni.node_index[nid(lg, current_node)];
            let current_layer_size = lg[layer].nodes.len() as i64;

            if (bal.vdir == VDirection::DOWN && current_index_in_layer > 0)
                || (bal.vdir == VDirection::UP && current_index_in_layer < current_layer_size - 1)
            {
                let neighbor = if bal.vdir == VDirection::UP {
                    lg[layer].nodes[(current_index_in_layer + 1) as usize]
                } else {
                    lg[layer].nodes[(current_index_in_layer - 1) as usize]
                };
                let Some(neighbor_root) = bal.root[nid(lg, neighbor)] else { break };

                self.place_block(lg, neighbor_root, bal);

                if let Some(ts) = self.thresh_strategy.as_mut() {
                    thresh = ts.calculate_threshold(lg, bal, thresh, root, current_node);
                }

                if bal.sink[nid(lg, root)] == Some(root) {
                    bal.sink[nid(lg, root)] = bal.sink[nid(lg, neighbor_root)];
                }

                if bal.sink[nid(lg, root)] == bal.sink[nid(lg, neighbor_root)] {
                    let spacing = self.spacings.get_vertical_spacing(lg, current_node, neighbor);

                    if bal.vdir == VDirection::UP {
                        let current_block_position = bal.y[nid(lg, root)];
                        let new_position = bal.y[nid(lg, neighbor_root)] + bal.inner_shift[nid(lg, neighbor)]
                            - lg[neighbor].margin.top
                            - spacing
                            - lg[current_node].margin.bottom
                            - lg[current_node].size.y
                            - bal.inner_shift[nid(lg, current_node)];

                        if is_initial_assignment {
                            is_initial_assignment = false;
                            bal.y[nid(lg, root)] = swift::min(new_position, thresh);
                        } else {
                            bal.y[nid(lg, root)] = swift::min(current_block_position, swift::min(new_position, thresh));
                        }
                    } else {
                        let current_block_position = bal.y[nid(lg, root)];
                        let new_position = bal.y[nid(lg, neighbor_root)]
                            + bal.inner_shift[nid(lg, neighbor)]
                            + lg[neighbor].size.y
                            + lg[neighbor].margin.bottom
                            + spacing
                            + lg[current_node].margin.top
                            - bal.inner_shift[nid(lg, current_node)];

                        if is_initial_assignment {
                            is_initial_assignment = false;
                            bal.y[nid(lg, root)] = swift::max(new_position, thresh);
                        } else {
                            bal.y[nid(lg, root)] = swift::max(current_block_position, swift::max(new_position, thresh));
                        }
                    }
                } else {
                    // The string-key overload: the stored value only.
                    let spacing = lg[self.layered_graph].props.get_by_id("org.eclipse.elk.spacing.nodeNode").and_then(|v| v.cast::<f64>()).unwrap_or(0.0);

                    let (Some(root_sink), Some(neighbor_sink)) = (bal.sink[nid(lg, root)], bal.sink[nid(lg, neighbor_root)]) else { break };

                    let sink_node = self.get_or_create_class_node(lg, root_sink);
                    let neighbor_sink_node = self.get_or_create_class_node(lg, neighbor_sink);

                    let required_space = if bal.vdir == VDirection::UP {
                        bal.y[nid(lg, root)] + bal.inner_shift[nid(lg, current_node)] + lg[current_node].size.y + lg[current_node].margin.bottom + spacing
                            - (bal.y[nid(lg, neighbor_root)] + bal.inner_shift[nid(lg, neighbor)] - lg[neighbor].margin.top)
                    } else {
                        bal.y[nid(lg, root)] + bal.inner_shift[nid(lg, current_node)]
                            - lg[current_node].margin.top
                            - bal.y[nid(lg, neighbor_root)]
                            - bal.inner_shift[nid(lg, neighbor)]
                            - lg[neighbor].size.y
                            - lg[neighbor].margin.bottom
                            - spacing
                    };
                    self.add_class_edge(sink_node, neighbor_sink_node, required_space);
                }
            } else if let Some(ts) = self.thresh_strategy.as_mut() {
                thresh = ts.calculate_threshold(lg, bal, thresh, root, current_node);
            }

            let Some(next) = bal.align[nid(lg, current_node)] else { break };
            current_node = next;
            if current_node == root {
                break;
            }
        }

        if let Some(ts) = self.thresh_strategy.as_mut() {
            ts.finish_block(lg, root);
        }
        self.placed_block_roots[nid(lg, root)] = true;
    }

    /// `ClassNode.addEdge(_:_:)`.
    fn add_class_edge(&mut self, source: usize, target: usize, separation: f64) {
        self.class_nodes[target].indegree += 1;
        self.class_nodes[source].outgoing.push(ClassEdge { separation, target });
    }

    /// `placeClasses(_:)`.
    ///
    /// NONDETERMINISTIC IN SWIFT (but not observably): `sinkNodes.values`
    /// iterates a dictionary keyed by `ObjectIdentifier`. The class shifts do
    /// not depend on the order: sources get 0, every other class the min (max)
    /// over all its predecessors, each taken once all predecessors are done,
    /// and classes on cycles are never reached; only the order of commutative
    /// `min`/`max` calls changes (which could only show on signed zeros).
    /// Insertion order is used.
    pub fn place_classes(&mut self, lg: &LGraphArena, bal: &mut BKAlignedLayout) {
        let mut sinks: Vec<usize> = (0..self.class_nodes.len()).filter(|&i| self.class_nodes[i].indegree == 0).collect();
        let mut cursor = 0;

        while cursor < sinks.len() {
            let n = sinks[cursor];
            cursor += 1;

            if self.class_nodes[n].class_shift.is_none() {
                self.class_nodes[n].class_shift = Some(0.0);
            }

            for ei in 0..self.class_nodes[n].outgoing.len() {
                let e = self.class_nodes[n].outgoing[ei];
                let n_shift = self.class_nodes[n].class_shift.unwrap_or(0.0);
                let target = &mut self.class_nodes[e.target];

                if target.class_shift.is_none() {
                    target.class_shift = Some(n_shift + e.separation);
                } else if bal.vdir == VDirection::DOWN {
                    target.class_shift = Some(swift::min(target.class_shift.unwrap_or(0.0), n_shift + e.separation));
                } else {
                    target.class_shift = Some(swift::max(target.class_shift.unwrap_or(0.0), n_shift + e.separation));
                }

                target.indegree -= 1;
                if target.indegree == 0 {
                    sinks.push(e.target);
                }
            }
        }

        for n in &self.class_nodes {
            if let Some(sink_node) = n.node {
                bal.shift[nid(lg, sink_node)] = n.class_shift.unwrap_or(0.0);
            }
        }
    }

    /// `getOrCreateClassNode(_:_:)`: the class node's index.
    pub fn get_or_create_class_node(&mut self, lg: &LGraphArena, sink_node: LNodeId) -> usize {
        let key = nid(lg, sink_node);
        if key >= self.sink_node_index.len() {
            self.sink_node_index.resize(key + 1, None);
        }
        if let Some(existing) = self.sink_node_index[key] {
            return existing;
        }

        let created = self.class_nodes.len();
        self.class_nodes.push(ClassNode { class_shift: None, node: Some(sink_node), outgoing: Vec::new(), indegree: 0 });
        self.sink_node_index[key] = Some(created);
        created
    }
}

impl<'a> ICompactor for BKCompactor<'a> {
    /// `horizontalCompaction(_:)`.
    fn horizontal_compaction(&mut self, lg: &LGraphArena, bal: &mut BKAlignedLayout) {
        for &layer in &lg[self.layered_graph].layers {
            for &node in &lg[layer].nodes {
                bal.sink[nid(lg, node)] = Some(node);
                bal.shift[nid(lg, node)] = if bal.vdir == VDirection::UP { -f64::INFINITY } else { f64::INFINITY };
            }
        }

        self.class_nodes.clear();
        self.sink_node_index.clear();
        self.sink_node_index.resize(self.ni.node_count, None);
        self.placed_block_roots.clear();
        self.placed_block_roots.resize(self.ni.node_count, false);

        let mut layers = lg[self.layered_graph].layers.clone();
        if bal.hdir == HDirection::LEFT {
            layers.reverse();
        }

        self.thresh_strategy = Some(if self.use_simple_threshold {
            ThresholdStrategy::simple_threshold_strategy(self.ni.node_count)
        } else {
            ThresholdStrategy::null_threshold_strategy(self.ni.node_count)
        });

        for &layer in &layers {
            let mut nodes = lg[layer].nodes.clone();
            if bal.vdir == VDirection::UP {
                nodes.reverse();
            }

            for v in nodes {
                if bal.root[nid(lg, v)] == Some(v) {
                    self.place_block(lg, v, bal);
                }
            }
        }

        self.place_classes(lg, bal);

        for &layer in &layers {
            for &v in &lg[layer].nodes {
                let Some(root) = bal.root[nid(lg, v)] else { continue };
                bal.y[nid(lg, v)] = bal.y[nid(lg, root)];

                if v == root {
                    if let Some(sink) = bal.sink[nid(lg, v)] {
                        let sink_shift = bal.shift[nid(lg, sink)];
                        if (bal.vdir == VDirection::UP && sink_shift > -f64::INFINITY) || (bal.vdir == VDirection::DOWN && sink_shift < f64::INFINITY) {
                            bal.y[nid(lg, v)] += sink_shift;
                        }
                    }
                }
            }
        }

        if let Some(mut ts) = self.thresh_strategy.take() {
            ts.post_process(lg, bal, self.ni);
            self.thresh_strategy = Some(ts);
        }
    }
}
