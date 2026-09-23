//! Differential test of the early-pipeline processors against elk-swift.
//!
//! Builds a random layered graph (nodes with model orders, layer, in-layer and
//! port constraints; ports; edges with model orders and priorities) exactly as
//! the instrumented elk-swift lab's `chain` mode does, runs
//! EdgeAndLayerConstraintEdgeReverser → GreedyCycleBreaker →
//! LayerConstraintPreprocessor → NetworkSimplexLayerer →
//! LayerConstraintPostprocessor → HighDegreeNodeLayeringProcessor →
//! LongEdgeSplitter → SortByInputModelProcessor → InLayerConstraintProcessor
//! → PortListSorter → ReversedEdgeRestorer, and compares a dump of the graph
//! after every step with the Swift one (stored as FNV-1a hashes in
//! `tests/data/group_a_chain_golden.txt`; regenerate a full dump with the lab:
//! `lab chain <seed> <n> <e>`).

use std::cell::RefCell;
use std::rc::Rc;

use upleft_elk::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use upleft_elk::org::eclipse::elk::alg::layered::graph_configurator::Random;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::{
    edge_and_layer_constraint_edge_reverser::EdgeAndLayerConstraintEdgeReverser,
    high_degree_node_layering_processor::HighDegreeNodeLayeringProcessor, in_layer_constraint_processor::InLayerConstraintProcessor,
    layer_constraint_postprocessor::LayerConstraintPostprocessor, layer_constraint_preprocessor::LayerConstraintPreprocessor,
    long_edge_splitter::LongEdgeSplitter, port_list_sorter::PortListSorter, reversed_edge_restorer::ReversedEdgeRestorer,
    sort_by_input_model_processor::SortByInputModelProcessor,
};
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::preserveorder::model_order_node_comparator::ModelOrderNodeComparator;
use upleft_elk::org::eclipse::elk::alg::layered::options::{
    group_order_strategy::GroupOrderStrategy, in_layer_constraint::InLayerConstraint, layer_constraint::LayerConstraint,
    long_edge_ordering_strategy::LongEdgeOrderingStrategy, ordering_strategy::OrderingStrategy,
};
use upleft_elk::org::eclipse::elk::alg::layered::p1cycles::greedy_cycle_breaker::GreedyCycleBreaker;
use upleft_elk::org::eclipse::elk::alg::layered::p2layers::network_simplex_layerer::NetworkSimplexLayerer;
use upleft_elk::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use upleft_elk::org::eclipse::elk::core::util::basic_progress_monitor::BasicProgressMonitor;
use upleft_elk::prelude::*;

fn side_char(s: PortSide) -> &'static str {
    match s {
        PortSide::NORTH => "N",
        PortSide::EAST => "E",
        PortSide::SOUTH => "S",
        PortSide::WEST => "W",
        _ => "U",
    }
}

fn nm(lg: &LGraphArena, n: Option<LNodeId>) -> String {
    let Some(n) = n else { return "nil".into() };
    if let Some(&l) = lg[n].labels.first() {
        return lg[l].text.clone();
    }
    if lg[n].node_type == NodeType::LONG_EDGE {
        if let Some(e) = lg[n].props.get_as::<LEdgeId>(&InternalProperties::ORIGIN) {
            return format!("d{}", lg[e].props.get_as::<i64>(&InternalProperties::MODEL_ORDER).unwrap_or(-1));
        }
    }
    format!("?{}", lg[n].node_type.raw_value())
}

fn pn(lg: &LGraphArena, p: Option<LPortId>) -> String {
    let Some(p) = p else { return "nil".into() };
    let name = lg[p].labels.first().map_or("q".to_string(), |&l| lg[l].text.clone());
    format!("{}{}", name, side_char(lg[p].side))
}

fn dump(lg: &LGraphArena, g: LGraphId, title: &str) -> String {
    let cyc = lg[g].props.get_as::<bool>(&InternalProperties::CYCLIC).unwrap_or(false);
    let mut s = format!("== {title} cyc={cyc}\n");
    s += "LL:";
    for &n in &lg[g].layerless_nodes {
        s += &format!(" {}", nm(lg, Some(n)));
    }
    s += "\n";
    let mut all = Vec::new();
    for (i, &layer) in lg[g].layers.iter().enumerate() {
        s += &format!("L{i}:");
        for &node in &lg[layer].nodes {
            let ports: Vec<String> = lg[node].ports.iter().map(|&p| pn(lg, Some(p))).collect();
            s += &format!(" {}[{}]", nm(lg, Some(node)), ports.join(","));
            all.push(node);
        }
        s += "\n";
    }
    all.extend(lg[g].layerless_nodes.iter().copied());
    for node in all {
        for &port in &lg[node].ports {
            for &e in &lg[port].outgoing_edges {
                let r = if lg[e].props.get_as::<bool>(&InternalProperties::REVERSED).unwrap_or(false) { "R" } else { "" };
                let src = lg[e].source;
                let tgt = lg[e].target;
                s += &format!(
                    "e{}{}:{}.{}->{}.{}\n",
                    lg[e].props.get_as::<i64>(&InternalProperties::MODEL_ORDER).unwrap_or(-1),
                    r,
                    nm(lg, src.and_then(|p| lg[p].owner)),
                    pn(lg, src),
                    nm(lg, tgt.and_then(|p| lg[p].owner)),
                    pn(lg, tgt)
                );
            }
        }
    }
    s
}

/// Pairwise `ModelOrderNodeComparator` results per layer (the lab's `moc`
/// stage): all pairs `(i, j)` with `i < j`, then neighbours in reverse, with
/// one comparator per layer so the transitive caches are exercised.
fn moc_dump(lg: &LGraphArena, graph: LGraphId) -> String {
    let mut s = "== moc\n".to_string();
    for strategy in [OrderingStrategy::NODES_AND_EDGES, OrderingStrategy::PREFER_EDGES] {
        let layers: Vec<Vec<LNodeId>> = lg[graph].layers.iter().map(|&l| lg[l].nodes.clone()).collect();
        let mut prev_idx: i64 = -1;
        for layer in &layers {
            let previous = if prev_idx == -1 { layers[0].clone() } else { layers[prev_idx as usize].clone() };
            let mut comp = ModelOrderNodeComparator::new(graph, previous, strategy, LongEdgeOrderingStrategy::EQUAL, GroupOrderStrategy::ONLY_WITHIN_GROUP, false);
            let mut line = String::new();
            if layer.len() > 1 {
                for i in 0..layer.len() - 1 {
                    for j in i + 1..layer.len() {
                        line += &format!("{} ", comp.compare(lg, layer[i], layer[j]));
                    }
                }
                for i in (1..layer.len()).rev() {
                    line += &format!("{} ", comp.compare(lg, layer[i], layer[i - 1]));
                }
            }
            s += &line;
            s += "\n";
            prev_idx += 1;
        }
    }
    s
}

fn new_port(lg: &mut LGraphArena, rnd: &mut Random, port_counter: &mut i64, node: LNodeId, side: PortSide) -> LPortId {
    let port = lg.new_port();
    lg.port_set_side(port, side);
    lg.port_set_node(port, Some(node));
    let label = lg.new_label(&format!("p{port_counter}"));
    lg[port].labels.push(label);
    lg[port].props.set(&LayeredOptions::PORT_INDEX, (*port_counter * 7) % 11);
    if rnd.next_int_bounded(2) == 0 {
        lg[port].props.set(&InternalProperties::MODEL_ORDER, *port_counter);
    }
    *port_counter += 1;
    port
}

/// The lab's `chain <seed> <n> <e>` as a list of `(stage, dump)`.
fn chain(seed: i64, n: i64, e: i64) -> Vec<(String, String)> {
    let mut rnd = Random::with_seed(seed);
    let mut lg = LGraphArena::new();
    let graph = lg.new_graph();
    lg[graph].props.set(&InternalProperties::RANDOM, Rc::new(RefCell::new(Random::with_seed(7))));
    lg[graph].props.set(&LayeredOptions::CONSIDER_MODEL_ORDER_STRATEGY, OrderingStrategy::NODES_AND_EDGES);
    lg[graph].props.set(&InternalProperties::MAX_MODEL_ORDER_NODES, n);
    lg[graph].props.set(&LayeredOptions::HIGH_DEGREE_NODES_THRESHOLD, 4i64);
    let mut nodes = Vec::new();
    for i in 0..n {
        let node = lg.new_node(Some(graph));
        lg[graph].layerless_nodes.push(node);
        let label = lg.new_label(&format!("n{i}"));
        lg[node].labels.push(label);
        if rnd.next_int_bounded(5) != 0 {
            lg[node].props.set(&InternalProperties::MODEL_ORDER, i);
        }
        let lc = match rnd.next_int_bounded(14) {
            0 => Some(LayerConstraint::FIRST),
            1 => Some(LayerConstraint::LAST),
            2 => Some(LayerConstraint::FIRST_SEPARATE),
            3 => Some(LayerConstraint::LAST_SEPARATE),
            _ => None,
        };
        if let Some(lc) = lc {
            lg[node].props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, lc);
        }
        match rnd.next_int_bounded(8) {
            0 => lg[node].props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::TOP),
            1 => lg[node].props.set(&InternalProperties::IN_LAYER_CONSTRAINT, InLayerConstraint::BOTTOM),
            _ => {}
        }
        match rnd.next_int_bounded(4) {
            0 => lg[node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_SIDE),
            1 => lg[node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_ORDER),
            _ => {}
        }
        nodes.push(node);
    }
    let mut port_counter = 0i64;
    for k in 0..e {
        let s = rnd.next_int_bounded(n) as usize;
        let t = rnd.next_int_bounded(n) as usize;
        if s == t {
            continue;
        }
        let mut sp = None;
        if rnd.next_int_bounded(3) == 0 {
            sp = lg[nodes[s]].ports.iter().copied().find(|&p| lg[p].side == PortSide::EAST);
        }
        let source_port = match sp {
            Some(p) => p,
            None => new_port(&mut lg, &mut rnd, &mut port_counter, nodes[s], PortSide::EAST),
        };
        let mut tp = None;
        if rnd.next_int_bounded(3) == 0 {
            tp = lg[nodes[t]].ports.iter().copied().find(|&p| lg[p].side == PortSide::WEST);
        }
        let target_port = match tp {
            Some(p) => p,
            None => new_port(&mut lg, &mut rnd, &mut port_counter, nodes[t], PortSide::WEST),
        };
        let edge = lg.new_edge();
        lg.edge_set_source(edge, Some(source_port));
        lg.edge_set_target(edge, Some(target_port));
        lg[edge].props.set(&InternalProperties::MODEL_ORDER, k);
        if rnd.next_int_bounded(6) == 0 {
            lg[edge].props.set(&LayeredOptions::PRIORITY_DIRECTION, 2i64);
            lg[edge].props.set(&LayeredOptions::PRIORITY_SHORTNESS, 3i64);
        }
    }

    let mut out = vec![("input".to_string(), dump(&lg, graph, "input"))];
    let steps: Vec<Box<dyn ILayoutProcessor>> = vec![
        Box::new(EdgeAndLayerConstraintEdgeReverser::new()),
        Box::new(GreedyCycleBreaker::new()),
        Box::new(LayerConstraintPreprocessor::new()),
        Box::new(NetworkSimplexLayerer::new()),
        Box::new(LayerConstraintPostprocessor::new()),
        Box::new(HighDegreeNodeLayeringProcessor::new()),
        Box::new(LongEdgeSplitter::new()),
        Box::new(SortByInputModelProcessor::new()),
        Box::new(InLayerConstraintProcessor::new()),
        Box::new(PortListSorter::new()),
        Box::new(ReversedEdgeRestorer::new()),
    ];
    for mut step in steps {
        step.process(&mut lg, graph, &mut BasicProgressMonitor::new());
        out.push((step.name().to_string(), dump(&lg, graph, step.name())));
        if step.name() == "SortByInputModelProcessor" {
            out.push(("moc".to_string(), moc_dump(&lg, graph)));
        }
    }
    out
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[test]
fn chain_matches_swift() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/group_a_chain_golden.txt");
    let data = std::fs::read_to_string(path).unwrap();
    let mut cases = 0;
    let mut lines = data.lines().peekable();
    while let Some(header) = lines.next() {
        let params: Vec<i64> = header.trim_start_matches("### ").split(' ').map(|x| x.parse().unwrap()).collect();
        let actual = chain(params[0], params[1], params[2]);
        for (stage, text) in &actual {
            let expected = lines.next().unwrap();
            let (exp_stage, exp_hash) = expected.split_once(' ').unwrap();
            assert_eq!(exp_stage, stage);
            let hash = format!("{:016x}", fnv1a(text));
            assert_eq!(hash, exp_hash, "case {header}, stage {stage} differs from Swift; Rust dump:\n{text}");
        }
        cases += 1;
    }
    assert_eq!(cases, 48);
}

/// `UPLEFT_CHAIN="seed n e" cargo test -p upleft-elk --test group_a_chain print_dump -- --nocapture`
/// prints the full Rust dump for comparison with `lab chain seed n e`.
#[test]
fn print_dump() {
    if let Ok(spec) = std::env::var("UPLEFT_CHAIN") {
        let p: Vec<i64> = spec.split(' ').map(|x| x.parse().unwrap()).collect();
        for (_, text) in chain(p[0], p[1], p[2]) {
            print!("{text}");
        }
    }
}
