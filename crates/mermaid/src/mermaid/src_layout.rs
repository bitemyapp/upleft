//! Port of `Mermaid/src_layout.swift` (from `original/src/layout.ts`): the
//! ELK-backed flowchart / state-diagram layout and its post-processing
//! (orthogonalisation, layer alignment, edge bundling, shape clipping).
//!
//! The ELK graph is built as JSON (`[String: Any]` in Swift); objects keep
//! the Swift literal's key order.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use super::src_elk_instance::elk_layout_sync;
use super::src_styles::{FONT_SIZES, FONT_WEIGHTS};
use super::src_text_metrics::measure_multiline_text;
use super::src_types::{
    strings_contain, Direction, MermaidEdge, MermaidGraph as ParsedGraph, MermaidNode, MermaidSubgraph, NodeShape,
    SDict, SSet,
};
use crate::error::MermaidError;
use crate::swift::{self, max, min};
use crate::types::{DiagramType, LayoutConfig, MermaidGraph, Payload, PositionedContent, PositionedGraph};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedPointPayload {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedNodePayload {
    pub id: String,
    pub label: String,
    pub shape: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub inline_style: SDict<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedEdgePayload {
    pub source: String,
    pub target: String,
    pub label: Option<String>,
    pub style: String,
    pub has_arrow_start: bool,
    pub has_arrow_end: bool,
    pub points: Vec<PositionedPointPayload>,
    pub label_position: Option<PositionedPointPayload>,
    pub inline_style: Option<SDict<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGroupPayload {
    pub id: String,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub header_height: f64,
    pub children: Vec<PositionedGroupPayload>,
}

type P = PositionedPointPayload;

// MARK: - JSON access (`_asDict`, `_asDictArray`, `_asString`, `_asDouble`)

fn as_dict(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value?.as_object()
}

fn as_dict_array(value: Option<&Value>) -> Vec<&Map<String, Value>> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_object).collect(),
        _ => Vec::new(),
    }
}

fn as_string(value: Option<&Value>) -> Option<&str> {
    value?.as_str()
}

fn as_double(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn map_direction(direction: Direction) -> &'static str {
    match direction {
        Direction::LR => "RIGHT",
        Direction::RL => "LEFT",
        Direction::BT => "UP",
        Direction::TD | Direction::TB => "DOWN",
    }
}

/// `_nodeSize(_:)`.
fn node_size(node: &MermaidNode) -> (f64, f64) {
    let metrics = measure_multiline_text(&node.label, FONT_SIZES.node_label, FONT_WEIGHTS.node_label);
    // Match TS NODE_PADDING: horizontal=20 (*2=40), vertical=10 (*2=20)
    let mut width = metrics.width + 40.0;
    let mut height = metrics.height + 20.0;

    match node.shape {
        NodeShape::Diamond => {
            let side = max(width, height) + 24.0;
            width = side;
            height = side;
        }
        NodeShape::Circle => {
            let d = (width * width + height * height).sqrt().ceil() + 8.0;
            width = d;
            height = d;
        }
        NodeShape::Doublecircle => {
            let d = (width * width + height * height).sqrt().ceil() + 8.0 + 12.0;
            width = d;
            height = d;
        }
        NodeShape::Hexagon => width += 20.0,
        NodeShape::Trapezoid | NodeShape::TrapezoidAlt => width += 20.0,
        NodeShape::Asymmetric => width += 12.0,
        NodeShape::Cylinder => height += 14.0,
        NodeShape::StateStart | NodeShape::StateEnd => return (28.0, 28.0),
        _ => {}
    }

    width = max(width, 60.0);
    height = max(height, 36.0);
    (width, height)
}

fn edge_label_json(label: &str) -> Value {
    let m = measure_multiline_text(label, FONT_SIZES.edge_label, FONT_WEIGHTS.edge_label);
    json!([{
        "text": label,
        "width": m.width + 8.0,
        "height": m.height + 6.0,
        "layoutOptions": {
            "elk.edgeLabels.inline": "true",
            "elk.edgeLabels.placement": "CENTER"
        }
    }])
}

/// `_edgeDict(_:_:)`.
fn edge_dict(idx: usize, edge: &MermaidEdge) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), Value::from(format!("e{idx}")));
    out.insert("sources".into(), json!([edge.source]));
    out.insert("targets".into(), json!([edge.target]));
    if let Some(label) = &edge.label {
        if !label.is_empty() {
            out.insert("labels".into(), edge_label_json(label));
        }
    }
    Value::Object(out)
}

fn node_child(id: &str, node: &MermaidNode, with_label: bool) -> Value {
    let size = node_size(node);
    let mut out = Map::new();
    out.insert("id".into(), Value::from(id));
    out.insert("width".into(), Value::from(size.0));
    out.insert("height".into(), Value::from(size.1));
    if with_label {
        out.insert("labels".into(), json!([{ "text": node.label }]));
    }
    Value::Object(out)
}

fn root_layout_options(direction: Direction, hierarchy_handling: Option<&str>, random_seed: bool) -> Map<String, Value> {
    let mut opts = Map::new();
    let mut set = |k: &str, v: &str| {
        opts.insert(k.into(), Value::from(v));
    };
    set("elk.algorithm", "layered");
    set("elk.direction", map_direction(direction));
    set("elk.spacing.nodeNode", "28");
    set("elk.spacing.edgeEdge", "12");
    set("elk.layered.spacing.nodeNodeBetweenLayers", "48");
    set("elk.layered.spacing.edgeEdgeBetweenLayers", "12");
    set("elk.layered.spacing.edgeNodeBetweenLayers", "12");
    set("elk.padding", "[top=40,left=40,bottom=40,right=40]");
    set("elk.edgeRouting", "ORTHOGONAL");
    set("elk.contentAlignment", "H_CENTER V_CENTER");
    set("elk.layered.nodePlacement.bk.fixedAlignment", "BALANCED");
    set("elk.layered.considerModelOrder.strategy", "NODES_AND_EDGES");
    set("elk.layered.thoroughness", "3");
    set("elk.layered.compaction.postCompaction.strategy", "LEFT_RIGHT_CONSTRAINT_LOCKING");
    set("elk.layered.highDegreeNodes.treatment", "true");
    set("elk.layered.highDegreeNodes.threshold", "8");
    if let Some(handling) = hierarchy_handling {
        set("elk.layered.wrapping.strategy", "OFF");
        set("elk.hierarchyHandling", handling);
    }
    if random_seed {
        set("elk.randomSeed", "1");
    }
    opts
}

fn subgraph_layout_options(direction: Option<Direction>) -> Map<String, Value> {
    let mut opts = Map::new();
    let mut set = |k: &str, v: &str| {
        opts.insert(k.into(), Value::from(v));
    };
    set("elk.algorithm", "layered");
    set("elk.padding", "[top=44,left=16,bottom=16,right=16]");
    set("elk.edgeRouting", "ORTHOGONAL");
    set("elk.contentAlignment", "H_CENTER V_CENTER");
    set("elk.spacing.edgeEdge", "12");
    set("elk.layered.spacing.edgeEdgeBetweenLayers", "12");
    set("elk.layered.spacing.edgeNodeBetweenLayers", "12");
    set("elk.layered.nodePlacement.bk.fixedAlignment", "BALANCED");
    set("elk.layered.spacing.nodeNodeBetweenLayers", "48");
    set("elk.spacing.nodeNode", "28");
    if let Some(dir) = direction {
        set("elk.direction", map_direction(dir));
    }
    opts
}

/// `_deepestSubgraph(for:in:)`.
fn deepest_subgraph<'a>(node_id: &str, subs: &'a [MermaidSubgraph]) -> Option<&'a str> {
    for sub in subs {
        if let Some(deeper) = deepest_subgraph(node_id, &sub.children) {
            return Some(deeper);
        }
        if strings_contain(&sub.node_ids, node_id) {
            return Some(&sub.id);
        }
    }
    None
}

/// `Set(subgraphOwnership.values.flatMap { $0 })`: every node any subgraph
/// claims, transitively.
fn all_claimed_nodes(subs: &[MermaidSubgraph]) -> SSet {
    fn visit(sub: &MermaidSubgraph, out: &mut SSet) {
        for id in &sub.node_ids {
            out.insert(id);
        }
        for child in &sub.children {
            visit(child, out);
        }
    }
    let mut out = SSet::new();
    for sub in subs {
        visit(sub, &mut out);
    }
    out
}

fn subgraph_contains_node(sub: &MermaidSubgraph, node_id: &str) -> bool {
    if strings_contain(&sub.node_ids, node_id) {
        return true;
    }
    sub.children.iter().any(|c| subgraph_contains_node(c, node_id))
}

fn direct_node_ids(sub: &MermaidSubgraph) -> Vec<&String> {
    sub.node_ids
        .iter()
        .filter(|node_id| !sub.children.iter().any(|child| subgraph_contains_node(child, node_id)))
        .collect()
}

/// `Dictionary(nodesInOrder.map { ($0.id, $0.node) }, uniquingKeysWith: { _, last in last })`.
fn node_by_id(graph: &ParsedGraph) -> SDict<MermaidNode> {
    let mut map = SDict::new();
    for (id, node) in &graph.nodes_in_order {
        map.insert(id, node.clone());
    }
    map
}

/// `_buildElkGraph(_:)`.
fn build_elk_graph(graph: &ParsedGraph) -> Value {
    if graph.subgraphs.is_empty() {
        // Fast path: flat graph
        let children: Vec<Value> = graph.nodes_in_order.iter().map(|(id, node)| node_child(id, node, true)).collect();
        let edges: Vec<Value> = graph.edges.iter().enumerate().map(|(idx, edge)| edge_dict(idx, edge)).collect();
        let mut root = Map::new();
        root.insert("id".into(), Value::from("root"));
        root.insert("layoutOptions".into(), Value::Object(root_layout_options(graph.direction, Some("INCLUDE_CHILDREN"), false)));
        root.insert("children".into(), Value::Array(children));
        root.insert("edges".into(), Value::Array(edges));
        return Value::Object(root);
    }

    let all_claimed = all_claimed_nodes(&graph.subgraphs);
    let nodes = node_by_id(graph);

    // Build nodeToSubgraph map (innermost subgraph for each node)
    let mut node_to_subgraph: SDict<String> = SDict::new();
    for (id, _) in &graph.nodes_in_order {
        if let Some(sg) = deepest_subgraph(id, &graph.subgraphs) {
            node_to_subgraph.insert(id, sg.to_owned());
        }
    }

    // Classify edges: internal (same subgraph), root (no subgraph), cross-hierarchy
    let mut edges_by_subgraph: SDict<Vec<Value>> = SDict::new();
    let mut root_only_edges: Vec<Value> = Vec::new();
    struct CrossEdge<'a> {
        idx: usize,
        edge: &'a MermaidEdge,
        src_sub: Option<String>,
        tgt_sub: Option<String>,
    }
    let mut cross_edges: Vec<CrossEdge> = Vec::new();

    for (idx, edge) in graph.edges.iter().enumerate() {
        let src_sub = node_to_subgraph.get(&edge.source).cloned();
        let tgt_sub = node_to_subgraph.get(&edge.target).cloned();
        match (&src_sub, &tgt_sub) {
            (Some(s), Some(t)) if swift::string_eq(s, t) => {
                edges_by_subgraph.entry_or(s, Vec::new).push(edge_dict(idx, edge));
            }
            (None, None) => root_only_edges.push(edge_dict(idx, edge)),
            _ => cross_edges.push(CrossEdge { idx, edge, src_sub, tgt_sub }),
        }
    }

    let mut root_edges = root_only_edges;

    // Build hierarchical ports for cross-hierarchy edges (SEPARATE mode)
    let mut ports_by_subgraph: SDict<Vec<(Value, Value)>> = SDict::new();

    for ce in &cross_edges {
        let idx = ce.idx;
        if let Some(src_sg) = &ce.src_sub {
            let port_id = format!("{src_sg}_out_{idx}");
            let port = json!({ "id": port_id });
            let mut internal_edge = Map::new();
            internal_edge.insert("id".into(), Value::from(format!("e{idx}_out")));
            internal_edge.insert("sources".into(), json!([ce.edge.source]));
            internal_edge.insert("targets".into(), json!([port_id]));
            if let Some(label) = &ce.edge.label {
                if !label.is_empty() {
                    internal_edge.insert("labels".into(), edge_label_json(label));
                }
            }
            ports_by_subgraph.entry_or(src_sg, Vec::new).push((port, Value::Object(internal_edge)));
        }

        if let Some(tgt_sg) = &ce.tgt_sub {
            let port_id = format!("{tgt_sg}_in_{idx}");
            let port = json!({ "id": port_id });
            let mut internal_edge = Map::new();
            internal_edge.insert("id".into(), Value::from(format!("e{idx}_in")));
            internal_edge.insert("sources".into(), json!([port_id]));
            internal_edge.insert("targets".into(), json!([ce.edge.target]));
            ports_by_subgraph.entry_or(tgt_sg, Vec::new).push((port, Value::Object(internal_edge)));
        }

        let src_id = ce.src_sub.as_ref().map_or_else(|| ce.edge.source.clone(), |s| format!("{s}_out_{idx}"));
        let tgt_id = ce.tgt_sub.as_ref().map_or_else(|| ce.edge.target.clone(), |t| format!("{t}_in_{idx}"));
        let mut root_edge = Map::new();
        root_edge.insert("id".into(), Value::from(format!("e{idx}")));
        root_edge.insert("sources".into(), json!([src_id]));
        root_edge.insert("targets".into(), json!([tgt_id]));
        if ce.src_sub.is_none() {
            if let Some(label) = &ce.edge.label {
                if !label.is_empty() {
                    root_edge.insert("labels".into(), edge_label_json(label));
                }
            }
        }
        root_edges.push(Value::Object(root_edge));
    }

    // Recursive builder for subgraph compound nodes
    fn build_subgraph_node(
        sub: &MermaidSubgraph,
        nodes: &SDict<MermaidNode>,
        ports_by_subgraph: &SDict<Vec<(Value, Value)>>,
        edges_by_subgraph: &SDict<Vec<Value>>,
    ) -> Value {
        let mut children: Vec<Value> = Vec::new();
        for node_id in direct_node_ids(sub) {
            let Some(node) = nodes.get(node_id) else { continue };
            children.push(node_child(node_id, node, true));
        }
        for child in &sub.children {
            children.push(build_subgraph_node(child, nodes, ports_by_subgraph, edges_by_subgraph));
        }

        // Add ports for cross-hierarchy edges
        let mut ports: Vec<Value> = Vec::new();
        let mut internal_edges: Vec<Value> = Vec::new();
        if let Some(pairs) = ports_by_subgraph.get(&sub.id) {
            for (port, edge) in pairs {
                ports.push(port.clone());
                internal_edges.push(edge.clone());
            }
        }

        let mut subgraph_edges = edges_by_subgraph.get(&sub.id).cloned().unwrap_or_default();
        subgraph_edges.extend(internal_edges);

        let mut result = Map::new();
        result.insert("id".into(), Value::from(sub.id.as_str()));
        result.insert("layoutOptions".into(), Value::Object(subgraph_layout_options(sub.direction)));
        result.insert("children".into(), Value::Array(children));
        result.insert("labels".into(), json!([{ "text": sub.label }]));
        if !ports.is_empty() {
            result.insert("ports".into(), Value::Array(ports));
        }
        if !subgraph_edges.is_empty() {
            result.insert("edges".into(), Value::Array(subgraph_edges));
        }
        Value::Object(result)
    }

    // Root children: top-level subgraphs + unclaimed nodes
    let mut root_children: Vec<Value> = Vec::new();
    for (id, node) in &graph.nodes_in_order {
        if !all_claimed.contains(id) {
            root_children.push(node_child(id, node, true));
        }
    }
    for sub in &graph.subgraphs {
        root_children.push(build_subgraph_node(sub, &nodes, &ports_by_subgraph, &edges_by_subgraph));
    }

    let mut root = Map::new();
    root.insert("id".into(), Value::from("root"));
    root.insert("layoutOptions".into(), Value::Object(root_layout_options(graph.direction, Some("SEPARATE"), false)));
    root.insert("children".into(), Value::Array(root_children));
    root.insert("edges".into(), Value::Array(root_edges));
    Value::Object(root)
}

/// `_buildElkGraphNoCrossEdges(_:)`: INCLUDE_CHILDREN mode, all
/// cross-hierarchy edges at the root.
fn build_elk_graph_no_cross_edges(graph: &ParsedGraph) -> Value {
    let all_claimed = all_claimed_nodes(&graph.subgraphs);
    let nodes = node_by_id(graph);

    let mut edges_by_subgraph: SDict<Vec<Value>> = SDict::new();
    let mut root_level_edges: Vec<Value> = Vec::new();
    let mut cross_hierarchy_edges: Vec<Value> = Vec::new();
    for (idx, edge) in graph.edges.iter().enumerate() {
        let src_sub = deepest_subgraph(&edge.source, &graph.subgraphs);
        let tgt_sub = deepest_subgraph(&edge.target, &graph.subgraphs);
        let edict = edge_dict(idx, edge);
        match (src_sub, tgt_sub) {
            (Some(s), Some(t)) if swift::string_eq(s, t) => edges_by_subgraph.entry_or(s, Vec::new).push(edict),
            (None, None) => root_level_edges.push(edict),
            _ => cross_hierarchy_edges.push(edict),
        }
    }
    // Match TS ordering: root-level edges first, then cross-hierarchy
    let mut root_edges = root_level_edges;
    root_edges.extend(cross_hierarchy_edges);

    fn build_subgraph_node(sub: &MermaidSubgraph, nodes: &SDict<MermaidNode>, edges_by_subgraph: &SDict<Vec<Value>>) -> Value {
        let mut children: Vec<Value> = Vec::new();
        for node_id in direct_node_ids(sub) {
            let Some(node) = nodes.get(node_id) else { continue };
            children.push(node_child(node_id, node, true));
        }
        for child in &sub.children {
            children.push(build_subgraph_node(child, nodes, edges_by_subgraph));
        }

        let mut result = Map::new();
        result.insert("id".into(), Value::from(sub.id.as_str()));
        result.insert("layoutOptions".into(), Value::Object(subgraph_layout_options(sub.direction)));
        result.insert("children".into(), Value::Array(children));
        result.insert("labels".into(), json!([{ "text": sub.label }]));
        if let Some(sub_edges) = edges_by_subgraph.get(&sub.id) {
            if !sub_edges.is_empty() {
                result.insert("edges".into(), Value::Array(sub_edges.clone()));
            }
        }
        Value::Object(result)
    }

    let mut root_children: Vec<Value> = Vec::new();
    for (id, node) in &graph.nodes_in_order {
        if !all_claimed.contains(id) {
            root_children.push(node_child(id, node, true));
        }
    }
    for sub in &graph.subgraphs {
        root_children.push(build_subgraph_node(sub, &nodes, &edges_by_subgraph));
    }

    let mut root = Map::new();
    root.insert("id".into(), Value::from("root"));
    root.insert("layoutOptions".into(), Value::Object(root_layout_options(graph.direction, Some("INCLUDE_CHILDREN"), false)));
    root.insert("children".into(), Value::Array(root_children));
    root.insert("edges".into(), Value::Array(root_edges));
    Value::Object(root)
}

/// `_buildFlatElkGraph(_:)`.
fn build_flat_elk_graph(graph: &ParsedGraph) -> Value {
    let children: Vec<Value> = graph.nodes_in_order.iter().map(|(id, node)| node_child(id, node, false)).collect();
    let edges: Vec<Value> = graph.edges.iter().enumerate().map(|(idx, edge)| edge_dict(idx, edge)).collect();
    let mut root = Map::new();
    root.insert("id".into(), Value::from("root"));
    root.insert("layoutOptions".into(), Value::Object(root_layout_options(graph.direction, None, true)));
    root.insert("children".into(), Value::Array(children));
    root.insert("edges".into(), Value::Array(edges));
    Value::Object(root)
}

/// `_applyLayoutConfig(_:to:)`.
fn apply_layout_config(config: &LayoutConfig, elk_graph: &mut Value) {
    let Some(root) = elk_graph.as_object_mut() else { return };
    let mut opts = match root.get("layoutOptions") {
        Some(Value::Object(o)) if o.values().all(Value::is_string) => o.clone(),
        _ => Map::new(),
    };
    let p = swift::int(config.padding);
    opts.insert("elk.spacing.nodeNode".into(), Value::from(swift::int(config.node_spacing).to_string()));
    opts.insert("elk.layered.spacing.nodeNodeBetweenLayers".into(), Value::from(swift::int(config.layer_spacing).to_string()));
    opts.insert("elk.padding".into(), Value::from(format!("[top={p},left={p},bottom={p},right={p}]")));
    opts.insert("elk.spacing.componentComponent".into(), Value::from(swift::int(config.component_spacing).to_string()));
    root.insert("layoutOptions".into(), Value::Object(opts));
}

/// `layoutGraphSync(_:config:)`.
pub fn layout_graph_sync(graph: &MermaidGraph, config: &LayoutConfig) -> Result<PositionedGraph, MermaidError> {
    let Payload::Flow(parsed) = &graph.payload else {
        return Ok(PositionedGraph::empty(graph.clone(), 0.0, 0.0));
    };

    let mut elk_graph = if !parsed.subgraphs.is_empty() {
        let has_direction_override = parsed.subgraphs.iter().any(|s| s.direction.is_some());
        if has_direction_override { build_elk_graph(parsed) } else { build_elk_graph_no_cross_edges(parsed) }
    } else {
        build_elk_graph(parsed)
    };

    // Override ELK spacing options with LayoutConfig values
    apply_layout_config(config, &mut elk_graph);

    match elk_layout_sync(&elk_graph) {
        Ok(laid_out) => Ok(extract_positioned_graph(parsed, &laid_out, graph.diagram_type)),
        Err(_) => {
            let mut flat_graph = build_flat_elk_graph(parsed);
            apply_layout_config(config, &mut flat_graph);
            let laid_out = elk_layout_sync(&flat_graph)?;
            Ok(extract_positioned_graph(parsed, &laid_out, graph.diagram_type))
        }
    }
}

/// The ELK input graph `layoutGraphSync(_:config:)` hands ELK first.
pub fn elk_input_graph(parsed: &ParsedGraph, config: &LayoutConfig) -> Value {
    let mut elk_graph = if !parsed.subgraphs.is_empty() {
        let has_direction_override = parsed.subgraphs.iter().any(|s| s.direction.is_some());
        if has_direction_override { build_elk_graph(parsed) } else { build_elk_graph_no_cross_edges(parsed) }
    } else {
        build_elk_graph(parsed)
    };
    apply_layout_config(config, &mut elk_graph);
    elk_graph
}

// MARK: - Extraction

/// `_collectAllChildren(_:nodeById:parentOffset:)`.
fn collect_all_children<'a>(
    elk_node: &'a Map<String, Value>,
    node_by_id: &SDict<MermaidNode>,
    parent_offset: (f64, f64),
    result: &mut Vec<(&'a Map<String, Value>, (f64, f64))>,
) {
    for child in as_dict_array(elk_node.get("children")) {
        let Some(id) = as_string(child.get("id")) else { continue };
        if node_by_id.contains_key(id) && as_dict_array(child.get("children")).is_empty() {
            // Leaf node
            result.push((child, parent_offset));
        } else {
            // Compound node — recurse into its children with accumulated offset
            let cx = as_double(child.get("x")).unwrap_or(0.0) + parent_offset.0;
            let cy = as_double(child.get("y")).unwrap_or(0.0) + parent_offset.1;
            collect_all_children(child, node_by_id, (cx, cy), result);
        }
    }
}

#[derive(Default, Clone)]
struct EdgeSegments {
    external: Option<Vec<P>>,
    incoming: Option<Vec<P>>,
    outgoing: Option<Vec<P>>,
    label_position: Option<P>,
}

/// `_collectEdgeSegments(_:segments:offsetX:offsetY:)`.
fn collect_edge_segments(elk_node: &Map<String, Value>, segments: &mut HashMap<i64, EdgeSegments>, offset_x: f64, offset_y: f64) {
    for elk_edge in as_dict_array(elk_node.get("edges")) {
        let Some(eid) = as_string(elk_edge.get("id")) else { continue };

        // Parse edge ID
        let is_out = swift::has_suffix(eid, "_out");
        let is_in = swift::has_suffix(eid, "_in");
        let is_internal = swift::has_suffix(eid, "_internal");
        let index_str = if is_out {
            swift::drop_last(swift::drop_first(eid, 1), 4)
        } else if is_in {
            swift::drop_last(swift::drop_first(eid, 1), 3)
        } else if is_internal {
            swift::drop_last(swift::drop_first(eid, 1), 9)
        } else {
            swift::drop_first(eid, 1)
        };
        let Some(edge_index) = swift::parse_int(index_str) else { continue };

        // Extract points from sections
        let mut points: Vec<P> = Vec::new();
        if let Some(section) = as_dict_array(elk_edge.get("sections")).first() {
            if let Some(s) = as_dict(section.get("startPoint")) {
                points.push(P {
                    x: as_double(s.get("x")).unwrap_or(0.0) + offset_x,
                    y: as_double(s.get("y")).unwrap_or(0.0) + offset_y,
                });
            }
            for bp in as_dict_array(section.get("bendPoints")) {
                points.push(P {
                    x: as_double(bp.get("x")).unwrap_or(0.0) + offset_x,
                    y: as_double(bp.get("y")).unwrap_or(0.0) + offset_y,
                });
            }
            if let Some(e) = as_dict(section.get("endPoint")) {
                points.push(P {
                    x: as_double(e.get("x")).unwrap_or(0.0) + offset_x,
                    y: as_double(e.get("y")).unwrap_or(0.0) + offset_y,
                });
            }
        }

        // Extract label position
        let mut label_pos: Option<P> = None;
        if let Some(label) = as_dict_array(elk_edge.get("labels")).first() {
            if let (Some(lx), Some(ly)) = (as_double(label.get("x")), as_double(label.get("y"))) {
                let lw = as_double(label.get("width")).unwrap_or(0.0);
                let lh = as_double(label.get("height")).unwrap_or(0.0);
                label_pos = Some(P { x: lx + lw / 2.0 + offset_x, y: ly + lh / 2.0 + offset_y });
            }
        }

        // Store segment
        let seg = segments.entry(edge_index).or_default();

        if is_out {
            seg.outgoing = Some(points);
        } else if is_in {
            seg.incoming = Some(points);
        } else if is_internal {
            let src = elk_edge
                .get("sources")
                .and_then(Value::as_array)
                .filter(|a| a.iter().all(Value::is_string))
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .unwrap_or("");
            if swift::contains(src, "_in_") || swift::contains(src, "_out_") {
                seg.incoming = Some(points);
            } else {
                seg.outgoing = Some(points);
            }
        } else {
            seg.external = Some(points);
            if let Some(lp) = label_pos {
                seg.label_position = Some(lp);
            }
        }
    }

    // Recurse into compound children with accumulated offset
    for child in as_dict_array(elk_node.get("children")) {
        if !as_dict_array(child.get("children")).is_empty() {
            let cx = as_double(child.get("x")).unwrap_or(0.0) + offset_x;
            let cy = as_double(child.get("y")).unwrap_or(0.0) + offset_y;
            collect_edge_segments(child, segments, cx, cy);
        }
    }
}

/// `_flattenGroupBounds(_:)`.
fn flatten_group_bounds(groups: &[PositionedGroupPayload], out: &mut Vec<PositionedGroupPayload>) {
    for g in groups {
        out.push(g.clone());
        flatten_group_bounds(&g.children, out);
    }
}

/// `_orthogonalizeEdgePoints(_:margins:edgeIndex:)`: `(points, changed)`.
fn orthogonalize_edge_points(points: &[P], margins: Option<(f64, f64)>, edge_index: i64) -> (Vec<P>, bool) {
    if points.len() < 2 {
        return (points.to_vec(), false);
    }

    let mut needs_work = false;
    for i in 1..points.len() {
        let dx = (points[i].x - points[i - 1].x).abs();
        let dy = (points[i].y - points[i - 1].y).abs();
        if dx > 1.0 && dy > 1.0 {
            needs_work = true;
            break;
        }
    }
    if !needs_work {
        return (points.to_vec(), false);
    }

    let edge_spacing = 12.0;
    let mut result: Vec<P> = vec![points[0]];

    for curr in &points[1..] {
        let prev = result[result.len() - 1];
        let curr = *curr;
        let dx = (curr.x - prev.x).abs();
        let dy = (curr.y - prev.y).abs();

        if dx > 1.0 && dy > 1.0 {
            if let Some((left_x, right_x)) = margins {
                // Margin routing: exit horizontally → travel vertically along margin → enter horizontally
                let use_right = edge_index % 2 == 0;
                let offset = (edge_index / 2) as f64 * edge_spacing;
                let margin_x = if use_right { right_x + offset } else { left_x - offset };
                result.push(P { x: margin_x, y: prev.y });
                result.push(P { x: margin_x, y: curr.y });
            } else {
                // Fallback: Z-path through vertical midpoint
                let mid_y = (prev.y + curr.y) / 2.0;
                result.push(P { x: prev.x, y: mid_y });
                result.push(P { x: curr.x, y: mid_y });
            }
        }
        result.push(curr);
    }
    (result, true)
}

fn flow_pos(node: &PositionedNodePayload, horizontal: bool) -> f64 {
    if horizontal { node.x } else { node.y }
}

/// `_alignLayerNodes(_:_:_:)`: snap same-layer nodes to uniform positions
/// along the flow axis.
fn align_layer_nodes(nodes: &mut [PositionedNodePayload], edges: &mut [PositionedEdgePayload], direction: Direction) {
    if nodes.is_empty() {
        return;
    }

    let is_horizontal = direction == Direction::LR || direction == Direction::RL;
    let layer_spacing = 48.0;
    let threshold = layer_spacing * 0.6;

    // Build connected pairs set
    let mut connected_pairs = SSet::new();
    for edge in edges.iter() {
        connected_pairs.insert(&format!("{}:{}", edge.source, edge.target));
        connected_pairs.insert(&format!("{}:{}", edge.target, edge.source));
    }

    // Sort nodes by flow-axis position
    let sorted = swift::sorted_by(nodes.iter().cloned(), |a, b| {
        if is_horizontal { a.x < b.x } else { a.y < b.y }
    });

    // Cluster into layers using single-linkage with connected-pair exclusion
    let mut layers: Vec<Vec<usize>> = Vec::new();
    let mut node_index_map: SDict<usize> = SDict::new();
    for (i, n) in nodes.iter().enumerate() {
        node_index_map.insert(&n.id, i);
    }
    let sorted_indices: Vec<usize> = sorted.iter().filter_map(|n| node_index_map.get(&n.id).copied()).collect();

    let mut current_layer: Vec<usize> = vec![sorted_indices[0]];
    for i in 1..sorted_indices.len() {
        let idx = sorted_indices[i];
        let prev_idx = sorted_indices[i - 1];
        let pos = flow_pos(&nodes[idx], is_horizontal);
        let prev_pos = flow_pos(&nodes[prev_idx], is_horizontal);
        let gap = pos - prev_pos;

        let has_edge_to_layer = current_layer
            .iter()
            .any(|&layer_idx| connected_pairs.contains(&format!("{}:{}", nodes[layer_idx].id, nodes[idx].id)));

        if gap <= threshold && !has_edge_to_layer {
            current_layer.push(idx);
        } else {
            layers.push(std::mem::take(&mut current_layer));
            current_layer = vec![idx];
        }
    }
    layers.push(current_layer);

    // Snap each layer's nodes to the center
    let mut deltas: SDict<f64> = SDict::new();
    for layer in &layers {
        if layer.len() <= 1 {
            continue;
        }
        let positions: Vec<f64> = layer.iter().map(|&i| flow_pos(&nodes[i], is_horizontal)).collect();
        let min_pos = swift::seq_min(positions.iter().copied()).unwrap_or(positions[0]);
        let max_pos = swift::seq_max(positions.iter().copied()).unwrap_or(positions[0]);
        if !(max_pos - min_pos > 1.0) {
            continue;
        }

        let target = (min_pos + max_pos) / 2.0;
        for &idx in layer {
            let old_pos = flow_pos(&nodes[idx], is_horizontal);
            let delta = target - old_pos;
            if delta.abs() > 0.5 {
                if is_horizontal {
                    nodes[idx].x = target;
                } else {
                    nodes[idx].y = target;
                }
                let id = nodes[idx].id.clone();
                deltas.insert(&id, delta);
            }
        }
    }

    if deltas.is_empty() {
        return;
    }

    // Adjust edge endpoints to match shifted nodes
    for edge in edges.iter_mut() {
        if edge.points.len() < 2 {
            continue;
        }

        if let Some(&src_delta) = deltas.get(&edge.source) {
            if is_horizontal {
                let old_x = edge.points[0].x;
                edge.points[0].x += src_delta;
                if edge.points.len() > 1 && edge.points[1].x == old_x {
                    edge.points[1].x += src_delta;
                }
            } else {
                let old_y = edge.points[0].y;
                edge.points[0].y += src_delta;
                if edge.points.len() > 1 && edge.points[1].y == old_y {
                    edge.points[1].y += src_delta;
                }
            }
        }

        if let Some(&tgt_delta) = deltas.get(&edge.target) {
            let last_idx = edge.points.len() - 1;
            if is_horizontal {
                let old_x = edge.points[last_idx].x;
                edge.points[last_idx].x += tgt_delta;
                if last_idx > 0 && edge.points[last_idx - 1].x == old_x {
                    edge.points[last_idx - 1].x += tgt_delta;
                }
            } else {
                let old_y = edge.points[last_idx].y;
                edge.points[last_idx].y += tgt_delta;
                if last_idx > 0 && edge.points[last_idx - 1].y == old_y {
                    edge.points[last_idx - 1].y += tgt_delta;
                }
            }
        }
    }
}

/// `Dictionary(nodes.map { ($0.id, $0) }, uniquingKeysWith: { _, last in last })`.
fn node_map(nodes: &[PositionedNodePayload]) -> SDict<PositionedNodePayload> {
    let mut map = SDict::new();
    for n in nodes {
        map.insert(&n.id, n.clone());
    }
    map
}

/// `_bundleEdgePaths(_:_:_:_:)`: fan-out and fan-in edges share a trunk.
/// Every edge belongs to at most one fan-out and one fan-in group, and no
/// group reads another group's points, so the Swift `Dictionary` iteration
/// order does not affect the result; groups are taken in first-seen order.
fn bundle_edge_paths(edges: &mut [PositionedEdgePayload], nodes: &[PositionedNodePayload], groups: &[PositionedGroupPayload], direction: Direction) {
    let node_map = node_map(nodes);
    let mut processed: std::collections::HashSet<usize> = std::collections::HashSet::new();

    let is_lr = direction == Direction::LR;
    let is_rl = direction == Direction::RL;
    let is_bt = direction == Direction::BT;
    let is_horizontal = is_lr || is_rl;

    // --- Fan-out: group edges by shared source ---
    let mut fan_out_order: Vec<String> = Vec::new();
    let mut fan_out_groups: SDict<Vec<usize>> = SDict::new();
    for (i, edge) in edges.iter().enumerate() {
        if swift::string_eq(&edge.source, &edge.target) {
            continue;
        }
        if !fan_out_groups.contains_key(&edge.source) {
            fan_out_order.push(edge.source.clone());
        }
        fan_out_groups.entry_or(&edge.source, Vec::new).push(i);
    }

    for source_id in &fan_out_order {
        let group = fan_out_groups.get(source_id).unwrap().clone();
        if group.len() < 2 {
            continue;
        }
        let style = edges[group[0]].style.clone();
        if group.iter().any(|&i| edges[i].label.is_some() || edges[i].style != style) {
            continue;
        }
        let Some(source) = node_map.get(source_id) else { continue };

        let forward: Vec<usize> = group
            .iter()
            .copied()
            .filter(|&idx| {
                let Some(t) = node_map.get(&edges[idx].target) else { return false };
                if is_lr {
                    return t.x > source.x + source.width;
                }
                if is_rl {
                    return t.x + t.width < source.x;
                }
                // y=0 at top: TD forward = target has higher y; BT forward = target has lower y
                if is_bt {
                    return t.y + t.height < source.y;
                }
                t.y > source.y + source.height // TD
            })
            .collect();
        if forward.len() < 2 {
            continue;
        }

        let src_cx = source.x + source.width / 2.0;
        let src_cy = source.y + source.height / 2.0;

        if is_horizontal {
            let exit_x = if is_lr { source.x + source.width } else { source.x };
            let exit_y = src_cy;
            let nearest_x = if is_lr {
                swift::seq_min(forward.iter().filter_map(|&i| node_map.get(&edges[i].target).map(|n| n.x))).unwrap_or(exit_x)
            } else {
                swift::seq_max(forward.iter().filter_map(|&i| node_map.get(&edges[i].target)).map(|n| n.x + n.width)).unwrap_or(exit_x)
            };
            let junction_x =
                adjust_junction_for_groups(exit_x + (nearest_x - exit_x) / 2.0, src_cx, src_cy, groups, direction);
            for &idx in &forward {
                let Some(target) = node_map.get(&edges[idx].target) else { continue };
                let entry_x = if is_lr { target.x } else { target.x + target.width };
                let entry_y = target.y + target.height / 2.0;
                edges[idx].points = vec![
                    P { x: exit_x, y: exit_y },
                    P { x: junction_x, y: exit_y },
                    P { x: junction_x, y: entry_y },
                    P { x: entry_x, y: entry_y },
                ];
                processed.insert(idx);
            }
        } else {
            let exit_x = src_cx;
            // y=0 at top: TD exit at bottom of node = node.y + height
            let exit_y = if is_bt { source.y } else { source.y + source.height };
            let nearest_y = if is_bt {
                swift::seq_max(forward.iter().filter_map(|&i| node_map.get(&edges[i].target)).map(|n| n.y + n.height)).unwrap_or(exit_y)
            } else {
                swift::seq_min(forward.iter().filter_map(|&i| node_map.get(&edges[i].target).map(|n| n.y))).unwrap_or(exit_y)
            };
            let junction_y =
                adjust_junction_for_groups(exit_y + (nearest_y - exit_y) / 2.0, src_cx, src_cy, groups, direction);
            for &idx in &forward {
                let Some(target) = node_map.get(&edges[idx].target) else { continue };
                let entry_x = target.x + target.width / 2.0;
                // y=0 at top: TD enter at top of node = node.y
                let entry_y = if is_bt { target.y + target.height } else { target.y };
                edges[idx].points = vec![
                    P { x: exit_x, y: exit_y },
                    P { x: exit_x, y: junction_y },
                    P { x: entry_x, y: junction_y },
                    P { x: entry_x, y: entry_y },
                ];
                processed.insert(idx);
            }
        }
    }

    // --- Fan-in: group edges by shared target (skip already-bundled) ---
    let mut fan_in_order: Vec<String> = Vec::new();
    let mut fan_in_groups: SDict<Vec<usize>> = SDict::new();
    for (i, edge) in edges.iter().enumerate() {
        if processed.contains(&i) || swift::string_eq(&edge.source, &edge.target) {
            continue;
        }
        if !fan_in_groups.contains_key(&edge.target) {
            fan_in_order.push(edge.target.clone());
        }
        fan_in_groups.entry_or(&edge.target, Vec::new).push(i);
    }

    for target_id in &fan_in_order {
        let group = fan_in_groups.get(target_id).unwrap().clone();
        if group.len() < 2 {
            continue;
        }
        let style = edges[group[0]].style.clone();
        if group.iter().any(|&i| edges[i].label.is_some() || edges[i].style != style) {
            continue;
        }
        let Some(target) = node_map.get(target_id) else { continue };

        // y=0 at top: TD "forward" means source is above target (source has lower y)
        let forward: Vec<usize> = group
            .iter()
            .copied()
            .filter(|&idx| {
                let Some(s) = node_map.get(&edges[idx].source) else { return false };
                if is_lr {
                    return s.x + s.width < target.x;
                }
                if is_rl {
                    return s.x > target.x + target.width;
                }
                if is_bt {
                    return s.y > target.y + target.height;
                }
                s.y + s.height < target.y // TD
            })
            .collect();
        if forward.len() < 2 {
            continue;
        }

        let tgt_cx = target.x + target.width / 2.0;
        let tgt_cy = target.y + target.height / 2.0;

        if is_horizontal {
            let entry_x = if is_lr { target.x } else { target.x + target.width };
            let entry_y = tgt_cy;
            let farthest_x = if is_lr {
                swift::seq_max(forward.iter().filter_map(|&i| node_map.get(&edges[i].source)).map(|n| n.x + n.width)).unwrap_or(entry_x)
            } else {
                swift::seq_min(forward.iter().filter_map(|&i| node_map.get(&edges[i].source).map(|n| n.x))).unwrap_or(entry_x)
            };
            let junction_x =
                adjust_junction_for_groups(farthest_x + (entry_x - farthest_x) / 2.0, tgt_cx, tgt_cy, groups, direction);
            for &idx in &forward {
                let Some(src) = node_map.get(&edges[idx].source) else { continue };
                let exit_x = if is_lr { src.x + src.width } else { src.x };
                let exit_y = src.y + src.height / 2.0;
                edges[idx].points = vec![
                    P { x: exit_x, y: exit_y },
                    P { x: junction_x, y: exit_y },
                    P { x: junction_x, y: entry_y },
                    P { x: entry_x, y: entry_y },
                ];
            }
        } else {
            let entry_x = tgt_cx;
            // y=0 at top: TD enter at top = node.y
            let entry_y = if is_bt { target.y + target.height } else { target.y };
            let farthest_y = if is_bt {
                swift::seq_min(forward.iter().filter_map(|&i| node_map.get(&edges[i].source).map(|n| n.y))).unwrap_or(entry_y)
            } else {
                swift::seq_max(forward.iter().filter_map(|&i| node_map.get(&edges[i].source)).map(|n| n.y + n.height)).unwrap_or(entry_y)
            };
            let junction_y =
                adjust_junction_for_groups(farthest_y + (entry_y - farthest_y) / 2.0, tgt_cx, tgt_cy, groups, direction);
            for &idx in &forward {
                let Some(src) = node_map.get(&edges[idx].source) else { continue };
                let exit_x = src.x + src.width / 2.0;
                // y=0 at top: TD exit at bottom = src.y + height
                let exit_y = if is_bt { src.y } else { src.y + src.height };
                edges[idx].points = vec![
                    P { x: exit_x, y: exit_y },
                    P { x: exit_x, y: junction_y },
                    P { x: entry_x, y: junction_y },
                    P { x: entry_x, y: entry_y },
                ];
            }
        }
    }
}

/// `_adjustJunctionForGroups(_:refX:refY:groups:direction:)`.
fn adjust_junction_for_groups(junction_main: f64, ref_x: f64, ref_y: f64, groups: &[PositionedGroupPayload], direction: Direction) -> f64 {
    let gap = 12.0;
    let is_lr = direction == Direction::LR;
    let is_rl = direction == Direction::RL;
    let is_bt = direction == Direction::BT;
    let is_horizontal = is_lr || is_rl;

    let mut ref_groups = Vec::new();
    find_groups_containing_point(ref_x, ref_y, groups, &mut ref_groups);
    let mut ref_group_ids = SSet::new();
    for g in &ref_groups {
        ref_group_ids.insert(&g.id);
    }
    let probe_x = if is_horizontal { junction_main } else { ref_x };
    let probe_y = if is_horizontal { ref_y } else { junction_main };
    let mut junction_groups = Vec::new();
    find_groups_containing_point(probe_x, probe_y, groups, &mut junction_groups);

    let Some(crossing_group) = junction_groups.iter().find(|g| !ref_group_ids.contains(&g.id)) else {
        return junction_main;
    };

    if is_lr {
        return crossing_group.x - gap;
    }
    if is_rl {
        return crossing_group.x + crossing_group.width + gap;
    }
    // y=0 at top: TD "above" = smaller y; BT "above" = larger y
    if is_bt {
        return crossing_group.y + crossing_group.height + gap;
    }
    crossing_group.y - gap // TD: above group = smaller y
}

/// `_findGroupsContainingPoint(_:_:_:)`.
fn find_groups_containing_point<'a>(x: f64, y: f64, groups: &'a [PositionedGroupPayload], result: &mut Vec<&'a PositionedGroupPayload>) {
    for group in groups {
        if x >= group.x && x <= group.x + group.width && y >= group.y && y <= group.y + group.height {
            result.push(group);
            find_groups_containing_point(x, y, &group.children, result);
        }
    }
}

/// `_extractSubgraphGroups(_:source:graphHeight:parentOffset:)`.
fn extract_subgraph_groups(elk_node: &Map<String, Value>, source: &ParsedGraph, subgraph_ids: &SSet, parent_offset: (f64, f64)) -> Vec<PositionedGroupPayload> {
    let mut groups = Vec::new();
    for child in as_dict_array(elk_node.get("children")) {
        let Some(id) = as_string(child.get("id")) else { continue };
        if !subgraph_ids.contains(id) {
            continue;
        }
        let raw_x = as_double(child.get("x")).unwrap_or(0.0) + parent_offset.0;
        let raw_y = as_double(child.get("y")).unwrap_or(0.0) + parent_offset.1;
        let w = as_double(child.get("width")).unwrap_or(0.0);
        let h = as_double(child.get("height")).unwrap_or(0.0);
        let label = find_subgraph_label(id, &source.subgraphs).unwrap_or(id).to_owned();
        let child_groups = extract_subgraph_groups(child, source, subgraph_ids, (raw_x, raw_y));
        groups.push(PositionedGroupPayload {
            id: id.to_owned(),
            label,
            x: raw_x,
            y: raw_y,
            width: w,
            height: h,
            header_height: 28.0,
            children: child_groups,
        });
    }
    groups
}

fn all_subgraph_ids(subs: &[MermaidSubgraph], out: &mut SSet) {
    for sub in subs {
        out.insert(&sub.id);
        all_subgraph_ids(&sub.children, out);
    }
}

fn find_subgraph_label<'a>(id: &str, subs: &'a [MermaidSubgraph]) -> Option<&'a str> {
    for sub in subs {
        if swift::string_eq(&sub.id, id) {
            return Some(&sub.label);
        }
        if let Some(found) = find_subgraph_label(id, &sub.children) {
            return Some(found);
        }
    }
    None
}

/// `_resolveInlineStyle(_:_:)`.
fn resolve_inline_style(id: &str, graph: &ParsedGraph) -> SDict<String> {
    let mut style = SDict::new();
    if let Some(class_name) = graph.class_assignments.get(id) {
        if let Some(class_style) = graph.class_defs.get(class_name) {
            for (k, v) in class_style.iter() {
                style.insert(k, v.clone());
            }
        }
    }
    if let Some(node_style) = graph.node_styles.get(id) {
        for (k, v) in node_style.iter() {
            style.insert(k, v.clone());
        }
    }
    style
}

/// `_resolveEdgeStyle(edgeIndex:graph:)`: `default` first, then the
/// index-specific overrides.
fn resolve_edge_style(edge_index: i64, graph: &ParsedGraph) -> Option<SDict<String>> {
    let mut result: Option<SDict<String>> = graph.link_styles.get(&-1).cloned();
    if let Some(index_style) = graph.link_styles.get(&edge_index) {
        match &mut result {
            Some(r) => {
                for (k, v) in index_style.iter() {
                    r.insert(k, v.clone());
                }
            }
            None => result = Some(index_style.clone()),
        }
    }
    result
}

/// `_extractPositionedGraph(_:_:diagramType:)`.
fn extract_positioned_graph(source: &ParsedGraph, laid_out: &Value, diagram_type: DiagramType) -> PositionedGraph {
    let empty = Map::new();
    let root = laid_out.as_object().unwrap_or(&empty);
    let node_by_id = node_by_id(source);
    let graph_height = as_double(root.get("height")).unwrap_or(0.0);

    // Collect nodes from root and all compound children (subgraphs) recursively
    let mut all_children = Vec::new();
    collect_all_children(root, &node_by_id, (0.0, 0.0), &mut all_children);

    // ELK coordinates (y=0 at top) — rendering handles CGContext flip
    let mut nodes: Vec<PositionedNodePayload> = all_children
        .iter()
        .filter_map(|(child, parent_offset)| {
            let id = as_string(child.get("id"))?;
            let original = node_by_id.get(id)?;
            let w = as_double(child.get("width")).unwrap_or_else(|| node_size(original).0);
            let h = as_double(child.get("height")).unwrap_or_else(|| node_size(original).1);
            let raw_x = as_double(child.get("x")).unwrap_or(0.0) + parent_offset.0;
            let raw_y = as_double(child.get("y")).unwrap_or(0.0) + parent_offset.1;
            Some(PositionedNodePayload {
                id: id.to_owned(),
                label: original.label.clone(),
                shape: original.shape.raw_value().to_owned(),
                x: raw_x,
                y: raw_y,
                width: w,
                height: h,
                inline_style: resolve_inline_style(id, source),
            })
        })
        .collect();

    // Collect edge segments from all levels (root + subgraphs) with coordinate offsets.
    let mut segments_by_index: HashMap<i64, EdgeSegments> = HashMap::new();
    collect_edge_segments(root, &mut segments_by_index, 0.0, 0.0);

    // Extract subgraph groups — needed for margin routing
    let mut subgraph_ids = SSet::new();
    all_subgraph_ids(&source.subgraphs, &mut subgraph_ids);
    let mut groups = extract_subgraph_groups(root, source, &subgraph_ids, (0.0, 0.0));

    // Margins sit outside all group bounding boxes so edges don't cross through subgraphs.
    let mut all_bounds = Vec::new();
    flatten_group_bounds(&groups, &mut all_bounds);
    let margins: Option<(f64, f64)> = if all_bounds.is_empty() {
        None
    } else {
        Some((
            swift::seq_min(all_bounds.iter().map(|g| g.x)).unwrap_or(0.0) - 20.0,
            swift::seq_max(all_bounds.iter().map(|g| g.x + g.width)).unwrap_or(0.0) + 20.0,
        ))
    };

    // Track margin-routed edge count for spacing offsets (matching TS marginEdgeIndex)
    let mut margin_edge_index: i64 = 0;

    let mut edges: Vec<PositionedEdgePayload> = Vec::new();
    for (idx, edge) in source.edges.iter().enumerate() {
        // outgoing (source→exit port) + external (exit port→entry port) + incoming (entry port→target)
        let seg = segments_by_index.get(&(idx as i64));
        let mut points: Vec<P> = Vec::new();

        if let Some(outgoing) = seg.and_then(|s| s.outgoing.as_ref()) {
            if !outgoing.is_empty() {
                points.extend_from_slice(outgoing);
            }
        }

        if let Some(external) = seg.and_then(|s| s.external.as_ref()) {
            if !external.is_empty() {
                if !points.is_empty() {
                    points.extend_from_slice(&external[1..]);
                } else {
                    points.extend_from_slice(external);
                }
            }
        }

        if let Some(incoming) = seg.and_then(|s| s.incoming.as_ref()) {
            if !incoming.is_empty() {
                if !points.is_empty() {
                    points.extend_from_slice(&incoming[1..]);
                } else {
                    points.extend_from_slice(incoming);
                }
            }
        }

        let label_pos = seg.and_then(|s| s.label_position);

        let (ortho_points, changed) = orthogonalize_edge_points(&points, margins, margin_edge_index);
        if changed {
            points = ortho_points;
            margin_edge_index += 1;
        }

        let mut final_label_pos: Option<P> = None;
        if edge.label.is_some() && !points.is_empty() {
            final_label_pos = if changed { Some(edge_path_midpoint(&points, source.direction)) } else { label_pos };
        }

        edges.push(PositionedEdgePayload {
            source: edge.source.clone(),
            target: edge.target.clone(),
            label: edge.label.clone(),
            style: edge.style.raw_value().to_owned(),
            has_arrow_start: edge.has_arrow_start,
            has_arrow_end: edge.has_arrow_end,
            points,
            label_position: final_label_pos,
            inline_style: resolve_edge_style(idx as i64, source),
        });
    }

    // Layer alignment: snap same-layer nodes to uniform positions
    align_layer_nodes(&mut nodes, &mut edges, source.direction);

    // Bundle fan-out/fan-in edge paths into shared trunks
    bundle_edge_paths(&mut edges, &nodes, &groups, source.direction);

    // Shape clipping: adjust edge endpoints to actual shape boundaries
    let nm = node_map(&nodes);
    for edge in edges.iter_mut() {
        if edge.points.len() < 2 {
            continue;
        }
        if let Some(src_node) = nm.get(&edge.source) {
            edge.points = clip_edge_to_shape(&edge.points, src_node, true);
        }
        if let Some(tgt_node) = nm.get(&edge.target) {
            edge.points = clip_edge_to_shape(&edge.points, tgt_node, false);
        }
    }

    // Compute label positions for edges that don't have an ELK-provided position
    for edge in edges.iter_mut() {
        if let Some(label) = &edge.label {
            if !label.is_empty() && edge.points.len() >= 2 && edge.label_position.is_none() {
                edge.label_position = Some(edge_path_midpoint(&edge.points, source.direction));
            }
        }
    }

    // Calculate final bounds including all edge points and labels
    let mut min_x: f64 = 0.0;
    let mut min_y: f64 = 0.0;
    let mut max_x = as_double(root.get("width")).unwrap_or(0.0);
    let mut max_y = graph_height;
    let arrow_margin = 10.0;
    let padding = 40.0;
    let label_half_w = 60.0; // estimated half-width of label pill
    let label_half_h = 16.0; // estimated half-height of label pill
    for edge in &edges {
        for p in &edge.points {
            max_x = max(max_x, p.x + arrow_margin + padding);
            max_y = max(max_y, p.y + arrow_margin + padding);
        }
        if let Some(lp) = edge.label_position {
            min_x = min(min_x, lp.x - label_half_w - padding);
            min_y = min(min_y, lp.y - label_half_h - padding);
            max_x = max(max_x, lp.x + label_half_w + padding);
            max_y = max(max_y, lp.y + label_half_h + padding);
        }
    }

    // If any label extends past origin, shift everything right/down
    if min_x < 0.0 || min_y < 0.0 {
        let shift_x = if min_x < 0.0 { -min_x } else { 0.0 };
        let shift_y = if min_y < 0.0 { -min_y } else { 0.0 };
        for n in nodes.iter_mut() {
            n.x += shift_x;
            n.y += shift_y;
        }
        for e in edges.iter_mut() {
            for p in e.points.iter_mut() {
                p.x += shift_x;
                p.y += shift_y;
            }
            if let Some(lp) = e.label_position.as_mut() {
                lp.x += shift_x;
                lp.y += shift_y;
            }
        }
        // Only the top-level groups move; their children keep their
        // coordinates (the Swift shifts `groups[i]` only).
        for g in groups.iter_mut() {
            g.x += shift_x;
            g.y += shift_y;
        }
        max_x += shift_x;
        max_y += shift_y;
    }

    let content = match diagram_type {
        DiagramType::StateDiagram => PositionedContent::StateDiagram { nodes, edges, groups },
        _ => PositionedContent::Flowchart { nodes, edges, groups },
    };
    PositionedGraph {
        diagram: MermaidGraph { diagram_type, payload: Payload::Flow(source.clone()) },
        width: max_x,
        height: max_y,
        content,
    }
}

// MARK: - Shape Clipping

/// `_edgePathMidpoint(_:direction:)`.
fn edge_path_midpoint(points: &[P], direction: Direction) -> P {
    if points.len() < 2 {
        return points.first().copied().unwrap_or(P { x: 0.0, y: 0.0 });
    }

    // For edges with bends, prefer the longest segment aligned with the flow direction.
    let is_vertical_flow = direction == Direction::TD || direction == Direction::TB || direction == Direction::BT;

    if points.len() >= 3 {
        let mut best_idx: i64 = -1;
        let mut best_len = 0.0;
        for i in 1..points.len() {
            let dx = points[i].x - points[i - 1].x;
            let dy = points[i].y - points[i - 1].y;
            let seg_len = (dx * dx + dy * dy).sqrt();
            let is_flow_aligned = if is_vertical_flow { dy.abs() > dx.abs() } else { dx.abs() > dy.abs() };
            if is_flow_aligned && seg_len > best_len {
                best_len = seg_len;
                best_idx = i as i64;
            }
        }
        if best_idx > 0 {
            let b = best_idx as usize;
            return P { x: (points[b - 1].x + points[b].x) / 2.0, y: (points[b - 1].y + points[b].y) / 2.0 };
        }
    }

    // Fallback: total path distance midpoint
    let mut total_len = 0.0;
    for i in 1..points.len() {
        let dx = points[i].x - points[i - 1].x;
        let dy = points[i].y - points[i - 1].y;
        total_len += (dx * dx + dy * dy).sqrt();
    }
    let half_len = total_len / 2.0;
    let mut accumulated = 0.0;
    for i in 1..points.len() {
        let dx = points[i].x - points[i - 1].x;
        let dy = points[i].y - points[i - 1].y;
        let seg_len = (dx * dx + dy * dy).sqrt();
        if accumulated + seg_len >= half_len {
            let remaining = half_len - accumulated;
            let t = if seg_len > 0.0 { remaining / seg_len } else { 0.5 };
            return P { x: points[i - 1].x + dx * t, y: points[i - 1].y + dy * t };
        }
        accumulated += seg_len;
    }
    P { x: (points[0].x + points[points.len() - 1].x) / 2.0, y: (points[0].y + points[points.len() - 1].y) / 2.0 }
}

/// `_clipEdgeToShape(points:node:isStart:)`.
fn clip_edge_to_shape(points: &[P], node: &PositionedNodePayload, is_start: bool) -> Vec<P> {
    if points.len() < 2 {
        return points.to_vec();
    }

    let shape = node.shape.as_str();
    // Rectangular shapes: bounding box is already correct
    if matches!(shape, "rectangle" | "rounded" | "stadium" | "subroutine" | "stateStart" | "stateEnd" | "stateFork") {
        return points.to_vec();
    }

    let mut result = points.to_vec();
    let cx = node.x + node.width / 2.0;
    let cy = node.y + node.height / 2.0;
    let half_w = node.width / 2.0;
    let half_h = node.height / 2.0;

    if is_start {
        if let Some(clipped) = clip_point(points[0], points[1], shape, cx, cy, half_w, half_h) {
            result[0] = clipped;
        }
    } else {
        let last_idx = points.len() - 1;
        if let Some(clipped) = clip_point(points[last_idx], points[last_idx - 1], shape, cx, cy, half_w, half_h) {
            result[last_idx] = clipped;
        }
    }

    result
}

fn clip_point(endpoint: P, adjacent: P, shape: &str, cx: f64, cy: f64, half_w: f64, half_h: f64) -> Option<P> {
    match shape {
        "diamond" | "rhombus" | "stateChoice" => clip_to_diamond(endpoint, adjacent, cx, cy, half_w, half_h),
        "circle" | "doublecircle" => clip_to_circle(endpoint, cx, cy, half_w, half_h),
        "hexagon" => clip_to_hexagon(endpoint, adjacent, cx, cy, half_w, half_h),
        // For other non-rect shapes, use general center→external intersection
        _ => clip_to_ellipse_approx(endpoint, cx, cy, half_w, half_h),
    }
}

fn clip_to_diamond(endpoint: P, adjacent: P, cx: f64, cy: f64, half_w: f64, half_h: f64) -> Option<P> {
    let top = (cx, cy - half_h);
    let right = (cx + half_w, cy);
    let bottom = (cx, cy + half_h);
    let left = (cx - half_w, cy);

    let dx = endpoint.x - adjacent.x;
    let dy = endpoint.y - adjacent.y;
    let is_vertical = dx.abs() < dy.abs();

    if is_vertical {
        let ray_x = endpoint.x;
        if dy > 0.0 {
            // Moving down → top half
            if ray_x <= cx {
                intersect_vertical_ray(ray_x, left.0, left.1, top.0, top.1)
            } else {
                intersect_vertical_ray(ray_x, top.0, top.1, right.0, right.1)
            }
        } else if ray_x <= cx {
            // Moving up → bottom half
            intersect_vertical_ray(ray_x, bottom.0, bottom.1, left.0, left.1)
        } else {
            intersect_vertical_ray(ray_x, right.0, right.1, bottom.0, bottom.1)
        }
    } else {
        let ray_y = endpoint.y;
        if dx > 0.0 {
            // Moving right → left half
            if ray_y <= cy {
                intersect_horizontal_ray(ray_y, top.0, top.1, left.0, left.1)
            } else {
                intersect_horizontal_ray(ray_y, left.0, left.1, bottom.0, bottom.1)
            }
        } else if ray_y <= cy {
            // Moving left → right half
            intersect_horizontal_ray(ray_y, top.0, top.1, right.0, right.1)
        } else {
            intersect_horizontal_ray(ray_y, right.0, right.1, bottom.0, bottom.1)
        }
    }
}

fn clip_to_circle(endpoint: P, cx: f64, cy: f64, half_w: f64, half_h: f64) -> Option<P> {
    let radius = min(half_w, half_h);
    let dx = endpoint.x - cx;
    let dy = endpoint.y - cy;
    let dist = (dx * dx + dy * dy).sqrt();
    if !(dist > 0.001) {
        return None;
    }
    let scale = radius / dist;
    Some(P { x: cx + dx * scale, y: cy + dy * scale })
}

fn clip_to_hexagon(endpoint: P, adjacent: P, cx: f64, cy: f64, half_w: f64, half_h: f64) -> Option<P> {
    // Hexagon has 6 vertices: left/right points and 4 angled corners
    let inset = half_w * 0.25;
    let vertices = [
        (cx - half_w, cy),                 // left point
        (cx - half_w + inset, cy - half_h), // top-left
        (cx + half_w - inset, cy - half_h), // top-right
        (cx + half_w, cy),                 // right point
        (cx + half_w - inset, cy + half_h), // bottom-right
        (cx - half_w + inset, cy + half_h), // bottom-left
    ];
    clip_to_polygon(endpoint, adjacent, &vertices)
}

fn clip_to_ellipse_approx(endpoint: P, cx: f64, cy: f64, half_w: f64, half_h: f64) -> Option<P> {
    let dx = endpoint.x - cx;
    let dy = endpoint.y - cy;
    if !(dx.abs() > 0.001 || dy.abs() > 0.001) {
        return None;
    }
    // Ellipse boundary: (dx/halfW)^2 + (dy/halfH)^2 = 1
    let norm_x = dx / half_w;
    let norm_y = dy / half_h;
    let dist = (norm_x * norm_x + norm_y * norm_y).sqrt();
    if !(dist > 0.001) {
        return None;
    }
    let scale = 1.0 / dist;
    Some(P { x: cx + dx * scale, y: cy + dy * scale })
}

fn clip_to_polygon(endpoint: P, adjacent: P, vertices: &[(f64, f64)]) -> Option<P> {
    let n = vertices.len();
    if n < 3 {
        return None;
    }

    // Ray from adjacent to endpoint, find closest intersection with polygon edges
    let (ox, oy) = (adjacent.x, adjacent.y);
    let (dx, dy) = (endpoint.x - adjacent.x, endpoint.y - adjacent.y);

    let mut best_t = f64::INFINITY;
    let mut best_point: Option<P> = None;

    for i in 0..n {
        let j = (i + 1) % n;
        let ex = vertices[j].0 - vertices[i].0;
        let ey = vertices[j].1 - vertices[i].1;

        let denom = dx * ey - dy * ex;
        if !(denom.abs() > 0.0001) {
            continue;
        }

        let t = ((vertices[i].0 - ox) * ey - (vertices[i].1 - oy) * ex) / denom;
        let u = ((vertices[i].0 - ox) * dy - (vertices[i].1 - oy) * dx) / denom;

        if t > 0.0 && u >= 0.0 && u <= 1.0 && t < best_t {
            best_t = t;
            best_point = Some(P { x: ox + dx * t, y: oy + dy * t });
        }
    }

    best_point
}

fn intersect_vertical_ray(ray_x: f64, p1x: f64, p1y: f64, p2x: f64, p2y: f64) -> Option<P> {
    let dx = p2x - p1x;
    if !(dx.abs() > 0.001) {
        return None;
    }
    let t = (ray_x - p1x) / dx;
    if !(t >= 0.0 && t <= 1.0) {
        return None;
    }
    Some(P { x: ray_x, y: p1y + t * (p2y - p1y) })
}

fn intersect_horizontal_ray(ray_y: f64, p1x: f64, p1y: f64, p2x: f64, p2y: f64) -> Option<P> {
    let dy = p2y - p1y;
    if !(dy.abs() > 0.001) {
        return None;
    }
    let t = (ray_y - p1y) / dy;
    if !(t >= 0.0 && t <= 1.0) {
        return None;
    }
    Some(P { x: p1x + t * (p2x - p1x), y: ray_y })
}

#[allow(dead_code)]
fn unused(_: &[&str]) {}
