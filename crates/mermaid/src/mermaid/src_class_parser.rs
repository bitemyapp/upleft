//! Port of `Mermaid/src_class_parser.swift` (from `original/src/class/parser.ts`),
//! which also declares the class diagram model types.

use super::src_multiline_utils::normalize_br_tags;
use super::src_types::SDict;
use crate::error::MermaidError;
use crate::swift::{self, regex, Text};

#[derive(Debug, Clone, PartialEq)]
pub struct ClassDiagram {
    pub classes: Vec<ClassNode>,
    pub relationships: Vec<ClassRelationship>,
    pub namespaces: Vec<ClassNamespace>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassNode {
    pub id: String,
    pub label: String,
    pub attributes: Vec<ClassMember>,
    pub methods: Vec<ClassMember>,
    pub annotation: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassMember {
    pub visibility: String,
    pub name: String,
    pub r#type: Option<String>,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_method: bool,
    pub params: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassRelationship {
    pub from: String,
    pub to: String,
    pub r#type: String,
    pub marker_at: String,
    pub label: Option<String>,
    pub from_cardinality: Option<String>,
    pub to_cardinality: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassNamespace {
    pub name: String,
    pub class_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedClassDiagram {
    pub width: f64,
    pub height: f64,
    pub classes: Vec<PositionedClassNode>,
    pub relationships: Vec<PositionedClassRelationship>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedClassNode {
    pub id: String,
    pub label: String,
    pub annotation: Option<String>,
    pub attributes: Vec<ClassMember>,
    pub methods: Vec<ClassMember>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub header_height: f64,
    pub attr_height: f64,
    pub method_height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedClassRelationship {
    pub from: String,
    pub to: String,
    pub r#type: String,
    pub marker_at: String,
    pub label: Option<String>,
    pub from_cardinality: Option<String>,
    pub to_cardinality: Option<String>,
    pub points: Vec<ClassPoint>,
    pub label_position: Option<ClassPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClassPoint {
    pub x: f64,
    pub y: f64,
}

struct ParsedMember {
    member: ClassMember,
    is_method: bool,
}

/// `parseClassDiagram(_:)`.
pub fn parse_class_diagram(lines: &[&str]) -> Result<ClassDiagram, MermaidError> {
    let mut diagram = ClassDiagram { classes: vec![], relationships: vec![], namespaces: vec![] };
    let Some(&header) = lines.first() else {
        return Ok(diagram);
    };
    if !swift::regex_test(r"^classdiagram\s*$", header, true) {
        return Err(MermaidError::ClassInvalidHeader { expected: "classDiagram".into(), found: header.to_owned() });
    }

    let mut class_map: SDict<ClassNode> = SDict::new();
    let mut class_order: Vec<String> = Vec::new();
    let mut current_namespace: Option<ClassNamespace> = None;
    let mut current_class_id: Option<String> = None;
    let mut brace_depth = 0;

    if lines.len() <= 1 {
        return Ok(diagram);
    }

    for &line in &lines[1..] {
        let raw_line = swift::trim_whitespaces_and_newlines(line);
        if raw_line.is_empty() {
            continue;
        }

        if let (Some(active_class_id), true) = (current_class_id.clone(), brace_depth > 0) {
            if raw_line == "}" {
                brace_depth -= 1;
                if brace_depth == 0 {
                    current_class_id = None;
                }
                continue;
            }

            if let Some(annot) = first_group(r"^<<(\w+)>>$", raw_line) {
                let mut cls = ensure_class(&mut class_map, &mut class_order, &active_class_id);
                cls.annotation = Some(annot);
                class_map.insert(&active_class_id, cls);
                continue;
            }

            if let Some(parsed) = parse_member(raw_line) {
                let mut cls = ensure_class(&mut class_map, &mut class_order, &active_class_id);
                if parsed.is_method {
                    cls.methods.push(parsed.member);
                } else {
                    cls.attributes.push(parsed.member);
                }
                class_map.insert(&active_class_id, cls);
            }
            continue;
        }

        if let Some(namespace_name) = first_group(r"^namespace\s+(\S+)\s*\{$", raw_line) {
            current_namespace = Some(ClassNamespace { name: namespace_name, class_ids: vec![] });
            continue;
        }

        if raw_line == "}" {
            if let Some(ns) = current_namespace.take() {
                diagram.namespaces.push(ns);
                continue;
            }
        }

        if let Some(g) = groups(r"^class\s+(\S+?)(?:\s*~(\w+)~)?\s*\{$", raw_line) {
            if let Some(id) = g.get(1).cloned() {
                let mut cls = ensure_class(&mut class_map, &mut class_order, &id);
                if let Some(generic) = g.get(2) {
                    if !generic.is_empty() {
                        cls.label = format!("{id}<{generic}>");
                    }
                }
                class_map.insert(&id, cls);
                current_class_id = Some(id.clone());
                brace_depth = 1;
                if let Some(ns) = current_namespace.as_mut() {
                    ns.class_ids.push(id);
                }
                continue;
            }
        }

        if let Some(g) = groups(r"^class\s+(\S+?)(?:\s*~(\w+)~)?\s*$", raw_line) {
            if let Some(id) = g.get(1).cloned() {
                let mut cls = ensure_class(&mut class_map, &mut class_order, &id);
                if let Some(generic) = g.get(2) {
                    if !generic.is_empty() {
                        cls.label = format!("{id}<{generic}>");
                    }
                }
                class_map.insert(&id, cls);
                if let Some(ns) = current_namespace.as_mut() {
                    ns.class_ids.push(id);
                }
                continue;
            }
        }

        if let Some(g) = groups(r"^class\s+(\S+?)\s*\{\s*<<(\w+)>>\s*\}$", raw_line) {
            if let (Some(id), Some(annot)) = (g.get(1).cloned(), g.get(2).cloned()) {
                let mut cls = ensure_class(&mut class_map, &mut class_order, &id);
                cls.annotation = Some(annot);
                class_map.insert(&id, cls);
                continue;
            }
        }

        if let Some(g) = groups(r"^(\S+?)\s*:\s*(.+)$", raw_line) {
            if let (Some(id), Some(rest)) = (g.get(1).cloned(), g.get(2).cloned()) {
                if !swift::regex_test(r"<\|--|--|\*--|o--|-->|\.\.>|\.\.\|>", &rest, false) {
                    let mut cls = ensure_class(&mut class_map, &mut class_order, &id);
                    if let Some(parsed) = parse_member(&rest) {
                        if parsed.is_method {
                            cls.methods.push(parsed.member);
                        } else {
                            cls.attributes.push(parsed.member);
                        }
                        class_map.insert(&id, cls);
                    }
                    continue;
                }
            }
        }

        if let Some(rel) = parse_relationship(raw_line) {
            let _ = ensure_class(&mut class_map, &mut class_order, &rel.from);
            let _ = ensure_class(&mut class_map, &mut class_order, &rel.to);
            diagram.relationships.push(rel);
            continue;
        }
    }

    diagram.classes = class_order.iter().filter_map(|id| class_map.get(id).cloned()).collect();
    Ok(diagram)
}

fn ensure_class(map: &mut SDict<ClassNode>, order: &mut Vec<String>, id: &str) -> ClassNode {
    if let Some(cls) = map.get(id) {
        return cls.clone();
    }
    let cls = ClassNode { id: id.to_owned(), label: id.to_owned(), attributes: vec![], methods: vec![], annotation: None };
    map.insert(id, cls.clone());
    order.push(id.to_owned());
    cls
}

fn parse_member(line: &str) -> Option<ParsedMember> {
    let trimmed = swift::regex_replace(swift::trim_whitespaces_and_newlines(line), r";$", "", false);
    if trimmed.is_empty() {
        return None;
    }

    let mut visibility = String::new();
    let mut rest = trimmed.clone();
    if let Some(first) = swift::first_grapheme(&rest) {
        if first.len() == 1 && "+-#~".contains(first) {
            visibility = first.to_owned();
            rest = swift::trim_whitespaces_and_newlines(swift::drop_first(&rest, 1)).to_owned();
        }
    }

    if let Some(g) = groups(r"^(.+?)\(([^)]*)\)(?:\s*(.+))?$", &rest) {
        if let Some(name_raw) = g.get(1) {
            let params = g.get(2).map(|p| swift::trim_whitespaces_and_newlines(p).to_owned());
            let r#type = g.get(3).map(|t| swift::trim_whitespaces_and_newlines(t).to_owned());
            let is_static = swift::has_suffix(name_raw, "$") || rest.contains('$');
            let is_abstract = swift::has_suffix(name_raw, "*") || rest.contains('*');
            let clean_name = swift::regex_replace(name_raw, r"[$*]$", "", false);
            return Some(ParsedMember {
                member: ClassMember {
                    visibility,
                    name: clean_name,
                    r#type: r#type.filter(|t| !t.is_empty()),
                    is_static,
                    is_abstract,
                    is_method: true,
                    params: params.filter(|p| !p.is_empty()),
                },
                is_method: true,
            });
        }
    }

    let parts: Vec<&str> = swift::split_character(&rest, ' ');
    let (name, r#type) = if parts.len() >= 2 {
        (parts[1..].join(" "), Some(parts[0].to_owned()))
    } else {
        (parts.first().map_or(rest.clone(), |p| (*p).to_owned()), None)
    };

    let is_static = swift::has_suffix(&name, "$");
    let is_abstract = swift::has_suffix(&name, "*");
    let clean_name = swift::regex_replace(&name, r"[$*]$", "", false);
    Some(ParsedMember {
        member: ClassMember {
            visibility,
            name: clean_name,
            r#type,
            is_static,
            is_abstract,
            is_method: false,
            params: None,
        },
        is_method: false,
    })
}

fn parse_relationship(line: &str) -> Option<ClassRelationship> {
    let pattern = r#"^(\S+?)\s+(?:"([^"]*?)"\s+)?(<\|--|<\|\.\.|\*--|o--|-->|--\*|--o|--\|>|\.\.>|\.\.\|>|<--|<\.\.?|--)\s+(?:"([^"]*?)"\s+)?(\S+?)(?:\s*:\s*(.+))?$"#;
    let g = groups(pattern, line)?;
    let from = g.get(1)?.clone();
    let arrow = g.get(3)?.clone();
    let to = g.get(5)?.clone();

    let from_cardinality = g.get(2).and_then(|v| if v.is_empty() { None } else { Some(normalize_br_tags(v)) });
    let to_cardinality = g.get(4).and_then(|v| if v.is_empty() { None } else { Some(normalize_br_tags(v)) });
    let label = g.get(6).and_then(|v| {
        let value = swift::trim_whitespaces_and_newlines(v);
        if value.is_empty() { None } else { Some(normalize_br_tags(value)) }
    });

    let (r#type, marker_at) = parse_arrow(swift::trim_whitespaces_and_newlines(&arrow))?;

    Some(ClassRelationship {
        from,
        to,
        r#type: r#type.into(),
        marker_at: marker_at.into(),
        label,
        from_cardinality,
        to_cardinality,
    })
}

fn parse_arrow(arrow: &str) -> Option<(&'static str, &'static str)> {
    Some(match arrow {
        "<|--" => ("inheritance", "from"),
        "--|>" => ("inheritance", "to"),
        "<|.." => ("realization", "from"),
        "..|>" => ("realization", "to"),
        "*--" => ("composition", "from"),
        "--*" => ("composition", "to"),
        "o--" => ("aggregation", "from"),
        "--o" => ("aggregation", "to"),
        "-->" => ("association", "to"),
        "<--" => ("association", "from"),
        "..>" => ("dependency", "to"),
        "<.." => ("dependency", "from"),
        "--" => ("association", "to"),
        _ => return None,
    })
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
