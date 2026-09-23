//! Port of `Parser.swift` (`MermaidParser`): routes a source to the parser
//! for its diagram type.

use crate::error::MermaidError;
use crate::mermaid::src_class_parser::parse_class_diagram;
use crate::mermaid::src_er_parser::parse_er_diagram;
use crate::mermaid::src_parser::parse_mermaid;
use crate::mermaid::src_sequence_parser::parse_sequence_diagram;
use crate::mermaid::src_xychart_parser::parse_xy_chart;
use crate::swift;
use crate::types::{DiagramType, MermaidGraph, Payload};

/// `_diagramLines(from:)`: split on `.newlines`, trim, drop blanks and `%%`
/// comments.
fn diagram_lines(source: &str) -> Vec<&str> {
    source
        .split(swift::is_newline)
        .map(swift::trim_whitespaces_and_newlines)
        .filter(|l| !l.is_empty() && !swift::has_prefix(l, "%%"))
        .collect()
}

/// `_decodeXMLEntities(_:)`.
fn decode_xml_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let s = swift::replacing_occurrences(s, "&amp;", "&");
    let s = swift::replacing_occurrences(&s, "&lt;", "<");
    let s = swift::replacing_occurrences(&s, "&gt;", ">");
    let s = swift::replacing_occurrences(&s, "&quot;", "\"");
    swift::replacing_occurrences(&s, "&#39;", "'")
}

/// `MermaidParser.parse(_:)`.
pub fn parse(source: &str) -> Result<MermaidGraph, MermaidError> {
    let decoded = decode_xml_entities(source);
    let lines = diagram_lines(&decoded);
    let first_line = lines.first().map_or(String::new(), |l| swift::lowercased(l));

    if swift::has_prefix(&first_line, "sequencediagram") {
        let parsed = parse_sequence_diagram(&lines)?;
        return Ok(MermaidGraph { diagram_type: DiagramType::SequenceDiagram, payload: Payload::Sequence(parsed) });
    }
    if swift::has_prefix(&first_line, "classdiagram") {
        let parsed = parse_class_diagram(&lines)?;
        return Ok(MermaidGraph { diagram_type: DiagramType::ClassDiagram, payload: Payload::Class(parsed) });
    }
    if swift::has_prefix(&first_line, "erdiagram") {
        let parsed = parse_er_diagram(&lines)?;
        return Ok(MermaidGraph { diagram_type: DiagramType::ErDiagram, payload: Payload::Er(parsed) });
    }
    if swift::has_prefix(&first_line, "xychart") {
        let chart = parse_xy_chart(&lines);
        return Ok(MermaidGraph { diagram_type: DiagramType::XyChart, payload: Payload::XyChart(chart) });
    }

    // Flowchart + stateDiagram-v2 share the same parser entry in the original TS.
    let parsed = parse_mermaid(&decoded)?;
    let parsed_type = if swift::has_prefix(&first_line, "statediagram") { DiagramType::StateDiagram } else { DiagramType::Flowchart };
    Ok(MermaidGraph { diagram_type: parsed_type, payload: parsed.payload })
}
