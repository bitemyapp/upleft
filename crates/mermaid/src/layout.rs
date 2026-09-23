//! Port of `Layout.swift` (`GraphLayout`).

use crate::error::MermaidError;
use crate::mermaid::src_class_layout::layout_class_diagram_sync;
use crate::mermaid::src_er_layout::layout_er_diagram_sync;
use crate::mermaid::src_layout::layout_graph_sync;
use crate::mermaid::src_sequence_layout::layout_sequence_diagram;
use crate::mermaid::src_xychart_layout::layout_xy_chart;
use crate::types::{DiagramType, LayoutConfig, MermaidGraph, Payload, PositionedContent, PositionedGraph};

#[derive(Debug, Clone, Copy, Default)]
pub struct GraphLayout {
    pub config: LayoutConfig,
}

impl GraphLayout {
    pub fn new(config: LayoutConfig) -> GraphLayout {
        GraphLayout { config }
    }

    /// `layout(_:)`.
    pub fn layout(&self, graph: &MermaidGraph) -> Result<PositionedGraph, MermaidError> {
        match graph.diagram_type {
            DiagramType::Flowchart | DiagramType::StateDiagram => layout_graph_sync(graph, &self.config),
            DiagramType::ClassDiagram => {
                let Payload::Class(parsed) = &graph.payload else {
                    return Ok(PositionedGraph::empty(graph.clone(), 0.0, 0.0));
                };
                let positioned = layout_class_diagram_sync(parsed)?;
                Ok(PositionedGraph {
                    diagram: graph.clone(),
                    width: positioned.width,
                    height: positioned.height,
                    content: PositionedContent::ClassDiagram {
                        classes: positioned.classes,
                        relationships: positioned.relationships,
                    },
                })
            }
            DiagramType::ErDiagram => {
                let Payload::Er(parsed) = &graph.payload else {
                    return Ok(PositionedGraph::empty(graph.clone(), 0.0, 0.0));
                };
                let positioned = layout_er_diagram_sync(parsed)?;
                Ok(PositionedGraph {
                    diagram: graph.clone(),
                    width: positioned.width,
                    height: positioned.height,
                    content: PositionedContent::ErDiagram {
                        entities: positioned.entities,
                        relationships: positioned.relationships,
                    },
                })
            }
            DiagramType::SequenceDiagram => {
                let Payload::Sequence(parsed) = &graph.payload else {
                    return Ok(PositionedGraph::empty(graph.clone(), 0.0, 0.0));
                };
                let positioned = layout_sequence_diagram(parsed)?;
                Ok(PositionedGraph {
                    diagram: graph.clone(),
                    width: positioned.width,
                    height: positioned.height,
                    content: PositionedContent::SequenceDiagram {
                        actors: positioned.actors,
                        messages: positioned.messages,
                        blocks: positioned.blocks,
                        lifelines: positioned.lifelines,
                        activations: positioned.activations,
                        notes: positioned.notes,
                    },
                })
            }
            DiagramType::XyChart => {
                let Payload::XyChart(chart) = &graph.payload else {
                    return Ok(PositionedGraph::empty(graph.clone(), 0.0, 0.0));
                };
                let positioned = layout_xy_chart(chart);
                Ok(PositionedGraph {
                    diagram: graph.clone(),
                    width: positioned.width,
                    height: positioned.height,
                    content: PositionedContent::XyChart(positioned),
                })
            }
        }
    }
}
