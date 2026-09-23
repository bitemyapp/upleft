//! Port of `alg/layered/p4nodes/bk/BKAlignedLayout.swift`.
//!
//! One of the (up to four) layouts the Brandes & Köpf node placer computes.
//! All arrays are indexed by the node `id`s `NeighborhoodInformation` assigned.

use std::rc::Rc;

use super::neighborhood_information::NeighborhoodInformation;
use crate::org::eclipse::elk::alg::layered::options::spacings::Spacings;
use crate::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VDirection {
    DOWN,
    UP,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HDirection {
    RIGHT,
    LEFT,
}

pub struct BKAlignedLayout {
    pub root: Vec<Option<LNodeId>>,
    pub block_size: Vec<f64>,
    pub align: Vec<Option<LNodeId>>,
    pub inner_shift: Vec<f64>,
    pub sink: Vec<Option<LNodeId>>,
    pub shift: Vec<f64>,
    pub y: Vec<f64>,
    pub vdir: VDirection,
    pub hdir: HDirection,
    pub su: Vec<bool>,
    pub od: Vec<bool>,
    pub layered_graph: LGraphId,
    pub spacings: Rc<Spacings>,
}

#[inline]
fn nid(lg: &LGraphArena, n: LNodeId) -> usize {
    lg[n].id as usize
}

impl BKAlignedLayout {
    /// `init(_:_:_:_:)`.
    pub fn new(lg: &mut LGraphArena, layered_graph: LGraphId, node_count: usize, vdir: VDirection, hdir: HDirection) -> BKAlignedLayout {
        let spacings = match lg[layered_graph].props.get_object::<Spacings>(&InternalProperties::SPACINGS) {
            Some(s) => s,
            // `?? Spacings()`: a spacings object reading an empty graph.
            None => {
                let empty = lg.new_graph();
                Rc::new(Spacings::new(lg, empty))
            }
        };
        BKAlignedLayout {
            root: vec![None; node_count],
            block_size: vec![0.0; node_count],
            align: vec![None; node_count],
            inner_shift: vec![0.0; node_count],
            sink: vec![None; node_count],
            shift: vec![0.0; node_count],
            y: vec![0.0; node_count],
            vdir,
            hdir,
            su: vec![false; node_count],
            od: vec![true; node_count],
            layered_graph,
            spacings,
        }
    }

    pub fn cleanup(&mut self) {
        self.root = Vec::new();
        self.block_size = Vec::new();
        self.align = Vec::new();
        self.inner_shift = Vec::new();
        self.sink = Vec::new();
        self.shift = Vec::new();
        self.y = Vec::new();
        self.su = Vec::new();
        self.od = Vec::new();
    }

    /// `layoutSize()`.
    pub fn layout_size(&self, lg: &LGraphArena) -> f64 {
        let mut min_val = f64::INFINITY;
        let mut max_val = -f64::INFINITY;

        for &layer in &lg[self.layered_graph].layers {
            for &n in &lg[layer].nodes {
                let y_min = self.y[nid(lg, n)];
                let root_id = self.root[nid(lg, n)].map_or(nid(lg, n), |r| nid(lg, r));
                let y_max = y_min + self.block_size[root_id];
                min_val = swift::min(min_val, y_min);
                max_val = swift::max(max_val, y_max);
            }
        }
        max_val - min_val
    }

    /// `calculateDelta(_:_:)`.
    pub fn calculate_delta(&self, lg: &LGraphArena, src: LPortId, tgt: LPortId) -> f64 {
        let (Some(src_node), Some(tgt_node)) = (lg[src].owner, lg[tgt].owner) else { return 0.0 };

        let src_pos = self.y[nid(lg, src_node)] + self.inner_shift[nid(lg, src_node)] + lg[src].position.y + lg[src].anchor.y;
        let tgt_pos = self.y[nid(lg, tgt_node)] + self.inner_shift[nid(lg, tgt_node)] + lg[tgt].position.y + lg[tgt].anchor.y;
        tgt_pos - src_pos
    }

    /// `shiftBlock(_:_:)`.
    pub fn shift_block(&mut self, lg: &LGraphArena, root_node: LNodeId, delta: f64) {
        let mut current = Some(root_node);
        loop {
            let Some(current_node) = current else { break };
            self.y[nid(lg, current_node)] += delta;
            current = self.align[nid(lg, current_node)];
            if current == Some(root_node) {
                break;
            }
        }
    }

    /// `checkSpaceAbove(_:_:_:)`.
    pub fn check_space_above(&self, lg: &LGraphArena, block_root: LNodeId, delta: f64, ni: &NeighborhoodInformation) -> f64 {
        let mut available_space = delta;
        let root_node = block_root;
        let mut current = Some(root_node);

        loop {
            let Some(c) = current else { break };
            let Some(next) = self.align[nid(lg, c)] else { break };
            current = Some(next);
            let min_y_current = self.get_min_y(lg, next);

            if let Some(neighbor) = self.get_upper_neighbor(lg, next, ni) {
                let max_y_neighbor = self.get_max_y(lg, neighbor);
                available_space = swift::min(
                    available_space,
                    min_y_current - (max_y_neighbor + self.spacings.get_vertical_spacing(lg, next, neighbor)),
                );
            }
            if current == Some(root_node) {
                break;
            }
        }

        available_space
    }

    /// `checkSpaceBelow(_:_:_:)`.
    pub fn check_space_below(&self, lg: &LGraphArena, block_root: LNodeId, delta: f64, ni: &NeighborhoodInformation) -> f64 {
        let mut available_space = delta;
        let root_node = block_root;
        let mut current = Some(root_node);

        loop {
            let Some(c) = current else { break };
            let Some(next) = self.align[nid(lg, c)] else { break };
            current = Some(next);
            let max_y_current = self.get_max_y(lg, next);

            if let Some(neighbor) = self.get_lower_neighbor(lg, next, ni) {
                let min_y_neighbor = self.get_min_y(lg, neighbor);
                available_space = swift::min(
                    available_space,
                    min_y_neighbor - (max_y_current + self.spacings.get_vertical_spacing(lg, next, neighbor)),
                );
            }
            if current == Some(root_node) {
                break;
            }
        }

        available_space
    }

    /// `getMinY(_:)`.
    pub fn get_min_y(&self, lg: &LGraphArena, n: LNodeId) -> f64 {
        let root_node = self.root[nid(lg, n)].unwrap_or(n);
        self.y[nid(lg, root_node)] + self.inner_shift[nid(lg, n)] - lg[n].margin.top
    }

    /// `getMaxY(_:)`.
    pub fn get_max_y(&self, lg: &LGraphArena, n: LNodeId) -> f64 {
        let root_node = self.root[nid(lg, n)].unwrap_or(n);
        self.y[nid(lg, root_node)] + self.inner_shift[nid(lg, n)] + lg[n].size.y + lg[n].margin.bottom
    }

    /// `getLowerNeighbor(_:_:)`.
    pub fn get_lower_neighbor(&self, lg: &LGraphArena, n: LNodeId, ni: &NeighborhoodInformation) -> Option<LNodeId> {
        let layer = lg[n].layer?;
        let layer_pos = ni.node_index[nid(lg, n)];
        let nodes = &lg[layer].nodes;
        if layer_pos < nodes.len() as i64 - 1 {
            return Some(nodes[(layer_pos + 1) as usize]);
        }
        None
    }

    /// `getUpperNeighbor(_:_:)`.
    pub fn get_upper_neighbor(&self, lg: &LGraphArena, n: LNodeId, ni: &NeighborhoodInformation) -> Option<LNodeId> {
        let layer = lg[n].layer?;
        let layer_pos = ni.node_index[nid(lg, n)];
        if layer_pos > 0 {
            return Some(lg[layer].nodes[(layer_pos - 1) as usize]);
        }
        None
    }

    /// `toString()`.
    pub fn to_string(&self) -> String {
        let mut result = String::new();
        result += if self.hdir == HDirection::RIGHT { "RIGHT" } else { "LEFT" };
        result += if self.vdir == VDirection::DOWN { "DOWN" } else { "UP" };
        result
    }
}
