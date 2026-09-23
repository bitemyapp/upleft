//! Port of `Bridge/ELK.swift`: the public entry point.

use std::fmt;
use std::time::Duration;

use serde_json::Value;

use super::elk_graph_impl::ElkGraph;
use super::json_exporter::JsonExporter;
use super::json_importer::{parse_option_value, JsonImporter};
use crate::org::eclipse::elk::core::recursive_graph_layout_engine::RecursiveGraphLayoutEngine;
use crate::org::eclipse::elk::core::util::basic_progress_monitor::BasicProgressMonitor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Debug, Clone)]
pub enum ElkError {
    Runtime(String),
    InvalidResult,
    TimedOut(f64),
}

impl fmt::Display for ElkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElkError::Runtime(message) => write!(f, "ELK layout error: {message}"),
            ElkError::InvalidResult => write!(f, "ELK returned an invalid graph"),
            ElkError::TimedOut(t) => write!(f, "ELK layout timed out after {t}s"),
        }
    }
}

impl std::error::Error for ElkError {}

/// `ELK`: lays out JSON graphs with ELK Layered.
///
/// Layout providers are pooled per thread, as elk-swift pools them per process
/// (see [`crate::org::eclipse::elk::alg::layered::layered_layout_provider`]).
#[derive(Default)]
pub struct Elk {
    engine: RecursiveGraphLayoutEngine,
}

impl Elk {
    pub fn new() -> Elk {
        Elk { engine: RecursiveGraphLayoutEngine::new() }
    }

    /// `layout(graph:options:timeout:)` with the defaults beautiful-mermaid
    /// uses (no extra options, 30 s timeout).
    pub fn layout(&mut self, graph: &Value) -> Result<Value, ElkError> {
        self.layout_with(graph, None, 30.0)
    }

    /// `layout(graph:options:timeout:)`.
    pub fn layout_with(&mut self, graph: &Value, options: Option<&serde_json::Map<String, Value>>, timeout: f64) -> Result<Value, ElkError> {
        let empty = serde_json::Map::new();
        let json = graph.as_object().unwrap_or(&empty);
        let mut elk_graph = ElkGraph::new();
        let root = JsonImporter::new().transform(&mut elk_graph, json);

        if let Some(options) = options {
            // `elkGraph.setProperty(key, value)`: raw key, raw value.
            for (key, value) in options {
                elk_graph[root].props.set_by_id(key, Some(raw_value(value)));
            }
        }

        let mut monitor = BasicProgressMonitor::with_timeout(Duration::from_secs_f64(timeout));
        self.engine.layout(&mut elk_graph, root, &mut monitor)?;
        if monitor.is_canceled() {
            return Err(ElkError::TimedOut(timeout));
        }
        Ok(JsonExporter::export(&elk_graph, root))
    }
}

/// A JSON value as a Swift `Any` (numbers are `Double`).
fn raw_value(value: &Value) -> crate::org::eclipse::elk::graph::properties::property::PropValue {
    use crate::org::eclipse::elk::graph::properties::property::PropValue;
    match value {
        Value::String(s) => PropValue::from(s.as_str()),
        Value::Number(_) | Value::Bool(_) => parse_option_value(value),
        other => PropValue::object(std::rc::Rc::new(other.clone())),
    }
}
