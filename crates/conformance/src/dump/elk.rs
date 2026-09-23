//! Mirrors `oracle/Sources/downright-oracle/ElkDump.swift`: lays out an ELK
//! JSON graph with `upleft-elk` through the entry point beautiful-mermaid
//! uses and dumps the exported graph.

use std::path::Path;

use serde_json::{Map, Value};

use super::json::{self, Object};
use super::Failure;

pub fn run(input: &Path, output: &Path) -> Result<(), Failure> {
    let text = std::fs::read_to_string(input)?;
    let graph: Value = serde_json::from_str(&text).map_err(|e| Failure::Error(format!("{}: {e}", input.display())))?;
    if !graph.is_object() {
        return Err(Failure::Error("ELK graph must be a JSON object".into()));
    }
    let result = upleft_elk::bridge::elk::Elk::new().layout(&graph).map_err(|e| Failure::Error(e.to_string()))?;
    Ok(json::write(&node(result.as_object().unwrap()), output)?)
}

fn objects(value: Option<&Value>) -> Vec<&Map<String, Value>> {
    match value.and_then(Value::as_array) {
        Some(array) => array.iter().filter_map(Value::as_object).collect(),
        None => Vec::new(),
    }
}

fn string(value: Option<&Value>) -> Value {
    value.and_then(Value::as_str).map_or(Value::Null, |s| Value::String(s.to_string()))
}

/// `value as? Double`: exported numbers are always doubles.
fn number(value: Option<&Value>) -> Value {
    match value {
        Some(Value::Number(n)) if n.is_f64() => json::double(n.as_f64().unwrap()),
        _ => Value::Null,
    }
}

fn point(value: Option<&Value>) -> Value {
    match value.and_then(Value::as_object) {
        Some(p) => Value::Array(vec![number(p.get("x")), number(p.get("y"))]),
        None => Value::Null,
    }
}

fn node(n: &Map<String, Value>) -> Value {
    Object::new()
        .with("id", string(n.get("id")))
        .with("x", number(n.get("x")))
        .with("y", number(n.get("y")))
        .with("width", number(n.get("width")))
        .with("height", number(n.get("height")))
        .with("labels", Value::Array(objects(n.get("labels")).into_iter().map(label).collect()))
        .with("ports", Value::Array(objects(n.get("ports")).into_iter().map(port).collect()))
        .with("children", Value::Array(objects(n.get("children")).into_iter().map(node).collect()))
        .with("edges", Value::Array(objects(n.get("edges")).into_iter().map(edge).collect()))
        .with("layoutOptions", options(n.get("layoutOptions")))
        .build()
}

fn port(p: &Map<String, Value>) -> Value {
    Object::new()
        .with("id", string(p.get("id")))
        .with("x", number(p.get("x")))
        .with("y", number(p.get("y")))
        .with("width", number(p.get("width")))
        .with("height", number(p.get("height")))
        .with("labels", Value::Array(objects(p.get("labels")).into_iter().map(label).collect()))
        .with("layoutOptions", options(p.get("layoutOptions")))
        .build()
}

fn label(l: &Map<String, Value>) -> Value {
    Object::new()
        .with("id", string(l.get("id")))
        .with("text", string(l.get("text")))
        .with("x", number(l.get("x")))
        .with("y", number(l.get("y")))
        .with("width", number(l.get("width")))
        .with("height", number(l.get("height")))
        .build()
}

fn edge(e: &Map<String, Value>) -> Value {
    let strings = |v: Option<&Value>| -> Value {
        Value::Array(v.and_then(Value::as_array).map_or_else(Vec::new, |a| a.iter().filter_map(Value::as_str).map(|s| Value::String(s.into())).collect()))
    };
    Object::new()
        .with("id", string(e.get("id")))
        .with("sources", strings(e.get("sources")))
        .with("targets", strings(e.get("targets")))
        .with("sections", Value::Array(objects(e.get("sections")).into_iter().map(section).collect()))
        .with("labels", Value::Array(objects(e.get("labels")).into_iter().map(label).collect()))
        .with("layoutOptions", options(e.get("layoutOptions")))
        .build()
}

fn section(s: &Map<String, Value>) -> Value {
    Object::new()
        .with("id", string(s.get("id")))
        .with("startPoint", point(s.get("startPoint")))
        .with("endPoint", point(s.get("endPoint")))
        .with("bendPoints", Value::Array(objects(s.get("bendPoints")).into_iter().map(|p| point(Some(&Value::Object(p.clone())))).collect()))
        .build()
}

/// Exported options sorted by key, each with its dynamic type.
fn options(value: Option<&Value>) -> Value {
    let Some(options) = value.and_then(Value::as_object) else { return Value::Array(Vec::new()) };
    let mut keys: Vec<&String> = options.keys().collect();
    keys.sort();
    Value::Array(
        keys.into_iter()
            .map(|key| {
                let typed = match &options[key] {
                    Value::String(s) => Value::Array(vec!["string".into(), Value::String(s.clone())]),
                    Value::Number(n) if n.is_f64() => Value::Array(vec!["double".into(), json::double(n.as_f64().unwrap())]),
                    Value::Number(n) => Value::Array(vec!["int".into(), Value::from(n.as_i64().unwrap_or(0))]),
                    Value::Bool(b) => Value::Array(vec!["bool".into(), Value::Bool(*b)]),
                    _ => Value::String("?".into()),
                };
                Value::Array(vec![Value::String(key.clone()), typed])
            })
            .collect(),
    )
}
