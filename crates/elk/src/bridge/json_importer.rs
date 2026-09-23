//! Port of `Bridge/JsonImporter.swift`: JSON graph → [`ElkGraph`].
//!
//! The input is the value beautiful-mermaid builds (`[String: Any]` in Swift).
//! JSON numbers are Swift `Double`s, as beautiful-mermaid produces them.

use std::collections::HashMap;
use std::rc::Rc;

use serde_json::{Map, Value};

use super::elk_graph_impl::{ElkEdgeId, ElkElement, ElkGraph, ElkLabelId, ElkNodeId, ElkPortId, ElkShape};
use crate::org::eclipse::elk::alg::layered::options::{
    edge_label_side_selection::EdgeLabelSideSelection, fixed_alignment::FixedAlignment,
    graph_compaction_strategy::GraphCompactionStrategy, ordering_strategy::OrderingStrategy,
    wrapping_strategy::WrappingStrategy,
};
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::org::eclipse::elk::core::options::{
    content_alignment::ContentAlignment, direction::Direction, edge_label_placement::EdgeLabelPlacement,
    edge_routing::EdgeRouting, hierarchy_handling::HierarchyHandling,
};
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;
use crate::org::eclipse::elk::graph::properties::property::PropValue;
use crate::swift;

#[derive(Default)]
pub struct JsonImporter {
    shape_index: HashMap<String, ElkShape>,
    deferred_edges: Vec<(Map<String, Value>, ElkNodeId)>,
}

/// `value as? [[String: Any]]`: every element must be an object.
fn dict_array(value: Option<&Value>) -> Option<Vec<&Map<String, Value>>> {
    let array = value?.as_array()?;
    array.iter().map(|v| v.as_object()).collect()
}

/// `value as? [String]`.
fn string_array(value: Option<&Value>) -> Option<Vec<&str>> {
    let array = value?.as_array()?;
    array.iter().map(|v| v.as_str()).collect()
}

/// `asDouble(_:)`.
fn as_double(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => swift::parse_double(s).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Swift's `.whitespaces` character set, for `trimmingCharacters(in:)`.
fn is_swift_whitespace(c: char) -> bool {
    c == '\t' || (c.is_whitespace() && !matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'))
}

impl JsonImporter {
    pub fn new() -> JsonImporter {
        JsonImporter::default()
    }

    /// `transform(_:)`: builds the graph; returns the root node.
    pub fn transform(&mut self, graph: &mut ElkGraph, json: &Map<String, Value>) -> ElkNodeId {
        self.shape_index.clear();
        self.deferred_edges.clear();
        let root = self.import_node(graph, json, None);
        let deferred = std::mem::take(&mut self.deferred_edges);
        for (edge, parent) in &deferred {
            self.import_edge(graph, edge, *parent);
        }
        root
    }

    fn import_node(&mut self, graph: &mut ElkGraph, dict: &Map<String, Value>, parent: Option<ElkNodeId>) -> ElkNodeId {
        let node = graph.new_node();
        graph[node].parent = parent;
        if let Some(id) = dict.get("id").and_then(Value::as_str) {
            graph[node].identifier = Some(id.to_string());
            self.shape_index.insert(id.to_string(), ElkShape::Node(node));
        }
        graph[node].x = as_double(dict.get("x"));
        graph[node].y = as_double(dict.get("y"));
        graph[node].width = as_double(dict.get("width"));
        graph[node].height = as_double(dict.get("height"));

        if let Some(options) = dict.get("layoutOptions").and_then(Value::as_object) {
            apply_layout_options(options, &mut graph[node].props);
        }
        if let Some(options) = dict.get("properties").and_then(Value::as_object) {
            apply_layout_options(options, &mut graph[node].props);
        }
        if let Some(labels) = dict_array(dict.get("labels")) {
            for label_dict in labels {
                let label = import_label(graph, label_dict);
                graph[label].parent = Some(ElkElement::Node(node));
                graph[node].labels.push(label);
            }
        }
        if let Some(ports) = dict_array(dict.get("ports")) {
            for port_dict in ports {
                let port = self.import_port(graph, port_dict, node);
                graph[node].ports.push(port);
            }
        }
        if let Some(children) = dict_array(dict.get("children")) {
            for child_dict in children {
                let child = self.import_node(graph, child_dict, Some(node));
                graph[node].children.push(child);
            }
        }
        if let Some(edges) = dict_array(dict.get("edges")) {
            for edge_dict in edges {
                self.deferred_edges.push((edge_dict.clone(), node));
            }
        }
        node
    }

    fn import_port(&mut self, graph: &mut ElkGraph, dict: &Map<String, Value>, parent: ElkNodeId) -> ElkPortId {
        let port = graph.new_port();
        graph[port].parent = Some(parent);
        if let Some(id) = dict.get("id").and_then(Value::as_str) {
            graph[port].identifier = Some(id.to_string());
            self.shape_index.insert(id.to_string(), ElkShape::Port(port));
        }
        graph[port].x = as_double(dict.get("x"));
        graph[port].y = as_double(dict.get("y"));
        graph[port].width = as_double(dict.get("width"));
        graph[port].height = as_double(dict.get("height"));
        if let Some(options) = dict.get("layoutOptions").and_then(Value::as_object) {
            apply_layout_options(options, &mut graph[port].props);
        }
        if let Some(options) = dict.get("properties").and_then(Value::as_object) {
            apply_layout_options(options, &mut graph[port].props);
        }
        if let Some(labels) = dict_array(dict.get("labels")) {
            for label_dict in labels {
                let label = import_label(graph, label_dict);
                graph[label].parent = Some(ElkElement::Port(port));
                graph[port].labels.push(label);
            }
        }
        port
    }

    fn import_edge(&mut self, graph: &mut ElkGraph, dict: &Map<String, Value>, containing_node: ElkNodeId) -> ElkEdgeId {
        let edge = graph.new_edge();
        graph[edge].containing_node = Some(containing_node);
        if let Some(id) = dict.get("id").and_then(Value::as_str) {
            graph[edge].identifier = Some(id.to_string());
        }
        if let Some(sources) = string_array(dict.get("sources")) {
            for source_id in sources {
                if let Some(&shape) = self.shape_index.get(source_id) {
                    graph[edge].sources.push(shape);
                    match shape {
                        ElkShape::Node(n) => graph[n].outgoing_edges.push(edge),
                        ElkShape::Port(p) => graph[p].outgoing_edges.push(edge),
                    }
                }
            }
        }
        if let Some(targets) = string_array(dict.get("targets")) {
            for target_id in targets {
                if let Some(&shape) = self.shape_index.get(target_id) {
                    graph[edge].targets.push(shape);
                    match shape {
                        ElkShape::Node(n) => graph[n].incoming_edges.push(edge),
                        ElkShape::Port(p) => graph[p].incoming_edges.push(edge),
                    }
                }
            }
        }
        if let Some(labels) = dict_array(dict.get("labels")) {
            for label_dict in labels {
                let label = import_label(graph, label_dict);
                graph[label].parent = Some(ElkElement::Edge(edge));
                graph[edge].labels.push(label);
            }
        }
        if let Some(options) = dict.get("layoutOptions").and_then(Value::as_object) {
            apply_layout_options(options, &mut graph[edge].props);
        }
        if let Some(options) = dict.get("properties").and_then(Value::as_object) {
            apply_layout_options(options, &mut graph[edge].props);
        }
        graph[containing_node].contained_edges.push(edge);
        edge
    }
}

fn import_label(graph: &mut ElkGraph, dict: &Map<String, Value>) -> ElkLabelId {
    let label = graph.new_label();
    if let Some(id) = dict.get("id").and_then(Value::as_str) {
        graph[label].identifier = Some(id.to_string());
    }
    if let Some(text) = dict.get("text").and_then(Value::as_str) {
        graph[label].text = text.to_string();
    }
    graph[label].x = as_double(dict.get("x"));
    graph[label].y = as_double(dict.get("y"));
    graph[label].width = as_double(dict.get("width"));
    graph[label].height = as_double(dict.get("height"));
    if let Some(options) = dict.get("layoutOptions").and_then(Value::as_object) {
        apply_layout_options(options, &mut graph[label].props);
    }
    label
}

/// `canonicalKey(_:)`: `elk.x` → `org.eclipse.elk.x`.
fn canonical_key(key: &str) -> String {
    if key.starts_with("elk.") { format!("org.eclipse.{key}") } else { key.to_string() }
}

/// `applyLayoutOptions(_:to:)`. No option metadata is ever registered in
/// elk-swift, so every value goes through `parseOptionValue` and is stored
/// under its canonical key.
///
/// Swift iterates the options dictionary in hash order; that only matters for
/// two keys with the same canonical form, which beautiful-mermaid never emits.
/// Here the JSON order is used.
pub fn apply_layout_options(options: &Map<String, Value>, holder: &mut PropertyMap) {
    for (key, value) in options {
        let full_key = canonical_key(key);
        let parsed = parse_option_value(value);
        holder.set_by_id(&full_key, Some(parsed));
    }
}

/// `parseOptionValue(_:_:)`: best-effort typing of an option value.
pub fn parse_option_value(value: &Value) -> PropValue {
    let s = match value {
        Value::Number(n) => return PropValue::Double(n.as_f64().unwrap_or(0.0)),
        Value::Bool(b) => return PropValue::Bool(*b),
        Value::String(s) => s.as_str(),
        other => return PropValue::object(Rc::new(other.clone())),
    };

    if s == "true" || s == "TRUE" {
        return PropValue::Bool(true);
    }
    if s == "false" || s == "FALSE" {
        return PropValue::Bool(false);
    }
    if let Some(d) = swift::parse_double(s) {
        return PropValue::Double(d);
    }
    if let Some(dir) = Direction::from_raw(s) {
        return PropValue::Direction(dir);
    }
    if let Some(er) = EdgeRouting::from_raw(s) {
        return PropValue::EdgeRouting(er);
    }
    match s {
        "INHERIT" => return PropValue::HierarchyHandling(HierarchyHandling::INHERIT),
        "INCLUDE_CHILDREN" => return PropValue::HierarchyHandling(HierarchyHandling::INCLUDE_CHILDREN),
        "SEPARATE_CHILDREN" => return PropValue::HierarchyHandling(HierarchyHandling::SEPARATE_CHILDREN),
        _ => {}
    }
    if let Some(os) = OrderingStrategy::from_raw(s) {
        return PropValue::OrderingStrategy(os);
    }
    if let Some(fa) = FixedAlignment::from_raw(s) {
        return PropValue::FixedAlignment(fa);
    }
    let upper = s.to_uppercase();
    match upper.as_str() {
        "CENTER" => return PropValue::EdgeLabelPlacement(EdgeLabelPlacement::CENTER),
        "HEAD" => return PropValue::EdgeLabelPlacement(EdgeLabelPlacement::HEAD),
        "TAIL" => return PropValue::EdgeLabelPlacement(EdgeLabelPlacement::TAIL),
        _ => {}
    }
    match upper.as_str() {
        "ALWAYS_UP" => return PropValue::EdgeLabelSideSelection(EdgeLabelSideSelection::ALWAYS_UP),
        "ALWAYS_DOWN" => return PropValue::EdgeLabelSideSelection(EdgeLabelSideSelection::ALWAYS_DOWN),
        "DIRECTION_UP" => return PropValue::EdgeLabelSideSelection(EdgeLabelSideSelection::DIRECTION_UP),
        "DIRECTION_DOWN" => return PropValue::EdgeLabelSideSelection(EdgeLabelSideSelection::DIRECTION_DOWN),
        "SMART_UP" => return PropValue::EdgeLabelSideSelection(EdgeLabelSideSelection::SMART_UP),
        "SMART_DOWN" => return PropValue::EdgeLabelSideSelection(EdgeLabelSideSelection::SMART_DOWN),
        _ => {}
    }
    if s.contains("H_") || s.contains("V_") {
        let mut alignment = ContentAlignment::empty();
        for part in s.split(' ').filter(|p| !p.is_empty()) {
            match part {
                "H_LEFT" => alignment.insert(ContentAlignment::H_LEFT),
                "H_CENTER" => alignment.insert(ContentAlignment::H_CENTER),
                "H_RIGHT" => alignment.insert(ContentAlignment::H_RIGHT),
                "V_TOP" => alignment.insert(ContentAlignment::V_TOP),
                "V_CENTER" => alignment.insert(ContentAlignment::V_CENTER),
                "V_BOTTOM" => alignment.insert(ContentAlignment::V_BOTTOM),
                _ => {}
            }
        }
        if !alignment.is_empty() {
            return PropValue::ContentAlignment(alignment);
        }
    }
    match s {
        "NONE" => return PropValue::GraphCompactionStrategy(GraphCompactionStrategy::NONE),
        "LEFT" => return PropValue::GraphCompactionStrategy(GraphCompactionStrategy::LEFT),
        "RIGHT" => return PropValue::GraphCompactionStrategy(GraphCompactionStrategy::RIGHT),
        "LEFT_RIGHT_CONSTRAINT_LOCKING" => return PropValue::GraphCompactionStrategy(GraphCompactionStrategy::LEFT_RIGHT_CONSTRAINT_LOCKING),
        "LEFT_RIGHT_CONNECTION_LOCKING" => return PropValue::GraphCompactionStrategy(GraphCompactionStrategy::LEFT_RIGHT_CONNECTION_LOCKING),
        "EDGE_LENGTH" => return PropValue::GraphCompactionStrategy(GraphCompactionStrategy::EDGE_LENGTH),
        _ => {}
    }
    match s {
        "OFF" => return PropValue::WrappingStrategy(WrappingStrategy::OFF),
        "SINGLE_EDGE" => return PropValue::WrappingStrategy(WrappingStrategy::SINGLE_EDGE),
        "MULTI_EDGE" => return PropValue::WrappingStrategy(WrappingStrategy::MULTI_EDGE),
        _ => {}
    }
    if s.starts_with('[') && s.contains("top=") {
        let (mut top, mut right, mut bottom, mut left) = (0.0, 0.0, 0.0, 0.0);
        let trimmed = s.trim_matches(|c| c == '[' || c == ']' || c == ' ');
        for part in trimmed.split(',').filter(|p| !p.is_empty()) {
            let kv: Vec<&str> = match part.split_once('=') {
                Some((k, v)) => [k, v].into_iter().filter(|x| !x.is_empty()).collect(),
                None => vec![part],
            };
            if kv.len() != 2 {
                continue;
            }
            let Some(val) = swift::parse_double(kv[1].trim_matches(is_swift_whitespace)) else { continue };
            match kv[0].trim_matches(is_swift_whitespace).to_lowercase().as_str() {
                "top" => top = val,
                "right" => right = val,
                "bottom" => bottom = val,
                "left" => left = val,
                _ => {}
            }
        }
        return PropValue::elk_padding(ElkPadding::new(top, right, bottom, left));
    }
    PropValue::Str(Rc::from(s))
}
