//! Group D golden tests: small layered graphs built by hand, run through the
//! direction transformer, the self-loop processors, the label dummy switcher,
//! the hierarchical node resizer and the compound graph processors, and dumped
//! in a canonical text form. `tests/golden/group_d.txt` is the output of the
//! same scenarios run through elk-swift (`tests/golden/group_d_golden.swift`,
//! built as an extra executable of a copy of the elk-swift lab).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use upleft_elk::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphArena, LGraphId, LLabelId, LNodeId, LPortId};
use upleft_elk::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::graph_transformer::{GraphTransformer, Mode};
use upleft_elk::org::eclipse::elk::alg::layered::options::direction_congruency::DirectionCongruency;
use upleft_elk::org::eclipse::elk::alg::layered::options::edge_label_side_selection::EdgeLabelSideSelection;
use upleft_elk::org::eclipse::elk::alg::layered::options::in_layer_constraint::InLayerConstraint;
use upleft_elk::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use upleft_elk::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
use upleft_elk::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use upleft_elk::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use upleft_elk::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use upleft_elk::org::eclipse::elk::core::math::k_vector::{KVector, KVectorRef};
use upleft_elk::org::eclipse::elk::core::math::k_vector_chain::{KVectorChain, KVectorChainRef};
use upleft_elk::org::eclipse::elk::core::math::spacing::Spacing;
use upleft_elk::org::eclipse::elk::core::options::alignment::Alignment;
use upleft_elk::org::eclipse::elk::core::options::direction::Direction;
use upleft_elk::org::eclipse::elk::core::options::node_label_placement::NodeLabelPlacement;
use upleft_elk::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use upleft_elk::org::eclipse::elk::core::options::port_side::PortSide;
use upleft_elk::org::eclipse::elk::core::util::basic_progress_monitor::BasicProgressMonitor;
use upleft_elk::org::eclipse::elk::graph::properties::property::PropValue;
use upleft_elk::swift::describe_double;

// MARK: - Golden file

fn golden_sections() -> HashMap<String, String> {
    let text = include_str!("golden/group_d.txt");
    let mut map = HashMap::new();
    let mut header: Option<String> = None;
    let mut body = String::new();
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("== ") {
            if let Some(prev) = header.take() {
                map.insert(prev, std::mem::take(&mut body));
            }
            header = Some(h.to_string());
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some(prev) = header {
        map.insert(prev, body);
    }
    map
}

fn check(golden: &HashMap<String, String>, header: &str, actual: &str) {
    let expected = golden.get(header).unwrap_or_else(|| panic!("no golden section {header:?}"));
    if expected != actual {
        let e: Vec<&str> = expected.lines().collect();
        let a: Vec<&str> = actual.lines().collect();
        let first = (0..e.len().max(a.len())).find(|&i| e.get(i) != a.get(i)).unwrap();
        panic!(
            "section {header:?} differs at line {}:\n  swift: {}\n  rust:  {}\n--- full rust output ---\n{actual}",
            first + 1,
            e.get(first).unwrap_or(&"<missing>"),
            a.get(first).unwrap_or(&"<missing>"),
        );
    }
}

// MARK: - Dump (same format as the Swift generator)

fn d(x: f64) -> String {
    describe_double(x)
}

fn v(k: KVector) -> String {
    format!("{},{}", d(k.x), d(k.y))
}

fn sp(s: &Spacing) -> String {
    format!("{},{},{},{}", d(s.top), d(s.right), d(s.bottom), d(s.left))
}

fn align_name(a: Option<Alignment>) -> String {
    a.map_or("-".to_string(), |a| format!("{a:?}"))
}

fn pc_name(pc: Option<PortConstraints>) -> String {
    match pc {
        None => "-".into(),
        Some(PortConstraints::UNDEFINED) => "undefined".into(),
        Some(PortConstraints::FREE) => "free".into(),
        Some(PortConstraints::FIXED_SIDE) => "fixedSide".into(),
        Some(PortConstraints::FIXED_ORDER) => "fixedOrder".into(),
        Some(PortConstraints::FIXED_RATIO) => "fixedRatio".into(),
        Some(PortConstraints::FIXED_POS) => "fixedPos".into(),
    }
}

fn opt<T: std::fmt::Debug>(x: Option<T>) -> String {
    x.map_or("-".to_string(), |x| format!("{x:?}"))
}

fn all_nodes(lg: &LGraphArena, g: LGraphId) -> Vec<LNodeId> {
    let mut nodes = lg[g].layerless_nodes.clone();
    for &l in &lg[g].layers {
        nodes.extend_from_slice(&lg[l].nodes);
    }
    nodes
}

fn port_ref(lg: &LGraphArena, p: Option<LPortId>, nodes: &[LNodeId]) -> String {
    let Some(p) = p else { return "?".into() };
    let Some(o) = lg[p].owner else { return "?".into() };
    let Some(ni) = nodes.iter().position(|&n| n == o) else { return "?".into() };
    let Some(pi) = lg[o].ports.iter().position(|&x| x == p) else { return "?".into() };
    format!("N{ni}P{pi}")
}

fn dump(lg: &LGraphArena, g: LGraphId) -> String {
    let mut s = String::new();
    let gp = &lg[g].props;
    let nlp = gp.get_as::<Rc<RefCell<ElkPadding>>>(&LayeredOptions::NODE_LABELS_PADDING).map_or("-".to_string(), |p| sp(&p.borrow().0));
    s += &format!(
        "graph size={} offset={} padding={} nlp={} elss={}\n",
        v(lg[g].size),
        v(lg[g].offset),
        sp(&lg[g].padding.0),
        nlp,
        opt(gp.get_as::<EdgeLabelSideSelection>(&LayeredOptions::EDGE_LABELS_SIDE_SELECTION))
    );
    let nodes = all_nodes(lg, g);
    for (i, &n) in nodes.iter().enumerate() {
        let node = &lg[n];
        let layer = node.layer.map_or("-".to_string(), |l| lg[g].layers.iter().position(|&x| x == l).map_or("x".to_string(), |i| i.to_string()));
        let pos = node.props.get_as::<KVectorRef>(&LayeredOptions::POSITION).map_or("-".to_string(), |p| v(*p.borrow()));
        let min = node.props.get_as::<KVectorRef>(&LayeredOptions::NODE_SIZE_MINIMUM).map_or("-".to_string(), |p| v(*p.borrow()));
        let nlplace = node.props.get_as::<NodeLabelPlacement>(&LayeredOptions::NODE_LABELS_PLACEMENT).map_or("-".to_string(), |p| p.0.to_string());
        s += &format!(
            "N{i} {} pos={} size={} margin={} padding={} layer={}",
            node.node_type.raw_value(),
            v(node.position),
            v(node.size),
            sp(&node.margin.0),
            sp(&node.padding.0),
            layer
        );
        s += &format!(
            " align={} nlplace={} position={} min={}",
            align_name(node.props.get_as::<Alignment>(&LayeredOptions::ALIGNMENT)),
            nlplace,
            pos,
            min
        );
        s += &format!(
            " lc={} ilc={} eps={}",
            opt(node.props.get_as::<LayerConstraint>(&LayeredOptions::LAYERING_LAYER_CONSTRAINT)),
            opt(node.props.get_as::<InLayerConstraint>(&InternalProperties::IN_LAYER_CONSTRAINT)),
            opt(node.props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE))
        );
        s += &format!(" pc={}\n", pc_name(node.props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS)));
        for (j, &p) in node.ports.iter().enumerate() {
            let port = &lg[p];
            s += &format!(
                " P{j} side={:?} pos={} anchor={} size={} idx={}\n",
                port.side,
                v(port.position),
                v(port.anchor),
                v(port.size),
                opt(port.props.get_as::<i64>(&LayeredOptions::PORT_INDEX))
            );
            for &l in &port.labels {
                s += &format!("  PL pos={} size={}\n", v(lg[l].position), v(lg[l].size));
            }
            for &e in &port.outgoing_edges {
                let edge = &lg[e];
                let jps = edge.props.get_as::<KVectorChainRef>(&LayeredOptions::JUNCTION_POINTS).map_or("-".to_string(), |c| {
                    format!("[{}]", c.borrow().iter().map(|&k| v(k)).collect::<Vec<_>>().join(";"))
                });
                let toff = edge.props.get_as::<KVectorRef>(&InternalProperties::TARGET_OFFSET).map_or("-".to_string(), |k| v(*k.borrow()));
                s += &format!(
                    "  E -> {} bends=[{}] jps={} toff={}\n",
                    port_ref(lg, edge.target, &nodes),
                    edge.bend_points.iter().map(|&k| v(k)).collect::<Vec<_>>().join(";"),
                    jps,
                    toff
                );
                for &l in &edge.labels {
                    s += &format!(
                        "   EL pos={} size={} inline={}\n",
                        v(lg[l].position),
                        v(lg[l].size),
                        opt(lg[l].props.get_as::<bool>(&LayeredOptions::EDGE_LABELS_INLINE))
                    );
                }
            }
        }
        for &l in &node.labels {
            let lp = lg[l].props.get_as::<NodeLabelPlacement>(&LayeredOptions::NODE_LABELS_PLACEMENT).map_or("-".to_string(), |p| p.0.to_string());
            s += &format!(" NL pos={} size={} nlplace={}\n", v(lg[l].position), v(lg[l].size), lp);
        }
        if let Some(nested) = node.nested_graph {
            s += " nested {\n";
            s += &dump(lg, nested);
            s += " }\n";
        }
    }
    s
}

// MARK: - Builders (same as the Swift generator)

fn node(lg: &mut LGraphArena, g: LGraphId, x: f64, y: f64, w: f64, h: f64) -> LNodeId {
    let n = lg.new_node(Some(g));
    lg[n].position = KVector::new(x, y);
    lg[n].size = KVector::new(w, h);
    lg[g].layerless_nodes.push(n);
    n
}

fn port(lg: &mut LGraphArena, n: LNodeId, side: PortSide, x: f64, y: f64, w: f64, h: f64) -> LPortId {
    let p = lg.new_port();
    lg.port_set_node(p, Some(n));
    lg[p].size = KVector::new(w, h);
    lg.port_set_side(p, side);
    lg[p].position = KVector::new(x, y);
    p
}

fn edge(lg: &mut LGraphArena, s: LPortId, t: LPortId) -> LEdgeId {
    let e = lg.new_edge();
    lg.edge_set_source(e, Some(s));
    lg.edge_set_target(e, Some(t));
    e
}

fn label(lg: &mut LGraphArena, w: f64, h: f64, x: f64, y: f64) -> LLabelId {
    let l = lg.new_label("l");
    lg[l].size = KVector::new(w, h);
    lg[l].position = KVector::new(x, y);
    l
}

fn run(lg: &mut LGraphArena, g: LGraphId, p: &mut dyn ILayoutProcessor) {
    let mut monitor = BasicProgressMonitor::new();
    p.process(lg, g, &mut monitor);
}

// MARK: - GraphTransformer

fn transformer_graph(lg: &mut LGraphArena, dir: Direction, congruency: Option<DirectionCongruency>) -> LGraphId {
    let g = lg.new_graph();
    lg[g].props.set(&LayeredOptions::DIRECTION, dir);
    if let Some(c) = congruency {
        lg[g].props.set(&LayeredOptions::DIRECTION_CONGRUENCY, c);
    }
    lg[g].size = KVector::new(300.0, 200.0);
    lg[g].offset = KVector::new(3.0, 7.0);
    lg[g].padding.0 = Spacing::new(1.0, 2.0, 3.0, 4.0);
    lg[g].props.set(&LayeredOptions::NODE_LABELS_PADDING, PropValue::elk_padding(ElkPadding::new(5.0, 6.0, 7.0, 8.0)));
    lg[g].props.set(&LayeredOptions::EDGE_LABELS_SIDE_SELECTION, EdgeLabelSideSelection::SMART_UP);

    let a = node(lg, g, 10.0, 20.0, 40.0, 30.0);
    lg[a].margin.0 = Spacing::new(1.0, 2.0, 3.0, 4.0);
    lg[a].padding.0 = Spacing::new(5.0, 6.0, 7.0, 8.0);
    lg[a].props.set(&LayeredOptions::ALIGNMENT, Alignment::LEFT);
    lg[a].props.set(&LayeredOptions::NODE_LABELS_PLACEMENT, NodeLabelPlacement::of(&[NodeLabelPlacement::INSIDE, NodeLabelPlacement::H_LEFT, NodeLabelPlacement::V_TOP]));
    lg[a].props.set(&LayeredOptions::POSITION, PropValue::kvector(KVector::new(11.0, 22.0)));
    lg[a].props.set(&LayeredOptions::NODE_SIZE_MINIMUM, PropValue::kvector(KVector::new(15.0, 25.0)));
    let al = label(lg, 12.0, 6.0, 1.0, 2.0);
    lg[al].props.set(
        &LayeredOptions::NODE_LABELS_PLACEMENT,
        NodeLabelPlacement::of(&[NodeLabelPlacement::OUTSIDE, NodeLabelPlacement::H_RIGHT, NodeLabelPlacement::V_BOTTOM, NodeLabelPlacement::H_PRIORITY]),
    );
    lg[a].labels.push(al);

    let b = node(lg, g, 120.0, 50.0, 30.0, 60.0);
    lg[b].props.set(&LayeredOptions::ALIGNMENT, Alignment::BOTTOM);
    let c = node(lg, g, 200.0, 5.0, 20.0, 20.0);
    lg[c].props.set(&LayeredOptions::ALIGNMENT, Alignment::TOP);

    let ext = node(lg, g, 0.0, 90.0, 10.0, 10.0);
    lg[ext].node_type = NodeType::EXTERNAL_PORT;
    lg[ext].props.set(&InternalProperties::EXT_PORT_SIDE, PortSide::WEST);
    lg[ext].props.set(&LayeredOptions::LAYERING_LAYER_CONSTRAINT, LayerConstraint::FIRST_SEPARATE);

    let ap = port(lg, a, PortSide::EAST, 40.0, 12.0, 4.0, 6.0);
    lg[ap].props.set(&LayeredOptions::PORT_INDEX, 3i64);
    let apl = label(lg, 5.0, 3.0, 2.0, -4.0);
    lg[ap].labels.push(apl);
    let an = port(lg, a, PortSide::NORTH, 10.0, -2.0, 6.0, 2.0);
    lg[an].explicitly_supplied_port_anchor = true;
    lg[an].anchor = KVector::new(1.5, 0.25);
    let bw = port(lg, b, PortSide::WEST, -4.0, 20.0, 4.0, 8.0);
    let bs = port(lg, b, PortSide::SOUTH, 13.0, 60.0, 4.0, 4.0);
    let cw = port(lg, c, PortSide::WEST, -2.0, 8.0, 2.0, 4.0);
    let ep = port(lg, ext, PortSide::EAST, 10.0, 5.0, 0.0, 0.0);

    let e1 = edge(lg, ap, bw);
    lg[e1].bend_points.add(KVector::new(80.0, 35.0));
    lg[e1].bend_points.add(KVector::new(80.0, 74.0));
    lg[e1].props.set(&LayeredOptions::JUNCTION_POINTS, PropValue::kvector_chain(KVectorChain::from_vec(vec![KVector::new(80.0, 50.0)])));
    let el = label(lg, 14.0, 8.0, 60.0, 40.0);
    lg[el].props.set(&LayeredOptions::EDGE_LABELS_INLINE, true);
    lg[e1].labels.push(el);
    let e2 = edge(lg, bs, cw);
    lg[e2].bend_points.add(KVector::new(135.0, 150.0));
    lg[e2].bend_points.add(KVector::new(170.0, 150.0));
    lg[e2].bend_points.add(KVector::new(170.0, 15.0));
    edge(lg, ep, an);
    edge(lg, an, cw);
    g
}

#[test]
fn graph_transformer_matches_swift() {
    let golden = golden_sections();
    let dirs = [Direction::RIGHT, Direction::LEFT, Direction::DOWN, Direction::UP, Direction::UNDEFINED];
    for congruency in [None, Some(DirectionCongruency::READING_DIRECTION), Some(DirectionCongruency::ROTATION)] {
        for dir in dirs {
            for mode in [Mode::TO_INTERNAL_LTR, Mode::TO_INPUT_DIRECTION] {
                let mut lg = LGraphArena::new();
                let g = transformer_graph(&mut lg, dir, congruency);
                run(&mut lg, g, &mut GraphTransformer::new(mode));
                check(&golden, &format!("transformer {} {dir:?} {mode:?}", opt(congruency)), &dump(&lg, g));
            }
            let mut lg = LGraphArena::new();
            let g = transformer_graph(&mut lg, dir, congruency);
            run(&mut lg, g, &mut GraphTransformer::new(Mode::TO_INPUT_DIRECTION));
            lg[g].size.x += 17.0;
            lg[g].size.y += 9.0;
            run(&mut lg, g, &mut GraphTransformer::new(Mode::TO_INTERNAL_LTR));
            check(&golden, &format!("transformer roundtrip {} {dir:?}", opt(congruency)), &dump(&lg, g));
        }
    }
    for dir in [Direction::LEFT, Direction::UP] {
        let mut lg = LGraphArena::new();
        let g = transformer_graph(&mut lg, dir, None);
        lg[g].size = KVector::new(0.0, 0.0);
        run(&mut lg, g, &mut GraphTransformer::new(Mode::TO_INPUT_DIRECTION));
        check(&golden, &format!("transformer zero-size {dir:?}"), &dump(&lg, g));
    }
}
