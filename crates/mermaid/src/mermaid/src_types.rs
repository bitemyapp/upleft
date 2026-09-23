//! Port of `Mermaid/src_types.swift` (from `original/src/types.ts`): the
//! parsed flowchart / state-diagram model (`original_src_types`).
//!
//! `MermaidSubgraph` is a Swift class, but the parser never aliases one: a
//! subgraph lives on the parser's stack and then in exactly one `children`
//! (or `subgraphs`) array, so a value type behaves identically.

use std::collections::HashMap;

use crate::swift::{string_key, string_eq};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    TD,
    TB,
    LR,
    BT,
    RL,
}

impl Direction {
    pub fn raw_value(self) -> &'static str {
        match self {
            Direction::TD => "TD",
            Direction::TB => "TB",
            Direction::LR => "LR",
            Direction::BT => "BT",
            Direction::RL => "RL",
        }
    }

    /// `Direction(rawValue:)`.
    pub fn from_raw(raw: &str) -> Option<Direction> {
        Some(match raw {
            "TD" => Direction::TD,
            "TB" => Direction::TB,
            "LR" => Direction::LR,
            "BT" => Direction::BT,
            "RL" => Direction::RL,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeShape {
    Rectangle,
    Rounded,
    Diamond,
    Stadium,
    Circle,
    Subroutine,
    Doublecircle,
    Hexagon,
    Cylinder,
    Asymmetric,
    Trapezoid,
    TrapezoidAlt,
    StateStart,
    StateEnd,
}

impl NodeShape {
    pub fn raw_value(self) -> &'static str {
        match self {
            NodeShape::Rectangle => "rectangle",
            NodeShape::Rounded => "rounded",
            NodeShape::Diamond => "diamond",
            NodeShape::Stadium => "stadium",
            NodeShape::Circle => "circle",
            NodeShape::Subroutine => "subroutine",
            NodeShape::Doublecircle => "doublecircle",
            NodeShape::Hexagon => "hexagon",
            NodeShape::Cylinder => "cylinder",
            NodeShape::Asymmetric => "asymmetric",
            NodeShape::Trapezoid => "trapezoid",
            NodeShape::TrapezoidAlt => "trapezoid-alt",
            NodeShape::StateStart => "state-start",
            NodeShape::StateEnd => "state-end",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStyle {
    Solid,
    Dotted,
    Thick,
}

impl EdgeStyle {
    pub fn raw_value(self) -> &'static str {
        match self {
            EdgeStyle::Solid => "solid",
            EdgeStyle::Dotted => "dotted",
            EdgeStyle::Thick => "thick",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MermaidNode {
    pub id: String,
    pub label: String,
    pub shape: NodeShape,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MermaidEdge {
    pub source: String,
    pub target: String,
    pub label: Option<String>,
    pub style: EdgeStyle,
    pub has_arrow_start: bool,
    pub has_arrow_end: bool,
    pub inline_style: Option<SDict<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MermaidSubgraph {
    pub id: String,
    pub label: String,
    pub node_ids: Vec<String>,
    pub children: Vec<MermaidSubgraph>,
    pub direction: Option<Direction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MermaidGraph {
    pub direction: Direction,
    /// Ordered node list (TS Map insertion order).
    pub nodes_in_order: Vec<(String, MermaidNode)>,
    pub edges: Vec<MermaidEdge>,
    pub subgraphs: Vec<MermaidSubgraph>,
    pub class_defs: SDict<SDict<String>>,
    pub class_assignments: SDict<String>,
    pub node_styles: SDict<SDict<String>>,
    /// Edge index (or -1 for `default`) → inline styles from `linkStyle`.
    pub link_styles: HashMap<i64, SDict<String>>,
}

impl MermaidGraph {
    /// `nodesById`.
    pub fn nodes_by_id(&self) -> SDict<MermaidNode> {
        let mut map = SDict::new();
        for (id, node) in &self.nodes_in_order {
            map.insert(id, node.clone());
        }
        map
    }
}

/// A Swift `[String: V]`: keys compare under canonical equivalence, and an
/// update keeps the key first stored.
#[derive(Debug, Clone, Default)]
pub struct SDict<V> {
    map: HashMap<String, (String, V)>,
}

impl<V: PartialEq> PartialEq for SDict<V> {
    fn eq(&self, other: &Self) -> bool {
        self.map.len() == other.map.len()
            && self.map.iter().all(|(k, (_, v))| other.map.get(k).is_some_and(|(_, w)| v == w))
    }
}

impl<V> SDict<V> {
    pub fn new() -> Self {
        SDict { map: HashMap::new() }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        if key.is_ascii() {
            return self.map.get(key).map(|(_, v)| v);
        }
        self.map.get(&string_key(key)).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        if key.is_ascii() {
            return self.map.get_mut(key).map(|(_, v)| v);
        }
        self.map.get_mut(&string_key(key)).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `dict[key] = value`.
    pub fn insert(&mut self, key: &str, value: V) {
        let k = string_key(key);
        match self.map.get_mut(&k) {
            Some(entry) => entry.1 = value,
            None => {
                self.map.insert(k, (key.to_owned(), value));
            }
        }
    }

    /// `dict.removeValue(forKey:)`.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        self.map.remove(&string_key(key)).map(|(_, v)| v)
    }

    /// `dict[key, default: d]` for mutation.
    pub fn entry_or(&mut self, key: &str, default: impl FnOnce() -> V) -> &mut V {
        let k = string_key(key);
        &mut self.map.entry(k).or_insert_with(|| (key.to_owned(), default())).1
    }

    /// The stored (original) keys and values, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.map.values().map(|(k, v)| (k.as_str(), v))
    }

    /// Keys sorted by Swift's `String <`.
    pub fn sorted_keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self.map.values().map(|(k, _)| k.as_str()).collect();
        keys.sort_by(|a, b| upleft_swift_text::str_cmp(a, b));
        keys
    }
}

/// A Swift `Set<String>`.
#[derive(Debug, Clone, Default)]
pub struct SSet {
    set: std::collections::HashSet<String>,
}

impl SSet {
    pub fn new() -> Self {
        SSet::default()
    }

    pub fn insert(&mut self, value: &str) {
        self.set.insert(string_key(value));
    }

    pub fn contains(&self, value: &str) -> bool {
        if value.is_ascii() {
            return self.set.contains(value);
        }
        self.set.contains(&string_key(value))
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

/// `array.contains(value)` on `[String]`.
pub fn strings_contain(values: &[String], value: &str) -> bool {
    values.iter().any(|v| string_eq(v, value))
}
