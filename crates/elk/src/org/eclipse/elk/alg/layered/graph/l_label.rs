//! Port of `alg/layered/graph/LLabel.swift`.

use super::l_graph::{LGraphArena, LLabelId};
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;

#[derive(Clone, Debug)]
pub struct LLabelData {
    pub props: PropertyMap,
    pub id: i32,
    pub position: KVector,
    pub size: KVector,
    pub text: String,
}

impl LGraphArena {
    /// `LLabel(text)`.
    pub fn new_label(&mut self, text: &str) -> LLabelId {
        let id = LLabelId(self.labels.len() as u32);
        self.labels.push(LLabelData { props: PropertyMap::new(), id: 0, position: KVector::default(), size: KVector::default(), text: text.to_string() });
        id
    }
}
