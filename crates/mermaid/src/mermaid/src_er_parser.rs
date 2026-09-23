//! Port of `Mermaid/src_er_parser.swift` (from `original/src/er/parser.ts`),
//! which also declares the ER diagram model types.

use super::src_multiline_utils::normalize_br_tags;
use super::src_types::SDict;
use crate::error::MermaidError;
use crate::swift::{self, regex, Text};

#[derive(Debug, Clone, PartialEq)]
pub struct ErDiagram {
    pub entities: Vec<ErEntity>,
    pub relationships: Vec<ErRelationship>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErEntity {
    pub id: String,
    pub label: String,
    pub attributes: Vec<ErAttribute>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErAttribute {
    pub r#type: String,
    pub name: String,
    pub keys: Vec<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErRelationship {
    pub entity1: String,
    pub entity2: String,
    pub cardinality1: String,
    pub cardinality2: String,
    pub label: String,
    pub identifying: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedErDiagram {
    pub width: f64,
    pub height: f64,
    pub entities: Vec<PositionedErEntity>,
    pub relationships: Vec<PositionedErRelationship>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedErEntity {
    pub id: String,
    pub label: String,
    pub attributes: Vec<ErAttribute>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub header_height: f64,
    pub row_height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedErRelationship {
    pub entity1: String,
    pub entity2: String,
    pub cardinality1: String,
    pub cardinality2: String,
    pub label: String,
    pub identifying: bool,
    pub points: Vec<ErPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErPoint {
    pub x: f64,
    pub y: f64,
}

/// `parseErDiagram(_:)`.
pub fn parse_er_diagram(lines: &[&str]) -> Result<ErDiagram, MermaidError> {
    let mut diagram = ErDiagram { entities: vec![], relationships: vec![] };
    let Some(&header) = lines.first() else {
        return Ok(diagram);
    };
    if !swift::regex_test(r"^erdiagram\s*$", header, true) {
        return Err(MermaidError::ErInvalidHeader { expected: "erDiagram".into(), found: header.to_owned() });
    }

    let mut entity_map: SDict<ErEntity> = SDict::new();
    let mut entity_order: Vec<String> = Vec::new();
    let mut current_entity_id: Option<String> = None;

    if lines.len() <= 1 {
        return Ok(diagram);
    }

    for &line in &lines[1..] {
        let raw_line = swift::trim_whitespaces_and_newlines(line);
        if raw_line.is_empty() {
            continue;
        }

        if let Some(active_entity_id) = current_entity_id.clone() {
            if raw_line == "}" {
                current_entity_id = None;
                continue;
            }

            if let Some(attr) = parse_attribute(raw_line) {
                let mut entity = ensure_entity(&mut entity_map, &mut entity_order, &active_entity_id);
                entity.attributes.push(attr);
                entity_map.insert(&active_entity_id, entity);
            }
            continue;
        }

        if let Some(id) = first_group(r"^(\S+)\s*\{$", raw_line) {
            let _ = ensure_entity(&mut entity_map, &mut entity_order, &id);
            current_entity_id = Some(id);
            continue;
        }

        if let Some(rel) = parse_relationship_line(raw_line) {
            let _ = ensure_entity(&mut entity_map, &mut entity_order, &rel.entity1);
            let _ = ensure_entity(&mut entity_map, &mut entity_order, &rel.entity2);
            diagram.relationships.push(rel);
        }
    }

    diagram.entities = entity_order.iter().filter_map(|id| entity_map.get(id).cloned()).collect();
    Ok(diagram)
}

fn ensure_entity(map: &mut SDict<ErEntity>, order: &mut Vec<String>, id: &str) -> ErEntity {
    if let Some(entity) = map.get(id) {
        return entity.clone();
    }
    let entity = ErEntity { id: id.to_owned(), label: id.to_owned(), attributes: vec![] };
    map.insert(id, entity.clone());
    order.push(id.to_owned());
    entity
}

fn parse_attribute(line: &str) -> Option<ErAttribute> {
    let g = groups(r"^(\S+)\s+(\S+)(?:\s+(.+))?$", line)?;
    let r#type = g.get(1)?.clone();
    let name = g.get(2)?.clone();

    let rest = g.get(3).map_or(String::new(), |r| swift::trim_whitespaces_and_newlines(r).to_owned());
    let mut keys: Vec<String> = Vec::new();
    let mut comment: Option<String> = None;

    if let Some(comment_match) = first_group(r#""([^"]*)""#, &rest) {
        comment = Some(normalize_br_tags(&comment_match));
    }

    let rest_without_comment = swift::regex_replace(&rest, r#""[^"]*""#, "", false);
    for part in swift::split_whitespace_characters(&rest_without_comment) {
        let token = swift::uppercased(part);
        if token == "PK" || token == "FK" || token == "UK" {
            keys.push(token);
        }
    }

    Some(ErAttribute { r#type, name, keys, comment })
}

fn parse_relationship_line(line: &str) -> Option<ErRelationship> {
    let g = groups(r"^(\S+)\s+([|o}{]+(?:--|\.\.)[|o}{]+)\s+(\S+)\s*:\s*(.+)$", line)?;
    let entity1 = g.get(1)?.clone();
    let cardinality_str = g.get(2)?.clone();
    let entity2 = g.get(3)?.clone();
    let raw_label = g.get(4)?.clone();

    let label = normalize_br_tags(&swift::regex_replace(
        swift::trim_whitespaces_and_newlines(&raw_label),
        r#"^["']|["']$"#,
        "",
        false,
    ));

    let line_match = groups(r"^([|o}{]+)(--|\.\.?)([|o}{]+)$", &cardinality_str)?;
    let left_str = line_match.get(1)?;
    let line_style = line_match.get(2)?;
    let right_str = line_match.get(3)?;
    let cardinality1 = parse_cardinality(left_str)?;
    let cardinality2 = parse_cardinality(right_str)?;

    Some(ErRelationship {
        entity1,
        entity2,
        cardinality1: cardinality1.into(),
        cardinality2: cardinality2.into(),
        label,
        identifying: line_style == "--",
    })
}

fn parse_cardinality(raw: &str) -> Option<&'static str> {
    // `String(raw.sorted())`: the characters here are all ASCII.
    let mut chars: Vec<char> = raw.chars().collect();
    chars.sort();
    let sorted: String = chars.into_iter().collect();
    match sorted.as_str() {
        "||" => Some("one"),
        "o|" => Some("zero-one"),
        "|}" | "{|" => Some("many"),
        "{o" | "o{" => Some("zero-many"),
        _ => None,
    }
}

fn first_group(pattern: &'static str, value: &str) -> Option<String> {
    groups(pattern, value).and_then(|g| g.get(1).cloned())
}

/// `_groups(_:_:caseInsensitive:)`: `""` for a group that did not participate.
fn groups(pattern: &'static str, value: &str) -> Option<Vec<String>> {
    let r = regex(pattern, false)?;
    let t = Text::new(value);
    let m = r.first_match(&t)?;
    Some((0..m.count()).map(|i| m.group(&t, i).unwrap_or("").to_owned()).collect())
}
