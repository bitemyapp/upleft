//! Golden tests for node sizing (`LabelAndNodeSizeProcessor`), node margins
//! (`InnermostNodeMarginCalculator`, `NodeMarginCalculator`), end labels
//! (`EndLabelPreprocessor`, `EndLabelSorter`, `EndLabelPostprocessor`) and
//! `LabelSideSelector`.
//!
//! Each `tests/data/group_e/<case>.json` describes a small layered graph and a
//! list of steps. The `<case>.golden` next to it is what elk-swift printed for
//! the same spec (a harness executable in a copy of the instrumented elk-swift
//! lab that builds the `LGraph` with the Swift API, runs the same processors
//! and dumps every coordinate with Swift's `Double.description`). This file
//! interprets the spec with the Rust port and compares the dumps verbatim.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use serde_json::Value;
use upleft_elk::org::eclipse::elk::alg::common::nodespacing::node_dimension_calculation::NodeDimensionCalculation;
use upleft_elk::org::eclipse::elk::alg::common::nodespacing::node_label_and_size_calculator::NodeLabelAndSizeCalculator;
use upleft_elk::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LGraphAdapters;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::end_label_postprocessor::EndLabelPostprocessor;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::end_label_preprocessor::{EndLabelCells, EndLabelPreprocessor};
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::end_label_sorter::EndLabelSorter;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::innermost_node_margin_calculator::InnermostNodeMarginCalculator;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::label_and_node_size_processor::LabelAndNodeSizeProcessor;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::label_side_selector::LabelSideSelector;
use upleft_elk::org::eclipse::elk::alg::layered::options::edge_label_side_selection::EdgeLabelSideSelection;
use upleft_elk::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use upleft_elk::org::eclipse::elk::core::math::elk_margin::ElkMargin;
use upleft_elk::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use upleft_elk::org::eclipse::elk::core::math::spacing::Spacing;
use upleft_elk::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use upleft_elk::org::eclipse::elk::core::options::label_side::LabelSide;
use upleft_elk::org::eclipse::elk::core::options::node_label_placement::NodeLabelPlacement;
use upleft_elk::org::eclipse::elk::core::options::port_alignment::PortAlignment;
use upleft_elk::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use upleft_elk::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use upleft_elk::org::eclipse::elk::core::options::size_options::SizeOptions;
use upleft_elk::org::eclipse::elk::core::util::basic_progress_monitor::BasicProgressMonitor;
use upleft_elk::prelude::*;

fn norm(s: &str) -> String {
    s.to_lowercase().replace('_', "")
}

fn pick<T: Copy + std::fmt::Debug>(name: &str, cases: &[T]) -> T {
    let n = norm(name);
    *cases.iter().find(|c| norm(&format!("{c:?}")) == n).unwrap_or_else(|| panic!("unknown case {name}"))
}

fn d(v: f64) -> String {
    swift::describe_double(v)
}

fn vec(v: KVector) -> String {
    format!("({},{})", d(v.x), d(v.y))
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

fn kv(v: &Value) -> KVector {
    let a = v.as_array().unwrap();
    KVector::new(num(&a[0]), num(&a[1]))
}

#[derive(Default)]
struct Ids {
    nodes: HashMap<String, LNodeId>,
    ports: HashMap<String, LPortId>,
    edges: HashMap<String, LEdgeId>,
    labels: HashMap<String, LLabelId>,
    node_names: HashMap<LNodeId, String>,
    port_names: HashMap<LPortId, String>,
    edge_names: HashMap<LEdgeId, String>,
}

#[derive(Clone, Copy)]
enum Holder {
    Graph(LGraphId),
    Node(LNodeId),
    Port(LPortId),
    Edge(LEdgeId),
    Label(LLabelId),
}

fn props_of(lg: &mut LGraphArena, h: Holder) -> &mut PropertyMap {
    match h {
        Holder::Graph(g) => &mut lg[g].props,
        Holder::Node(n) => &mut lg[n].props,
        Holder::Port(p) => &mut lg[p].props,
        Holder::Edge(e) => &mut lg[e].props,
        Holder::Label(l) => &mut lg[l].props,
    }
}

fn value(ty: &str, raw: &Value) -> PropValue {
    match ty {
        "Double" => PropValue::Double(num(raw)),
        "Int" => PropValue::Int(num(raw) as i64),
        "Bool" => PropValue::Bool(raw.as_bool().unwrap()),
        "String" => PropValue::from(raw.as_str().unwrap()),
        "SizeConstraint" => PropValue::SizeConstraint(SizeConstraint::from_raw(num(raw) as i64)),
        "SizeOptions" => PropValue::SizeOptions(SizeOptions::from_raw(num(raw) as i64)),
        "NodeLabelPlacement" => PropValue::NodeLabelPlacement(NodeLabelPlacement::from_raw(num(raw) as i64)),
        "PortLabelPlacement" => PropValue::PortLabelPlacement(PortLabelPlacement::from_raw(num(raw) as i64)),
        "PortConstraints" => PropValue::PortConstraints(pick(raw.as_str().unwrap(), &PortConstraints::ALL)),
        "PortAlignment" => PropValue::PortAlignment(pick(raw.as_str().unwrap(), &PortAlignment::ALL)),
        "Direction" => PropValue::Direction(pick(raw.as_str().unwrap(), &Direction::ALL)),
        "EdgeLabelPlacement" => PropValue::EdgeLabelPlacement(pick(raw.as_str().unwrap(), &EdgeLabelPlacement::ALL)),
        "EdgeLabelSideSelection" => PropValue::EdgeLabelSideSelection(pick(raw.as_str().unwrap(), &EdgeLabelSideSelection::ALL)),
        "LabelSide" => PropValue::LabelSide(pick(raw.as_str().unwrap(), &LabelSide::ALL)),
        "PortSide" => PropValue::PortSide(pick(raw.as_str().unwrap(), &PortSide::ALL)),
        "ElkPadding" => {
            let a = raw.as_array().unwrap();
            PropValue::elk_padding(ElkPadding::new(num(&a[0]), num(&a[1]), num(&a[2]), num(&a[3])))
        }
        "ElkMargin" => {
            let a = raw.as_array().unwrap();
            PropValue::elk_margin(ElkMargin::new(num(&a[0]), num(&a[1]), num(&a[2]), num(&a[3])))
        }
        "KVector" => PropValue::kvector(kv(raw)),
        "GraphProperties" => {
            let mut s = EnumSet::<GraphProperties>::new();
            for n in raw.as_array().unwrap() {
                s.insert(pick(n.as_str().unwrap(), &GraphProperties::ALL));
            }
            PropValue::GraphPropertiesSet(s)
        }
        _ => panic!("unknown type {ty}"),
    }
}

type Deferred = Vec<(Holder, String, String, Value)>;

fn set_props(lg: &mut LGraphArena, h: Holder, props: Option<&Value>, deferred: &mut Deferred) {
    for p in props.and_then(|p| p.as_array()).into_iter().flatten() {
        let p = p.as_array().unwrap();
        let key = p[0].as_str().unwrap();
        let ty = p[1].as_str().unwrap();
        match ty {
            "EdgeRef" | "PortRef" | "NodeRef" | "LabelRefs" => deferred.push((h, key.to_string(), ty.to_string(), p[2].clone())),
            _ => {
                let v = value(ty, &p[2]);
                props_of(lg, h).set_by_id(key, Some(v));
            }
        }
    }
}

fn make_labels(lg: &mut LGraphArena, ids: &mut Ids, list: Option<&Value>, deferred: &mut Deferred) -> Vec<LLabelId> {
    let mut out = Vec::new();
    for lo in list.and_then(|l| l.as_array()).into_iter().flatten() {
        let text = lo["text"].as_str().unwrap();
        let l = lg.new_label(text);
        if let Some(s) = lo.get("size") {
            lg[l].size = kv(s);
        }
        if let Some(p) = lo.get("pos") {
            lg[l].position = kv(p);
        }
        set_props(lg, Holder::Label(l), lo.get("props"), deferred);
        ids.labels.insert(text.to_string(), l);
        out.push(l);
    }
    out
}

fn build(spec: &Value) -> (LGraphArena, LGraphId, Ids) {
    let mut lg = LGraphArena::new();
    let mut ids = Ids::default();
    let mut deferred: Deferred = Vec::new();
    let graph = lg.new_graph();
    if let Some(g) = spec.get("graph") {
        set_props(&mut lg, Holder::Graph(graph), g.get("props"), &mut deferred);
        if let Some(s) = g.get("size") {
            lg[graph].size = kv(s);
        }
    }

    for no in spec["nodes"].as_array().into_iter().flatten() {
        let n = lg.new_node(Some(graph));
        let id = no["id"].as_str().unwrap().to_string();
        ids.nodes.insert(id.clone(), n);
        ids.node_names.insert(n, id);
        if let Some(t) = no.get("type") {
            lg[n].node_type = pick(t.as_str().unwrap(), &NodeType::ALL);
        }
        if let Some(s) = no.get("size") {
            lg[n].size = kv(s);
        }
        if let Some(p) = no.get("pos") {
            lg[n].position = kv(p);
        }
        set_props(&mut lg, Holder::Node(n), no.get("props"), &mut deferred);
        if let Some(m) = no.get("margin") {
            let m = m.as_array().unwrap();
            lg[n].margin.top = num(&m[0]);
            lg[n].margin.right = num(&m[1]);
            lg[n].margin.bottom = num(&m[2]);
            lg[n].margin.left = num(&m[3]);
        }
        let labels = make_labels(&mut lg, &mut ids, no.get("labels"), &mut deferred);
        lg[n].labels = labels;
        for po in no.get("ports").and_then(|p| p.as_array()).into_iter().flatten() {
            let p = lg.new_port();
            let pid = po["id"].as_str().unwrap().to_string();
            ids.ports.insert(pid.clone(), p);
            ids.port_names.insert(p, pid);
            if let Some(s) = po.get("size") {
                lg[p].size = kv(s);
            }
            if let Some(ps) = po.get("pos") {
                lg[p].position = kv(ps);
            }
            lg.port_set_node(p, Some(n));
            lg.port_set_side(p, pick(po["side"].as_str().unwrap(), &PortSide::ALL));
            if let Some(a) = po.get("anchor") {
                lg[p].anchor = kv(a);
            }
            set_props(&mut lg, Holder::Port(p), po.get("props"), &mut deferred);
            let labels = make_labels(&mut lg, &mut ids, po.get("labels"), &mut deferred);
            lg[p].labels = labels;
        }
    }
    for eo in spec.get("edges").and_then(|e| e.as_array()).into_iter().flatten() {
        let e = lg.new_edge();
        let id = eo["id"].as_str().unwrap().to_string();
        ids.edges.insert(id.clone(), e);
        ids.edge_names.insert(e, id);
        let s = ids.ports[eo["source"].as_str().unwrap()];
        let t = ids.ports[eo["target"].as_str().unwrap()];
        lg.edge_set_source(e, Some(s));
        lg.edge_set_target(e, Some(t));
        set_props(&mut lg, Holder::Edge(e), eo.get("props"), &mut deferred);
        let labels = make_labels(&mut lg, &mut ids, eo.get("labels"), &mut deferred);
        lg[e].labels = labels;
    }
    for lids in spec.get("layers").and_then(|l| l.as_array()).into_iter().flatten() {
        let layer = lg.graph_add_layer(graph);
        for nid in lids.as_array().unwrap() {
            let n = ids.nodes[nid.as_str().unwrap()];
            lg.node_set_layer(n, Some(layer));
        }
    }
    for (h, key, ty, raw) in deferred {
        let v = match ty.as_str() {
            "EdgeRef" => PropValue::LEdge(ids.edges[raw.as_str().unwrap()]),
            "PortRef" => PropValue::LPort(ids.ports[raw.as_str().unwrap()]),
            "NodeRef" => PropValue::LNode(ids.nodes[raw.as_str().unwrap()]),
            "LabelRefs" => PropValue::from(
                raw.as_array().unwrap().iter().map(|t| ids.labels[t.as_str().unwrap()]).collect::<Vec<LLabelId>>(),
            ),
            _ => unreachable!(),
        };
        props_of(&mut lg, h).set_by_id(&key, Some(v));
    }
    (lg, graph, ids)
}

fn spacing(s: &Spacing) -> String {
    format!("({},{},{},{})", d(s.top), d(s.right), d(s.bottom), d(s.left))
}

fn opt(ids: &Ids, v: Option<PropValue>) -> String {
    match v {
        None => "-".into(),
        Some(PropValue::Double(x)) => d(x),
        Some(PropValue::LEdge(e)) => ids.edge_names.get(&e).cloned().unwrap_or("?".into()),
        Some(PropValue::LPort(p)) => ids.port_names.get(&p).cloned().unwrap_or("?".into()),
        Some(PropValue::LNode(n)) => ids.node_names.get(&n).cloned().unwrap_or("?".into()),
        Some(PropValue::LabelSide(s)) => norm(&format!("{s:?}")),
        Some(other) => format!("{other:?}"),
    }
}

fn label_line(lg: &LGraphArena, ids: &Ids, l: LLabelId, indent: &str) -> String {
    format!(
        "{indent}label {} pos={} size={} side={} edge={}\n",
        lg[l].text,
        vec(lg[l].position),
        vec(lg[l].size),
        opt(ids, lg[l].props.get(&InternalProperties::LABEL_SIDE)),
        opt(ids, lg[l].props.get(&InternalProperties::END_LABEL_EDGE)),
    )
}

fn dump(lg: &LGraphArena, graph: LGraphId, ids: &Ids) -> String {
    let mut s = format!("graph size={}\n", vec(lg[graph].size));
    for (li, &layer) in lg[graph].layers.iter().enumerate() {
        for &n in &lg[layer].nodes {
            s += &format!(
                "L{li} node {} {} id={} pos={} size={} margin={} padding={}\n",
                ids.node_names[&n],
                norm(&format!("{:?}", lg[n].node_type)),
                lg[n].id,
                vec(lg[n].position),
                vec(lg[n].size),
                spacing(&lg[n].margin),
                spacing(&lg[n].padding),
            );
            for &l in &lg[n].labels {
                s += &label_line(lg, ids, l, "  ");
            }
            for &p in &lg[n].ports {
                s += &format!(
                    "  port {} {} id={} pos={} size={} anchor={} met={}\n",
                    ids.port_names[&p],
                    norm(&format!("{:?}", lg[p].side)),
                    lg[p].id,
                    vec(lg[p].position),
                    vec(lg[p].size),
                    vec(lg[p].anchor),
                    opt(ids, lg[p].props.get(&InternalProperties::MAX_EDGE_THICKNESS)),
                );
                for &l in &lg[p].labels {
                    s += &label_line(lg, ids, l, "    ");
                }
            }
            if let Some(cells) = lg[n].props.get_object::<EndLabelCells>(&InternalProperties::END_LABELS) {
                for &p in &lg[n].ports {
                    if let Some(c) = cells.get(p) {
                        let c = c.borrow();
                        let r = c.cell.cell_rectangle;
                        s += &format!(
                            "  endlabels {} cell rect=({},{},{},{}) h={} v={} labels=[{}]\n",
                            ids.port_names[&p],
                            d(r.x),
                            d(r.y),
                            d(r.width),
                            d(r.height),
                            norm(&format!("{:?}", c.horizontal_alignment)),
                            norm(&format!("{:?}", c.vertical_alignment)),
                            c.labels.iter().map(|l| lg[l.element].text.clone()).collect::<Vec<_>>().join(","),
                        );
                    }
                }
            }
        }
    }
    let mut edges: Vec<(&String, &LEdgeId)> = ids.edges.iter().collect();
    edges.sort();
    for (id, &e) in edges {
        s += &format!("edge {id}\n");
        for &l in &lg[e].labels {
            s += &label_line(lg, ids, l, "  ");
        }
    }
    s
}

fn run(spec: &Value) -> String {
    let (mut lg, graph, ids) = build(spec);
    let mut monitor = BasicProgressMonitor::new();
    let mut out = String::new();
    for step in spec.get("run").and_then(|r| r.as_array()).into_iter().flatten() {
        let step = step.as_str().unwrap();
        let parts: Vec<&str> = step.split(':').collect();
        match parts[0] {
            "LabelAndNodeSizeProcessor" => LabelAndNodeSizeProcessor::new().process(&mut lg, graph, &mut monitor),
            "InnermostNodeMarginCalculator" => InnermostNodeMarginCalculator::new().process(&mut lg, graph, &mut monitor),
            "EndLabelPreprocessor" => EndLabelPreprocessor::new().process(&mut lg, graph, &mut monitor),
            "EndLabelSorter" => EndLabelSorter::new().process(&mut lg, graph, &mut monitor),
            "EndLabelPostprocessor" => EndLabelPostprocessor::new().process(&mut lg, graph, &mut monitor),
            "LabelSideSelector" => LabelSideSelector::new().process(&mut lg, graph, &mut monitor),
            "MarginCalcNode" => {
                NodeDimensionCalculation::get_node_margin_calculator(LGraphAdapters::adapt_ns(graph, false))
                    .process_node(&mut lg, &LGraphAdapters::adapt_node(ids.nodes[parts[1]], false));
            }
            "InsidePadding" => {
                let p = NodeLabelAndSizeCalculator::compute_inside_node_label_padding(
                    &lg,
                    &LGraphAdapters::adapt(graph),
                    &LGraphAdapters::adapt_node(ids.nodes[parts[1]], false),
                    pick(parts[2], &Direction::ALL),
                );
                out += &format!("insidePadding {} {}\n", parts[1], spacing(&p));
            }
            "SetPos" => {
                lg[ids.nodes[parts[1]]].position = KVector::new(parts[2].parse().unwrap(), parts[3].parse().unwrap());
            }
            "Dump" => {
                out += &format!("== {}\n", parts.get(1).copied().unwrap_or(""));
                out += &dump(&lg, graph, &ids);
            }
            _ => panic!("unknown step {step}"),
        }
    }
    let _ = Rc::new(());
    out
}

#[test]
fn group_e_golden_cases() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/group_e");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    names.sort();
    assert!(!names.is_empty());
    let mut failures = Vec::new();
    for spec_path in names {
        let golden_path = spec_path.with_extension("golden");
        let spec: Value = serde_json::from_str(&std::fs::read_to_string(&spec_path).unwrap()).unwrap();
        let actual = run(&spec);
        let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|_| panic!("missing {}", golden_path.display()));
        if actual != expected {
            let first_diff = actual
                .lines()
                .zip(expected.lines())
                .enumerate()
                .find(|(_, (a, e))| a != e)
                .map(|(i, (a, e))| format!("line {}:\n  rust:  {a}\n  swift: {e}", i + 1))
                .unwrap_or_else(|| format!("length differs ({} vs {} lines)", actual.lines().count(), expected.lines().count()));
            failures.push(format!("{}: {first_diff}", spec_path.file_name().unwrap().to_string_lossy()));
            if std::env::var("GROUP_E_WRITE_ACTUAL").is_ok() {
                std::fs::write(spec_path.with_extension("actual"), &actual).unwrap();
            }
        }
    }
    assert!(failures.is_empty(), "{} case(s) differ from elk-swift:\n{}", failures.len(), failures.join("\n"));
}
