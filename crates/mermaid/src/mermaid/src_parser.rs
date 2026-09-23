//! Port of `Mermaid/src_parser.swift` (from `original/src/parser.ts`): the
//! flowchart and state-diagram parser.

use std::collections::HashMap;

use super::src_multiline_utils::normalize_br_tags;
use super::src_types::{
    strings_contain, Direction, EdgeStyle, MermaidEdge, MermaidGraph as ParsedGraph, MermaidNode, MermaidSubgraph,
    NodeShape, SDict, SSet,
};
use crate::error::MermaidError;
use crate::swift::{self, regex, Regex, Text};
use crate::types::{DiagramType, MermaidGraph, Payload};

struct WorkingGraph {
    direction: Direction,
    nodes_by_id: SDict<MermaidNode>,
    node_order: Vec<String>,
    edges: Vec<MermaidEdge>,
    subgraphs: Vec<MermaidSubgraph>,
    subgraph_ids: SSet,
    class_defs: SDict<SDict<String>>,
    class_assignments: SDict<String>,
    node_styles: SDict<SDict<String>>,
    link_styles: HashMap<i64, SDict<String>>,
}

impl WorkingGraph {
    fn new(direction: Direction) -> WorkingGraph {
        WorkingGraph {
            direction,
            nodes_by_id: SDict::new(),
            node_order: Vec::new(),
            edges: Vec::new(),
            subgraphs: Vec::new(),
            subgraph_ids: SSet::new(),
            class_defs: SDict::new(),
            class_assignments: SDict::new(),
            node_styles: SDict::new(),
            link_styles: HashMap::new(),
        }
    }

    fn upsert_node(&mut self, node: MermaidNode) {
        if !self.nodes_by_id.contains_key(&node.id) {
            self.node_order.push(node.id.clone());
        }
        let id = node.id.clone();
        self.nodes_by_id.insert(&id, node);
    }

    fn merge_node_style(&mut self, id: &str, props: &SDict<String>) {
        let merged = self.node_styles.entry_or(id, SDict::new);
        for (k, v) in props.iter() {
            merged.insert(k, v.clone());
        }
    }

    fn merge_link_style(&mut self, index: i64, props: &SDict<String>) {
        let merged = self.link_styles.entry(index).or_default();
        for (k, v) in props.iter() {
            merged.insert(k, v.clone());
        }
    }

    fn to_parsed_graph(self) -> ParsedGraph {
        let nodes_in_order = self
            .node_order
            .iter()
            .filter_map(|id| self.nodes_by_id.get(id).map(|node| (id.clone(), node.clone())))
            .collect();
        ParsedGraph {
            direction: self.direction,
            nodes_in_order,
            edges: self.edges,
            subgraphs: self.subgraphs,
            class_defs: self.class_defs,
            class_assignments: self.class_assignments,
            node_styles: self.node_styles,
            link_styles: self.link_styles,
        }
    }
}

const ARROW: &str = r"^(<)?(-->|-.->|==>|---|-\.-|===)(?:\|([^|]*)\|)?";
/// Text-embedded labels: `-- label -->`, `-. label .->`, `== label ==>`.
const TEXT_EMBEDDED_ARROW: &str = r"^(<)?(--|-\.|==)\s+(.+?)\s+(-->|---|\.\->|-\.\-|==>|===)";
const BARE_NODE: &str = r"^([\w-]+)";
const CLASS_SHORTHAND: &str = r"^:::([\w][\w-]*)";

const NODE_PATTERNS: [(&str, NodeShape); 12] = [
    (r"^([\w-]+)\(\(\((.+?)\)\)\)", NodeShape::Doublecircle),
    (r"^([\w-]+)\(\[(.+?)\]\)", NodeShape::Stadium),
    (r"^([\w-]+)\(\((.+?)\)\)", NodeShape::Circle),
    (r"^([\w-]+)\[\[(.+?)\]\]", NodeShape::Subroutine),
    (r"^([\w-]+)\[\((.+?)\)\]", NodeShape::Cylinder),
    (r"^([\w-]+)\[\/(.+?)\\\]", NodeShape::Trapezoid),
    (r"^([\w-]+)\[\\(.+?)\/\]", NodeShape::TrapezoidAlt),
    (r"^([\w-]+)>(.+?)\]", NodeShape::Asymmetric),
    (r"^([\w-]+)\{\{(.+?)\}\}", NodeShape::Hexagon),
    (r"^([\w-]+)\[(.+?)\]", NodeShape::Rectangle),
    (r"^([\w-]+)\((.+?)\)", NodeShape::Rounded),
    (r"^([\w-]+)\{(.+?)\}", NodeShape::Diamond),
];

/// `parseMermaid(_:)`.
pub fn parse_mermaid(text: &str) -> Result<MermaidGraph, MermaidError> {
    let lines: Vec<&str> = swift::components_separated_by_set(text, |c| c == '\n' || c == ';')
        .into_iter()
        .map(swift::trim_whitespaces_and_newlines)
        .filter(|l| !l.is_empty() && !swift::has_prefix(l, "%%"))
        .collect();

    if lines.is_empty() {
        return Err(MermaidError::EmptyDiagram);
    }

    let header = lines[0];
    if regex_test(r"^stateDiagram(-v2)?\s*$", header, true) {
        let parsed = parse_state_diagram(&lines)?;
        Ok(MermaidGraph { diagram_type: DiagramType::StateDiagram, payload: Payload::Flow(parsed) })
    } else {
        let parsed = parse_flowchart(&lines)?;
        Ok(MermaidGraph { diagram_type: DiagramType::Flowchart, payload: Payload::Flow(parsed) })
    }
}

fn parse_flowchart(lines: &[&str]) -> Result<ParsedGraph, MermaidError> {
    let Some(&header) = lines.first() else {
        return Err(MermaidError::InvalidHeader(String::new()));
    };

    let direction = regex_groups(r"^(?:graph|flowchart)\s+(TD|TB|LR|BT|RL)\s*$", header, true)
        .and_then(|m| m.get(1).cloned())
        .and_then(|token| parse_direction(&token));
    let Some(direction) = direction else {
        return Err(MermaidError::InvalidHeader(header.to_owned()));
    };

    let mut graph = WorkingGraph::new(direction);
    let mut subgraph_stack: Vec<MermaidSubgraph> = Vec::new();

    if lines.len() <= 1 {
        return Ok(graph.to_parsed_graph());
    }

    // Pre-scan: collect all subgraph IDs so forward-referenced subgraphs aren't
    // mistaken for bare nodes during edge parsing.
    for &line in &lines[1..] {
        if let Some(rest_raw) = regex_groups(r"^subgraph\s+(.+)$", line, false).and_then(|m| m.get(1).cloned()) {
            let rest = swift::trim_whitespaces_and_newlines(&rest_raw);
            if let Some(found_id) = regex_groups(r"^([\w-]+)\s*\[(.+)\]$", rest, false).and_then(|m| m.get(1).cloned()) {
                graph.subgraph_ids.insert(&found_id);
            } else {
                graph.subgraph_ids.insert(&sanitize_subgraph_id(rest));
            }
        }
    }

    for &line in &lines[1..] {
        if let Some(m) = regex_groups(r"^classDef\s+(\w+)\s+(.+)$", line, false) {
            let props = parse_style_props(&m[2]);
            graph.class_defs.insert(&m[1], props);
            continue;
        }

        if let Some(m) = regex_groups(r"^class\s+([\w,-]+)\s+(\w+)$", line, false) {
            for id in swift::split_character(&m[1], ',') {
                let id = swift::trim_whitespaces_and_newlines(id);
                if !id.is_empty() {
                    graph.class_assignments.insert(id, m[2].clone());
                }
            }
            continue;
        }

        if let Some(m) = regex_groups(r"^style\s+([\w,-]+)\s+(.+)$", line, false) {
            let props = parse_style_props(&m[2]);
            for id in swift::split_character(&m[1], ',') {
                let id = swift::trim_whitespaces_and_newlines(id);
                if !id.is_empty() {
                    graph.merge_node_style(id, &props);
                }
            }
            continue;
        }

        // --- linkStyle: `linkStyle 0 stroke:#f00` or `linkStyle default stroke:#f00` ---
        if let Some(m) = regex_groups(r"^linkStyle\s+(default|[\d,\s]+)\s+(.+)$", line, false) {
            let props = parse_style_props(&m[2]);
            let target = &m[1];
            if swift::trim_whitespaces_and_newlines(target) == "default" {
                graph.merge_link_style(-1, &props);
            } else {
                for part in swift::split_character(target, ',') {
                    if let Some(idx) = swift::parse_int(swift::trim_whitespaces_and_newlines(part)) {
                        graph.merge_link_style(idx, &props);
                    }
                }
            }
            continue;
        }

        if let Some(m) = regex_groups(r"^direction\s+(TD|TB|LR|BT|RL)\s*$", line, true) {
            if let Some(dir) = parse_direction(&m[1]) {
                if let Some(top) = subgraph_stack.last_mut() {
                    top.direction = Some(dir);
                    continue;
                }
            }
        }

        if let Some(rest_raw) = regex_groups(r"^subgraph\s+(.+)$", line, false).and_then(|m| m.get(1).cloned()) {
            let rest = swift::trim_whitespaces_and_newlines(&rest_raw);
            let (id, label) = match regex_groups(r"^([\w-]+)\s*\[(.+)\]$", rest, false) {
                Some(m) => (m[1].clone(), normalize_br_tags(&m[2])),
                None => (sanitize_subgraph_id(rest), normalize_br_tags(rest)),
            };

            graph.subgraph_ids.insert(&id);
            subgraph_stack.push(MermaidSubgraph { id, label, node_ids: Vec::new(), children: Vec::new(), direction: None });
            continue;
        }

        if line == "end" {
            if let Some(completed) = subgraph_stack.pop() {
                if let Some(parent) = subgraph_stack.last_mut() {
                    parent.children.push(completed);
                } else {
                    graph.subgraphs.push(completed);
                }
            }
            continue;
        }

        parse_edge_line(line, &mut graph, &mut subgraph_stack);
    }

    Ok(graph.to_parsed_graph())
}

/// `rest.replacingOccurrences(of: #"\s+"#, with: "_", options: .regularExpression)
///      .replacingOccurrences(of: #"[^\w]"#, with: "", options: .regularExpression)`.
fn sanitize_subgraph_id(rest: &str) -> String {
    let underscored = swift::regex_replace(rest, r"\s+", "_", false);
    swift::regex_replace(&underscored, r"[^\w]", "", false)
}

fn parse_state_diagram(lines: &[&str]) -> Result<ParsedGraph, MermaidError> {
    let mut graph = WorkingGraph::new(Direction::TD);

    let mut composite_stack: Vec<MermaidSubgraph> = Vec::new();
    let mut composite_state_ids = SSet::new();
    let mut start_count = 0;
    let mut end_count = 0;

    if lines.len() <= 1 {
        return Ok(graph.to_parsed_graph());
    }

    for &line in &lines[1..] {
        if let Some(m) = regex_groups(r"^direction\s+(TD|TB|LR|BT|RL)\s*$", line, true) {
            if let Some(direction) = parse_direction(&m[1]) {
                if let Some(top) = composite_stack.last_mut() {
                    top.direction = Some(direction);
                } else {
                    graph.direction = direction;
                }
                continue;
            }
        }

        // --- linkStyle in state diagrams ---
        if let Some(m) = regex_groups(r"^linkStyle\s+(default|[\d,\s]+)\s+(.+)$", line, false) {
            let props = parse_style_props(&m[2]);
            let target = &m[1];
            if swift::trim_whitespaces_and_newlines(target) == "default" {
                graph.merge_link_style(-1, &props);
            } else {
                let indices: Vec<i64> = swift::split_character(target, ',')
                    .into_iter()
                    .filter_map(|p| swift::parse_int(swift::trim_whitespaces_and_newlines(p)))
                    .collect();
                for idx in indices {
                    graph.merge_link_style(idx, &props);
                }
            }
            continue;
        }

        if let Some(m) = regex_groups(r#"^state\s+(?:\"([^\"]+)\"\s+as\s+)?([\w\p{L}]+)\s*\{$"#, line, false) {
            if m.len() > 2 {
                let id = m[2].clone();
                let raw1 = &m[1];
                let label = if !raw1.is_empty() { raw1.clone() } else { id.clone() };
                composite_stack.push(MermaidSubgraph {
                    id: id.clone(),
                    label,
                    node_ids: Vec::new(),
                    children: Vec::new(),
                    direction: None,
                });
                composite_state_ids.insert(&id);
                graph.nodes_by_id.remove(&id);
                graph.node_order.retain(|existing| !swift::string_eq(existing, &id));
                continue;
            }
        }

        if line == "}" {
            if let Some(completed) = composite_stack.pop() {
                if let Some(parent) = composite_stack.last_mut() {
                    parent.children.push(completed);
                } else {
                    graph.subgraphs.push(completed);
                }
            }
            continue;
        }

        if let Some(m) = regex_groups(r#"^state\s+\"([^\"]+)\"\s+as\s+([\w\p{L}]+)\s*$"#, line, false) {
            let label = normalize_br_tags(&m[1]);
            let id = m[2].clone();
            register_state_node(&mut graph, &mut composite_stack, MermaidNode { id, label, shape: NodeShape::Rounded });
            continue;
        }

        if let Some(m) = regex_groups(
            r"^(\[\*\]|[\w\p{L}-]+)\s*(-->)\s*(\[\*\]|[\w\p{L}-]+)(?:\s*:\s*(.+))?$",
            line,
            false,
        ) {
            let mut source_id = m[1].clone();
            let mut target_id = m[3].clone();
            let raw_label = m.get(4).map(|s| swift::trim_whitespaces_and_newlines(s).to_owned());
            let edge_label = match raw_label {
                Some(raw) if !raw.is_empty() => Some(normalize_br_tags(&raw)),
                _ => None,
            };

            if source_id == "[*]" {
                start_count += 1;
                source_id = if start_count > 1 { format!("_start{start_count}") } else { "_start".into() };
                register_state_node(
                    &mut graph,
                    &mut composite_stack,
                    MermaidNode { id: source_id.clone(), label: String::new(), shape: NodeShape::StateStart },
                );
            } else if !composite_state_ids.contains(&source_id) {
                ensure_state_node(&mut graph, &mut composite_stack, &source_id);
            }

            if target_id == "[*]" {
                end_count += 1;
                target_id = if end_count > 1 { format!("_end{end_count}") } else { "_end".into() };
                register_state_node(
                    &mut graph,
                    &mut composite_stack,
                    MermaidNode { id: target_id.clone(), label: String::new(), shape: NodeShape::StateEnd },
                );
            } else if !composite_state_ids.contains(&target_id) {
                ensure_state_node(&mut graph, &mut composite_stack, &target_id);
            }

            graph.edges.push(MermaidEdge {
                source: source_id,
                target: target_id,
                label: edge_label,
                style: EdgeStyle::Solid,
                has_arrow_start: false,
                has_arrow_end: true,
                inline_style: None,
            });
            continue;
        }

        if let Some(m) = regex_groups(r"^([\w\p{L}-]+)\s*:\s*(.+)$", line, false) {
            let id = m[1].clone();
            let label = normalize_br_tags(swift::trim_whitespaces_and_newlines(&m[2]));
            register_state_node(&mut graph, &mut composite_stack, MermaidNode { id, label, shape: NodeShape::Rounded });
            continue;
        }
    }

    Ok(graph.to_parsed_graph())
}

fn register_state_node(graph: &mut WorkingGraph, composite_stack: &mut [MermaidSubgraph], node: MermaidNode) {
    let is_new = !graph.nodes_by_id.contains_key(&node.id);
    let id = node.id.clone();
    if is_new {
        graph.upsert_node(node);
    }
    if let Some(current) = composite_stack.last_mut() {
        if !strings_contain(&current.node_ids, &id) {
            current.node_ids.push(id);
        }
    }
}

fn ensure_state_node(graph: &mut WorkingGraph, composite_stack: &mut [MermaidSubgraph], id: &str) {
    if !graph.nodes_by_id.contains_key(id) {
        register_state_node(
            graph,
            composite_stack,
            MermaidNode { id: id.to_owned(), label: id.to_owned(), shape: NodeShape::Rounded },
        );
    } else if let Some(current) = composite_stack.last_mut() {
        if !strings_contain(&current.node_ids, id) {
            current.node_ids.push(id.to_owned());
        }
    }
}

/// `_parseStyleProps(_:)`.
pub(crate) fn parse_style_props(props_str: &str) -> SDict<String> {
    // Strip trailing semicolons — Mermaid tolerates them (e.g. `stroke:#f00;`)
    let cleaned = swift::regex_replace(props_str, r";[\s]*$", "", false);
    let mut props = SDict::new();
    for item in swift::split_character_keeping_empty(&cleaned, ',') {
        let Some(idx) = swift::first_index_of(item, ':') else { continue };
        let key = swift::trim_whitespaces_and_newlines(&item[..idx]);
        let value = swift::trim_whitespaces_and_newlines(&item[swift::index_after(item, idx)..]);
        if !key.is_empty() && !value.is_empty() {
            props.insert(key, value.to_owned());
        }
    }
    props
}

fn parse_edge_line(line: &str, graph: &mut WorkingGraph, subgraph_stack: &mut [MermaidSubgraph]) {
    let remaining = swift::trim_whitespaces_and_newlines(line).to_owned();
    let Some(first_group) = consume_node_group(&remaining, graph, subgraph_stack) else {
        return;
    };
    if first_group.ids.is_empty() {
        return;
    }

    let mut remaining = swift::trim_whitespaces_and_newlines(&first_group.remaining).to_owned();
    let mut prev_group_ids = first_group.ids;

    let arrow = regex(ARROW, false);
    let text_embedded = regex(TEXT_EMBEDDED_ARROW, false);

    while !remaining.is_empty() {
        let has_arrow_start;
        let edge_label;
        let style;
        let has_arrow_end;
        let next_remaining;

        let t = Text::new(&remaining);
        if let Some(m) = arrow.as_ref().and_then(|r| r.first_match(&t)) {
            let full = m.group(&t, 0).unwrap_or("");
            let op = m.group(&t, 2).unwrap_or("");
            has_arrow_start = !m.group(&t, 1).unwrap_or("").is_empty();
            let raw_label = swift::trim_whitespaces_and_newlines(m.group(&t, 3).unwrap_or(""));
            edge_label = if !raw_label.is_empty() { Some(normalize_br_tags(raw_label)) } else { None };
            style = arrow_style_from_op(op);
            has_arrow_end = swift::has_suffix(op, ">");
            let full_count = swift::character_count(full);
            next_remaining = swift::trim_whitespaces_and_newlines(swift::drop_first(&remaining, full_count)).to_owned();
        } else if let Some(m) = text_embedded.as_ref().and_then(|r| r.first_match(&t)) {
            // Fallback: text-embedded label syntax (-- Yes -->, -. Maybe .->, == Sure ==>)
            let full = m.group(&t, 0).unwrap_or("");
            let open_op = m.group(&t, 2).unwrap_or("");
            let label_text = m.group(&t, 3).unwrap_or("");
            let close_op = m.group(&t, 4).unwrap_or("");
            has_arrow_start = !m.group(&t, 1).unwrap_or("").is_empty();
            let trimmed_label = swift::trim_whitespaces_and_newlines(label_text);
            edge_label = if trimmed_label.is_empty() { None } else { Some(normalize_br_tags(trimmed_label)) };
            style = text_arrow_style_from_ops(open_op, close_op);
            has_arrow_end = swift::has_suffix(close_op, ">");
            let full_count = swift::character_count(full);
            next_remaining = swift::trim_whitespaces_and_newlines(swift::drop_first(&remaining, full_count)).to_owned();
        } else {
            break;
        }
        drop(t);
        remaining = next_remaining;

        let Some(next_group) = consume_node_group(&remaining, graph, subgraph_stack) else {
            break;
        };
        if next_group.ids.is_empty() {
            break;
        }

        remaining = swift::trim_whitespaces_and_newlines(&next_group.remaining).to_owned();

        for source_id in &prev_group_ids {
            for target_id in &next_group.ids {
                graph.edges.push(MermaidEdge {
                    source: source_id.clone(),
                    target: target_id.clone(),
                    label: edge_label.clone(),
                    style,
                    has_arrow_start,
                    has_arrow_end,
                    inline_style: None,
                });
            }
        }

        prev_group_ids = next_group.ids;
    }
}

struct ConsumedNodeGroup {
    ids: Vec<String>,
    remaining: String,
}

fn consume_node_group(text: &str, graph: &mut WorkingGraph, subgraph_stack: &mut [MermaidSubgraph]) -> Option<ConsumedNodeGroup> {
    let (first_id, first_remaining) = consume_node(text, graph, subgraph_stack)?;

    let mut ids = vec![first_id];
    let mut remaining = swift::trim_whitespaces_and_newlines(&first_remaining).to_owned();

    while swift::has_prefix(&remaining, "&") {
        let rest = swift::trim_whitespaces_and_newlines(swift::drop_first(&remaining, 1)).to_owned();
        remaining = rest;
        let Some((next_id, next_remaining)) = consume_node(&remaining, graph, subgraph_stack) else {
            break;
        };
        ids.push(next_id);
        remaining = swift::trim_whitespaces_and_newlines(&next_remaining).to_owned();
    }

    Some(ConsumedNodeGroup { ids, remaining })
}

fn consume_node(text: &str, graph: &mut WorkingGraph, subgraph_stack: &mut [MermaidSubgraph]) -> Option<(String, String)> {
    let mut id: Option<String> = None;
    let mut remaining = text.to_owned();
    let t = Text::new(text);

    for (pattern, shape) in NODE_PATTERNS {
        let Some(r) = regex(pattern, false) else { continue };
        let Some(m) = r.first_match(&t) else { continue };
        let full = m.group(&t, 0).unwrap_or("");
        let matched_id = m.group(&t, 1).unwrap_or("");
        let raw_label = m.group(&t, 2).unwrap_or("");

        let label = normalize_br_tags(raw_label);
        register_node(graph, subgraph_stack, MermaidNode { id: matched_id.to_owned(), label, shape });
        id = Some(matched_id.to_owned());
        remaining = swift::drop_first(text, swift::character_count(full)).to_owned();
        break;
    }

    if id.is_none() {
        if let Some(m) = regex(BARE_NODE, false).and_then(|r| r.first_match(&t)) {
            {
                let full = m.group(&t, 0).unwrap_or("");
                let bare_id = m.group(&t, 1).unwrap_or("");
                id = Some(bare_id.to_owned());
                if !graph.nodes_by_id.contains_key(bare_id) && !graph.subgraph_ids.contains(bare_id) {
                    register_node(
                        graph,
                        subgraph_stack,
                        MermaidNode { id: bare_id.to_owned(), label: bare_id.to_owned(), shape: NodeShape::Rectangle },
                    );
                }
                remaining = swift::drop_first(text, swift::character_count(full)).to_owned();
            }
        }
    }

    let node_id = id?;

    let rt = Text::new(&remaining);
    if let Some(m) = regex(CLASS_SHORTHAND, false).and_then(|r| r.first_match(&rt)) {
        {
            let full = m.group(&rt, 0).unwrap_or("");
            let class_name = m.group(&rt, 1).unwrap_or("");
            graph.class_assignments.insert(&node_id, class_name.to_owned());
            let rest = swift::drop_first(&remaining, swift::character_count(full)).to_owned();
            drop(rt);
            return Some((node_id, rest));
        }
    }
    drop(rt);

    Some((node_id, remaining))
}

fn register_node(graph: &mut WorkingGraph, subgraph_stack: &mut [MermaidSubgraph], node: MermaidNode) {
    let is_new = !graph.nodes_by_id.contains_key(&node.id);
    let id = node.id.clone();
    if is_new {
        graph.upsert_node(node);
    }
    track_in_subgraph(subgraph_stack, &id);
}

fn track_in_subgraph(subgraph_stack: &mut [MermaidSubgraph], node_id: &str) {
    if let Some(current) = subgraph_stack.last_mut() {
        if !strings_contain(&current.node_ids, node_id) {
            current.node_ids.push(node_id.to_owned());
        }
    }
}

fn text_arrow_style_from_ops(open_op: &str, close_op: &str) -> EdgeStyle {
    if open_op == "-." || close_op == ".->" || close_op == "-.-" {
        return EdgeStyle::Dotted;
    }
    if open_op == "==" || close_op == "==>" || close_op == "===" {
        return EdgeStyle::Thick;
    }
    EdgeStyle::Solid
}

fn arrow_style_from_op(op: &str) -> EdgeStyle {
    if op == "-.->" || op == "-.-" {
        return EdgeStyle::Dotted;
    }
    if op == "==>" || op == "===" {
        return EdgeStyle::Thick;
    }
    EdgeStyle::Solid
}

fn parse_direction(token: &str) -> Option<Direction> {
    Direction::from_raw(&swift::uppercased(token))
}

fn regex_test(pattern: &'static str, input: &str, case_insensitive: bool) -> bool {
    regex_groups(pattern, input, case_insensitive).is_some()
}

/// `_regexGroups(_:_:caseInsensitive:)`: every group's text, `""` for a group
/// that did not participate.
fn regex_groups(pattern: &'static str, input: &str, case_insensitive: bool) -> Option<Vec<String>> {
    let r: Regex = regex(pattern, case_insensitive)?;
    let t = Text::new(input);
    let m = r.first_match(&t)?;
    Some((0..m.count()).map(|i| m.group(&t, i).unwrap_or("").to_owned()).collect())
}
