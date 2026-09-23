//! Differential test of `ComponentsProcessor` (split, and combine with every
//! graph placer) against elk-swift.
//!
//! Mirrors the instrumented lab's `comp <seed> <n> <e> <mode>`: a random graph
//! (mode 0: no external ports → `SimpleRowGraphPlacer`; 1: external port
//! dummies → `ComponentGroupGraphPlacer`; 2 and 4: `MODEL_ORDER` components →
//! `ModelOrderRowGraphPlacer`; 3: `GROUP_MODEL_ORDER` →
//! `ComponentGroupModelOrderGraphPlacer`) is split, each component gets a
//! random size, offset, node positions, bend points, junction points and
//! label positions, and the components are combined. The lab's
//! `ComponentGroup` keeps its dictionary in insertion order, like the port
//! (plain elk-swift varies the combined node order from run to run). The
//! golden file stores an FNV-1a hash of each case's dump.

use std::cell::RefCell;
use std::rc::Rc;

use upleft_elk::org::eclipse::elk::alg::layered::components::component_ordering_strategy::ComponentOrderingStrategy;
use upleft_elk::org::eclipse::elk::alg::layered::components::components_processor::ComponentsProcessor;
use upleft_elk::org::eclipse::elk::alg::layered::graph_configurator::Random;
use upleft_elk::prelude::*;
use upleft_elk::swift::describe_double as d;

fn side_char(s: PortSide) -> &'static str {
    match s {
        PortSide::NORTH => "N",
        PortSide::EAST => "E",
        PortSide::SOUTH => "S",
        PortSide::WEST => "W",
        _ => "U",
    }
}

fn nm(lg: &LGraphArena, n: LNodeId) -> String {
    lg[n].labels.first().map_or("?".to_string(), |&l| lg[l].text.clone())
}

fn comp(seed: i64, n: i64, e: i64, mode: i64) -> String {
    let mut rnd = Random::with_seed(seed);
    let mut lg = LGraphArena::new();
    let graph = lg.new_graph();
    lg[graph].props.set(&LayeredOptions::SPACING_COMPONENT_COMPONENT, rnd.next_int_bounded(30) as f64 + 0.5);
    if rnd.next_int_bounded(2) == 0 {
        lg[graph].props.set(&LayeredOptions::ASPECT_RATIO, rnd.next_int_bounded(20) as f64 / 7.0 + 0.3);
    }
    if mode >= 1 {
        lg[graph].props.set(&InternalProperties::GRAPH_PROPERTIES, EnumSet::of(&[GraphProperties::EXTERNAL_PORTS]));
    }
    if mode == 2 || mode == 4 {
        lg[graph].props.set(&LayeredOptions::CONSIDER_MODEL_ORDER_COMPONENTS, ComponentOrderingStrategy::MODEL_ORDER);
    }
    if mode == 3 {
        lg[graph].props.set(&LayeredOptions::CONSIDER_MODEL_ORDER_COMPONENTS, ComponentOrderingStrategy::GROUP_MODEL_ORDER);
    }
    if rnd.next_int_bounded(2) == 0 {
        lg[graph].props.set(&LayeredOptions::DIRECTION, Direction::DOWN);
    } else {
        lg[graph].props.set(&LayeredOptions::DIRECTION, Direction::RIGHT);
    }
    lg[graph].padding.top = 3.5;
    lg[graph].padding.left = 1.25;

    let mut nodes = Vec::new();
    for i in 0..n {
        let node = lg.new_node(Some(graph));
        lg[graph].layerless_nodes.push(node);
        let label = lg.new_label(&format!("n{i}"));
        lg[node].labels.push(label);
        if rnd.next_int_bounded(6) != 0 {
            lg[node].props.set(&InternalProperties::MODEL_ORDER, (i * 5) % 17);
        }
        if mode >= 1 && rnd.next_int_bounded(4) == 0 {
            lg[node].node_type = NodeType::EXTERNAL_PORT;
            let sides = [PortSide::NORTH, PortSide::EAST, PortSide::SOUTH, PortSide::WEST];
            lg[node].props.set(&InternalProperties::EXT_PORT_SIDE, sides[rnd.next_int_bounded(4) as usize]);
        }
        nodes.push(node);
    }
    let mut pc = 0;
    for _ in 0..e {
        let s = rnd.next_int_bounded(n) as usize;
        let t = rnd.next_int_bounded(n) as usize;
        if s == t {
            continue;
        }
        let sp = lg.new_port();
        lg.port_set_side(sp, PortSide::EAST);
        lg.port_set_node(sp, Some(nodes[s]));
        let l = lg.new_label(&format!("p{pc}"));
        lg[sp].labels.push(l);
        pc += 1;
        let tp = lg.new_port();
        lg.port_set_side(tp, PortSide::WEST);
        lg.port_set_node(tp, Some(nodes[t]));
        let l = lg.new_label(&format!("p{pc}"));
        lg[tp].labels.push(l);
        pc += 1;
        let edge = lg.new_edge();
        lg.edge_set_source(edge, Some(sp));
        lg.edge_set_target(edge, Some(tp));
        if rnd.next_int_bounded(3) == 0 {
            let l = lg.new_label("l");
            lg[edge].labels.push(l);
        }
    }

    let mut cp = ComponentsProcessor::new();
    let comps = cp.split(&mut lg, graph);
    let mut out = format!("comps {}\n", comps.len());
    for &c in &comps {
        let sides_set = lg[c].props.get_as::<EnumSet<PortSide>>(&InternalProperties::EXT_PORT_CONNECTIONS).unwrap_or_default();
        let mut sides: Vec<&str> = sides_set.iter().map(side_char).collect();
        sides.sort();
        out += &format!("C[{}]:", sides.join(""));
        for &node in &lg[c].layerless_nodes {
            out += &format!(" {}", nm(&lg, node));
        }
        out += "\n";
        lg[c].size.x = rnd.next_int_bounded(200) as f64 / 3.0;
        lg[c].size.y = rnd.next_int_bounded(150) as f64 / 7.0;
        lg[c].offset.x = rnd.next_int_bounded(10) as f64 / 3.0;
        lg[c].offset.y = rnd.next_int_bounded(10) as f64 / 9.0;
        for node in lg[c].layerless_nodes.clone() {
            lg[node].position.x = rnd.next_int_bounded(100) as f64 / 3.0;
            lg[node].position.y = rnd.next_int_bounded(100) as f64 / 7.0;
            for port in lg[node].ports.clone() {
                for edge in lg[port].outgoing_edges.clone() {
                    lg[edge].bend_points.add(KVector::new(rnd.next_int_bounded(50) as f64 / 3.0, 1.5));
                    if rnd.next_int_bounded(3) == 0 {
                        let mut jp = KVectorChain::new();
                        jp.add(KVector::new(1.0 / 3.0, 2.0 / 3.0));
                        lg[edge].props.set(&LayeredOptions::JUNCTION_POINTS, Rc::new(RefCell::new(jp)));
                    }
                    for l in lg[edge].labels.clone() {
                        lg[l].position.x = 0.1;
                        lg[l].position.y = 0.2;
                    }
                }
            }
        }
    }
    cp.combine(&mut lg, &comps, graph);
    let g = &lg[graph];
    out += &format!(
        "size {} {} offset {} {} pad {} {}\n",
        d(g.size.x),
        d(g.size.y),
        d(g.offset.x),
        d(g.offset.y),
        d(g.padding.top),
        d(g.padding.left)
    );
    for &node in &lg[graph].layerless_nodes {
        out += &format!(
            "{} {} {} g={}\n",
            nm(&lg, node),
            d(lg[node].position.x),
            d(lg[node].position.y),
            lg.node_graph(node) == Some(graph)
        );
        for &port in &lg[node].ports {
            for &edge in &lg[port].outgoing_edges {
                out += "  bp";
                for p in lg[edge].bend_points.iter() {
                    out += &format!(" {},{}", d(p.x), d(p.y));
                }
                if let Some(jp) = lg[edge].props.get_as::<Rc<RefCell<KVectorChain>>>(&LayeredOptions::JUNCTION_POINTS) {
                    out += " jp";
                    for p in jp.borrow().iter() {
                        out += &format!(" {},{}", d(p.x), d(p.y));
                    }
                }
                for &l in &lg[edge].labels {
                    out += &format!(" l {},{}", d(lg[l].position.x), d(lg[l].position.y));
                }
                out += "\n";
            }
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
fn components_match_swift() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/group_a_components_golden.txt");
    let data = std::fs::read_to_string(path).unwrap();
    let mut cases = 0;
    for line in data.lines() {
        let (params, hash) = line.trim_start_matches("### ").rsplit_once(' ').unwrap();
        let p: Vec<i64> = params.split(' ').map(|x| x.parse().unwrap()).collect();
        let actual = comp(p[0], p[1], p[2], p[3]);
        assert_eq!(format!("{:016x}", fnv1a(&actual)), hash, "case {params} differs from Swift; Rust dump:\n{actual}");
        cases += 1;
    }
    assert_eq!(cases, 120);
}

/// `UPLEFT_COMP="seed n e mode" cargo test -p upleft-elk --test group_a_components print_dump -- --nocapture`
#[test]
fn print_dump() {
    if let Ok(spec) = std::env::var("UPLEFT_COMP") {
        let p: Vec<i64> = spec.split(' ').map(|x| x.parse().unwrap()).collect();
        print!("{}", comp(p[0], p[1], p[2], p[3]));
    }
}
