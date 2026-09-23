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
use upleft_elk::org::eclipse::elk::alg::layered::compound::compound_graph_postprocessor::CompoundGraphPostprocessor;
use upleft_elk::org::eclipse::elk::alg::layered::compound::compound_graph_preprocessor::CompoundGraphPreprocessor;
use upleft_elk::org::eclipse::elk::alg::layered::compound::cross_hierarchy_edge::CrossHierarchyMap;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::hierarchical_node_resizing_processor::HierarchicalNodeResizingProcessor;
use upleft_elk::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::label_dummy_switcher::{LabelDummySwitcher, INCLUDE_LABEL};
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::self_loop_port_restorer::SelfLoopPortRestorer;
use upleft_elk::org::eclipse::elk::alg::layered::options::center_edge_label_placement_strategy::CenterEdgeLabelPlacementStrategy;
use upleft_elk::org::eclipse::elk::alg::layered::options::graph_properties::GraphProperties;
use upleft_elk::org::eclipse::elk::alg::layered::graph::l_graph::LayerId;
use upleft_elk::bridge::java_compat::EnumSet;
use upleft_elk::org::eclipse::elk::core::options::content_alignment::ContentAlignment;
use upleft_elk::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use upleft_elk::org::eclipse::elk::core::options::size_options::SizeOptions;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::self_loop_post_processor::SelfLoopPostProcessor;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::self_loop_pre_processor::SelfLoopPreProcessor;
use upleft_elk::org::eclipse::elk::alg::layered::intermediate::self_loop_router::SelfLoopRouter;
use upleft_elk::org::eclipse::elk::alg::layered::options::self_loop_distribution_strategy::SelfLoopDistributionStrategy;
use upleft_elk::org::eclipse::elk::alg::layered::options::self_loop_ordering_strategy::SelfLoopOrderingStrategy;
use upleft_elk::org::eclipse::elk::core::options::edge_routing::EdgeRouting;
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

// MARK: - Self loops

fn place_ports(lg: &mut LGraphArena, n: LNodeId) {
    let size = lg[n].size;
    for (i, p) in lg[n].ports.clone().into_iter().enumerate() {
        let k = i as f64;
        let side = lg[p].side;
        let pos = &mut lg[p].position;
        match side {
            PortSide::NORTH => {
                pos.x = 5.0 + 3.0 * k;
                pos.y = 0.0;
            }
            PortSide::SOUTH => {
                pos.x = 5.0 + 3.0 * k;
                pos.y = size.y;
            }
            PortSide::EAST => {
                pos.x = size.x;
                pos.y = 2.0 + 2.0 * k;
            }
            PortSide::WEST => {
                pos.x = 0.0;
                pos.y = 2.0 + 2.0 * k;
            }
            _ => {}
        }
    }
}

fn self_loop_graph(lg: &mut LGraphArena, fixed_sides: bool, dir: Direction) -> (LGraphId, LNodeId) {
    let g = lg.new_graph();
    lg[g].props.set(&LayeredOptions::DIRECTION, dir);
    let n = node(lg, g, 0.0, 0.0, 60.0, 40.0);
    let m = node(lg, g, 200.0, 0.0, 20.0, 20.0);
    let sd = |s: PortSide| if fixed_sides { s } else { PortSide::UNDEFINED };
    let p_e = port(lg, n, PortSide::EAST, 60.0, 20.0, 0.0, 0.0);
    let m_w = port(lg, m, PortSide::WEST, 0.0, 10.0, 0.0, 0.0);
    edge(lg, p_e, m_w);
    let sides = [
        PortSide::NORTH, PortSide::NORTH, PortSide::EAST, PortSide::SOUTH, PortSide::WEST, PortSide::EAST, PortSide::NORTH, PortSide::EAST,
        PortSide::SOUTH, PortSide::NORTH, PortSide::EAST, PortSide::SOUTH, PortSide::WEST, PortSide::SOUTH, PortSide::WEST,
    ];
    let mut ps = Vec::new();
    for side in sides {
        ps.push(port(lg, n, sd(side), 0.0, 0.0, 0.0, 0.0));
    }
    let [p1, p2, p3, p4, p5, p6, p7, p8, p9, p10, p11, p12, p13, p14, p16] = ps.try_into().unwrap();
    edge(lg, p1, p2);
    edge(lg, p3, p4);
    let e3 = edge(lg, p5, p6);
    let l = label(lg, 20.0, 8.0, 0.0, 0.0);
    lg[e3].labels.push(l);
    edge(lg, p7, p8);
    let e4 = edge(lg, p8, p9);
    let l4 = label(lg, 16.0, 6.0, 0.0, 0.0);
    lg[l4].props.set(&LayeredOptions::EDGE_LABELS_INLINE, true);
    lg[e4].labels.push(l4);
    edge(lg, p10, p11);
    edge(lg, p11, p12);
    let e5 = edge(lg, p12, p13);
    let l = label(lg, 10.0, 10.0, 0.0, 0.0);
    lg[e5].labels.push(l);
    let l = label(lg, 4.0, 3.0, 0.0, 0.0);
    lg[e5].labels.push(l);
    let e6 = edge(lg, p14, p14);
    let l = label(lg, 12.0, 4.0, 0.0, 0.0);
    lg[e6].labels.push(l);
    edge(lg, p_e, p16);
    (g, n)
}

#[test]
fn self_loops_match_swift() {
    let golden = golden_sections();
    use SelfLoopDistributionStrategy as D;
    use SelfLoopOrderingStrategy as O;
    let cases: [(&str, PortConstraints, Option<D>, Option<O>, Direction, Option<EdgeRouting>); 9] = [
        ("free", PortConstraints::FREE, Some(D::NORTH), Some(O::STACKED), Direction::RIGHT, None),
        ("free", PortConstraints::FREE, Some(D::NORTH_SOUTH), Some(O::SEQUENCED), Direction::RIGHT, Some(EdgeRouting::ORTHOGONAL)),
        ("free", PortConstraints::UNDEFINED, Some(D::EQUALLY), Some(O::REVERSE_STACKED), Direction::RIGHT, None),
        ("free", PortConstraints::FREE, Some(D::EQUALLY), Some(O::STACKED), Direction::DOWN, Some(EdgeRouting::POLYLINE)),
        ("free", PortConstraints::FREE, None, Some(O::SEQUENCED), Direction::UP, None),
        ("fixedSide", PortConstraints::FIXED_SIDE, None, Some(O::STACKED), Direction::RIGHT, None),
        ("fixedSide", PortConstraints::FIXED_SIDE, None, Some(O::SEQUENCED), Direction::DOWN, None),
        ("fixedSide", PortConstraints::FIXED_SIDE, None, Some(O::REVERSE_STACKED), Direction::LEFT, Some(EdgeRouting::POLYLINE)),
        ("fixedOrder", PortConstraints::FIXED_ORDER, None, Some(O::STACKED), Direction::RIGHT, None),
    ];
    for (kind, opc, dist, ordering, dir, routing) in cases {
        let mut lg = LGraphArena::new();
        let (g, n) = self_loop_graph(&mut lg, kind != "free", dir);
        if kind == "fixedOrder" {
            lg[n].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_ORDER);
        }
        if let Some(dist) = dist {
            lg[n].props.set(&LayeredOptions::EDGE_ROUTING_SELF_LOOP_DISTRIBUTION, dist);
        }
        if let Some(ordering) = ordering {
            lg[n].props.set(&LayeredOptions::EDGE_ROUTING_SELF_LOOP_ORDERING, ordering);
        }
        if let Some(routing) = routing {
            lg[g].props.set(&LayeredOptions::EDGE_ROUTING, routing);
        }
        lg[n].props.set(&InternalProperties::ORIGINAL_PORT_CONSTRAINTS, opc);
        let header = format!("selfloops {kind} {opc:?} {} {} {dir:?} {}", opt(dist), opt(ordering), opt(routing));

        run(&mut lg, g, &mut SelfLoopPreProcessor::new());
        check(&golden, &format!("{header} pre"), &dump(&lg, g));

        let layer = lg.new_layer(g);
        lg[g].layers.push(layer);
        for x in lg[g].layerless_nodes.clone() {
            lg.node_set_layer(x, Some(layer));
        }
        lg[g].layerless_nodes.clear();

        run(&mut lg, g, &mut SelfLoopPortRestorer::new());
        place_ports(&mut lg, n);
        check(&golden, &format!("{header} restored"), &dump(&lg, g));

        lg[n].position = KVector::new(100.0, 50.0);
        run(&mut lg, g, &mut SelfLoopRouter::new());
        run(&mut lg, g, &mut SelfLoopPostProcessor::new());
        check(&golden, &format!("{header} routed"), &dump(&lg, g));
    }
}

// MARK: - Label dummy switcher

fn chain_node(lg: &mut LGraphArena, g: LGraphId, layer: LayerId, t: NodeType, w: f64, h: f64) -> LNodeId {
    let n = lg.new_node(Some(g));
    lg[n].node_type = t;
    lg[n].size = KVector::new(w, h);
    lg.node_set_layer(n, Some(layer));
    let i = lg.new_port();
    lg.port_set_node(i, Some(n));
    lg.port_set_side(i, PortSide::WEST);
    let o = lg.new_port();
    lg.port_set_node(o, Some(n));
    lg.port_set_side(o, PortSide::EAST);
    n
}

fn chain(lg: &mut LGraphArena, nodes: &[LNodeId], reversed: bool) {
    for k in 0..nodes.len() - 1 {
        let e = lg.new_edge();
        let s = lg[nodes[k]].ports[1];
        let t = lg[nodes[k + 1]].ports[0];
        lg.edge_set_source(e, Some(s));
        lg.edge_set_target(e, Some(t));
        if reversed {
            lg[e].props.set(&InternalProperties::REVERSED, true);
        }
    }
}

type Cs = CenterEdgeLabelPlacementStrategy;

fn switcher_graph(lg: &mut LGraphArena, strategy: Option<Cs>, override_: Option<Cs>, trivial: bool) -> (LGraphId, Vec<LLabelId>) {
    let g = lg.new_graph();
    lg[g].props.set(&LayeredOptions::DIRECTION, Direction::RIGHT);
    if let Some(s) = strategy {
        lg[g].props.set(&LayeredOptions::EDGE_LABELS_CENTER_LABEL_PLACEMENT_STRATEGY, s);
    }
    let widths = [30.0, 50.0, 10.0, 25.0, 70.0, 20.0, 30.0];
    let mut layers = Vec::new();
    for w in widths {
        let l = lg.new_layer(g);
        lg[l].size.x = w + 5.0;
        lg[g].layers.push(l);
        layers.push(l);
    }
    for k in 0..7 {
        chain_node(lg, g, layers[k], NodeType::NORMAL, widths[k], 10.0);
    }
    let mut labels = Vec::new();
    let mut label_dummy = |lg: &mut LGraphArena, layer: LayerId, w: f64| {
        let n = chain_node(lg, g, layer, NodeType::LABEL, w, 12.0);
        let l = label(lg, w, 12.0, 0.0, 0.0);
        if let Some(o) = override_ {
            lg[l].props.set(&LayeredOptions::EDGE_LABELS_CENTER_LABEL_PLACEMENT_STRATEGY, o);
        }
        lg[n].props.set(&InternalProperties::REPRESENTED_LABELS, vec![l]);
        labels.push(l);
        n
    };
    let c1 = vec![
        chain_node(lg, g, layers[0], NodeType::NORMAL, 30.0, 20.0),
        chain_node(lg, g, layers[1], NodeType::LONG_EDGE, 0.0, 0.0),
        chain_node(lg, g, layers[2], NodeType::LONG_EDGE, 0.0, 0.0),
        label_dummy(lg, layers[3], 40.0),
        chain_node(lg, g, layers[4], NodeType::LONG_EDGE, 0.0, 0.0),
        chain_node(lg, g, layers[5], NodeType::LONG_EDGE, 0.0, 0.0),
        chain_node(lg, g, layers[6], NodeType::NORMAL, 30.0, 20.0),
    ];
    chain(lg, &c1, false);
    if trivial {
        let c2 = vec![
            chain_node(lg, g, layers[2], NodeType::NORMAL, 10.0, 10.0),
            label_dummy(lg, layers[3], 8.0),
            chain_node(lg, g, layers[4], NodeType::NORMAL, 10.0, 10.0),
        ];
        chain(lg, &c2, false);
    }
    let c3 = vec![
        chain_node(lg, g, layers[1], NodeType::NORMAL, 10.0, 10.0),
        chain_node(lg, g, layers[2], NodeType::LONG_EDGE, 0.0, 0.0),
        label_dummy(lg, layers[3], 90.0),
        chain_node(lg, g, layers[4], NodeType::LONG_EDGE, 0.0, 0.0),
        chain_node(lg, g, layers[5], NodeType::NORMAL, 10.0, 10.0),
    ];
    chain(lg, &c3, true);
    let c4 = vec![
        chain_node(lg, g, layers[0], NodeType::NORMAL, 10.0, 10.0),
        label_dummy(lg, layers[1], 60.0),
        chain_node(lg, g, layers[2], NodeType::LONG_EDGE, 0.0, 0.0),
        chain_node(lg, g, layers[3], NodeType::LONG_EDGE, 0.0, 0.0),
        chain_node(lg, g, layers[4], NodeType::LONG_EDGE, 0.0, 0.0),
        chain_node(lg, g, layers[5], NodeType::NORMAL, 10.0, 10.0),
    ];
    chain(lg, &c4, false);
    (g, labels)
}

fn switcher_dump(lg: &LGraphArena, g: LGraphId, labels: &[LLabelId]) -> String {
    let mut s = String::new();
    for (li, &l) in lg[g].layers.iter().enumerate() {
        s += &format!("L{li} id={}:", lg[l].id);
        for &n in &lg[l].nodes {
            let lebld = lg[n].props.get_as::<bool>(&InternalProperties::LONG_EDGE_BEFORE_LABEL_DUMMY).map_or("", |b| if b { "b" } else { "n" });
            let pred = lg.node_incoming_edges(n).first().and_then(|&e| lg.edge_source_node(e));
            let pred_ref = pred
                .and_then(|p| {
                    let pl = lg[p].layer?;
                    let pli = lg[g].layers.iter().position(|&x| x == pl)?;
                    let pi = lg[pl].nodes.iter().position(|&x| x == p)?;
                    Some(format!("{pli}.{pi}"))
                })
                .unwrap_or_else(|| "-".to_string());
            s += &format!(
                " {}{lebld}<{pred_ref} {}",
                &lg[n].node_type.raw_value()[..2],
                align_name(lg[n].props.get_as::<Alignment>(&LayeredOptions::ALIGNMENT))
            );
        }
        s += "\n";
    }
    s += "labels:";
    for &l in labels {
        s += &format!(" {}", opt(lg[l].props.get_as::<bool>(&INCLUDE_LABEL)));
    }
    s += "\n";
    s
}

#[test]
fn label_dummy_switcher_matches_swift() {
    let golden = golden_sections();
    let strategies = [None, Some(Cs::MEDIAN_LAYER), Some(Cs::TAIL_LAYER), Some(Cs::HEAD_LAYER), Some(Cs::SPACE_EFFICIENT_LAYER), Some(Cs::WIDEST_LAYER), Some(Cs::CENTER_LAYER)];
    for strategy in strategies {
        for override_ in [None, Some(Cs::HEAD_LAYER), Some(Cs::CENTER_LAYER), Some(Cs::WIDEST_LAYER)] {
            let widest = strategy == Some(Cs::WIDEST_LAYER) || override_ == Some(Cs::WIDEST_LAYER);
            let mut lg = LGraphArena::new();
            let (g, labels) = switcher_graph(&mut lg, strategy, override_, !widest);
            run(&mut lg, g, &mut LabelDummySwitcher::new());
            check(&golden, &format!("switcher {} {}", opt(strategy), opt(override_)), &switcher_dump(&lg, g, &labels));
        }
    }
}

/// Swift traps on `(l + 1)...r` when a WIDEST_LAYER label dummy sits between
/// two normal nodes (l == r).
#[test]
#[should_panic(expected = "Range requires lowerBound <= upperBound")]
fn label_dummy_switcher_widest_layer_traps_like_swift() {
    let mut lg = LGraphArena::new();
    let (g, _) = switcher_graph(&mut lg, Some(Cs::WIDEST_LAYER), None, true);
    run(&mut lg, g, &mut LabelDummySwitcher::new());
}

// MARK: - Hierarchical node resizing

fn graph_props_names(lg: &LGraphArena, g: LGraphId) -> String {
    match lg[g].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES) {
        None => "-".into(),
        Some(set) => {
            let mut names: Vec<String> = set.iter().map(|p| format!("{p:?}")).collect();
            names.sort();
            names.join(",")
        }
    }
}

#[test]
fn hierarchical_node_resizing_matches_swift() {
    let golden = golden_sections();
    for (parent_dir, child_dir) in [(Direction::RIGHT, Direction::RIGHT), (Direction::DOWN, Direction::RIGHT), (Direction::RIGHT, Direction::UP), (Direction::UP, Direction::DOWN)] {
        for variant in 0..3 {
            let mut lg = LGraphArena::new();
            let g = lg.new_graph();
            lg[g].props.set(&LayeredOptions::DIRECTION, parent_dir);
            lg[g].props.set(&InternalProperties::GRAPH_PROPERTIES, EnumSet::of(&[GraphProperties::HYPEREDGES]));
            let p = node(&mut lg, g, 5.0, 6.0, 10.0, 10.0);
            let pp = port(&mut lg, p, PortSide::EAST, 10.0, 5.0, 2.0, 3.0);
            let pl = label(&mut lg, 4.0, 4.0, 11.0, 12.0);
            lg[p].labels.push(pl);
            let c = lg.new_graph();
            lg[c].parent_node = Some(p);
            lg[p].nested_graph = Some(c);
            lg[c].props.set(&LayeredOptions::DIRECTION, child_dir);
            lg[c].size = KVector::new(80.0, 40.0);
            lg[c].padding.0 = Spacing::new(2.0, 3.0, 4.0, 5.0);
            lg[c].offset = KVector::new(1.0, 2.0);
            let layer = lg.new_layer(c);
            lg[c].layers.push(layer);
            let inner = lg.new_node(Some(c));
            lg[inner].position = KVector::new(10.0, 12.0);
            lg[inner].size = KVector::new(20.0, 10.0);
            lg.node_set_layer(inner, Some(layer));
            let ext = lg.new_node(Some(c));
            lg[ext].node_type = NodeType::EXTERNAL_PORT;
            lg[ext].size = KVector::new(4.0, 6.0);
            lg[ext].position = KVector::new(90.0, 17.0);
            lg[ext].props.set(&InternalProperties::ORIGIN, PropValue::LPort(pp));
            lg[ext].props.set(&InternalProperties::EXT_PORT_SIDE, PortSide::EAST);
            lg[ext].props.set(&LayeredOptions::PORT_BORDER_OFFSET, 1.5);
            lg.node_set_layer(ext, Some(layer));
            if variant >= 1 {
                lg[c].props.set(&InternalProperties::GRAPH_PROPERTIES, EnumSet::of(&[GraphProperties::EXTERNAL_PORTS]));
            }
            if variant == 2 {
                lg[c].props.set(&LayeredOptions::NODE_SIZE_CONSTRAINTS, SizeConstraint::MINIMUM_SIZE);
                lg[c].props.set(&LayeredOptions::NODE_SIZE_OPTIONS, SizeOptions::DEFAULT_MINIMUM_SIZE);
                lg[c].props.set(&LayeredOptions::NODE_SIZE_MINIMUM, PropValue::kvector(KVector::new(120.0, 0.0)));
                lg[c].props.set(&LayeredOptions::CONTENT_ALIGNMENT, ContentAlignment::H_CENTER | ContentAlignment::V_BOTTOM);
            }
            run(&mut lg, c, &mut HierarchicalNodeResizingProcessor::new());
            let cmin = lg[c].props.get_as::<KVectorRef>(&LayeredOptions::NODE_SIZE_MINIMUM).map_or("-".to_string(), |k| v(*k.borrow()));
            let out = format!(
                "parent props={} layers={} inner.layer={} cmin={}\n{}",
                graph_props_names(&lg, g),
                lg[c].layers.len(),
                if lg[inner].layer.is_none() { "nil" } else { "set" },
                cmin,
                dump(&lg, g)
            );
            check(&golden, &format!("resizer {parent_dir:?} {child_dir:?} {variant}"), &out);
        }
    }
}

// MARK: - Compound graphs

/// `dump`, with every port's outgoing edges sorted by their text.
fn sorted_dump(lg: &LGraphArena, g: LGraphId) -> String {
    let text = dump(lg, g);
    let mut out: Vec<String> = Vec::new();
    let mut block: Vec<String> = Vec::new();
    fn flush(out: &mut Vec<String>, block: &mut Vec<String>) {
        block.sort();
        out.append(block);
    }
    for line in text.split('\n') {
        if line.starts_with("  E ") || line.starts_with("   EL ") {
            if line.starts_with("  E ") {
                block.push(line.to_string());
            } else {
                let last = block.len() - 1;
                block[last] += "\n";
                block[last] += line;
            }
        } else {
            flush(&mut out, &mut block);
            out.push(line.to_string());
        }
    }
    flush(&mut out, &mut block);
    out.join("\n")
}

fn compound_scenario(golden: &HashMap<String, String>, merge: bool, fixed_side: bool, inside_loops: bool, dir: Direction) {
    let mut lg = LGraphArena::new();
    let lg = &mut lg;
    let r = lg.new_graph();
    lg[r].props.set(&LayeredOptions::DIRECTION, dir);
    let x = node(lg, r, 300.0, 10.0, 20.0, 20.0);
    let y = node(lg, r, 300.0, 80.0, 20.0, 20.0);
    let p = node(lg, r, 50.0, 0.0, 100.0, 100.0);
    let q = node(lg, r, 50.0, 150.0, 60.0, 40.0);
    if fixed_side {
        lg[p].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_SIDE);
    }
    let c = lg.new_graph();
    lg[c].parent_node = Some(p);
    lg[p].nested_graph = Some(c);
    let dd = lg.new_graph();
    lg[dd].parent_node = Some(q);
    lg[q].nested_graph = Some(dd);
    lg[c].props.set(&LayeredOptions::DIRECTION, dir);
    lg[dd].props.set(&LayeredOptions::DIRECTION, dir);
    if merge {
        lg[c].props.set(&LayeredOptions::MERGE_HIERARCHY_EDGES, true);
    }
    lg[c].padding.top = 3.0;
    lg[c].padding.left = 4.0;
    lg[c].offset = KVector::new(2.0, 1.0);
    lg[dd].padding.top = 5.0;
    lg[dd].padding.left = 6.0;
    let c1 = node(lg, c, 10.0, 10.0, 20.0, 20.0);
    let c2 = node(lg, c, 10.0, 50.0, 20.0, 20.0);
    let d1 = node(lg, dd, 5.0, 5.0, 20.0, 20.0);
    let xw = port(lg, x, PortSide::WEST, 0.0, 5.0, 0.0, 0.0);
    let xe = port(lg, x, PortSide::EAST, 20.0, 5.0, 0.0, 0.0);
    let yw = port(lg, y, PortSide::WEST, 0.0, 5.0, 0.0, 0.0);
    let pe = port(lg, p, PortSide::EAST, 100.0, 50.0, 2.0, 2.0);
    let pw = port(lg, p, PortSide::WEST, 0.0, 50.0, 2.0, 2.0);
    let c1p1 = port(lg, c1, PortSide::EAST, 20.0, 5.0, 0.0, 0.0);
    let c1p2 = port(lg, c1, PortSide::SOUTH, 10.0, 20.0, 0.0, 0.0);
    let c1p3 = port(lg, c1, PortSide::WEST, 0.0, 5.0, 0.0, 0.0);
    let c1p4 = port(lg, c1, PortSide::EAST, 20.0, 15.0, 0.0, 0.0);
    let c2p1 = port(lg, c2, PortSide::WEST, 0.0, 5.0, 0.0, 0.0);
    let c2p2 = port(lg, c2, PortSide::NORTH, 10.0, 0.0, 0.0, 0.0);
    let d1p1 = port(lg, d1, PortSide::NORTH, 10.0, 0.0, 0.0, 0.0);
    let lab = |lg: &mut LGraphArena, e: LEdgeId, w: f64, placement: EdgeLabelPlacement| {
        let l = label(lg, w, 5.0, 0.0, 0.0);
        lg[l].props.set(&LayeredOptions::EDGE_LABELS_PLACEMENT, placement);
        lg[e].labels.push(l);
    };
    let e1 = edge(lg, c1p1, xw);
    lab(lg, e1, 10.0, EdgeLabelPlacement::CENTER);
    lab(lg, e1, 4.0, EdgeLabelPlacement::HEAD);
    lab(lg, e1, 3.0, EdgeLabelPlacement::TAIL);
    let e2 = edge(lg, xe, c2p1);
    lab(lg, e2, 11.0, EdgeLabelPlacement::CENTER);
    lg[e2].props.set(&LayeredOptions::EDGE_THICKNESS, 2.0);
    let e3 = edge(lg, c1p2, d1p1);
    lab(lg, e3, 12.0, EdgeLabelPlacement::CENTER);
    let e4 = edge(lg, c2p2, c1p3);
    let e5 = edge(lg, c1p4, pe);
    lab(lg, e5, 13.0, EdgeLabelPlacement::CENTER);
    let e6 = edge(lg, c1p1, yw);
    lab(lg, e6, 14.0, EdgeLabelPlacement::CENTER);
    lg[e6].props.set(&LayeredOptions::EDGE_THICKNESS, 3.0);
    let mut orig_edges = vec![e1, e2, e3, e4, e5, e6];
    if inside_loops {
        lg[p].props.set(&LayeredOptions::INSIDE_SELF_LOOPS_ACTIVATE, true);
        let e7 = edge(lg, pe, pw);
        lg[e7].props.set(&LayeredOptions::INSIDE_SELF_LOOPS_YO, true);
        lab(lg, e7, 15.0, EdgeLabelPlacement::CENTER);
        orig_edges.push(e7);
    }
    let graphs = [("R", r), ("C", c), ("D", dd)];
    let header = format!("compound merge={merge} fixedSide={fixed_side} insideLoops={inside_loops} {dir:?}");

    run(lg, r, &mut CompoundGraphPreprocessor::new());
    let mut out = sorted_dump(lg, r) + "\n";
    let map = lg[r].props.get_object::<CrossHierarchyMap>(&InternalProperties::CROSS_HIERARCHY_MAP).expect("map");
    for (i, &e) in orig_edges.iter().enumerate() {
        let segs: Vec<String> = map
            .get(e)
            .unwrap_or(&[])
            .iter()
            .map(|che| {
                let nodes = all_nodes(lg, che.graph);
                let name = graphs.iter().find(|(_, g)| *g == che.graph).map_or("?", |(n, _)| n);
                format!("{name}:{}->{}:{:?}", port_ref(lg, lg[che.new_edge].source, &nodes), port_ref(lg, lg[che.new_edge].target, &nodes), che.port_type)
            })
            .collect();
        let src = if lg[e].source.is_none() { "nil" } else { "set" };
        out += &format!("e{} src={src} labels={} segs=[{}]\n", i + 1, lg[e].labels.len(), segs.join(" "));
    }
    check(golden, &format!("{header} pre"), &out);

    // simulate a layout
    let mut k = 0.0f64;
    for (_, g) in graphs {
        for (i, n) in all_nodes(lg, g).into_iter().enumerate() {
            if lg[n].node_type == NodeType::EXTERNAL_PORT {
                lg[n].position = KVector::new(i as f64 * 7.0, i as f64 * 3.0);
            }
        }
        for n in all_nodes(lg, g) {
            for pt in lg[n].ports.clone() {
                for e in lg[pt].outgoing_edges.clone() {
                    k += 1.0;
                    lg[e].bend_points.add(KVector::new(k * 10.0 + 1.0, k * 10.0 + 2.0));
                    if (k as i64) % 3 == 0 {
                        lg[e].props.set(&LayeredOptions::JUNCTION_POINTS, PropValue::kvector_chain(KVectorChain::from_vec(vec![KVector::new(k, -k)])));
                    }
                    for l in lg[e].labels.clone() {
                        lg[l].position = KVector::new(k, -k);
                    }
                }
            }
        }
    }
    run(lg, r, &mut CompoundGraphPostprocessor::new());
    check(golden, &format!("{header} post"), &(sorted_dump(lg, r) + "\n"));
}

#[test]
fn compound_graph_processors_match_swift() {
    let golden = golden_sections();
    for merge in [false, true] {
        for fixed_side in [false, true] {
            for inside_loops in [false, true] {
                compound_scenario(&golden, merge, fixed_side, inside_loops, Direction::RIGHT);
            }
        }
    }
    compound_scenario(&golden, true, false, true, Direction::DOWN);
}
