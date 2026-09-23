//! Rust counterpart of `oracle/Sources/downright-oracle/MermaidDump.swift`:
//! `mermaid-parse`, `mermaid-layout` and `mermaid` (the bridge's PNG), plus
//! `mermaid-bench`. Field names and nesting must match the Swift.

use std::path::Path;
use std::time::Instant;

use objc2::AnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSImage};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGBitmapContextCreateImage, CGColor, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB};
use objc2_foundation::NSDictionary;
use serde_json::Value;
use upleft_mermaid::downright::mermaid_renderer_bridge as bridge;
use upleft_mermaid::mermaid::src_class_parser::{ClassDiagram, ClassMember};
use upleft_mermaid::mermaid::src_elk_instance::{elk_available, replay};
use upleft_mermaid::mermaid::src_er_parser::{ErAttribute, ErDiagram};
use upleft_mermaid::mermaid::src_layout::{PositionedEdgePayload, PositionedGroupPayload, PositionedNodePayload};
use upleft_mermaid::mermaid::src_sequence_parser::SequenceDiagram;
use upleft_mermaid::mermaid::src_types::{MermaidGraph as ParsedGraphModel, MermaidSubgraph, SDict};
use upleft_mermaid::mermaid::src_xychart_types::{PositionedXYAxis, PositionedXYChart, XYAxis, XYAxisTick, XYChart};
use upleft_mermaid::{DiagramTheme, GraphLayout, LayoutConfig, MermaidError, MermaidGraph, Payload, PositionedContent, PositionedGraph};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

use super::Failure;
use super::attribute_dump::{color_json, font_json};
use super::json::{Object, double, write};
use super::markup::read_text;
use super::style_sheet::appearance_named;

fn style_sheet(theme_name: &str, dark: bool) -> Result<StyleSheet, Failure> {
    let store = ThemeStore::shared();
    let Some(theme) = store.themes().into_iter().find(|t| t.name == theme_name) else {
        let names: Vec<String> = store.themes().into_iter().map(|t| t.name).collect();
        return Err(Failure::Error(format!("unknown theme {theme_name}; have {names:?}")));
    };
    Ok(StyleSheet::new(theme, &appearance_named(dark), Some(true)))
}

fn trimmed(text: &str) -> String {
    upleft_mermaid::swift::trim_whitespaces_and_newlines(text).to_owned()
}

/// An ELK-backed layout this build cannot run.
fn is_elk_unavailable(error: &MermaidError) -> bool {
    matches!(error, MermaidError::Elk(_)) && !elk_available()
}

/// With `UPLEFT_MERMAID_ELK_REPLAY=<dir>`, answer ELK calls for `input` from
/// Swift's record `<dir>/<stem>.elkrec` (see crates/mermaid/tools/elk-capture)
/// instead of an engine. Returns whether a record was installed.
fn install_env_replay(input: &Path) -> Result<bool, Failure> {
    let Ok(dir) = std::env::var("UPLEFT_MERMAID_ELK_REPLAY") else { return Ok(false) };
    let Some(stem) = input.file_stem() else { return Ok(false) };
    let path = Path::new(&dir).join(stem).with_extension("elkrec");
    if !path.exists() {
        return Ok(false);
    }
    let record: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)
        .map_err(|e| Failure::Error(format!("{}: {e}", path.display())))?;
    replay::install(recorded_outputs(&record));
    Ok(true)
}

fn recorded_outputs(record: &Value) -> Vec<Option<Value>> {
    record["calls"]
        .as_array()
        .map(|calls| calls.iter().map(|c| (!c["output"].is_null()).then(|| c["output"].clone())).collect())
        .unwrap_or_default()
}

/// `mermaid-replay <x.elkrec> <out.json>`: lay the record's source out with
/// ELK answered from the record, and write the record's shape back — the
/// graphs this port handed ELK, the recorded answers, and the positioned
/// graph. The Swift oracle writes the record itself.
pub fn replay_record(input: &Path, output: &Path) -> Result<(), Failure> {
    let record: Value = serde_json::from_str(&std::fs::read_to_string(input)?)
        .map_err(|e| Failure::Error(format!("{}: {e}", input.display())))?;
    let source = record["source"].as_str().ok_or_else(|| Failure::Error("record without source".into()))?.to_owned();
    let outputs = recorded_outputs(&record);
    replay::install(outputs.clone());
    let result = upleft_mermaid::parser::parse(&source).and_then(|g| GraphLayout::new(LayoutConfig::default()).layout(&g));
    let inputs = replay::finish();
    let calls: Vec<Value> = inputs
        .into_iter()
        .enumerate()
        .map(|(i, graph)| {
            Object::new().with("input", graph).with("output", outputs.get(i).cloned().flatten().unwrap_or(Value::Null)).build()
        })
        .collect();
    let mut out = Object::new().with("source", source.as_str()).with("calls", Value::Array(calls));
    out = match result {
        Ok(positioned) => out.with("positioned", positioned_graph_json(&positioned)),
        Err(error) => out.with("layoutError", error_json(&error)),
    };
    Ok(write(&out.build(), output)?)
}

// MARK: - Commands

pub fn parse(input: &Path, output: &Path) -> Result<(), Failure> {
    let source = trimmed(&read_text(input)?);
    let mut out = Object::new().with("empty", source.is_empty());
    out = match upleft_mermaid::parser::parse(&source) {
        Ok(graph) => out.with("parsed", graph_json(&graph)),
        Err(error) => out.with("error", error_json(&error)),
    };
    Ok(write(&out.build(), output)?)
}

pub fn layout(input: &Path, output: &Path, theme_name: &str, dark: bool) -> Result<(), Failure> {
    let source = trimmed(&read_text(input)?);
    let sheet = style_sheet(theme_name, dark)?;
    install_env_replay(input)?;
    let mut out = Object::new().with("empty", source.is_empty()).with("theme", theme_json(&bridge::theme(&sheet)));
    match upleft_mermaid::parser::parse(&source) {
        Ok(parsed) => {
            out = out.with("parsed", graph_json(&parsed));
            match GraphLayout::new(LayoutConfig::default()).layout(&parsed) {
                Ok(positioned) => {
                    let width = upleft_mermaid::swift::max(1.0, positioned.width);
                    let height = upleft_mermaid::swift::max(1.0, positioned.height);
                    out = out
                        .with("positioned", positioned_graph_json(&positioned))
                        .with("bounds", Value::Array(vec![double(0.0), double(0.0), double(width), double(height)]));
                }
                Err(error) if is_elk_unavailable(&error) => return Err(Failure::NotPorted),
                Err(error) => out = out.with("layoutError", error_json(&error)),
            }
        }
        Err(error) => out = out.with("error", error_json(&error)),
    }
    Ok(write(&out.build(), output)?)
}

pub fn image(input: &Path, output: &Path, theme_name: &str, dark: bool) -> Result<(), Failure> {
    let source = read_text(input)?;
    let sheet = style_sheet(theme_name, dark)?;
    // Layouts that need ELK cannot be rendered without it.
    install_env_replay(input)?;
    if let Some(t) = bridge::trimmed_source(&source) {
        if let Ok(graph) = upleft_mermaid::parser::parse(t) {
            if let Err(error) = GraphLayout::new(LayoutConfig::default()).layout(&graph) {
                if is_elk_unavailable(&error) {
                    return Err(Failure::NotPorted);
                }
            }
        }
    }
    replay::finish();
    install_env_replay(input)?;
    let png = match bridge::image(&source, &sheet) {
        Some(image) => encode(&image.ns_image())?,
        None => sentinel()?,
    };
    std::fs::write(output, png)?;
    Ok(())
}

/// The bitmap behind the `NSImage` the bridge returns.
fn encode(image: &NSImage) -> Result<Vec<u8>, Failure> {
    let reps = image.representations();
    let rep = reps.firstObject().ok_or_else(|| Failure::Error("the bridge's image has no representation".into()))?;
    let cg_image = unsafe { rep.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }
        .ok_or_else(|| Failure::Error("the bridge's image has no CGImage".into()))?;
    if CGImage::width(Some(&cg_image)) as isize != rep.pixelsWide() || CGImage::height(Some(&cg_image)) as isize != rep.pixelsHigh() {
        return Err(Failure::Error("the bridge's image was resampled".into()));
    }
    png(&cg_image)
}

fn png(image: &CGImage) -> Result<Vec<u8>, Failure> {
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), image);
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
        .ok_or_else(|| Failure::Error("PNG encoding failed".into()))?;
    Ok(data.to_vec())
}

/// A 1×1 opaque magenta pixel: "the bridge returned nil".
fn sentinel() -> Result<Vec<u8>, Failure> {
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).ok_or(Failure::Error("no sRGB".into()))?;
    let context: objc2_core_foundation::CFRetained<CGContext> = unsafe {
        CGBitmapContextCreate(std::ptr::null_mut(), 1, 1, 8, 0, Some(&space), CGImageAlphaInfo::PremultipliedLast.0)
    }
    .ok_or_else(|| Failure::Error("no bitmap context".into()))?;
    let magenta = CGColor::new_srgb(1.0, 0.0, 1.0, 1.0);
    CGContext::set_fill_color_with_color(Some(&context), Some(&magenta));
    CGContext::fill_rect(Some(&context), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1.0, 1.0)));
    let image = CGBitmapContextCreateImage(Some(&context)).ok_or_else(|| Failure::Error("no image".into()))?;
    png(&image)
}

// MARK: - Errors

fn error_json(error: &MermaidError) -> Value {
    let (case, values) = error.case_and_values();
    Object::new()
        .with("case", case)
        .with("values", Value::Array(values.into_iter().map(|v| Value::String(v.to_owned())).collect()))
        .build()
}

// MARK: - Theme

fn color(color: Option<&NSColor>) -> Value {
    color.map_or(Value::Null, color_json)
}

fn theme_json(theme: &DiagramTheme) -> Value {
    Object::new()
        .with("background", color_json(&theme.background))
        .with("foreground", color_json(&theme.foreground))
        .with("line", color(theme.line.as_deref()))
        .with("accent", color(theme.accent.as_deref()))
        .with("muted", color(theme.muted.as_deref()))
        .with("surface", color(theme.surface.as_deref()))
        .with("border", color(theme.border.as_deref()))
        .with("font", font_json(&theme.font))
        .with("lineWidth", double(theme.line_width))
        .with("cornerRadius", double(theme.corner_radius))
        .with("transparent", theme.transparent)
        .with("effectiveLine", color_json(&theme.effective_line()))
        .with("effectiveAccent", color_json(&theme.effective_accent()))
        .with("effectiveMuted", color_json(&theme.effective_muted()))
        .with("effectiveSurface", color_json(&theme.effective_surface()))
        .with("effectiveBorder", color_json(&theme.effective_border()))
        .with("effectiveTextSecondary", color_json(&theme.effective_text_secondary()))
        .with("effectiveTextFaint", color_json(&theme.effective_text_faint()))
        .with("effectiveArrow", color_json(&theme.effective_arrow()))
        .with("effectiveInnerStroke", color_json(&theme.effective_inner_stroke()))
        .with("subgraphHeaderColor", color_json(&theme.subgraph_header_color()))
        .with("keyBadgeColor", color_json(&theme.key_badge_color()))
        .build()
}

// MARK: - Parsed diagrams

fn string(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |s| Value::String(s.to_owned()))
}

fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().map(|s| Value::String(s.clone())).collect())
}

fn dictionary(dict: Option<&SDict<String>>) -> Value {
    let Some(dict) = dict else { return Value::Null };
    let mut out = Object::new();
    for key in dict.sorted_keys() {
        out = out.with(key, dict.get(key).unwrap().as_str());
    }
    out.build()
}

fn graph_json(graph: &MermaidGraph) -> Value {
    let model = match &graph.payload {
        Payload::Flow(model) => flow_model(model),
        Payload::Sequence(d) => sequence(d),
        Payload::Class(d) => class_diagram(d),
        Payload::Er(d) => er_diagram(d),
        Payload::XyChart(c) => xy_chart(c),
    };
    Object::new().with("type", graph.diagram_type.raw_value()).with("model", model).build()
}

fn flow_model(model: &ParsedGraphModel) -> Value {
    let mut class_defs = Object::new();
    for key in model.class_defs.sorted_keys() {
        class_defs = class_defs.with(key, dictionary(model.class_defs.get(key)));
    }
    let mut node_styles = Object::new();
    for key in model.node_styles.sorted_keys() {
        node_styles = node_styles.with(key, dictionary(model.node_styles.get(key)));
    }
    let mut link_keys: Vec<i64> = model.link_styles.keys().copied().collect();
    link_keys.sort();
    Object::new()
        .with("direction", model.direction.raw_value())
        .with(
            "nodes",
            Value::Array(
                model
                    .nodes_in_order
                    .iter()
                    .map(|(id, node)| {
                        Object::new()
                            .with("id", id.as_str())
                            .with("nodeId", node.id.as_str())
                            .with("label", node.label.as_str())
                            .with("shape", node.shape.raw_value())
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "edges",
            Value::Array(
                model
                    .edges
                    .iter()
                    .map(|e| {
                        Object::new()
                            .with("source", e.source.as_str())
                            .with("target", e.target.as_str())
                            .with("label", string(e.label.as_deref()))
                            .with("style", e.style.raw_value())
                            .with("hasArrowStart", e.has_arrow_start)
                            .with("hasArrowEnd", e.has_arrow_end)
                            .with("inlineStyle", dictionary(e.inline_style.as_ref()))
                            .build()
                    })
                    .collect(),
            ),
        )
        .with("subgraphs", Value::Array(model.subgraphs.iter().map(subgraph).collect()))
        .with("classDefs", class_defs.build())
        .with("classAssignments", dictionary(Some(&model.class_assignments)))
        .with("nodeStyles", node_styles.build())
        .with(
            "linkStyles",
            Value::Array(
                link_keys
                    .iter()
                    .map(|k| Value::Array(vec![Value::from(*k), dictionary(model.link_styles.get(k))]))
                    .collect(),
            ),
        )
        .build()
}

fn subgraph(sub: &MermaidSubgraph) -> Value {
    Object::new()
        .with("id", sub.id.as_str())
        .with("label", sub.label.as_str())
        .with("nodeIds", strings(&sub.node_ids))
        .with("direction", string(sub.direction.map(|d| d.raw_value())))
        .with("children", Value::Array(sub.children.iter().map(subgraph).collect()))
        .build()
}

fn sequence(d: &SequenceDiagram) -> Value {
    Object::new()
        .with(
            "actors",
            Value::Array(
                d.actors
                    .iter()
                    .map(|a| Object::new().with("id", a.id.as_str()).with("label", a.label.as_str()).with("type", a.r#type.as_str()).build())
                    .collect(),
            ),
        )
        .with(
            "messages",
            Value::Array(
                d.messages
                    .iter()
                    .map(|m| {
                        Object::new()
                            .with("from", m.from.as_str())
                            .with("to", m.to.as_str())
                            .with("label", m.label.as_str())
                            .with("lineStyle", m.line_style.as_str())
                            .with("arrowHead", m.arrow_head.as_str())
                            .with("activate", m.activate)
                            .with("deactivate", m.deactivate)
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "blocks",
            Value::Array(
                d.blocks
                    .iter()
                    .map(|b| {
                        Object::new()
                            .with("type", b.r#type.as_str())
                            .with("label", b.label.as_str())
                            .with("startIndex", b.start_index)
                            .with("endIndex", b.end_index)
                            .with(
                                "dividers",
                                Value::Array(
                                    b.dividers
                                        .iter()
                                        .map(|dv| Object::new().with("index", dv.index).with("label", dv.label.as_str()).build())
                                        .collect(),
                                ),
                            )
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "notes",
            Value::Array(
                d.notes
                    .iter()
                    .map(|n| {
                        Object::new()
                            .with("actorIds", strings(&n.actor_ids))
                            .with("text", n.text.as_str())
                            .with("position", n.position.as_str())
                            .with("afterIndex", n.after_index)
                            .build()
                    })
                    .collect(),
            ),
        )
        .build()
}

fn member(m: &ClassMember) -> Value {
    Object::new()
        .with("visibility", m.visibility.as_str())
        .with("name", m.name.as_str())
        .with("type", string(m.r#type.as_deref()))
        .with("isStatic", m.is_static)
        .with("isAbstract", m.is_abstract)
        .with("isMethod", m.is_method)
        .with("params", string(m.params.as_deref()))
        .build()
}

fn members(ms: &[ClassMember]) -> Value {
    Value::Array(ms.iter().map(member).collect())
}

fn class_diagram(d: &ClassDiagram) -> Value {
    Object::new()
        .with(
            "classes",
            Value::Array(
                d.classes
                    .iter()
                    .map(|c| {
                        Object::new()
                            .with("id", c.id.as_str())
                            .with("label", c.label.as_str())
                            .with("attributes", members(&c.attributes))
                            .with("methods", members(&c.methods))
                            .with("annotation", string(c.annotation.as_deref()))
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "relationships",
            Value::Array(
                d.relationships
                    .iter()
                    .map(|r| {
                        Object::new()
                            .with("from", r.from.as_str())
                            .with("to", r.to.as_str())
                            .with("type", r.r#type.as_str())
                            .with("markerAt", r.marker_at.as_str())
                            .with("label", string(r.label.as_deref()))
                            .with("fromCardinality", string(r.from_cardinality.as_deref()))
                            .with("toCardinality", string(r.to_cardinality.as_deref()))
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "namespaces",
            Value::Array(
                d.namespaces.iter().map(|n| Object::new().with("name", n.name.as_str()).with("classIds", strings(&n.class_ids)).build()).collect(),
            ),
        )
        .build()
}

fn attribute(a: &ErAttribute) -> Value {
    Object::new()
        .with("type", a.r#type.as_str())
        .with("name", a.name.as_str())
        .with("keys", strings(&a.keys))
        .with("comment", string(a.comment.as_deref()))
        .build()
}

fn attributes(attrs: &[ErAttribute]) -> Value {
    Value::Array(attrs.iter().map(attribute).collect())
}

fn er_diagram(d: &ErDiagram) -> Value {
    Object::new()
        .with(
            "entities",
            Value::Array(
                d.entities
                    .iter()
                    .map(|e| {
                        Object::new()
                            .with("id", e.id.as_str())
                            .with("label", e.label.as_str())
                            .with("attributes", attributes(&e.attributes))
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "relationships",
            Value::Array(
                d.relationships
                    .iter()
                    .map(|r| {
                        Object::new()
                            .with("entity1", r.entity1.as_str())
                            .with("entity2", r.entity2.as_str())
                            .with("cardinality1", r.cardinality1.as_str())
                            .with("cardinality2", r.cardinality2.as_str())
                            .with("label", r.label.as_str())
                            .with("identifying", r.identifying)
                            .build()
                    })
                    .collect(),
            ),
        )
        .build()
}

fn axis(a: &XYAxis) -> Value {
    Object::new()
        .with("title", string(a.title.as_deref()))
        .with("categories", a.categories.as_ref().map_or(Value::Null, |c| strings(c)))
        .with("range", a.range.map_or(Value::Null, |(min, max)| Value::Array(vec![double(min), double(max)])))
        .build()
}

fn xy_chart(c: &XYChart) -> Value {
    Object::new()
        .with("title", string(c.title.as_deref()))
        .with("horizontal", c.horizontal)
        .with("xAxis", axis(&c.x_axis))
        .with("yAxis", axis(&c.y_axis))
        .with(
            "series",
            Value::Array(
                c.series
                    .iter()
                    .map(|s| {
                        Object::new()
                            .with("type", s.r#type.raw_value())
                            .with("data", Value::Array(s.data.iter().map(|v| double(*v)).collect()))
                            .build()
                    })
                    .collect(),
            ),
        )
        .build()
}

// MARK: - Positioned diagrams

fn point(x: f64, y: f64) -> Value {
    Value::Array(vec![double(x), double(y)])
}

fn positioned_graph_json(g: &PositionedGraph) -> Value {
    let mut out = Object::new()
        .with("type", g.diagram.diagram_type.raw_value())
        .with("width", double(g.width))
        .with("height", double(g.height));
    match &g.content {
        PositionedContent::Flowchart { nodes, edges, groups } => {
            out = flow_content(out.with("kind", "flowchart"), nodes, edges, groups);
        }
        PositionedContent::StateDiagram { nodes, edges, groups } => {
            out = flow_content(out.with("kind", "stateDiagram"), nodes, edges, groups);
        }
        PositionedContent::SequenceDiagram { actors, messages, blocks, lifelines, activations, notes } => {
            out = out.with("kind", "sequenceDiagram");
            out = out.with(
                "actors",
                Value::Array(
                    actors
                        .iter()
                        .map(|a| {
                            Object::new()
                                .with("id", a.id.as_str())
                                .with("label", a.label.as_str())
                                .with("type", a.r#type.as_str())
                                .with("x", double(a.x))
                                .with("y", double(a.y))
                                .with("width", double(a.width))
                                .with("height", double(a.height))
                                .build()
                        })
                        .collect(),
                ),
            );
            out = out.with(
                "messages",
                Value::Array(
                    messages
                        .iter()
                        .map(|m| {
                            Object::new()
                                .with("from", m.from.as_str())
                                .with("to", m.to.as_str())
                                .with("label", m.label.as_str())
                                .with("lineStyle", m.line_style.as_str())
                                .with("arrowHead", m.arrow_head.as_str())
                                .with("x1", double(m.x1))
                                .with("x2", double(m.x2))
                                .with("y", double(m.y))
                                .with("isSelf", m.is_self)
                                .build()
                        })
                        .collect(),
                ),
            );
            out = out.with(
                "blocks",
                Value::Array(
                    blocks
                        .iter()
                        .map(|b| {
                            Object::new()
                                .with("type", b.r#type.as_str())
                                .with("label", b.label.as_str())
                                .with("x", double(b.x))
                                .with("y", double(b.y))
                                .with("width", double(b.width))
                                .with("height", double(b.height))
                                .with(
                                    "dividers",
                                    Value::Array(
                                        b.dividers
                                            .iter()
                                            .map(|d| Object::new().with("y", double(d.y)).with("label", d.label.as_str()).build())
                                            .collect(),
                                    ),
                                )
                                .build()
                        })
                        .collect(),
                ),
            );
            out = out.with(
                "lifelines",
                Value::Array(
                    lifelines
                        .iter()
                        .map(|l| {
                            Object::new()
                                .with("actorId", l.actor_id.as_str())
                                .with("x", double(l.x))
                                .with("topY", double(l.top_y))
                                .with("bottomY", double(l.bottom_y))
                                .build()
                        })
                        .collect(),
                ),
            );
            // Activations still open at the end come out in Swift Dictionary
            // order (random per process): compare them sorted.
            let mut sorted = activations.clone();
            upleft_mermaid::swift::sort_by(&mut sorted, |a, b| {
                if a.top_y != b.top_y {
                    return a.top_y < b.top_y;
                }
                if a.x != b.x {
                    return a.x < b.x;
                }
                a.bottom_y < b.bottom_y
            });
            out = out.with(
                "activations",
                Value::Array(
                    sorted
                        .iter()
                        .map(|a| {
                            Object::new()
                                .with("actorId", a.actor_id.as_str())
                                .with("x", double(a.x))
                                .with("topY", double(a.top_y))
                                .with("bottomY", double(a.bottom_y))
                                .with("width", double(a.width))
                                .build()
                        })
                        .collect(),
                ),
            );
            out = out.with(
                "notes",
                Value::Array(
                    notes
                        .iter()
                        .map(|n| {
                            Object::new()
                                .with("text", n.text.as_str())
                                .with("x", double(n.x))
                                .with("y", double(n.y))
                                .with("width", double(n.width))
                                .with("height", double(n.height))
                                .with("position", n.position.as_str())
                                .with("actors", strings(&n.actors))
                                .build()
                        })
                        .collect(),
                ),
            );
        }
        PositionedContent::ClassDiagram { classes, relationships } => {
            out = out.with("kind", "classDiagram");
            out = out.with(
                "classes",
                Value::Array(
                    classes
                        .iter()
                        .map(|c| {
                            Object::new()
                                .with("id", c.id.as_str())
                                .with("label", c.label.as_str())
                                .with("annotation", string(c.annotation.as_deref()))
                                .with("attributes", members(&c.attributes))
                                .with("methods", members(&c.methods))
                                .with("x", double(c.x))
                                .with("y", double(c.y))
                                .with("width", double(c.width))
                                .with("height", double(c.height))
                                .with("headerHeight", double(c.header_height))
                                .with("attrHeight", double(c.attr_height))
                                .with("methodHeight", double(c.method_height))
                                .build()
                        })
                        .collect(),
                ),
            );
            out = out.with(
                "relationships",
                Value::Array(
                    relationships
                        .iter()
                        .map(|r| {
                            Object::new()
                                .with("from", r.from.as_str())
                                .with("to", r.to.as_str())
                                .with("type", r.r#type.as_str())
                                .with("markerAt", r.marker_at.as_str())
                                .with("label", string(r.label.as_deref()))
                                .with("fromCardinality", string(r.from_cardinality.as_deref()))
                                .with("toCardinality", string(r.to_cardinality.as_deref()))
                                .with("points", Value::Array(r.points.iter().map(|p| point(p.x, p.y)).collect()))
                                .with("labelPosition", r.label_position.map_or(Value::Null, |p| point(p.x, p.y)))
                                .build()
                        })
                        .collect(),
                ),
            );
        }
        PositionedContent::ErDiagram { entities, relationships } => {
            out = out.with("kind", "erDiagram");
            out = out.with(
                "entities",
                Value::Array(
                    entities
                        .iter()
                        .map(|e| {
                            Object::new()
                                .with("id", e.id.as_str())
                                .with("label", e.label.as_str())
                                .with("attributes", attributes(&e.attributes))
                                .with("x", double(e.x))
                                .with("y", double(e.y))
                                .with("width", double(e.width))
                                .with("height", double(e.height))
                                .with("headerHeight", double(e.header_height))
                                .with("rowHeight", double(e.row_height))
                                .build()
                        })
                        .collect(),
                ),
            );
            out = out.with(
                "relationships",
                Value::Array(
                    relationships
                        .iter()
                        .map(|r| {
                            Object::new()
                                .with("entity1", r.entity1.as_str())
                                .with("entity2", r.entity2.as_str())
                                .with("cardinality1", r.cardinality1.as_str())
                                .with("cardinality2", r.cardinality2.as_str())
                                .with("label", r.label.as_str())
                                .with("identifying", r.identifying)
                                .with("points", Value::Array(r.points.iter().map(|p| point(p.x, p.y)).collect()))
                                .build()
                        })
                        .collect(),
                ),
            );
        }
        PositionedContent::XyChart(chart) => {
            out = out.with("kind", "xyChart").with("chart", positioned_chart(chart));
        }
    }
    out.build()
}

fn flow_content(
    out: Object,
    nodes: &[PositionedNodePayload],
    edges: &[PositionedEdgePayload],
    groups: &[PositionedGroupPayload],
) -> Object {
    out.with(
        "nodes",
        Value::Array(
            nodes
                .iter()
                .map(|n| {
                    Object::new()
                        .with("id", n.id.as_str())
                        .with("label", n.label.as_str())
                        .with("shape", n.shape.as_str())
                        .with("x", double(n.x))
                        .with("y", double(n.y))
                        .with("width", double(n.width))
                        .with("height", double(n.height))
                        .with("inlineStyle", dictionary(Some(&n.inline_style)))
                        .build()
                })
                .collect(),
        ),
    )
    .with(
        "edges",
        Value::Array(
            edges
                .iter()
                .map(|e| {
                    Object::new()
                        .with("source", e.source.as_str())
                        .with("target", e.target.as_str())
                        .with("label", string(e.label.as_deref()))
                        .with("style", e.style.as_str())
                        .with("hasArrowStart", e.has_arrow_start)
                        .with("hasArrowEnd", e.has_arrow_end)
                        .with("points", Value::Array(e.points.iter().map(|p| point(p.x, p.y)).collect()))
                        .with("labelPosition", e.label_position.map_or(Value::Null, |p| point(p.x, p.y)))
                        .with("inlineStyle", dictionary(e.inline_style.as_ref()))
                        .build()
                })
                .collect(),
        ),
    )
    .with("groups", Value::Array(groups.iter().map(group).collect()))
}

fn group(g: &PositionedGroupPayload) -> Value {
    Object::new()
        .with("id", g.id.as_str())
        .with("label", g.label.as_str())
        .with("x", double(g.x))
        .with("y", double(g.y))
        .with("width", double(g.width))
        .with("height", double(g.height))
        .with("headerHeight", double(g.header_height))
        .with("children", Value::Array(g.children.iter().map(group).collect()))
        .build()
}

fn tick(t: &XYAxisTick) -> Value {
    Object::new()
        .with("label", t.label.as_str())
        .with("x", double(t.x))
        .with("y", double(t.y))
        .with("tx", double(t.tx))
        .with("ty", double(t.ty))
        .with("labelX", double(t.label_x))
        .with("labelY", double(t.label_y))
        .with("textAnchor", t.text_anchor.as_str())
        .build()
}

fn positioned_axis(a: &PositionedXYAxis) -> Value {
    Object::new()
        .with(
            "title",
            a.title.as_ref().map_or(Value::Null, |t| {
                Object::new()
                    .with("text", t.text.as_str())
                    .with("x", double(t.x))
                    .with("y", double(t.y))
                    .with("rotate", t.rotate.map_or(Value::Null, double))
                    .build()
            }),
        )
        .with("ticks", Value::Array(a.ticks.iter().map(tick).collect()))
        .with("line", Value::Array(vec![double(a.line.x1), double(a.line.y1), double(a.line.x2), double(a.line.y2)]))
        .build()
}

fn positioned_chart(c: &PositionedXYChart) -> Value {
    Object::new()
        .with("width", double(c.width))
        .with("height", double(c.height))
        .with("horizontal", c.horizontal)
        .with(
            "title",
            c.title.as_ref().map_or(Value::Null, |t| {
                Object::new().with("text", t.text.as_str()).with("x", double(t.x)).with("y", double(t.y)).build()
            }),
        )
        .with("xAxis", positioned_axis(&c.x_axis))
        .with("yAxis", positioned_axis(&c.y_axis))
        .with(
            "plotArea",
            Value::Array(vec![double(c.plot_area.x), double(c.plot_area.y), double(c.plot_area.width), double(c.plot_area.height)]),
        )
        .with(
            "bars",
            Value::Array(
                c.bars
                    .iter()
                    .map(|b| {
                        Object::new()
                            .with("x", double(b.x))
                            .with("y", double(b.y))
                            .with("width", double(b.width))
                            .with("height", double(b.height))
                            .with("value", double(b.value))
                            .with("label", string(b.label.as_deref()))
                            .with("seriesIndex", b.series_index)
                            .with("colorIndex", b.color_index)
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "lines",
            Value::Array(
                c.lines
                    .iter()
                    .map(|l| {
                        Object::new()
                            .with(
                                "points",
                                Value::Array(
                                    l.points
                                        .iter()
                                        .map(|p| {
                                            Object::new()
                                                .with("x", double(p.x))
                                                .with("y", double(p.y))
                                                .with("value", double(p.value))
                                                .with("label", string(p.label.as_deref()))
                                                .build()
                                        })
                                        .collect(),
                                ),
                            )
                            .with("seriesIndex", l.series_index)
                            .with("colorIndex", l.color_index)
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "gridLines",
            Value::Array(
                c.grid_lines.iter().map(|g| Value::Array(vec![double(g.x1), double(g.y1), double(g.x2), double(g.y2)])).collect(),
            ),
        )
        .with(
            "legend",
            Value::Array(
                c.legend
                    .iter()
                    .map(|i| {
                        Object::new()
                            .with("label", i.label.as_str())
                            .with("x", double(i.x))
                            .with("y", double(i.y))
                            .with("type", i.r#type.raw_value())
                            .with("seriesIndex", i.series_index)
                            .with("colorIndex", i.color_index)
                            .build()
                    })
                    .collect(),
            ),
        )
        .build()
}

// MARK: - Bench

/// `mermaid-bench <dir> <out.json>`: `MermaidBench.run` — prepare and render
/// every `.mmd` the way the bridge does, uncached; best of N rounds.
pub fn bench(directory: &Path, output: &Path) -> Result<(), Failure> {
    let sheet = style_sheet("Paper Light", false)?;
    let theme = bridge::theme(&sheet);
    let mut files: Vec<_> = std::fs::read_dir(directory)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mmd"))
        .collect();
    files.sort();
    // With UPLEFT_MERMAID_ELK_REPLAY set and no engine linked, ELK-backed
    // diagrams take ELK's answers from Swift's records (a clone per call).
    let replay_dir = std::env::var("UPLEFT_MERMAID_ELK_REPLAY").ok().filter(|_| !elk_available());
    let mut sources = Vec::new();
    for f in &files {
        let text = read_text(f)?;
        if !trimmed(&text).is_empty() {
            let outputs = match &replay_dir {
                Some(dir) => {
                    let path = Path::new(dir).join(f.file_stem().unwrap()).with_extension("elkrec");
                    match std::fs::read_to_string(&path) {
                        Ok(record) => Some(recorded_outputs(
                            &serde_json::from_str(&record).map_err(|e| Failure::Error(format!("{}: {e}", path.display())))?,
                        )),
                        Err(_) => None,
                    }
                }
                None => None,
            };
            sources.push((text, outputs));
        }
    }
    let rounds: usize = std::env::var("MERMAID_BENCH_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);

    // `prepareMs`: `MermaidImageRenderer.prepare`; `bridgeMs`: the whole
    // uncached `MermaidRendererBridge.image` path, `NSImage` included.
    let once = || {
        let (mut prepare, mut whole, mut images) = (0.0f64, 0.0f64, 0usize);
        for (source, outputs) in &sources {
            let start = Instant::now();
            if let Some(outputs) = outputs {
                replay::install(outputs.clone());
            }
            let renderer = upleft_mermaid::MermaidImageRenderer::new(theme.clone(), LayoutConfig::default());
            let _ = renderer.prepare(&trimmed(source));
            replay::finish();
            prepare += start.elapsed().as_secs_f64() * 1e3;

            let start = Instant::now();
            if let Some(outputs) = outputs {
                replay::install(outputs.clone());
            }
            objc2::rc::autoreleasepool(|_| {
                if let Some(image) = bridge::image(source, &sheet) {
                    let _ = image.ns_image();
                    images += 1;
                }
            });
            replay::finish();
            whole += start.elapsed().as_secs_f64() * 1e3;
        }
        (prepare, whole, images)
    };

    let _ = once();
    let (mut best_prepare, mut best_bridge, mut images) = (f64::INFINITY, f64::INFINITY, 0);
    for _ in 0..rounds {
        let (p, b, n) = once();
        best_prepare = best_prepare.min(p);
        best_bridge = best_bridge.min(b);
        images = n;
    }
    let value = Object::new()
        .with("diagrams", sources.len())
        .with("images", images)
        .with("rounds", rounds)
        .with("prepareMs", double(best_prepare))
        .with("bridgeMs", double(best_bridge))
        .build();
    Ok(write(&value, output)?)
}
