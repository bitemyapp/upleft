//! Port of `core/math/KVectorChain.swift`.
//!
//! Swift's `KVectorChain` is a class holding `KVector` class instances. Here a
//! chain is a plain `Vec<KVector>` value; a chain stored in a property (junction
//! points) is shared through [`KVectorChainRef`]. Where Swift relies on one
//! `KVector` object being in two chains at once, the port handles that at the
//! call site.

use std::cell::RefCell;
use std::rc::Rc;

use super::k_vector::KVector;

pub type KVectorChainRef = Rc<RefCell<KVectorChain>>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KVectorChain {
    pub elements: Vec<KVector>,
}

impl KVectorChain {
    pub fn new() -> KVectorChain {
        KVectorChain { elements: Vec::new() }
    }

    pub fn from_vec(elements: Vec<KVector>) -> KVectorChain {
        KVectorChain { elements }
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn size(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, KVector> {
        self.elements.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, KVector> {
        self.elements.iter_mut()
    }

    pub fn get(&self, index: usize) -> KVector {
        self.elements[index]
    }

    pub fn to_array(&self) -> Vec<KVector> {
        self.elements.clone()
    }

    pub fn add(&mut self, v: KVector) {
        self.elements.push(v);
    }

    pub fn add_xy(&mut self, x: f64, y: f64) {
        self.elements.push(KVector::new(x, y));
    }

    pub fn add_first(&mut self, v: KVector) {
        self.elements.insert(0, v);
    }

    pub fn add_first_xy(&mut self, x: f64, y: f64) {
        self.elements.insert(0, KVector::new(x, y));
    }

    pub fn add_last(&mut self, v: KVector) {
        self.elements.push(v);
    }

    pub fn add_last_xy(&mut self, x: f64, y: f64) {
        self.elements.push(KVector::new(x, y));
    }

    pub fn add_all(&mut self, vectors: &[KVector]) {
        self.elements.extend_from_slice(vectors);
    }

    pub fn add_all_as_copies(&mut self, index: usize, chain: &[KVector]) {
        let tail = self.elements.split_off(index);
        self.elements.extend_from_slice(chain);
        self.elements.extend(tail);
    }

    pub fn insert(&mut self, index: usize, v: KVector) {
        self.elements.insert(index, v);
    }

    pub fn remove(&mut self, index: usize) -> KVector {
        self.elements.remove(index)
    }

    pub fn clear(&mut self) {
        self.elements.clear();
    }

    pub fn get_first(&self) -> Option<KVector> {
        self.elements.first().copied()
    }

    pub fn get_last(&self) -> Option<KVector> {
        self.elements.last().copied()
    }

    /// `reverse()`: a new chain with copies in reverse order.
    pub fn reversed(&self) -> KVectorChain {
        KVectorChain { elements: self.elements.iter().rev().copied().collect() }
    }

    pub fn scale(&mut self, scale: f64) -> &mut Self {
        for v in &mut self.elements {
            v.scale(scale);
        }
        self
    }

    pub fn scale_xy(&mut self, scalex: f64, scaley: f64) -> &mut Self {
        for v in &mut self.elements {
            v.scale_xy(scalex, scaley);
        }
        self
    }

    pub fn offset(&mut self, offset: KVector) -> &mut Self {
        for v in &mut self.elements {
            v.add(offset);
        }
        self
    }

    pub fn offset_xy(&mut self, dx: f64, dy: f64) -> &mut Self {
        for v in &mut self.elements {
            v.add_xy(dx, dy);
        }
        self
    }

    pub fn total_length(&self) -> f64 {
        let mut length = 0.0;
        if self.elements.len() >= 2 {
            for i in 0..self.elements.len() - 1 {
                length += self.elements[i].distance(self.elements[i + 1]);
            }
        }
        length
    }

    pub fn has_nan(&self) -> bool {
        self.elements.iter().any(|v| v.is_nan())
    }

    pub fn has_infinite(&self) -> bool {
        self.elements.iter().any(|v| v.is_infinite())
    }

    pub fn point_on_line(&self, dist: f64) -> KVector {
        let elements = &self.elements;
        if elements.len() >= 2 {
            let abs_distance = dist.abs();
            let mut distance_sum = 0.0;
            if dist >= 0.0 {
                let mut current_point = elements[0];
                for i in 1..elements.len() {
                    let old_distance_sum = distance_sum;
                    let next_point = elements[i];
                    let additional = current_point.distance(next_point);
                    if additional > 0.0 {
                        distance_sum += additional;
                        if distance_sum >= abs_distance {
                            let this_relative = (abs_distance - old_distance_sum) / additional;
                            return next_point.subtracted(current_point).scaled(this_relative).added(current_point);
                        }
                    }
                    current_point = next_point;
                }
                *elements.last().unwrap()
            } else {
                let mut current_point = *elements.last().unwrap();
                let mut i = elements.len() - 1;
                while i > 0 {
                    let old_distance_sum = distance_sum;
                    let next_point = elements[i - 1];
                    let additional = current_point.distance(next_point);
                    if additional > 0.0 {
                        distance_sum += additional;
                        if distance_sum >= abs_distance {
                            let this_relative = (abs_distance - old_distance_sum) / additional;
                            return next_point.subtracted(current_point).scaled(this_relative).added(current_point);
                        }
                    }
                    current_point = next_point;
                    i -= 1;
                }
                elements[0]
            }
        } else if elements.len() == 1 {
            elements[0]
        } else {
            KVector::default()
        }
    }

    pub fn angle_on_line(&self, dist: f64) -> f64 {
        let elements = &self.elements;
        if elements.len() >= 2 {
            let abs_distance = dist.abs();
            let mut distance_sum = 0.0;
            if dist >= 0.0 {
                let mut current_point = elements[0];
                for i in 1..elements.len() {
                    let next_point = elements[i];
                    let additional = current_point.distance(next_point);
                    if additional > 0.0 {
                        distance_sum += additional;
                        if distance_sum >= abs_distance {
                            return next_point.subtracted(current_point).to_radians();
                        }
                    }
                    current_point = next_point;
                }
                elements[elements.len() - 1].subtracted(elements[elements.len() - 2]).to_radians()
            } else {
                let mut current_point = elements[elements.len() - 1];
                let mut i = elements.len() - 1;
                while i > 0 {
                    let next_point = elements[i - 1];
                    let additional = current_point.distance(next_point);
                    if additional > 0.0 {
                        distance_sum += additional;
                        if distance_sum >= abs_distance {
                            return next_point.subtracted(current_point).to_radians();
                        }
                    }
                    current_point = next_point;
                    i -= 1;
                }
                elements[1].subtracted(elements[0]).to_radians()
            }
        } else {
            0.0
        }
    }
}

impl<'a> IntoIterator for &'a KVectorChain {
    type Item = &'a KVector;
    type IntoIter = std::slice::Iter<'a, KVector>;
    fn into_iter(self) -> Self::IntoIter {
        self.elements.iter()
    }
}

pub fn kvector_chain_ref(chain: KVectorChain) -> KVectorChainRef {
    Rc::new(RefCell::new(chain))
}
