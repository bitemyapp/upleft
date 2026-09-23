//! Port of `Mermaid/src_er_layout.swift` (from `original/src/er/layout.ts`).
//! The Swift round-trips the ELK graph through private structs
//! (`_encodeElkNode` / `_decodeElkNode`); the decode rules that can drop
//! elements are kept.

use serde_json::{json, Map, Value};

use super::src_elk_instance::elk_layout_sync;
use super::src_er_parser::*;
use super::src_styles::{estimate_mono_text_width, estimate_text_width, FONT_SIZES, FONT_WEIGHTS};
use super::src_text_metrics::measure_multiline_text;
use super::src_types::SDict;
use crate::error::MermaidError;
use crate::swift;

const PADDING: f64 = 40.0;
const BOX_PAD_X: f64 = 14.0;
const HEADER_HEIGHT: f64 = 34.0;
const ROW_HEIGHT: f64 = 22.0;
const MIN_WIDTH: f64 = 140.0;
const ATTR_FONT_SIZE: f64 = 11.0;
const NODE_SPACING: f64 = 70.0;
const LAYER_SPACING: f64 = 90.0;

/// `_anyToDouble(_:)`.
fn any_to_double(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct ElkPoint {
    x: f64,
    y: f64,
}

struct ElkSection {
    start_point: ElkPoint,
    end_point: ElkPoint,
    bend_points: Option<Vec<ElkPoint>>,
}

struct ElkEdge {
    sections: Option<Vec<ElkSection>>,
}

struct ElkNodeOut {
    id: String,
    width: Option<f64>,
    height: Option<f64>,
    x: Option<f64>,
    y: Option<f64>,
    children: Option<Vec<ElkNodeOut>>,
    edges: Option<Vec<ElkEdge>>,
}

/// `(section["bendPoints"] as? [Any])?.compactMap { _decodeElkPoint($0) }`.
fn decode_point(value: Option<&Value>) -> Option<ElkPoint> {
    let point = value?.as_object()?;
    Some(ElkPoint { x: any_to_double(point.get("x"))?, y: any_to_double(point.get("y"))? })
}

fn decode_section(value: &Value) -> Option<ElkSection> {
    let section = value.as_object()?;
    let start_point = decode_point(section.get("startPoint"))?;
    let end_point = decode_point(section.get("endPoint"))?;
    let bend_points = section
        .get("bendPoints")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|v| decode_point(Some(v))).collect());
    Some(ElkSection { start_point, end_point, bend_points })
}

/// `_decodeElkLabel` requires `text`, `width`, `height`; `_decodeElkEdge`
/// requires `id`, `sources` and `targets` as string arrays.
fn decode_edge(value: &Value) -> Option<ElkEdge> {
    let edge = value.as_object()?;
    edge.get("id")?.as_str()?;
    let strings = |v: Option<&Value>| v.and_then(Value::as_array).filter(|a| a.iter().all(Value::is_string)).is_some();
    if !strings(edge.get("sources")) || !strings(edge.get("targets")) {
        return None;
    }
    let sections = edge
        .get("sections")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(decode_section).collect());
    Some(ElkEdge { sections })
}

fn decode_node(value: &Value) -> Option<ElkNodeOut> {
    let node = value.as_object()?;
    let id = node.get("id")?.as_str()?.to_owned();
    let children = node.get("children").and_then(Value::as_array).map(|items| items.iter().filter_map(decode_node).collect());
    let edges = node.get("edges").and_then(Value::as_array).map(|items| items.iter().filter_map(decode_edge).collect());
    Some(ElkNodeOut {
        id,
        width: any_to_double(node.get("width")),
        height: any_to_double(node.get("height")),
        x: any_to_double(node.get("x")),
        y: any_to_double(node.get("y")),
        children,
        edges,
    })
}

/// `_buildErElkGraph(_:_:)` followed by `_encodeElkNode`.
fn build_er_elk_graph(diagram: &ErDiagram) -> (Value, SDict<(f64, f64)>) {
    let mut entity_sizes: SDict<(f64, f64)> = SDict::new();

    for entity in &diagram.entities {
        let header_text_width = estimate_text_width(&entity.label, FONT_SIZES.node_label, FONT_WEIGHTS.node_label);

        let mut max_attr_width = 0.0;
        for attr in &entity.attributes {
            let key_part = if attr.keys.is_empty() { String::new() } else { format!("  {}", attr.keys.join(",")) };
            let attr_text = format!("{}  {}{}", attr.r#type, attr.name, key_part);
            let width = estimate_mono_text_width(&attr_text, ATTR_FONT_SIZE);
            max_attr_width = swift::max(max_attr_width, width);
        }

        let width = swift::max(swift::max(MIN_WIDTH, header_text_width + BOX_PAD_X * 2.0), max_attr_width + BOX_PAD_X * 2.0);
        let height = HEADER_HEIGHT + swift::max(entity.attributes.len(), 1) as f64 * ROW_HEIGHT;
        entity_sizes.insert(&entity.id, (width, height));
    }

    let mut children: Vec<Value> = Vec::new();
    for entity in &diagram.entities {
        let size = entity_sizes.get(&entity.id).copied().unwrap_or((MIN_WIDTH, HEADER_HEIGHT + ROW_HEIGHT));
        let mut child = Map::new();
        child.insert("id".into(), Value::from(entity.id.as_str()));
        child.insert("width".into(), Value::from(size.0));
        child.insert("height".into(), Value::from(size.1));
        children.push(Value::Object(child));
    }

    let mut edges: Vec<Value> = Vec::new();
    for (idx, rel) in diagram.relationships.iter().enumerate() {
        let mut edge = Map::new();
        edge.insert("id".into(), Value::from(format!("e{idx}")));
        edge.insert("sources".into(), json!([rel.entity1]));
        edge.insert("targets".into(), json!([rel.entity2]));
        if !rel.label.is_empty() {
            let metrics = measure_multiline_text(&rel.label, FONT_SIZES.edge_label, FONT_WEIGHTS.edge_label);
            edge.insert(
                "labels".into(),
                json!([{ "text": rel.label, "width": metrics.width + 8.0, "height": metrics.height + 6.0 }]),
            );
        }
        edges.push(Value::Object(edge));
    }

    let padding = swift::double_description(PADDING);
    let mut options = Map::new();
    let mut set = |k: &str, v: String| {
        options.insert(k.into(), Value::from(v));
    };
    set("elk.algorithm", "layered".into());
    set("elk.direction", "RIGHT".into());
    set("elk.spacing.nodeNode", swift::double_description(NODE_SPACING));
    set("elk.layered.spacing.nodeNodeBetweenLayers", swift::double_description(LAYER_SPACING));
    set("elk.padding", format!("[top={padding},left={padding},bottom={padding},right={padding}]"));
    set("elk.edgeRouting", "ORTHOGONAL".into());
    set("elk.edgeLabels.placement", "CENTER".into());

    let mut root = Map::new();
    root.insert("id".into(), Value::from("root"));
    root.insert("layoutOptions".into(), Value::Object(options));
    root.insert("children".into(), Value::Array(children));
    root.insert("edges".into(), Value::Array(edges));

    (Value::Object(root), entity_sizes)
}

/// The ELK input graph for an ER diagram.
pub fn er_elk_input_graph(diagram: &ErDiagram) -> Value {
    build_er_elk_graph(diagram).0
}

/// `_extractErLayout(_:_:_:)`.
fn extract_er_layout(result: &ElkNodeOut, diagram: &ErDiagram, entity_sizes: &SDict<(f64, f64)>) -> PositionedErDiagram {
    let mut entity_lookup: SDict<&ErEntity> = SDict::new();
    for e in &diagram.entities {
        entity_lookup.insert(&e.id, e);
    }

    let mut positioned_entities: Vec<PositionedErEntity> = Vec::new();
    for child in result.children.as_deref().unwrap_or(&[]) {
        let Some(entity) = entity_lookup.get(&child.id) else { continue };
        let fallback = entity_sizes.get(&entity.id).copied().unwrap_or((MIN_WIDTH, HEADER_HEIGHT + ROW_HEIGHT));
        positioned_entities.push(PositionedErEntity {
            id: entity.id.clone(),
            label: entity.label.clone(),
            attributes: entity.attributes.clone(),
            x: child.x.unwrap_or(0.0),
            y: child.y.unwrap_or(0.0),
            width: child.width.unwrap_or(fallback.0),
            height: child.height.unwrap_or(fallback.1),
            header_height: HEADER_HEIGHT,
            row_height: ROW_HEIGHT,
        });
    }

    let mut relationships: Vec<PositionedErRelationship> = Vec::new();
    for (idx, elk_edge) in result.edges.as_deref().unwrap_or(&[]).iter().enumerate() {
        if idx >= diagram.relationships.len() {
            continue;
        }
        let rel = &diagram.relationships[idx];
        let mut points: Vec<ErPoint> = Vec::new();
        if let Some(section) = elk_edge.sections.as_ref().and_then(|s| s.first()) {
            points.push(ErPoint { x: section.start_point.x, y: section.start_point.y });
            for bp in section.bend_points.as_deref().unwrap_or(&[]) {
                points.push(ErPoint { x: bp.x, y: bp.y });
            }
            points.push(ErPoint { x: section.end_point.x, y: section.end_point.y });
        }
        relationships.push(PositionedErRelationship {
            entity1: rel.entity1.clone(),
            entity2: rel.entity2.clone(),
            cardinality1: rel.cardinality1.clone(),
            cardinality2: rel.cardinality2.clone(),
            label: rel.label.clone(),
            identifying: rel.identifying,
            points,
        });
    }

    PositionedErDiagram {
        width: result.width.unwrap_or(600.0),
        height: result.height.unwrap_or(400.0),
        entities: positioned_entities,
        relationships,
    }
}

/// `layoutErDiagramSync(_:options:)`.
pub fn layout_er_diagram_sync(diagram: &ErDiagram) -> Result<PositionedErDiagram, MermaidError> {
    if diagram.entities.is_empty() {
        return Ok(PositionedErDiagram { width: 0.0, height: 0.0, entities: vec![], relationships: vec![] });
    }

    let (graph, entity_sizes) = build_er_elk_graph(diagram);
    let laid_out = elk_layout_sync(&graph)?;
    // `_decodeElkNode(laidOut) ?? graph`: the input graph decodes to a node
    // without positions.
    let result = decode_node(&laid_out).or_else(|| decode_node(&graph)).expect("the input graph has an id");
    Ok(extract_er_layout(&result, diagram, &entity_sizes))
}
