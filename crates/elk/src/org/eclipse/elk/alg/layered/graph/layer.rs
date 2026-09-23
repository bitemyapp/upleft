//! Port of `alg/layered/graph/Layer.swift`.

use super::l_graph::{LGraphArena, LGraphId, LNodeId, LayerId};
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;

#[derive(Clone, Debug)]
pub struct LayerData {
    pub props: PropertyMap,
    pub id: i32,
    pub owner: LGraphId,
    pub size: KVector,
    pub nodes: Vec<LNodeId>,
}

impl LGraphArena {
    /// `Layer(graph)`: a layer owned by `graph` but not yet in its layer list.
    pub fn new_layer(&mut self, graph: LGraphId) -> LayerId {
        let id = LayerId(self.layers.len() as u32);
        self.layers.push(LayerData { props: PropertyMap::new(), id: 0, owner: graph, size: KVector::default(), nodes: Vec::new() });
        id
    }

    /// `getIndex()`: the layer's position in its graph, or -1.
    pub fn layer_index(&self, layer: LayerId) -> i32 {
        let owner = self[layer].owner;
        self[owner].layers.iter().position(|&l| l == layer).map_or(-1, |i| i as i32)
    }

    /// `setNodes(_:)`: replaces the node list and points each node at this layer.
    pub fn layer_set_nodes(&mut self, layer: LayerId, nodes: Vec<LNodeId>) {
        for &n in &nodes {
            self[n].layer = Some(layer);
        }
        self[layer].nodes = nodes;
    }
}
