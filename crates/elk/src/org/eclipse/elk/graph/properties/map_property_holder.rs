//! Port of `graph/properties/MapPropertyHolder.swift` (and of the identical
//! `ElkPropertyHolder` in `Bridge/ElkGraphImpl.swift`): a property map keyed by
//! the property id.

use std::any::Any;
use std::rc::Rc;

use super::keys::PropKey;
use super::property::{PropCast, PropValue, Property};

/// A `[String: Any]` property map. Declared ids are interned [`PropKey`]s;
/// ids only the importer knows (unrecognised layout option names) are kept by
/// string. Iteration order is irrelevant to every consumer (the exporter's
/// output is a dictionary), so entries are simply appended.
#[derive(Clone, Default, Debug)]
pub struct PropertyMap {
    entries: Vec<(PropKey, PropValue)>,
    extra: Vec<(Rc<str>, PropValue)>,
}

impl PropertyMap {
    pub fn new() -> PropertyMap {
        PropertyMap::default()
    }

    #[inline]
    fn find(&self, key: PropKey) -> Option<&PropValue> {
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// `setProperty(p, value)` with a non-nil value.
    pub fn set(&mut self, p: &Property, value: impl Into<PropValue>) {
        self.set_key(p.key, value.into());
    }

    /// `setProperty(p, value)` with an optional value: `nil` removes.
    pub fn set_opt(&mut self, p: &Property, value: Option<PropValue>) {
        match value {
            Some(v) => self.set_key(p.key, v),
            None => self.remove(p),
        }
    }

    pub fn set_key(&mut self, key: PropKey, value: PropValue) {
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }

    pub fn remove(&mut self, p: &Property) {
        self.entries.retain(|(k, _)| *k != p.key);
    }

    /// The string-key overload `setProperty(_ key: String, _ value: Any?)`.
    pub fn set_by_id(&mut self, id: &str, value: Option<PropValue>) {
        if let Some(key) = super::keys::lookup(id) {
            match value {
                Some(v) => self.set_key(key, v),
                None => self.entries.retain(|(k, _)| *k != key),
            }
        } else {
            match value {
                Some(v) => {
                    if let Some(slot) = self.extra.iter_mut().find(|(k, _)| &**k == id) {
                        slot.1 = v;
                    } else {
                        self.extra.push((Rc::from(id), v));
                    }
                }
                None => self.extra.retain(|(k, _)| &**k != id),
            }
        }
    }

    /// The string-key overload `getProperty(_ key: String) -> Any?` (stored
    /// value only, no default).
    pub fn get_by_id(&self, id: &str) -> Option<PropValue> {
        match super::keys::lookup(id) {
            Some(key) => self.find(key).cloned(),
            None => self.extra.iter().find(|(k, _)| &**k == id).map(|(_, v)| v.clone()),
        }
    }

    /// `getProperty(p) -> Any?`: the stored value, else `p`'s default.
    pub fn get(&self, p: &Property) -> Option<PropValue> {
        match self.find(p.key) {
            Some(v) => Some(v.clone()),
            None => p.default_value(),
        }
    }

    /// The stored value only.
    pub fn get_stored(&self, p: &Property) -> Option<&PropValue> {
        self.find(p.key)
    }

    /// `getProperty(p) as? T`: the stored value (else the default) cast to `T`.
    /// A stored value of another type gives `None`, not the default.
    #[inline]
    pub fn get_as<T: PropCast>(&self, p: &Property) -> Option<T> {
        match self.find(p.key) {
            Some(v) => T::from_value(v),
            None => p.default.and_then(|d| T::from_value(&d())),
        }
    }

    /// `let v: T? = getProperty(p)` (the generic overload): the stored value
    /// cast to `T`, and if that fails `p`'s default cast to `T`.
    #[inline]
    pub fn get_typed<T: PropCast>(&self, p: &Property) -> Option<T> {
        if let Some(v) = self.find(p.key) {
            if let Some(t) = T::from_value(v) {
                return Some(t);
            }
        }
        p.default.and_then(|d| T::from_value(&d()))
    }

    /// `getProperty(p) as? SomeClass` for values kept as [`PropValue::Object`].
    pub fn get_object<T: Any>(&self, p: &Property) -> Option<Rc<T>> {
        match self.find(p.key) {
            Some(v) => v.downcast::<T>(),
            None => p.default_value().and_then(|d| d.downcast::<T>()),
        }
    }

    /// `hasProperty(p)`: whether a value is stored.
    pub fn has(&self, p: &Property) -> bool {
        self.find(p.key).is_some()
    }

    /// `copyProperties(other)`: merges, `other` winning.
    pub fn copy_properties(&mut self, other: &PropertyMap) {
        for (k, v) in &other.entries {
            self.set_key(*k, v.clone());
        }
        for (k, v) in &other.extra {
            self.set_by_id(k, Some(v.clone()));
        }
    }

    /// `getAllProperties()` as `(id, value)` pairs, in no particular order.
    pub fn all(&self) -> impl Iterator<Item = (&str, &PropValue)> {
        self.entries.iter().map(|(k, v)| (k.name(), v)).chain(self.extra.iter().map(|(k, v)| (&**k, v)))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.extra.is_empty()
    }
}
