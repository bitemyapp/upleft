//! Port of `Mermaid/src_class_layout.swift` (from `original/src/class/layout.ts`).

use serde_json::{json, Map, Value};

use super::src_class_parser::*;
use super::src_elk_instance::elk_layout_sync;
use super::src_styles::{estimate_mono_text_width, estimate_text_width, FONT_SIZES, FONT_WEIGHTS};
use super::src_text_metrics::measure_multiline_text;
use super::src_types::SDict;
use crate::error::MermaidError;
use crate::swift;

/// `CLS`.
pub mod cls {
    pub const PADDING: f64 = 40.0;
    pub const BOX_PAD_X: f64 = 8.0;
    pub const HEADER_BASE_HEIGHT: f64 = 32.0;
    pub const ANNOTATION_HEIGHT: f64 = 16.0;
    pub const MEMBER_ROW_HEIGHT: f64 = 20.0;
    pub const SECTION_PAD_Y: f64 = 8.0;
    pub const EMPTY_SECTION_HEIGHT: f64 = 8.0;
    pub const MIN_WIDTH: f64 = 120.0;
    pub const MEMBER_FONT_SIZE: f64 = 11.0;
    pub const MEMBER_FONT_WEIGHT: f64 = 400.0;
    pub const NODE_SPACING: f64 = 40.0;
    pub const LAYER_SPACING: f64 = 60.0;
}

#[derive(Debug, Clone, Copy)]
struct ClassSize {
    width: f64,
    height: f64,
    header_height: f64,
    attr_height: f64,
    method_height: f64,
}

fn as_double(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn as_dict_array(value: Option<&Value>) -> Vec<&Map<String, Value>> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_object).collect(),
        _ => Vec::new(),
    }
}

/// `buildClassElkGraph(_:_:)`.
fn build_class_elk_graph(diagram: &ClassDiagram) -> (Value, SDict<ClassSize>) {
    let mut class_sizes: SDict<ClassSize> = SDict::new();

    for c in &diagram.classes {
        let header_height =
            if c.annotation.is_some() { cls::HEADER_BASE_HEIGHT + cls::ANNOTATION_HEIGHT } else { cls::HEADER_BASE_HEIGHT };

        let attr_height = if !c.attributes.is_empty() {
            c.attributes.len() as f64 * cls::MEMBER_ROW_HEIGHT + cls::SECTION_PAD_Y
        } else {
            cls::EMPTY_SECTION_HEIGHT
        };

        let method_height = if !c.methods.is_empty() {
            c.methods.len() as f64 * cls::MEMBER_ROW_HEIGHT + cls::SECTION_PAD_Y
        } else {
            cls::EMPTY_SECTION_HEIGHT
        };

        let header_text_w = estimate_text_width(&c.label, FONT_SIZES.node_label, FONT_WEIGHTS.node_label);
        let max_attr_w = max_member_width(&c.attributes);
        let max_method_w = max_member_width(&c.methods);

        let width = swift::max_n(&[
            cls::MIN_WIDTH,
            header_text_w + cls::BOX_PAD_X * 2.0,
            max_attr_w + cls::BOX_PAD_X * 2.0,
            max_method_w + cls::BOX_PAD_X * 2.0,
        ]);
        let height = header_height + attr_height + method_height;

        class_sizes.insert(&c.id, ClassSize { width, height, header_height, attr_height, method_height });
    }

    let mut children: Vec<Value> = Vec::new();
    for c in &diagram.classes {
        let Some(size) = class_sizes.get(&c.id) else { continue };
        let mut child = Map::new();
        child.insert("id".into(), Value::from(c.id.as_str()));
        child.insert("width".into(), Value::from(size.width));
        child.insert("height".into(), Value::from(size.height));
        children.push(Value::Object(child));
    }

    let mut edges: Vec<Value> = Vec::new();
    for (i, rel) in diagram.relationships.iter().enumerate() {
        let mut edge = Map::new();
        edge.insert("id".into(), Value::from(format!("e{i}")));
        edge.insert("sources".into(), json!([rel.from]));
        edge.insert("targets".into(), json!([rel.to]));

        if let Some(label) = &rel.label {
            if !label.is_empty() {
                let metrics = measure_multiline_text(label, FONT_SIZES.edge_label, FONT_WEIGHTS.edge_label);
                edge.insert(
                    "labels".into(),
                    json!([{ "text": label, "width": metrics.width + 8.0, "height": metrics.height + 6.0 }]),
                );
            }
        }

        edges.push(Value::Object(edge));
    }

    let padding = swift::double_description(cls::PADDING);
    let mut options = Map::new();
    let mut set = |k: &str, v: String| {
        options.insert(k.into(), Value::from(v));
    };
    set("elk.algorithm", "layered".into());
    set("elk.direction", "DOWN".into());
    set("elk.spacing.nodeNode", swift::double_description(cls::NODE_SPACING));
    set("elk.layered.spacing.nodeNodeBetweenLayers", swift::double_description(cls::LAYER_SPACING));
    set("elk.padding", format!("[top={padding},left={padding},bottom={padding},right={padding}]"));
    set("elk.edgeRouting", "ORTHOGONAL".into());
    set("elk.edgeLabels.placement", "CENTER".into());
    set("elk.layered.edgeLabels.sideSelection", "ALWAYS_DOWN".into());

    let mut root = Map::new();
    root.insert("id".into(), Value::from("root"));
    root.insert("layoutOptions".into(), Value::Object(options));
    root.insert("children".into(), Value::Array(children));
    root.insert("edges".into(), Value::Array(edges));

    (Value::Object(root), class_sizes)
}

/// `extractClassLayout(_:_:_:)`.
fn extract_class_layout(result: &Value, diagram: &ClassDiagram, class_sizes: &SDict<ClassSize>) -> PositionedClassDiagram {
    let mut class_lookup: SDict<&ClassNode> = SDict::new();
    for c in &diagram.classes {
        class_lookup.insert(&c.id, c);
    }

    let mut positioned_classes: Vec<PositionedClassNode> = Vec::new();
    for child in as_dict_array(result.get("children")) {
        let Some(id) = child.get("id").and_then(Value::as_str) else { continue };
        let (Some(c), Some(size)) = (class_lookup.get(id), class_sizes.get(id)) else { continue };

        positioned_classes.push(PositionedClassNode {
            id: c.id.clone(),
            label: c.label.clone(),
            annotation: c.annotation.clone(),
            attributes: c.attributes.clone(),
            methods: c.methods.clone(),
            x: as_double(child.get("x")).unwrap_or(0.0),
            y: as_double(child.get("y")).unwrap_or(0.0),
            width: as_double(child.get("width")).unwrap_or(size.width),
            height: as_double(child.get("height")).unwrap_or(size.height),
            header_height: size.header_height,
            attr_height: size.attr_height,
            method_height: size.method_height,
        });
    }

    let mut relationships: Vec<PositionedClassRelationship> = Vec::new();
    let result_edges = as_dict_array(result.get("edges"));
    for (i, elk_edge) in result_edges.iter().enumerate() {
        if i >= diagram.relationships.len() {
            continue;
        }
        let rel = &diagram.relationships[i];

        let mut points: Vec<ClassPoint> = Vec::new();
        if let Some(section) = as_dict_array(elk_edge.get("sections")).first() {
            if let Some(start) = section.get("startPoint").and_then(Value::as_object) {
                if let (Some(sx), Some(sy)) = (as_double(start.get("x")), as_double(start.get("y"))) {
                    points.push(ClassPoint { x: sx, y: sy });
                }
            }

            for bp in as_dict_array(section.get("bendPoints")) {
                if let (Some(bx), Some(by)) = (as_double(bp.get("x")), as_double(bp.get("y"))) {
                    points.push(ClassPoint { x: bx, y: by });
                }
            }

            if let Some(end) = section.get("endPoint").and_then(Value::as_object) {
                if let (Some(ex), Some(ey)) = (as_double(end.get("x")), as_double(end.get("y"))) {
                    points.push(ClassPoint { x: ex, y: ey });
                }
            }
        }

        let mut label_position: Option<ClassPoint> = None;
        if let Some(label) = as_dict_array(elk_edge.get("labels")).first() {
            if let (Some(lx), Some(ly)) = (as_double(label.get("x")), as_double(label.get("y"))) {
                label_position = Some(ClassPoint {
                    x: lx + as_double(label.get("width")).unwrap_or(0.0) / 2.0,
                    y: ly + as_double(label.get("height")).unwrap_or(0.0) / 2.0,
                });
            }
        }

        relationships.push(PositionedClassRelationship {
            from: rel.from.clone(),
            to: rel.to.clone(),
            r#type: rel.r#type.clone(),
            marker_at: rel.marker_at.clone(),
            label: rel.label.clone(),
            from_cardinality: rel.from_cardinality.clone(),
            to_cardinality: rel.to_cardinality.clone(),
            points,
            label_position,
        });
    }

    PositionedClassDiagram {
        width: as_double(result.get("width")).unwrap_or(600.0),
        height: as_double(result.get("height")).unwrap_or(400.0),
        classes: positioned_classes,
        relationships,
    }
}

/// `layoutClassDiagramSync(_:options:)`.
pub fn layout_class_diagram_sync(diagram: &ClassDiagram) -> Result<PositionedClassDiagram, MermaidError> {
    if diagram.classes.is_empty() {
        return Ok(PositionedClassDiagram { width: 0.0, height: 0.0, classes: vec![], relationships: vec![] });
    }

    let (elk_graph, class_sizes) = build_class_elk_graph(diagram);
    let result = elk_layout_sync(&elk_graph)?;
    Ok(extract_class_layout(&result, diagram, &class_sizes))
}

/// The ELK input graph for a class diagram.
pub fn class_elk_input_graph(diagram: &ClassDiagram) -> Value {
    build_class_elk_graph(diagram).0
}

fn max_member_width(members: &[ClassMember]) -> f64 {
    if members.is_empty() {
        return 0.0;
    }
    let mut max_w = 0.0;
    for member in members {
        let text = member_to_string(member);
        let w = estimate_mono_text_width(&text, cls::MEMBER_FONT_SIZE);
        if w > max_w {
            max_w = w;
        }
    }
    max_w
}

/// `memberToString(_:)`.
pub fn member_to_string(m: &ClassMember) -> String {
    let vis = if m.visibility.is_empty() { String::new() } else { format!("{} ", m.visibility) };
    let name = if m.is_method { format!("{}({})", m.name, m.params.as_deref().unwrap_or("")) } else { m.name.clone() };
    let r#type = m.r#type.as_ref().map_or(String::new(), |t| format!(": {t}"));
    format!("{vis}{name}{type}")
}
