//! Differential test of `LayerSweepCrossingMinimizer` against elk-swift.
//!
//! Not a port of an elk-swift test. An instrumented elk-swift build dumps the
//! layered graph(s) every `LayerSweepCrossingMinimizer` run sees before
//! (`cm-NNNN-pre.json`) and after (`cm-NNNN-post.json`) it runs: layers,
//! nodes, ports, edges, every property with its dynamic Swift type, the
//! cached port-side indices and the random generators' seeds. This test
//! rebuilds the `pre` state in an arena, runs the Rust processor and compares
//! the whole resulting state with `post`, exactly (doubles by bit pattern).
//!
//! The dumps under `tests/data/crossmin/` (a sample of the Downright/mermaid
//! corpus and of option variants: model-order barycenter, bottom-up
//! hierarchy sweeps, in-layer constraints between non-dummies) run by
//! default; set `UPLEFT_ELK_CMDUMP_DIR` to a directory tree of dumps to check
//! those too (every `cm-*-pre.json` below it). `tests/data/crossmin/CMDump.swift`
//! is the instrumentation that writes them.
//!
//! Dumps that reach a cross-group placeholder (`unimplemented!("... is not
//! wired yet")`) are counted as waiting, not as differences.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde_json::Value;
use upleft_elk::org::eclipse::elk::alg::layered::graph_configurator::Random;
use upleft_elk::org::eclipse::elk::alg::layered::options::group_order_strategy::GroupOrderStrategy;
use upleft_elk::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
use upleft_elk::org::eclipse::elk::alg::layered::options::ordering_strategy::OrderingStrategy;
use upleft_elk::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::{CrossMinType, LayerSweepCrossingMinimizer};
use upleft_elk::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use upleft_elk::org::eclipse::elk::core::options::hierarchy_handling::HierarchyHandling;
use upleft_elk::org::eclipse::elk::core::util::basic_progress_monitor::BasicProgressMonitor;
use upleft_elk::prelude::*;

type Oid = u64;

fn norm(s: &str) -> String {
    s.replace('_', "").to_lowercase()
}

fn parse_enum<E: Copy + std::fmt::Debug>(all: &[E], name: &str) -> E {
    let n = norm(name);
    *all.iter().find(|v| norm(&format!("{v:?}")) == n).unwrap_or_else(|| panic!("unknown enum value {name}"))
}

fn oid(v: &Value) -> Option<Oid> {
    v.as_u64()
}

fn oids(v: &Value) -> Vec<Oid> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect()
}

fn f64_bits(v: &Value) -> f64 {
    f64::from_bits(v.as_str().unwrap().parse::<u64>().unwrap())
}

#[derive(Default)]
struct Built {
    lg: LGraphArena,
    graphs: HashMap<Oid, LGraphId>,
    layers: HashMap<Oid, LayerId>,
    nodes: HashMap<Oid, LNodeId>,
    ports: HashMap<Oid, LPortId>,
    edges: HashMap<Oid, LEdgeId>,
    randoms: HashMap<Oid, Rc<RefCell<Random>>>,
    root: Option<LGraphId>,
    // Reverse maps for serialising the Rust state.
    graph_oid: HashMap<LGraphId, Oid>,
    layer_oid: HashMap<LayerId, Oid>,
    node_oid: HashMap<LNodeId, Oid>,
    port_oid: HashMap<LPortId, Oid>,
    edge_oid: HashMap<LEdgeId, Oid>,
}

impl Built {
    fn prop_value(&mut self, v: &Value) -> Option<PropValue> {
        let t = v["t"].as_str().unwrap();
        let x = &v["v"];
        Some(match t {
            "Bool" => PropValue::Bool(x.as_bool().unwrap()),
            "Int" => PropValue::Int(x.as_i64().unwrap()),
            "Double" => PropValue::Double(f64_bits(&v["bits"])),
            "Str" => PropValue::from(x.as_str().unwrap()),
            "LNode" => PropValue::LNode(self.nodes[&oid(x).unwrap()]),
            "LPort" => PropValue::LPort(self.ports[&oid(x).unwrap()]),
            "LNodes" => PropValue::LNodes(Rc::new(oids(x).iter().map(|o| self.nodes[o]).collect())),
            "LPorts" => PropValue::LPorts(Rc::new(oids(x).iter().map(|o| self.ports[o]).collect())),
            "PortConstraints" => parse_enum(&PortConstraints::ALL, x.as_str().unwrap()).into(),
            "HierarchyHandling" => parse_enum(&HierarchyHandling::ALL, x.as_str().unwrap()).into(),
            "OrderingStrategy" => parse_enum(&OrderingStrategy::ALL, x.as_str().unwrap()).into(),
            "GroupOrderStrategy" => parse_enum(&GroupOrderStrategy::ALL, x.as_str().unwrap()).into(),
            "LayerConstraint" => parse_enum(&LayerConstraint::ALL, x.as_str().unwrap()).into(),
            "PortSide" => parse_enum(&PortSide::ALL, x.as_str().unwrap()).into(),
            "GraphProperties" => {
                let set: EnumSet<GraphProperties> =
                    x.as_array().unwrap().iter().map(|s| parse_enum(&GraphProperties::ALL, s.as_str().unwrap())).collect();
                set.into()
            }
            "Random" => {
                let o = oid(x).unwrap();
                let r = self.randoms.entry(o).or_insert_with(|| Rc::new(RefCell::new(Random::with_seed(0)))).clone();
                r.borrow_mut().seed = v["seed"].as_str().unwrap().parse().unwrap();
                PropValue::Random(r)
            }
            "Other" => return None,
            other => panic!("unknown property type {other}"),
        })
    }

    fn set_props(&mut self, props: &Value) -> PropertyMap {
        let mut map = PropertyMap::new();
        for (k, v) in props.as_object().unwrap() {
            if let Some(value) = self.prop_value(v) {
                map.set_by_id(k, Some(value));
            }
        }
        map
    }

    fn build(pre: &Value) -> Built {
        let mut b = Built::default();
        let graphs = pre["graphs"].as_array().unwrap();
        let nodes = pre["nodes"].as_array().unwrap();
        let ports = pre["ports"].as_array().unwrap();
        let edges = pre["edges"].as_array().unwrap();

        for g in graphs {
            let id = b.lg.new_graph();
            b.graphs.insert(oid(&g["oid"]).unwrap(), id);
        }
        for n in nodes {
            let graph = oid(&n["graph"]).map(|o| b.graphs[&o]);
            let id = b.lg.new_node(graph);
            b.nodes.insert(oid(&n["oid"]).unwrap(), id);
        }
        for p in ports {
            let id = b.lg.new_port();
            b.ports.insert(oid(&p["oid"]).unwrap(), id);
        }
        for e in edges {
            let id = b.lg.new_edge();
            b.edges.insert(oid(&e["oid"]).unwrap(), id);
        }
        for g in graphs {
            let gid = b.graphs[&oid(&g["oid"]).unwrap()];
            for l in g["layers"].as_array().unwrap() {
                let lid = b.lg.new_layer(gid);
                b.lg[gid].layers.push(lid);
                b.layers.insert(oid(&l["oid"]).unwrap(), lid);
                b.lg[lid].id = l["id"].as_i64().unwrap() as i32;
                b.lg[lid].nodes = oids(&l["nodes"]).iter().map(|o| b.nodes[o]).collect();
            }
        }
        for g in graphs {
            let gid = b.graphs[&oid(&g["oid"]).unwrap()];
            b.lg[gid].id = g["id"].as_i64().unwrap() as i32;
            b.lg[gid].parent_node = oid(&g["parent"]).map(|o| b.nodes[&o]);
            b.lg[gid].layerless_nodes = oids(&g["layerless"]).iter().map(|o| b.nodes[o]).collect();
        }
        for n in nodes {
            let nid = b.nodes[&oid(&n["oid"]).unwrap()];
            let layer = oid(&n["layerOf"]).map(|o| *b.layers.get(&o).expect("node layer outside the dump"));
            let nested = oid(&n["nested"]).map(|o| b.graphs[&o]);
            let ports: Vec<LPortId> = oids(&n["ports"]).iter().map(|o| b.ports[o]).collect();
            let node = &mut b.lg[nid];
            node.id = n["id"].as_i64().unwrap() as i32;
            node.node_type = parse_enum(&NodeType::ALL, n["type"].as_str().unwrap());
            node.layer = layer;
            node.nested_graph = nested;
            node.ports = ports;
            node.port_sides_cached = n["cached"].as_bool().unwrap();
            node.port_side_indices = match n["indices"].as_object() {
                None => None,
                Some(m) => {
                    let mut idx: [Option<(usize, usize)>; 5] = [None; 5];
                    for (side, r) in m {
                        let side = parse_enum(&PortSide::ALL, side);
                        let r = r.as_array().unwrap();
                        idx[side.ordinal()] = Some((r[0].as_u64().unwrap() as usize, r[1].as_u64().unwrap() as usize));
                    }
                    Some(idx)
                }
            };
        }
        for p in ports {
            let pid = b.ports[&oid(&p["oid"]).unwrap()];
            let owner = oid(&p["owner"]).map(|o| b.nodes[&o]);
            let incoming: Vec<LEdgeId> = oids(&p["in"]).iter().map(|o| b.edges[o]).collect();
            let outgoing: Vec<LEdgeId> = oids(&p["out"]).iter().map(|o| b.edges[o]).collect();
            let port = &mut b.lg[pid];
            port.id = p["id"].as_i64().unwrap() as i32;
            port.side = parse_enum(&PortSide::ALL, p["side"].as_str().unwrap());
            port.owner = owner;
            port.anchor = KVector::new(f64_bits(&p["anchor"][0]), f64_bits(&p["anchor"][1]));
            port.size = KVector::new(f64_bits(&p["size"][0]), f64_bits(&p["size"][1]));
            if p.get("pos").is_some() {
                port.position = KVector::new(f64_bits(&p["pos"][0]), f64_bits(&p["pos"][1]));
            }
            port.explicitly_supplied_port_anchor = p["explicit"].as_bool().unwrap();
            port.incoming_edges = incoming;
            port.outgoing_edges = outgoing;
        }
        for e in edges {
            let eid = b.edges[&oid(&e["oid"]).unwrap()];
            b.lg[eid].id = e["id"].as_i64().unwrap() as i32;
            b.lg[eid].source = oid(&e["source"]).map(|o| b.ports[&o]);
            b.lg[eid].target = oid(&e["target"]).map(|o| b.ports[&o]);
        }
        // Properties last: they reference every kind of element.
        for g in graphs {
            let gid = b.graphs[&oid(&g["oid"]).unwrap()];
            let props = b.set_props(&g["props"]);
            b.lg[gid].props = props;
        }
        for n in nodes {
            let nid = b.nodes[&oid(&n["oid"]).unwrap()];
            let props = b.set_props(&n["props"]);
            b.lg[nid].props = props;
        }
        for p in ports {
            let pid = b.ports[&oid(&p["oid"]).unwrap()];
            let props = b.set_props(&p["props"]);
            b.lg[pid].props = props;
        }
        b.root = Some(b.graphs[&oid(&graphs[0]["oid"]).unwrap()]);

        b.graph_oid = b.graphs.iter().map(|(&o, &i)| (i, o)).collect();
        b.layer_oid = b.layers.iter().map(|(&o, &i)| (i, o)).collect();
        b.node_oid = b.nodes.iter().map(|(&o, &i)| (i, o)).collect();
        b.port_oid = b.ports.iter().map(|(&o, &i)| (i, o)).collect();
        b.edge_oid = b.edges.iter().map(|(&o, &i)| (i, o)).collect();
        b
    }

    /// A property value in the dump's JSON shape (`None` for types the dump
    /// cannot represent).
    fn prop_json(&self, v: &PropValue) -> Option<Value> {
        use serde_json::json;
        Some(match v {
            PropValue::Bool(b) => json!({"t": "Bool", "v": b}),
            PropValue::Int(i) => json!({"t": "Int", "v": i}),
            PropValue::Double(d) => json!({"t": "Double", "bits": d.to_bits().to_string()}),
            PropValue::Str(s) => json!({"t": "Str", "v": s.to_string()}),
            PropValue::LNode(n) => json!({"t": "LNode", "v": self.node_oid[n]}),
            PropValue::LPort(p) => json!({"t": "LPort", "v": self.port_oid[p]}),
            PropValue::LNodes(ns) => json!({"t": "List", "v": ns.iter().map(|n| self.node_oid[n]).collect::<Vec<_>>()}),
            PropValue::LPorts(ps) => json!({"t": "List", "v": ps.iter().map(|p| self.port_oid[p]).collect::<Vec<_>>()}),
            PropValue::PortConstraints(x) => json!({"t": "PortConstraints", "v": norm(&format!("{x:?}"))}),
            PropValue::HierarchyHandling(x) => json!({"t": "HierarchyHandling", "v": norm(&format!("{x:?}"))}),
            PropValue::OrderingStrategy(x) => json!({"t": "OrderingStrategy", "v": norm(&format!("{x:?}"))}),
            PropValue::GroupOrderStrategy(x) => json!({"t": "GroupOrderStrategy", "v": norm(&format!("{x:?}"))}),
            PropValue::LayerConstraint(x) => json!({"t": "LayerConstraint", "v": norm(&format!("{x:?}"))}),
            PropValue::PortSide(x) => json!({"t": "PortSide", "v": norm(&format!("{x:?}"))}),
            PropValue::GraphPropertiesSet(s) => {
                let mut names: Vec<String> = s.iter().map(|g| norm(&format!("{g:?}"))).collect();
                names.sort();
                json!({"t": "GraphProperties", "v": names})
            }
            PropValue::Random(r) => json!({"t": "Random", "seed": r.borrow().seed.to_string()}),
            _ => return None,
        })
    }

    /// The Swift dump's property value, normalised like [`Self::prop_json`].
    fn swift_prop_json(v: &Value) -> Option<Value> {
        use serde_json::json;
        let t = v["t"].as_str().unwrap();
        Some(match t {
            "Other" => return None,
            "Double" => json!({"t": "Double", "bits": v["bits"]}),
            "LNodes" | "LPorts" => json!({"t": "List", "v": v["v"]}),
            "Random" => json!({"t": "Random", "seed": v["seed"]}),
            "PortConstraints" | "HierarchyHandling" | "OrderingStrategy" | "GroupOrderStrategy" | "LayerConstraint" | "PortSide" => {
                json!({"t": t, "v": norm(v["v"].as_str().unwrap())})
            }
            "GraphProperties" => {
                let mut names: Vec<String> = v["v"].as_array().unwrap().iter().map(|s| norm(s.as_str().unwrap())).collect();
                names.sort();
                json!({"t": t, "v": names})
            }
            _ => v.clone(),
        })
    }

    fn props_json(&self, props: &PropertyMap) -> BTreeMap<String, Value> {
        props.all().filter_map(|(k, v)| self.prop_json(v).map(|j| (k.to_string(), j))).collect()
    }

    fn swift_props_json(props: &Value) -> BTreeMap<String, Value> {
        props.as_object().unwrap().iter().filter_map(|(k, v)| Self::swift_prop_json(v).map(|j| (k.clone(), j))).collect()
    }
}

/// Compares the Rust state with the Swift `post` dump; returns the differences.
fn compare(b: &Built, post: &Value) -> Vec<String> {
    let mut diffs = Vec::new();
    let lg = &b.lg;
    let mut check = |what: String, rust: Value, swift: Value| {
        if rust != swift {
            diffs.push(format!("{what}: rust {rust} != swift {swift}"));
        }
    };
    let node_oids = |v: &[LNodeId]| Value::from(v.iter().map(|n| b.node_oid[n]).collect::<Vec<_>>());

    for g in post["graphs"].as_array().unwrap() {
        let o = oid(&g["oid"]).unwrap();
        let Some(&gid) = b.graphs.get(&o) else {
            check(format!("graph {o}"), Value::from("missing"), Value::from("present"));
            continue;
        };
        check(format!("graph {o} id"), Value::from(lg[gid].id), g["id"].clone());
        let layers: Vec<Value> = lg[gid]
            .layers
            .iter()
            .map(|&l| serde_json::json!({"oid": b.layer_oid.get(&l).copied(), "id": lg[l].id, "nodes": node_oids(&lg[l].nodes)}))
            .collect();
        let swift_layers: Vec<Value> = g["layers"].as_array().unwrap().iter().map(|l| serde_json::json!({"oid": l["oid"], "id": l["id"], "nodes": l["nodes"]})).collect();
        check(format!("graph {o} layers"), Value::from(layers), Value::from(swift_layers));
        check(format!("graph {o} layerless"), node_oids(&lg[gid].layerless_nodes), g["layerless"].clone());
        let rp = b.props_json(&lg[gid].props);
        let sp = Built::swift_props_json(&g["props"]);
        if rp != sp {
            check(format!("graph {o} props"), serde_json::to_value(&rp).unwrap(), serde_json::to_value(&sp).unwrap());
        }
    }
    for n in post["nodes"].as_array().unwrap() {
        let o = oid(&n["oid"]).unwrap();
        let Some(&nid) = b.nodes.get(&o) else {
            check(format!("node {o}"), Value::from("missing"), Value::from("present"));
            continue;
        };
        let node = &lg[nid];
        check(format!("node {o} id"), Value::from(node.id), n["id"].clone());
        check(format!("node {o} type"), Value::from(node.node_type.raw_value()), n["type"].clone());
        check(format!("node {o} layer"), Value::from(node.layer.map(|l| b.layer_oid[&l])), n["layerOf"].clone());
        check(format!("node {o} ports"), Value::from(node.ports.iter().map(|p| b.port_oid[p]).collect::<Vec<_>>()), n["ports"].clone());
        check(format!("node {o} cached"), Value::from(node.port_sides_cached), n["cached"].clone());
        if node.port_sides_cached {
            let mut idx = serde_json::Map::new();
            if let Some(indices) = node.port_side_indices {
                for side in PortSide::ALL {
                    if let Some((a, z)) = indices[side.ordinal()] {
                        idx.insert(format!("{side:?}"), Value::from(vec![a as u64, z as u64]));
                    }
                }
            }
            check(format!("node {o} indices"), Value::Object(idx), n["indices"].clone());
        }
        let rp = b.props_json(&node.props);
        let sp = Built::swift_props_json(&n["props"]);
        if rp != sp {
            check(format!("node {o} props"), serde_json::to_value(&rp).unwrap(), serde_json::to_value(&sp).unwrap());
        }
    }
    for p in post["ports"].as_array().unwrap() {
        let o = oid(&p["oid"]).unwrap();
        let Some(&pid) = b.ports.get(&o) else {
            check(format!("port {o}"), Value::from("missing"), Value::from("present"));
            continue;
        };
        let port = &lg[pid];
        check(format!("port {o} id"), Value::from(port.id), p["id"].clone());
        check(format!("port {o} side"), Value::from(format!("{:?}", port.side)), p["side"].clone());
        check(format!("port {o} owner"), Value::from(port.owner.map(|n| b.node_oid[&n])), p["owner"].clone());
        check(
            format!("port {o} anchor"),
            Value::from(vec![port.anchor.x.to_bits().to_string(), port.anchor.y.to_bits().to_string()]),
            p["anchor"].clone(),
        );
        check(format!("port {o} in"), Value::from(port.incoming_edges.iter().map(|e| b.edge_oid[e]).collect::<Vec<_>>()), p["in"].clone());
        check(format!("port {o} out"), Value::from(port.outgoing_edges.iter().map(|e| b.edge_oid[e]).collect::<Vec<_>>()), p["out"].clone());
        let rp = b.props_json(&port.props);
        let sp = Built::swift_props_json(&p["props"]);
        if rp != sp {
            check(format!("port {o} props"), serde_json::to_value(&rp).unwrap(), serde_json::to_value(&sp).unwrap());
        }
    }
    for r in post["randoms"].as_array().unwrap() {
        let o = oid(&r["oid"]).unwrap();
        if let Some(rr) = b.randoms.get(&o) {
            check(format!("random {o} seed"), Value::from(rr.borrow().seed.to_string()), r["seed"].clone());
        }
    }
    diffs
}

fn cross_min_type(name: &str) -> CrossMinType {
    match name {
        "BARYCENTER" => CrossMinType::BARYCENTER,
        "ONE_SIDED_GREEDY_SWITCH" => CrossMinType::ONE_SIDED_GREEDY_SWITCH,
        "TWO_SIDED_GREEDY_SWITCH" => CrossMinType::TWO_SIDED_GREEDY_SWITCH,
        "MEDIAN" => CrossMinType::MEDIAN,
        other => panic!("unknown cross min type {other}"),
    }
}

thread_local! {
    /// Time spent inside `process`, for comparing with elk-swift's.
    static PROCESS_TIME: std::cell::Cell<std::time::Duration> = const { std::cell::Cell::new(std::time::Duration::ZERO) };
}

/// Runs one dump pair; `Err` holds the differences (or the panic).
fn run_case(pre_path: &Path) -> Result<(), Vec<String>> {
    let post_path = PathBuf::from(pre_path.to_string_lossy().replace("-pre.json", "-post.json"));
    let pre: Value = serde_json::from_str(&std::fs::read_to_string(pre_path).unwrap()).unwrap();
    let post: Value = serde_json::from_str(&std::fs::read_to_string(&post_path).unwrap()).unwrap();
    let mut b = Built::build(&pre);
    let root = b.root.unwrap();
    let ty = cross_min_type(pre["type"].as_str().unwrap());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut minimizer = LayerSweepCrossingMinimizer::new(ty);
        let start = std::time::Instant::now();
        minimizer.process(&mut b.lg, root, &mut BasicProgressMonitor::new());
        PROCESS_TIME.with(|t| t.set(t.get() + start.elapsed()));
    }));
    if let Err(e) = result {
        let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        return Err(vec![format!("panicked: {msg}")]);
    }
    let diffs = compare(&b, &post);
    if diffs.is_empty() { Ok(()) } else { Err(diffs) }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("cm-") && n.ends_with("-pre.json")) {
            out.push(path);
        }
    }
}

fn run_dir(dir: &Path) {
    // Keep the expected placeholder panics out of the test output.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.payload().downcast_ref::<String>().map(|s| s.as_str()).or_else(|| info.payload().downcast_ref::<&str>().copied()).unwrap_or("");
        if !msg.contains("is not wired yet") {
            default_hook(info);
        }
    }));
    let mut cases = Vec::new();
    collect(dir, &mut cases);
    cases.sort();
    assert!(!cases.is_empty(), "no dumps under {}", dir.display());
    let mut failures = Vec::new();
    let mut waiting = 0;
    for case in &cases {
        if let Err(diffs) = run_case(case) {
            // A cross-group placeholder (`unimplemented!("... (group A) is not
            // wired yet")`) is not a divergence of this port.
            if diffs.len() == 1 && diffs[0].contains("is not wired yet") {
                waiting += 1;
                continue;
            }
            failures.push(format!("{}:\n    {}", case.display(), diffs.iter().take(8).cloned().collect::<Vec<_>>().join("\n    ")));
        }
    }
    let show: Vec<String> = failures.iter().take(20).cloned().collect();
    assert!(failures.is_empty(), "{} of {} dumps differ:\n{}", failures.len(), cases.len(), show.join("\n"));
    eprintln!(
        "{} dumps identical, {} waiting for group A ({:?} in LayerSweepCrossingMinimizer.process)",
        cases.len() - waiting,
        waiting,
        PROCESS_TIME.with(|t| t.get())
    );
}

#[test]
fn crossmin_matches_swift_on_checked_in_dumps() {
    run_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/crossmin"));
}

#[test]
fn crossmin_matches_swift_on_external_dumps() {
    let Some(dir) = std::env::var_os("UPLEFT_ELK_CMDUMP_DIR") else { return };
    run_dir(Path::new(&dir));
}
