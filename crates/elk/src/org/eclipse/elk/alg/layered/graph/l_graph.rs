//! Port of `alg/layered/graph/LGraph.swift`, plus the arena every layered
//! graph element lives in.
//!
//! Swift's layered graph is a web of class instances (`LGraph`, `Layer`,
//! `LNode`, `LPort`, `LEdge`, `LLabel`). Here each element is a record in an
//! [`LGraphArena`] addressed by a typed index; an index stands in for the Swift
//! object reference, so `===` becomes `==`. Elements are never freed during a
//! layout (Swift frees them when unreferenced, which is unobservable).
//! `arena[id]` borrows an element; the Swift methods that touch several
//! elements (`setLayer`, `setSource`, …) are arena methods.

use std::ops::{Index, IndexMut};

use super::l_edge::LEdgeData;
use super::l_label::LLabelData;
use super::l_node::LNodeData;
use super::l_padding::LPadding;
use super::l_port::LPortData;
use super::layer::LayerData;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;

macro_rules! ids {
    ($($name:ident),*) => {$(
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
        pub struct $name(pub u32);
        impl $name {
            #[inline]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    )*};
}

ids!(LGraphId, LayerId, LNodeId, LPortId, LEdgeId, LLabelId);

/// `LGraph`: a layered graph (a set of layers plus not-yet-layered nodes).
#[derive(Clone, Debug, Default)]
pub struct LGraphData {
    pub props: PropertyMap,
    /// `LGraphElement.id`.
    pub id: i32,
    pub size: KVector,
    pub padding: LPadding,
    pub offset: KVector,
    pub layerless_nodes: Vec<LNodeId>,
    pub layers: Vec<LayerId>,
    pub parent_node: Option<LNodeId>,
}

/// All layered-graph elements of one layout run.
#[derive(Clone, Debug, Default)]
pub struct LGraphArena {
    pub graphs: Vec<LGraphData>,
    pub layers: Vec<LayerData>,
    pub nodes: Vec<LNodeData>,
    pub ports: Vec<LPortData>,
    pub edges: Vec<LEdgeData>,
    pub labels: Vec<LLabelData>,
}

macro_rules! arena_index {
    ($($id:ident => $field:ident: $data:ident),*) => {$(
        impl Index<$id> for LGraphArena {
            type Output = $data;
            #[inline]
            fn index(&self, id: $id) -> &$data {
                &self.$field[id.0 as usize]
            }
        }
        impl IndexMut<$id> for LGraphArena {
            #[inline]
            fn index_mut(&mut self, id: $id) -> &mut $data {
                &mut self.$field[id.0 as usize]
            }
        }
    )*};
}

arena_index!(
    LGraphId => graphs: LGraphData,
    LayerId => layers: LayerData,
    LNodeId => nodes: LNodeData,
    LPortId => ports: LPortData,
    LEdgeId => edges: LEdgeData,
    LLabelId => labels: LLabelData
);

impl LGraphArena {
    pub fn new() -> LGraphArena {
        LGraphArena::default()
    }

    /// `LGraph()`.
    pub fn new_graph(&mut self) -> LGraphId {
        let id = LGraphId(self.graphs.len() as u32);
        self.graphs.push(LGraphData::default());
        id
    }

    /// `getActualSize()`: size plus padding.
    pub fn graph_actual_size(&self, g: LGraphId) -> KVector {
        let graph = &self[g];
        KVector::new(
            graph.size.x + graph.padding.left + graph.padding.right,
            graph.size.y + graph.padding.top + graph.padding.bottom,
        )
    }

    /// `addLayer()`: creates a layer and appends it to the graph's layers.
    pub fn graph_add_layer(&mut self, g: LGraphId) -> LayerId {
        let layer = self.new_layer(g);
        self[g].layers.push(layer);
        layer
    }

    /// `removeLayer(_:)`.
    pub fn graph_remove_layer(&mut self, g: LGraphId, layer: LayerId) {
        self[g].layers.retain(|&l| l != layer);
    }

    /// `removeLayerlessNode(_:)`.
    pub fn graph_remove_layerless_node(&mut self, g: LGraphId, node: LNodeId) {
        self[g].layerless_nodes.retain(|&n| n != node);
    }

    /// `toNodeArray()`.
    pub fn graph_to_node_array(&self, g: LGraphId) -> Vec<Vec<LNodeId>> {
        self[g].layers.iter().map(|&l| self[l].nodes.clone()).collect()
    }
}
