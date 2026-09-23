//! Ports of beautiful-mermaid-swift's `Tests/BeautifulMermaidSwiftTests` that
//! exercise the image path Downright uses (`MermaidParser`, `GraphLayout`,
//! `MermaidImageRenderer`, `DiagramRenderer`).
//!
//! Not ported: the SVG assertions (`testFlowSvgContainsNodes` and the SVG
//! halves of the others: `renderMermaidSVG` is not on Downright's path), the
//! ASCII renderer test, and the image-export "tests" that only write files.
//! Tests whose diagrams need ELK run on the engine with the `elk` feature,
//! and otherwise on Swift's recorded ELK answers (see `common`).

mod common;

use upleft_mermaid::mermaid::src_sequence_layout::layout_sequence_diagram;
use upleft_mermaid::mermaid::src_sequence_parser::parse_sequence_diagram;
use upleft_mermaid::types::PositionedContent;
use upleft_mermaid::{parser, DiagramTheme, DiagramType, GraphLayout, LayoutConfig, MermaidError, MermaidImageRenderer, Payload};

/// The test files' `lines(_:)`: split on newlines and `;`, trim, drop blanks
/// and `%%` comments.
fn lines(source: &str) -> Vec<&str> {
    source
        .split(['\n', ';'])
        .map(upleft_mermaid::swift::trim_whitespaces_and_newlines)
        .filter(|l| !l.is_empty() && !l.starts_with("%%"))
        .collect()
}

fn layout(source: &str) -> upleft_mermaid::PositionedGraph {
    let graph = parser::parse(source).expect("parses");
    GraphLayout::new(LayoutConfig::default()).layout(&graph).expect("lays out")
}

// MARK: - SequenceBrTagTests

#[test]
fn note_html_breaks_normalize_to_newlines() {
    let source = "sequenceDiagram\n    Note right of B: line 1<br/>line 2<BR>line 3<br />line 4\n    A->>B: Hello";
    let diagram = parse_sequence_diagram(&lines(source)).unwrap();
    assert_eq!(diagram.notes.len(), 1);
    assert_eq!(diagram.notes[0].text, "line 1\nline 2\nline 3\nline 4");
    assert!(!diagram.notes[0].text.contains("<br"));
}

#[test]
fn multiline_note_increases_note_height() {
    let single = "sequenceDiagram\n    participant A\n    participant B\n    Note right of B: Single line\n    A->>B: Hello";
    let multi = "sequenceDiagram\n    participant A\n    participant B\n    Note right of B: Line 1<br/>Line 2\n    A->>B: Hello";
    let single = layout_sequence_diagram(&parse_sequence_diagram(&lines(single)).unwrap()).unwrap();
    let multi = layout_sequence_diagram(&parse_sequence_diagram(&lines(multi)).unwrap()).unwrap();
    assert_eq!(single.notes.len(), 1);
    assert_eq!(multi.notes.len(), 1);
    assert!(multi.notes[0].height > single.notes[0].height);
}

// MARK: - SemicolonSeparatorTests, through MermaidParser

#[test]
fn flowchart_with_semicolon_separator() {
    let graph = parser::parse("graph LR; A --> B").unwrap();
    let Payload::Flow(model) = &graph.payload else { panic!("flow payload") };
    assert_eq!(model.nodes_in_order.len(), 2);
    assert_eq!(model.edges.len(), 1);
}

#[test]
fn flowchart_td_with_multiple_semicolons() {
    let graph = parser::parse("graph TD; A --> B; B --> C").unwrap();
    let Payload::Flow(model) = &graph.payload else { panic!("flow payload") };
    assert_eq!(model.nodes_in_order.len(), 3);
    assert_eq!(model.edges.len(), 2);
}

/// `MermaidParser` splits sequence, class and ER sources on newlines only
/// (the SVG entry point also splits on `;`), so a one-line diagram's header
/// is invalid there.
#[test]
fn sequence_and_er_with_semicolon_have_invalid_headers() {
    assert!(matches!(
        parser::parse("sequenceDiagram; Alice ->> Bob: hi"),
        Err(MermaidError::SequenceInvalidHeader { .. })
    ));
    assert!(matches!(parser::parse("erDiagram; CUSTOMER ||--o{ ORDER : places"), Err(MermaidError::ErInvalidHeader { .. })));
}

#[test]
fn newline_separated_diagrams_still_work() {
    let graph = parser::parse("graph LR\n    A --> B\n    B --> C").unwrap();
    assert_eq!(graph.diagram_type, DiagramType::Flowchart);
    let seq = layout("sequenceDiagram\n    Alice ->> Bob: hello");
    let PositionedContent::SequenceDiagram { actors, messages, .. } = &seq.content else { panic!("sequence") };
    assert_eq!(actors.len(), 2);
    assert_eq!(messages.len(), 1);
}

// MARK: - XYChartCrashRegressionTests, through the image path

fn render(source: &str) -> Option<(usize, usize)> {
    let renderer = MermaidImageRenderer::new(DiagramTheme::zinc_light(), LayoutConfig::default());
    let prepared = renderer.prepare(source).ok().flatten()?;
    let image = upleft_mermaid::downright::mermaid_renderer_bridge::render(&prepared, 2.0)?;
    Some((
        objc2_core_graphics::CGImage::width(Some(&image.cg_image)),
        objc2_core_graphics::CGImage::height(Some(&image.cg_image)),
    ))
}

#[test]
fn data_longer_than_categories_does_not_crash() {
    for source in [
        "xychart-beta\n    title \"Vertical mixed\"\n    x-axis [Jan, Feb, Mar]\n    y-axis \"Revenue\" 0 --> 1000\n    bar [100, 200, 300, 400, 500]\n    line [150, 250, 350, 450, 550]",
        "xychart-beta horizontal\n    title \"Horizontal mixed\"\n    x-axis [Jan, Feb, Mar]\n    y-axis \"Revenue\" 0 --> 1000\n    bar [100, 200, 300, 400, 500]\n    line [150, 250, 350, 450, 550]",
    ] {
        let (w, h) = render(source).expect("renders");
        assert!(w > 0 && h > 0);
    }
}

#[test]
fn zero_y_range_renders() {
    let source = "xychart-beta\n    title \"Inf coords\"\n    x-axis [A, B, C]\n    y-axis \"v\" 0 --> 0\n    bar [1, 2, 3]\n    line [1, 2, 3]";
    assert!(render(source).is_some());
}

/// Values near `1e300` overflow the nice ticks to infinity and the
/// renderer's `Int((xBase / 20).rounded())` traps in Swift; the port traps
/// the same way (corpus/mermaid-traps).
#[test]
#[should_panic(expected = "traps")]
fn overflowing_coordinates_trap_like_swift() {
    let source = "xychart-beta\n    title \"Overflow coords\"\n    x-axis [A, B, C]\n    y-axis \"v\" 0 --> 1e308\n    bar [1e300, 2e300, 3e300]\n    line [1e300, 2e300, 3e300]";
    let _ = render(source);
}

// MARK: - BeautifulMermaidSwiftTests (ELK-backed)

#[test]
fn render_image_for_simple_flow_is_non_nil() {
    let source = "graph TD\n  A[Start] --> B[End]";
    assert!(common::with_elk(source, 1, || render(source)).is_some());
}

#[test]
fn flow15_subgraph_direction_lays_out() {
    let source = "graph TD\n  subgraph pipeline [Processing Pipeline]\n    direction LR\n    A[Input] --> B[Parse] --> C[Transform] --> D[Output]\n  end\n  E[Source] --> A\n  D --> F[Sink]";
    assert!(common::with_elk(source, 1, || render(source)).is_some());
}

#[test]
fn state2_composite_lays_out() {
    let source = "stateDiagram-v2\n  [*] --> Idle\n  Idle --> Processing : submit\n  state Processing {\n    parse --> validate\n    validate --> execute\n  }\n  Processing --> Complete : done\n  Processing --> Error : fail\n  Error --> Idle : retry\n  Complete --> [*]";
    assert!(common::with_elk(source, 1, || render(source)).is_some());
}

#[test]
fn flow6_edge_styles_and_flow8_bidirectional_labels() {
    for source in [
        "graph TD\n  A[Source] -->|solid| B[Target 1]\n  A -.->|dotted| C[Target 2]\n  A ==>|thick| D[Target 3]",
        "graph LR\n  A[Client] <-->|sync| B[Server]\n  B <-.->|heartbeat| C[Monitor]\n  C <==>|data| D[Storage]",
    ] {
        assert!(common::with_elk(source, 1, || render(source)).is_some());
    }
}

#[test]
fn simple_td_node_order() {
    let source = "graph TD\n  A[Start] --> B[End]";
    let pos = common::with_elk(source, 1, || layout(source));
    let (nodes, _, _) = pos.flowchart().unwrap();
    let a = nodes.iter().find(|n| n.id == "A").unwrap();
    let b = nodes.iter().find(|n| n.id == "B").unwrap();
    assert!(a.y < b.y, "In graph TD, source A should be above target B (lower y)");
}

#[test]
fn simple_bt_node_order() {
    let source = "graph BT\n  A[Foundation] --> B[Layer 2] --> C[Top]";
    let pos = common::with_elk(source, 1, || layout(source));
    let (nodes, _, _) = pos.flowchart().unwrap();
    let a = nodes.iter().find(|n| n.id == "A").unwrap();
    let c = nodes.iter().find(|n| n.id == "C").unwrap();
    assert!(a.y > c.y, "In graph BT, source A should be below target C (higher y)");
}

#[test]
fn flow14_nested_subgraphs_lay_out() {
    let source = "graph TD\n  subgraph Cloud\n    subgraph us-east [US East Region]\n      A[Web Server] --> B[App Server]\n    end\n    subgraph us-west [US West Region]\n      C[Web Server] --> D[App Server]\n    end\n  end\n  E[Load Balancer] --> A\n  E --> C";
    let pos = common::with_elk(source, 1, || layout(source));
    let (nodes, edges, groups) = pos.flowchart().unwrap();
    assert_eq!(nodes.len(), 5);
    assert_eq!(edges.len(), 4);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].children.len(), 2);
}
