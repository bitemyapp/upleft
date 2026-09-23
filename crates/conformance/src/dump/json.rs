//! Helpers matching `oracle/Sources/downright-oracle/JSON.swift`: ranges are
//! `[location, length]`, optionals are `null`, 64-bit hashes are 16-digit
//! lower-case hex strings, objects keep insertion order.

use serde_json::{Map, Value};

/// An ordered JSON object under construction.
#[derive(Default)]
pub struct Object(Map<String, Value>);

impl Object {
    pub fn new() -> Self {
        Object(Map::new())
    }

    pub fn with(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.0.insert(key.to_owned(), value.into());
        self
    }

    pub fn build(self) -> Value {
        Value::Object(self.0)
    }
}

impl From<Object> for Value {
    fn from(object: Object) -> Value {
        object.build()
    }
}

/// `[location, length]`.
pub fn range(location: usize, length: usize) -> Value {
    Value::Array(vec![location.into(), length.into()])
}

pub fn optional<T>(value: Option<T>, map: impl FnOnce(T) -> Value) -> Value {
    value.map_or(Value::Null, map)
}

pub fn hex(value: u64) -> Value {
    Value::String(format!("{value:016x}"))
}

/// A double, with non-finite values spelled as the Swift oracle spells them.
pub fn double(value: f64) -> Value {
    if value.is_nan() {
        Value::String("nan".into())
    } else if value.is_infinite() {
        Value::String(if value < 0.0 { "-inf" } else { "inf" }.into())
    } else {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

pub fn write(value: &Value, path: &std::path::Path) -> std::io::Result<()> {
    std::fs::write(path, serde_json::to_string(value).expect("serialisable"))
}
