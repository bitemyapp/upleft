//! Port of elk-swift's `Tests/ElkSwiftTests/ElkPhaseSnapshotTest.swift`:
//! steps through the layered pipeline processor by processor with the
//! layout-test API and captures a JSON snapshot of the layered graph after
//! each of the five main phases (P1–P5).
//!
//! The snapshot is written to `$PHASE_SNAPSHOT_OUTPUT` (default: the system
//! temporary directory's `upleft-elk-phase-snapshot.json`).
//! `PHASE_SNAPSHOT_DIAGRAM` picks a built-in diagram (default
//! `class-13-labels`); `PHASE_SNAPSHOT_JSON` supplies ELK JSON inline.
//!
//! In elk-swift every processor is wrapped in an `AnyGraphProcessor`, so
//! `String(describing: type(of: processor))` names the wrapper and no phase
//! is ever classified; the port classifies by the wrapped processor's name,
//! which is what the test means, and additionally asserts that all five
//! phases were seen.

use serde_json::{json, Map, Value};
use upleft_elk::bridge::elk_graph_impl::ElkGraph;
use upleft_elk::bridge::json_importer::JsonImporter;
use upleft_elk::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LNodeId};
use upleft_elk::org::eclipse::elk::alg::layered::layered_layout_provider::LayeredLayoutProvider;
use upleft_elk::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use upleft_elk::org::eclipse::elk::graph::properties::property::PropValue;

const CLASS_13_LABELS: &str = r#"{"id":"root","layoutOptions":{"elk.algorithm":"layered","elk.direction":"DOWN","elk.spacing.nodeNode":"40","elk.layered.spacing.nodeNodeBetweenLayers":"60","elk.padding":"[top=40,left=40,bottom=40,right=40]","elk.edgeRouting":"ORTHOGONAL","elk.edgeLabels.placement":"CENTER"},"children":[{"id":"Teacher","width":120,"height":68},{"id":"Student","width":120,"height":68},{"id":"Course","width":120,"height":68}],"edges":[{"id":"e0","sources":["Teacher"],"targets":["Course"],"labels":[{"text":"teaches","width":56,"height":21}]},{"id":"e1","sources":["Student"],"targets":["Course"],"labels":[{"text":"enrolled in","width":82,"height":21}]}]}"#;

/// Processor names → phases.
const PHASE_PATTERNS: [(&str, &str); 16] = [
    ("CycleBreaker", "p1-cycle-breaking"),
    ("InteractiveCycleBreaker", "p1-cycle-breaking"),
    ("GreedyCycleBreaker", "p1-cycle-breaking"),
    ("DepthFirstCycleBreaker", "p1-cycle-breaking"),
    ("NetworkSimplexLayerer", "p2-layering"),
    ("LongestPathLayerer", "p2-layering"),
    ("InteractiveLayerer", "p2-layering"),
    ("CoffmanGrahamLayerer", "p2-layering"),
    ("LayerSweepCrossingMinimizer", "p3-crossing-minimization"),
    ("InteractiveCrossingMinimizer", "p3-crossing-minimization"),
    ("BKNodePlacer", "p4-node-placement"),
    ("LinearSegmentsNodePlacer", "p4-node-placement"),
    ("NetworkSimplexPlacer", "p4-node-placement"),
    ("OrthogonalEdgeRouter", "p5-edge-routing"),
    ("PolylineEdgeRouter", "p5-edge-routing"),
    ("SplineEdgeRouter", "p5-edge-routing"),
];

const PHASE_KEYS: [&str; 5] = ["p1-cycle-breaking", "p2-layering", "p3-crossing-minimization", "p4-node-placement", "p5-edge-routing"];

fn classify_processor(name: &str) -> Option<&'static str> {
    PHASE_PATTERNS.iter().find(|(pattern, _)| name.contains(pattern)).map(|&(_, phase)| phase)
}

fn origin_identifier(lg: &LGraphArena, elk: &ElkGraph, node: LNodeId, allow_label: bool) -> Option<String> {
    let Some(PropValue::ElkNode(origin)) = lg[node].props.get(&InternalProperties::ORIGIN) else { return None };
    if let Some(id) = elk[origin].identifier.as_ref().filter(|id| !id.is_empty()) {
        return Some(id.clone());
    }
    if allow_label {
        let text = elk[origin].labels.first().map(|&l| elk[l].text.clone()).unwrap_or_default();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

fn capture_snapshot(lg: &LGraphArena, elk: &ElkGraph, graphs: &[LGraphId], slot_index: usize, processor_name: &str) -> Value {
    let Some(&lgraph) = graphs.first() else {
        return json!({"slotIndex": slot_index, "processor": processor_name, "layers": []});
    };

    let mut layers_data = Vec::new();
    for (li, &layer) in lg[lgraph].layers.iter().enumerate() {
        let mut nodes_data = Vec::new();
        for &node in &lg[layer].nodes {
            let n = &lg[node];
            let mut node_dict = Map::new();
            node_dict.insert("id".into(), json!(n.id));
            node_dict.insert("type".into(), json!(format!("{:?}", n.node_type)));
            node_dict.insert("position".into(), json!({"x": n.position.x, "y": n.position.y}));
            node_dict.insert("size".into(), json!({"w": n.size.x, "h": n.size.y}));
            if let Some(origin) = origin_identifier(lg, elk, node, true) {
                node_dict.insert("origin".into(), json!(origin));
            }
            let mut ports_data = Vec::new();
            for &port in &n.ports {
                let p = &lg[port];
                let incoming: Vec<Value> = p
                    .incoming_edges
                    .iter()
                    .filter_map(|&e| lg[e].source)
                    .map(|src| json!({"sourceNode": lg[src].owner.map_or(-1, |n| lg[n].id), "sourcePort": lg[src].id}))
                    .collect();
                let outgoing: Vec<Value> = p
                    .outgoing_edges
                    .iter()
                    .filter_map(|&e| lg[e].target)
                    .map(|tgt| json!({"targetNode": lg[tgt].owner.map_or(-1, |n| lg[n].id), "targetPort": lg[tgt].id}))
                    .collect();
                ports_data.push(json!({
                    "id": p.id,
                    "side": format!("{:?}", p.side),
                    "position": {"x": p.position.x, "y": p.position.y},
                    "incoming": incoming,
                    "outgoing": outgoing,
                }));
            }
            node_dict.insert("ports".into(), Value::Array(ports_data));
            nodes_data.push(Value::Object(node_dict));
        }
        layers_data.push(json!({"index": li, "nodes": nodes_data}));
    }

    let mut layerless_data = Vec::new();
    for &node in &lg[lgraph].layerless_nodes {
        let mut d = Map::new();
        d.insert("id".into(), json!(lg[node].id));
        d.insert("type".into(), json!(format!("{:?}", lg[node].node_type)));
        if let Some(origin) = origin_identifier(lg, elk, node, false) {
            d.insert("origin".into(), json!(origin));
        }
        layerless_data.push(Value::Object(d));
    }

    let mut result = Map::new();
    result.insert("slotIndex".into(), json!(slot_index));
    result.insert("processor".into(), json!(processor_name));
    result.insert("layers".into(), Value::Array(layers_data));
    if !layerless_data.is_empty() {
        result.insert("layerlessNodes".into(), Value::Array(layerless_data));
    }
    Value::Object(result)
}

#[test]
fn capture_phase_snapshots() {
    let diagram_id = std::env::var("PHASE_SNAPSHOT_DIAGRAM").unwrap_or_else(|_| "class-13-labels".into());
    let output_path = std::env::var("PHASE_SNAPSHOT_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("upleft-elk-phase-snapshot.json"));

    let json_string = match std::env::var("PHASE_SNAPSHOT_JSON") {
        Ok(inline) => inline,
        Err(_) if diagram_id == "class-13-labels" => CLASS_13_LABELS.to_string(),
        Err(_) => panic!("Unknown diagram: {diagram_id}"),
    };

    // Parse and import.
    let graph: Value = serde_json::from_str(&json_string).expect("valid JSON");
    let mut elk = ElkGraph::new();
    let root = JsonImporter::new().transform(&mut elk, graph.as_object().expect("a JSON object"));

    // Prepare: configures the graph and splits it into components.
    let mut provider = LayeredLayoutProvider::new();
    let (mut lg, mut state) = provider.start_layout_test(&mut elk, root).expect("import");
    let elk_layered = provider.get_layout_algorithm();
    let processors = elk_layered.get_layout_test_configuration(&lg, &state);
    assert!(!processors.is_empty(), "the configured pipeline has processors");

    let mut all_processors = Vec::new();
    let mut phase_snapshots = Map::new();
    for (index, &name) in processors.iter().enumerate() {
        elk_layered.run_layout_test_step(&mut lg, &mut state);
        let snapshot = capture_snapshot(&lg, &elk, state.get_graphs(), index, name);
        let mut entry = Map::new();
        entry.insert("slot".into(), json!(index));
        entry.insert("processor".into(), json!(name));
        if let Some(phase) = classify_processor(name) {
            entry.insert("phase".into(), json!(phase));
            // Keep the last snapshot per phase (the main phase processor).
            phase_snapshots.insert(phase.into(), snapshot);
        }
        all_processors.push(Value::Object(entry));
    }
    assert!(elk_layered.is_layout_test_finished(&lg, &state));
    assert_eq!(state.get_step(), processors.len());

    let mut phases = Map::new();
    for key in PHASE_KEYS {
        let snapshot = phase_snapshots.get(key).unwrap_or_else(|| panic!("no snapshot for phase {key}"));
        phases.insert(key.into(), snapshot.clone());
    }

    let output = json!({
        "diagram": diagram_id,
        "source": "rust",
        "allProcessors": all_processors,
        "phases": phases,
    });
    std::fs::write(&output_path, serde_json::to_string_pretty(&output).unwrap()).expect("write snapshot");

    // The built-in diagram (laid out internally left to right): the two
    // centre edge labels become label dummies in a middle layer. Positions
    // are elk-swift's (the instrumented lab's per-processor trace).
    if std::env::var("PHASE_SNAPSHOT_JSON").is_err() {
        let layer_sizes = |phase: &str| -> Vec<usize> {
            phases[phase]["layers"].as_array().unwrap().iter().map(|l| l["nodes"].as_array().unwrap().len()).collect()
        };
        assert_eq!(layer_sizes("p2-layering"), [2, 2, 1]);
        let nodes = |phase: &str| -> Vec<(String, f64, f64)> {
            let mut out = Vec::new();
            for layer in phases[phase]["layers"].as_array().unwrap() {
                for node in layer["nodes"].as_array().unwrap() {
                    let name = node.get("origin").and_then(Value::as_str).unwrap_or_else(|| node["type"].as_str().unwrap());
                    out.push((name.to_string(), node["position"]["x"].as_f64().unwrap(), node["position"]["y"].as_f64().unwrap()));
                }
            }
            out
        };
        let expect = |list: &[(&str, f64, f64)]| -> Vec<(String, f64, f64)> { list.iter().map(|&(n, x, y)| (n.to_string(), x, y)).collect() };
        assert_eq!(
            nodes("p3-crossing-minimization"),
            expect(&[("Student", 0.0, 0.0), ("Teacher", 0.0, 0.0), ("LABEL", 0.0, 0.0), ("LABEL", 0.0, 0.0), ("Course", 0.0, 0.0)])
        );
        assert_eq!(
            nodes("p4-node-placement"),
            expect(&[("Student", 0.0, 0.0), ("Teacher", 0.0, 160.0), ("LABEL", 0.0, 60.0), ("LABEL", 0.0, 220.0), ("Course", 0.0, 20.0)])
        );
        assert_eq!(
            nodes("p5-edge-routing"),
            expect(&[("Student", 0.0, 0.0), ("Teacher", 0.0, 160.0), ("LABEL", 128.0, 60.0), ("LABEL", 128.0, 220.0), ("Course", 209.0, 20.0)])
        );
    }
}
