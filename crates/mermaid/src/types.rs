//! Port of `Types.swift`: the diagram model the public API passes around.

use crate::mermaid::src_class_parser::{ClassDiagram, PositionedClassNode, PositionedClassRelationship};
use crate::mermaid::src_er_parser::{ErDiagram, PositionedErEntity, PositionedErRelationship};
use crate::mermaid::src_layout::{PositionedEdgePayload, PositionedGroupPayload, PositionedNodePayload};
use crate::mermaid::src_sequence_parser::{
    PositionedSequenceActor, PositionedSequenceBlock, PositionedSequenceMessage, PositionedSequenceNote,
    SequenceActivation, SequenceDiagram, SequenceLifeline,
};
use crate::mermaid::src_types::MermaidGraph as ParsedGraphModel;
use crate::mermaid::src_xychart_types::{PositionedXYChart, XYChart};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramType {
    Flowchart,
    StateDiagram,
    SequenceDiagram,
    ClassDiagram,
    ErDiagram,
    XyChart,
}

impl DiagramType {
    pub fn raw_value(self) -> &'static str {
        match self {
            DiagramType::Flowchart => "flowchart",
            DiagramType::StateDiagram => "stateDiagram",
            DiagramType::SequenceDiagram => "sequenceDiagram",
            DiagramType::ClassDiagram => "classDiagram",
            DiagramType::ErDiagram => "erDiagram",
            DiagramType::XyChart => "xyChart",
        }
    }
}

/// `MermaidGraph.payload`: the Swift `Any?`, which always holds the model
/// matching `type` on every path the library takes.
#[derive(Debug, Clone, PartialEq)]
pub enum Payload {
    Flow(ParsedGraphModel),
    Sequence(SequenceDiagram),
    Class(ClassDiagram),
    Er(ErDiagram),
    XyChart(XYChart),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MermaidGraph {
    pub diagram_type: DiagramType,
    pub payload: Payload,
}

pub type PositionedNode = PositionedNodePayload;
pub type PositionedEdge = PositionedEdgePayload;
pub type PositionedGroup = PositionedGroupPayload;

#[derive(Debug, Clone, PartialEq)]
pub enum PositionedContent {
    Flowchart { nodes: Vec<PositionedNode>, edges: Vec<PositionedEdge>, groups: Vec<PositionedGroup> },
    StateDiagram { nodes: Vec<PositionedNode>, edges: Vec<PositionedEdge>, groups: Vec<PositionedGroup> },
    SequenceDiagram {
        actors: Vec<PositionedSequenceActor>,
        messages: Vec<PositionedSequenceMessage>,
        blocks: Vec<PositionedSequenceBlock>,
        lifelines: Vec<SequenceLifeline>,
        activations: Vec<SequenceActivation>,
        notes: Vec<PositionedSequenceNote>,
    },
    ClassDiagram { classes: Vec<PositionedClassNode>, relationships: Vec<PositionedClassRelationship> },
    ErDiagram { entities: Vec<PositionedErEntity>, relationships: Vec<PositionedErRelationship> },
    XyChart(PositionedXYChart),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedGraph {
    pub diagram: MermaidGraph,
    pub width: f64,
    pub height: f64,
    pub content: PositionedContent,
}

impl PositionedGraph {
    /// `PositionedGraph(diagram:width:height:)`: empty content for the type.
    pub fn empty(diagram: MermaidGraph, width: f64, height: f64) -> PositionedGraph {
        let content = match diagram.diagram_type {
            DiagramType::Flowchart => PositionedContent::Flowchart { nodes: vec![], edges: vec![], groups: vec![] },
            DiagramType::StateDiagram => PositionedContent::StateDiagram { nodes: vec![], edges: vec![], groups: vec![] },
            DiagramType::SequenceDiagram => PositionedContent::SequenceDiagram {
                actors: vec![],
                messages: vec![],
                blocks: vec![],
                lifelines: vec![],
                activations: vec![],
                notes: vec![],
            },
            DiagramType::ClassDiagram => PositionedContent::ClassDiagram { classes: vec![], relationships: vec![] },
            DiagramType::ErDiagram => PositionedContent::ErDiagram { entities: vec![], relationships: vec![] },
            DiagramType::XyChart => PositionedContent::XyChart(PositionedXYChart::empty()),
        };
        PositionedGraph { diagram, width, height, content }
    }

    /// `flowchartNodes`, `flowchartEdges`, `flowchartGroups`.
    pub fn flowchart(&self) -> Option<(&[PositionedNode], &[PositionedEdge], &[PositionedGroup])> {
        match &self.content {
            PositionedContent::Flowchart { nodes, edges, groups } | PositionedContent::StateDiagram { nodes, edges, groups } => {
                Some((nodes, edges, groups))
            }
            _ => None,
        }
    }
}

/// `LayoutConfig`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutConfig {
    /// Padding around the diagram (default: 40, matches TS/ELK).
    pub padding: f64,
    /// Horizontal space between nodes in the same layer (default: 28).
    pub node_spacing: f64,
    /// Vertical space between layers (default: 48).
    pub layer_spacing: f64,
    /// Space between disconnected components (default: 20).
    pub component_spacing: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        LayoutConfig { padding: 40.0, node_spacing: 28.0, layer_spacing: 48.0, component_spacing: 20.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    Dotted,
    Dashed,
    Thick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowHead {
    None,
    Arrow,
    Open,
    Circle,
    Cross,
    Diamond,
}

/// `EdgeStyle` (the renderer's, not the parsed model's).
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeStyle {
    pub line_style: LineStyle,
    pub source_arrow: ArrowHead,
    pub target_arrow: ArrowHead,
    pub color: Option<String>,
    /// Explicit stroke width from a `linkStyle` directive (e.g. "2px" → 2.0).
    pub stroke_width: Option<f64>,
}

impl Default for EdgeStyle {
    fn default() -> Self {
        EdgeStyle {
            line_style: LineStyle::Solid,
            source_arrow: ArrowHead::None,
            target_arrow: ArrowHead::Arrow,
            color: None,
            stroke_width: None,
        }
    }
}
