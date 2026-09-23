//! Port of `Mermaid/src_sequence_parser.swift` (from
//! `original/src/sequence/parser.ts`), which also declares the sequence
//! diagram model types.

use super::src_types::SSet;
use crate::error::MermaidError;
use crate::swift::{self, regex, Text};

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceDiagram {
    pub actors: Vec<SequenceActor>,
    pub messages: Vec<SequenceMessage>,
    pub blocks: Vec<SequenceBlock>,
    pub notes: Vec<SequenceNote>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceActor {
    pub id: String,
    pub label: String,
    pub r#type: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceMessage {
    pub from: String,
    pub to: String,
    pub label: String,
    pub line_style: String,
    pub arrow_head: String,
    pub activate: bool,
    pub deactivate: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceBlockDivider {
    pub index: i64,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceBlock {
    pub r#type: String,
    pub label: String,
    pub start_index: i64,
    pub end_index: i64,
    pub dividers: Vec<SequenceBlockDivider>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceNote {
    pub actor_ids: Vec<String>,
    pub text: String,
    pub position: String,
    pub after_index: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedSequenceDiagram {
    pub width: f64,
    pub height: f64,
    pub actors: Vec<PositionedSequenceActor>,
    pub lifelines: Vec<SequenceLifeline>,
    pub messages: Vec<PositionedSequenceMessage>,
    pub activations: Vec<SequenceActivation>,
    pub blocks: Vec<PositionedSequenceBlock>,
    pub notes: Vec<PositionedSequenceNote>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedSequenceActor {
    pub id: String,
    pub label: String,
    pub r#type: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceLifeline {
    pub actor_id: String,
    pub x: f64,
    pub top_y: f64,
    pub bottom_y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedSequenceMessage {
    pub from: String,
    pub to: String,
    pub label: String,
    pub line_style: String,
    pub arrow_head: String,
    pub x1: f64,
    pub x2: f64,
    pub y: f64,
    pub is_self: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceActivation {
    pub actor_id: String,
    pub x: f64,
    pub top_y: f64,
    pub bottom_y: f64,
    pub width: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedSequenceBlockDivider {
    pub y: f64,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedSequenceBlock {
    pub r#type: String,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub dividers: Vec<PositionedSequenceBlockDivider>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedSequenceNote {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub position: String,
    pub actors: Vec<String>,
}

struct OpenBlock {
    r#type: String,
    label: String,
    start_index: i64,
    dividers: Vec<SequenceBlockDivider>,
}

/// `parseSequenceDiagram(_:)`.
pub fn parse_sequence_diagram(lines: &[&str]) -> Result<SequenceDiagram, MermaidError> {
    let mut diagram = SequenceDiagram { actors: vec![], messages: vec![], blocks: vec![], notes: vec![] };
    let Some(&header) = lines.first() else {
        return Ok(diagram);
    };

    if !swift::regex_test(r"^sequencediagram\s*$", header, true) {
        return Err(MermaidError::SequenceInvalidHeader {
            expected: "sequenceDiagram".into(),
            found: header.to_owned(),
        });
    }

    let mut actor_ids = SSet::new();
    let mut block_stack: Vec<OpenBlock> = Vec::new();

    if lines.len() <= 1 {
        return Ok(diagram);
    }

    for &raw_line in &lines[1..] {
        let line = swift::trim_whitespaces_and_newlines(raw_line);
        if line.is_empty() {
            continue;
        }

        if let Some(m) = matches(r"^(participant|actor)\s+(\S+?)(?:\s+as\s+(.+))?$", line, false) {
            let r#type = swift::lowercased(&m[1]);
            let id = m[2].clone();
            let alias = if m.len() > 3 { m[3].as_str() } else { "" };
            let label = normalize_br_tags(if alias.is_empty() { &id } else { alias });
            if !actor_ids.contains(&id) {
                actor_ids.insert(&id);
                diagram.actors.push(SequenceActor { id, label, r#type });
            }
            continue;
        }

        if let Some(m) = matches(r"^Note\s+(left of|right of|over)\s+([^:]+):\s*(.+)$", line, true) {
            let position_raw = swift::lowercased(&m[1]);
            let actor_tokens: Vec<String> = swift::split_character(&m[2], ',')
                .into_iter()
                .map(|s| swift::trim_whitespaces_and_newlines(s).to_owned())
                .collect();
            let text = br_tags_to_newlines(swift::trim_whitespaces_and_newlines(&m[3]));
            for id in &actor_tokens {
                if !id.is_empty() {
                    ensure_actor(&mut diagram, &mut actor_ids, id);
                }
            }
            let pos = if position_raw == "left of" {
                "left"
            } else if position_raw == "right of" {
                "right"
            } else {
                "over"
            };
            diagram.notes.push(SequenceNote {
                actor_ids: actor_tokens,
                text,
                position: pos.into(),
                after_index: diagram.messages.len() as i64 - 1,
            });
            continue;
        }

        if let Some(m) = matches(r"^(loop|alt|opt|par|critical|break|rect)\s*(.*)$", line, false) {
            block_stack.push(OpenBlock {
                r#type: m[1].clone(),
                label: normalize_br_tags(swift::trim_whitespaces_and_newlines(&m[2])),
                start_index: diagram.messages.len() as i64,
                dividers: vec![],
            });
            continue;
        }

        if !block_stack.is_empty() {
            if let Some(m) = matches(r"^(else|and)\s*(.*)$", line, false) {
                let index = diagram.messages.len() as i64;
                block_stack.last_mut().unwrap().dividers.push(SequenceBlockDivider {
                    index,
                    label: normalize_br_tags(swift::trim_whitespaces_and_newlines(&m[2])),
                });
                continue;
            }
        }

        if line == "end" && !block_stack.is_empty() {
            let completed = block_stack.pop().unwrap();
            diagram.blocks.push(SequenceBlock {
                r#type: completed.r#type,
                label: completed.label,
                start_index: completed.start_index,
                end_index: swift::max(diagram.messages.len() as i64 - 1, completed.start_index),
                dividers: completed.dividers,
            });
            continue;
        }

        if let Some(msg) = parse_sequence_message(line) {
            let (from, to) = (msg.from.clone(), msg.to.clone());
            ensure_actor(&mut diagram, &mut actor_ids, &from);
            ensure_actor(&mut diagram, &mut actor_ids, &to);
            diagram.messages.push(msg);
            continue;
        }
    }

    Ok(diagram)
}

fn parse_sequence_message(line: &str) -> Option<SequenceMessage> {
    if let Some(m) = matches(r"^(\S+?)\s*(--?>?>|--?[)x]|--?>>|--?>)\s*([+-]?)(\S+?)\s*:\s*(.+)$", line, false) {
        return Some(build_message(&m[1], &m[2], &m[3], &m[4], &m[5]));
    }
    if let Some(m) = matches(r"^(\S+?)\s*(->>|-->>|-\)|--\)|-x|--x|->|-->)\s*([+-]?)(\S+?)\s*:\s*(.+)$", line, false) {
        return Some(build_message(&m[1], &m[2], &m[3], &m[4], &m[5]));
    }
    None
}

fn build_message(from: &str, arrow: &str, activation: &str, to: &str, label: &str) -> SequenceMessage {
    let line_style = if swift::has_prefix(arrow, "--") { "dashed" } else { "solid" };
    let arrow_head = if arrow.contains(">>") || arrow.contains('x') { "filled" } else { "open" };
    SequenceMessage {
        from: from.to_owned(),
        to: to.to_owned(),
        label: normalize_br_tags(swift::trim_whitespaces_and_newlines(label)),
        line_style: line_style.into(),
        arrow_head: arrow_head.into(),
        activate: activation == "+",
        deactivate: activation == "-",
    }
}

fn ensure_actor(diagram: &mut SequenceDiagram, actor_ids: &mut SSet, id: &str) {
    if actor_ids.contains(id) {
        return;
    }
    actor_ids.insert(id);
    diagram.actors.push(SequenceActor { id: id.to_owned(), label: id.to_owned(), r#type: "participant".into() });
}

fn normalize_br_tags(text: &str) -> String {
    swift::regex_replace(text, r"<br\s*/?>", "<br>", true)
}

fn br_tags_to_newlines(text: &str) -> String {
    swift::regex_replace(text, r"<br\s*/?>", "\n", true)
}

/// `_match(_:_:caseInsensitive:)`.
fn matches(pattern: &'static str, text: &str, case_insensitive: bool) -> Option<Vec<String>> {
    let r = regex(pattern, case_insensitive)?;
    let t = Text::new(text);
    let m = r.first_match(&t)?;
    Some((0..m.count()).map(|i| m.group(&t, i).unwrap_or("").to_owned()).collect())
}
