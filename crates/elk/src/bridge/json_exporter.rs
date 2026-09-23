//! Port of `Bridge/JsonExporter.swift`: [`ElkGraph`] → JSON.

use serde_json::{Map, Value};

use super::elk_graph_impl::{ElkEdgeId, ElkEdgeSectionId, ElkGraph, ElkLabelId, ElkNodeId, ElkPortId, ElkShape};
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;

fn number(v: f64) -> Value {
    serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
}

fn point(x: f64, y: f64) -> Value {
    let mut m = Map::new();
    m.insert("x".into(), number(x));
    m.insert("y".into(), number(y));
    Value::Object(m)
}

pub struct JsonExporter;

impl JsonExporter {
    pub fn export(graph: &ElkGraph, root: ElkNodeId) -> Value {
        Self::export_node(graph, root)
    }

    fn export_node(graph: &ElkGraph, node: ElkNodeId) -> Value {
        let n = &graph[node];
        let mut dict = Map::new();
        if let Some(id) = &n.identifier {
            dict.insert("id".into(), Value::String(id.clone()));
        }
        dict.insert("x".into(), number(n.x));
        dict.insert("y".into(), number(n.y));
        dict.insert("width".into(), number(n.width));
        dict.insert("height".into(), number(n.height));
        if !n.labels.is_empty() {
            dict.insert("labels".into(), Value::Array(n.labels.iter().map(|&l| Self::export_label(graph, l)).collect()));
        }
        if !n.ports.is_empty() {
            dict.insert("ports".into(), Value::Array(n.ports.iter().map(|&p| Self::export_port(graph, p)).collect()));
        }
        if !n.children.is_empty() {
            dict.insert("children".into(), Value::Array(n.children.iter().map(|&c| Self::export_node(graph, c)).collect()));
        }
        if !n.contained_edges.is_empty() {
            dict.insert("edges".into(), Value::Array(n.contained_edges.iter().map(|&e| Self::export_edge(graph, e)).collect()));
        }
        let props = Self::export_layout_options(&n.props);
        if !props.is_empty() {
            dict.insert("layoutOptions".into(), Value::Object(props));
        }
        Value::Object(dict)
    }

    fn export_port(graph: &ElkGraph, port: ElkPortId) -> Value {
        let p = &graph[port];
        let mut dict = Map::new();
        if let Some(id) = &p.identifier {
            dict.insert("id".into(), Value::String(id.clone()));
        }
        dict.insert("x".into(), number(p.x));
        dict.insert("y".into(), number(p.y));
        dict.insert("width".into(), number(p.width));
        dict.insert("height".into(), number(p.height));
        if !p.labels.is_empty() {
            dict.insert("labels".into(), Value::Array(p.labels.iter().map(|&l| Self::export_label(graph, l)).collect()));
        }
        let props = Self::export_layout_options(&p.props);
        if !props.is_empty() {
            dict.insert("layoutOptions".into(), Value::Object(props));
        }
        Value::Object(dict)
    }

    fn export_label(graph: &ElkGraph, label: ElkLabelId) -> Value {
        let l = &graph[label];
        let mut dict = Map::new();
        if let Some(id) = &l.identifier {
            dict.insert("id".into(), Value::String(id.clone()));
        }
        dict.insert("text".into(), Value::String(l.text.clone()));
        dict.insert("x".into(), number(l.x));
        dict.insert("y".into(), number(l.y));
        dict.insert("width".into(), number(l.width));
        dict.insert("height".into(), number(l.height));
        Value::Object(dict)
    }

    fn shape_id(graph: &ElkGraph, shape: ElkShape) -> Option<String> {
        match shape {
            ElkShape::Node(n) => graph[n].identifier.clone(),
            ElkShape::Port(p) => graph[p].identifier.clone(),
        }
    }

    fn export_edge(graph: &ElkGraph, edge: ElkEdgeId) -> Value {
        let e = &graph[edge];
        let mut dict = Map::new();
        if let Some(id) = &e.identifier {
            dict.insert("id".into(), Value::String(id.clone()));
        }
        dict.insert("sources".into(), Value::Array(e.sources.iter().filter_map(|&s| Self::shape_id(graph, s)).map(Value::String).collect()));
        dict.insert("targets".into(), Value::Array(e.targets.iter().filter_map(|&s| Self::shape_id(graph, s)).map(Value::String).collect()));
        if !e.sections.is_empty() {
            dict.insert("sections".into(), Value::Array(e.sections.iter().map(|&s| Self::export_section(graph, s)).collect()));
        }
        if !e.labels.is_empty() {
            dict.insert("labels".into(), Value::Array(e.labels.iter().map(|&l| Self::export_label(graph, l)).collect()));
        }
        let props = Self::export_layout_options(&e.props);
        if !props.is_empty() {
            dict.insert("layoutOptions".into(), Value::Object(props));
        }
        Value::Object(dict)
    }

    fn export_section(graph: &ElkGraph, section: ElkEdgeSectionId) -> Value {
        let s = &graph[section];
        let mut dict = Map::new();
        if let Some(id) = &s.identifier {
            dict.insert("id".into(), Value::String(id.clone()));
        }
        dict.insert("startPoint".into(), point(s.start_x, s.start_y));
        dict.insert("endPoint".into(), point(s.end_x, s.end_y));
        if !s.bend_points.is_empty() {
            dict.insert("bendPoints".into(), Value::Array(s.bend_points.iter().map(|bp| point(bp.x, bp.y)).collect()));
        }
        Value::Object(dict)
    }

    /// Only `String`, `Double`, `Int` and `Bool` values are exported. Swift's
    /// dictionary has no order; here keys are sorted.
    fn export_layout_options(props: &PropertyMap) -> Map<String, Value> {
        let mut entries: Vec<(String, Value)> = props.all().filter_map(|(k, v)| v.exportable().map(|v| (k.to_string(), v))).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries.into_iter().collect()
    }
}
