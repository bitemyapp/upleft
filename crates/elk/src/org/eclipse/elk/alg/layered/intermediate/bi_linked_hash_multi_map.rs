//! Port of `alg/layered/intermediate/BiLinkedHashMultiMap.swift` (with its
//! `OrderedDictionary`).
//!
//! A multimap from keys to ordered value lists that also maps each value back
//! to its key, so values can be moved between keys cheaply. Keys keep their
//! insertion order. Nothing in elk-swift uses it; ported for completeness.

use std::collections::HashMap;
use std::hash::Hash;

/// `OrderedDictionary`: a dictionary that remembers key insertion order.
#[derive(Clone, Debug)]
pub struct OrderedDictionary<K: Hash + Eq + Clone, V> {
    pub keys: Vec<K>,
    pub values: HashMap<K, V>,
}

impl<K: Hash + Eq + Clone, V> Default for OrderedDictionary<K, V> {
    fn default() -> Self {
        OrderedDictionary { keys: Vec::new(), values: HashMap::new() }
    }
}

impl<K: Hash + Eq + Clone, V> OrderedDictionary<K, V> {
    pub fn get(&self, key: &K) -> Option<&V> {
        self.values.get(key)
    }

    /// `self[key] = newValue` (`nil` removes the key).
    pub fn set(&mut self, key: K, new_value: Option<V>) {
        match new_value {
            None => {
                if let Some(index) = self.keys.iter().position(|k| *k == key) {
                    self.keys.remove(index);
                }
                self.values.remove(&key);
            }
            Some(v) => {
                if !self.keys.contains(&key) {
                    self.keys.push(key.clone());
                }
                self.values.insert(key, v);
            }
        }
    }

    pub fn ordered_keys(&self) -> &[K] {
        &self.keys
    }
}

#[derive(Clone, Debug)]
pub struct BiLinkedHashMultiMap<K: Ord + Hash + Clone, V: Hash + Eq + Clone> {
    pub multi_map_key_to_values: OrderedDictionary<K, Vec<V>>,
    pub hash_map_values_to_key: HashMap<V, K>,
}

impl<K: Ord + Hash + Clone, V: Hash + Eq + Clone> Default for BiLinkedHashMultiMap<K, V> {
    fn default() -> Self {
        BiLinkedHashMultiMap { multi_map_key_to_values: OrderedDictionary::default(), hash_map_values_to_key: HashMap::new() }
    }
}

impl<K: Ord + Hash + Clone, V: Hash + Eq + Clone> BiLinkedHashMultiMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `putAll(key:values:)`.
    pub fn put_all(&mut self, key: K, values: &[V]) {
        for value in values {
            self.put(key.clone(), value.clone());
        }
    }

    /// `put(key:value:)`: moves `value` to the end of `key`'s list.
    pub fn put(&mut self, key: K, value: V) {
        // Remove old value.
        if let Some(old_key) = self.hash_map_values_to_key.get(&value).cloned() {
            if let Some(values) = self.multi_map_key_to_values.get(&old_key) {
                let values: Vec<V> = values.iter().filter(|v| **v != value).cloned().collect();
                self.multi_map_key_to_values.set(old_key, Some(values));
            }
        }
        // Add new value
        let mut values = self.multi_map_key_to_values.get(&key).cloned().unwrap_or_default();
        values.push(value.clone());
        self.multi_map_key_to_values.set(key.clone(), Some(values));
        self.hash_map_values_to_key.insert(value, key);
    }

    /// `getKey(value:)`.
    pub fn get_key(&self, value: &V) -> Option<K> {
        self.hash_map_values_to_key.get(value).cloned()
    }

    /// `getValues(key:)`.
    pub fn get_values(&self, key: &K) -> Vec<V> {
        self.multi_map_key_to_values.get(key).cloned().unwrap_or_default()
    }

    /// `keySet`, in insertion order.
    pub fn key_set(&self) -> Vec<K> {
        self.multi_map_key_to_values.keys.clone()
    }

    pub fn is_maximal_key(&self, key: &K) -> bool {
        !self.multi_map_key_to_values.keys.iter().any(|other| key < other)
    }

    pub fn is_minimal_key(&self, key: &K) -> bool {
        !self.multi_map_key_to_values.keys.iter().any(|other| key > other)
    }
}
