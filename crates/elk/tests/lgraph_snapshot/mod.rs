//! Loads and dumps layered-graph snapshots written by the instrumented
//! elk-swift lab (`_LabSnap.swift`: `ELKLAB_SNAP=Proc1,Proc2
//! ELKLAB_SNAPDIR=dir lab graph.json` writes `NNNN-Proc-before.json` and
//! `-after.json` around every run of a listed processor).
//!
//! A snapshot is everything reachable from the processed `LGraph`: its layers,
//! nodes, ports, edges and labels (numbered in breadth-first order from the
//! graph), with all their properties. Doubles are Swift `description`
//! strings; shared class-typed property values (`KVector`, `KVectorChain`,
//! margins, other objects) carry an object number so sharing survives the
//! round trip, and an external port dummy's `PORT_ANCHOR` that *is* a port's
//! `position` names that port (`aliasPort`).
//!
//! `load` rebuilds an arena from a snapshot; `dump` writes an arena in the
//! same format, so a processor's result can be compared with the Swift
//! `-after` snapshot (`compare`, which ignores the spelling of enum cases).
//!
//! Making snapshots: copy the lab (`target/elklab`, built by
//! `tools/elklab.sh`) somewhere else, add `LabSnap.swift` (next to this file)
//! to its `Sources/ElkSwift/ELK/`, and in `ElkLayered.swift` wrap each
//! `processor.process(graph, sub)` call as `_labSnapBefore(graph,
//! processor.name); …; _labSnapAfter(graph, processor.name)` (three call
//! sites). The committed group C cases were made that way from the elk corpus
//! and from `gen_stress.py` graphs, the latter also with a lab-only importer
//! change (`ELKLAB_PARSE_MORE`) that parses `FIXED_*` port constraints and
//! `IMPROVE_STRAIGHTNESS` into their enums (the real importer keeps them as
//! strings, which makes the north/south hierarchical port paths and BK's
//! `SimpleThresholdStrategy` unreachable from JSON), and with
//! `HyperEdgeCycleDetector.nextRandomInt` returning 0 instead of
//! `Int.random` (see the port).

#![allow(dead_code)]

use std::collections::HashMap;
use std::rc::Rc;

use serde_json::Value;
use upleft_elk::bridge::elk_graph_impl::{ElkEdgeId, ElkLabelId, ElkNodeId, ElkPortId};
use upleft_elk::org::eclipse::elk::alg::layered::options::spacings::Spacings;
use upleft_elk::org::eclipse::elk::core::math::elk_margin::ElkMargin;
use upleft_elk::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use upleft_elk::org::eclipse::elk::core::math::k_vector::kvector_ref;
use upleft_elk::org::eclipse::elk::core::math::k_vector_chain::kvector_chain_ref;
use upleft_elk::prelude::*;

/// A property value the harness does not model, kept verbatim.
pub struct Opaque(pub Value);

/// Another Swift object (`"t":"obj"`) the harness does not model.
pub struct OpaqueObj {
    pub type_name: String,
    pub id: i64,
}

/// Swift type names of the ELK graph objects stored as `PropValue::Elk*`.
#[derive(Default)]
pub struct ElkTypes {
    pub names: HashMap<(u8, u32), String>,
}

pub struct Loaded {
    pub lg: LGraphArena,
    pub graph: LGraphId,
    pub elk_types: ElkTypes,
}

fn dbl(v: &Value) -> f64 {
    swift::parse_double(v.as_str().expect("double as string")).expect("double")
}

fn vec2(v: &Value) -> KVector {
    let a = v.as_array().unwrap();
    KVector::new(dbl(&a[0]), dbl(&a[1]))
}

fn spacing(v: &Value) -> (f64, f64, f64, f64) {
    let a = v.as_array().unwrap();
    (dbl(&a[0]), dbl(&a[1]), dbl(&a[2]), dbl(&a[3]))
}

fn ids(v: &Value) -> Vec<u32> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect()
}

fn opt_id(v: &Value) -> Option<u32> {
    v.as_u64().map(|x| x as u32)
}

fn norm(s: &str) -> String {
    s.chars().filter(|&c| c != '_').flat_map(|c| c.to_lowercase()).collect()
}

fn short_type(s: &str) -> &str {
    s.rsplit('_').next().unwrap_or(s)
}

macro_rules! enum_table {
    ($($tn:literal => $variant:ident : $ty:path),* $(,)?) => {
        fn decode_enum(type_name: &str, case: &str) -> Option<PropValue> {
            let t = short_type(type_name);
            let n = norm(case);
            $(
                if t == $tn {
                    use $ty as E;
                    return E::ALL.iter().copied().find(|e| norm(e.name()) == n).map(PropValue::$variant);
                }
            )*
            None
        }

        fn encode_enum(v: &PropValue) -> Option<(&'static str, String)> {
            match v {
                $(PropValue::$variant(e) => Some(($tn, e.name().to_string())),)*
                _ => None,
            }
        }
    };
}

enum_table! {
    "Direction" => Direction: upleft_elk::org::eclipse::elk::core::options::direction::Direction,
    "PortSide" => PortSide: upleft_elk::org::eclipse::elk::core::options::port_side::PortSide,
    "EdgeRouting" => EdgeRouting: upleft_elk::org::eclipse::elk::core::options::edge_routing::EdgeRouting,
    "HierarchyHandling" => HierarchyHandling: upleft_elk::org::eclipse::elk::core::options::hierarchy_handling::HierarchyHandling,
    "PortConstraints" => PortConstraints: upleft_elk::org::eclipse::elk::core::options::port_constraints::PortConstraints,
    "EdgeLabelPlacement" => EdgeLabelPlacement: upleft_elk::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement,
    "Alignment" => Alignment: upleft_elk::org::eclipse::elk::core::options::alignment::Alignment,
    "PortAlignment" => PortAlignment: upleft_elk::org::eclipse::elk::core::options::port_alignment::PortAlignment,
    "EdgeType" => EdgeType: upleft_elk::org::eclipse::elk::core::options::edge_type::EdgeType,
    "LabelSide" => LabelSide: upleft_elk::org::eclipse::elk::core::options::label_side::LabelSide,
    "EdgeCoords" => EdgeCoords: upleft_elk::org::eclipse::elk::core::options::edge_coords::EdgeCoords,
    "ShapeCoords" => ShapeCoords: upleft_elk::org::eclipse::elk::core::options::shape_coords::ShapeCoords,
    "CycleBreakingStrategy" => CycleBreakingStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::cycle_breaking_strategy::CycleBreakingStrategy,
    "LayeringStrategy" => LayeringStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::layering_strategy::LayeringStrategy,
    "CrossingMinimizationStrategy" => CrossingMinimizationStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::crossing_minimization_strategy::CrossingMinimizationStrategy,
    "NodePlacementStrategy" => NodePlacementStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::node_placement_strategy::NodePlacementStrategy,
    "EdgeStraighteningStrategy" => EdgeStraighteningStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::edge_straightening_strategy::EdgeStraighteningStrategy,
    "FixedAlignment" => FixedAlignment: upleft_elk::org::eclipse::elk::alg::layered::options::fixed_alignment::FixedAlignment,
    "GreedySwitchType" => GreedySwitchType: upleft_elk::org::eclipse::elk::alg::layered::options::greedy_switch_type::GreedySwitchType,
    "GraphCompactionStrategy" => GraphCompactionStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::graph_compaction_strategy::GraphCompactionStrategy,
    "WrappingStrategy" => WrappingStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::wrapping_strategy::WrappingStrategy,
    "OrderingStrategy" => OrderingStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::ordering_strategy::OrderingStrategy,
    "LayerConstraint" => LayerConstraint: upleft_elk::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint,
    "InLayerConstraint" => InLayerConstraint: upleft_elk::org::eclipse::elk::alg::layered::options::in_layer_constraint::InLayerConstraint,
    "EdgeConstraint" => EdgeConstraint: upleft_elk::org::eclipse::elk::alg::layered::options::edge_constraint::EdgeConstraint,
    "PortType" => PortType: upleft_elk::org::eclipse::elk::alg::layered::options::port_type::PortType,
    "EdgeLabelSideSelection" => EdgeLabelSideSelection: upleft_elk::org::eclipse::elk::alg::layered::options::edge_label_side_selection::EdgeLabelSideSelection,
    "CenterEdgeLabelPlacementStrategy" => CenterEdgeLabelPlacementStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::center_edge_label_placement_strategy::CenterEdgeLabelPlacementStrategy,
    "SelfLoopDistributionStrategy" => SelfLoopDistributionStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::self_loop_distribution_strategy::SelfLoopDistributionStrategy,
    "SelfLoopOrderingStrategy" => SelfLoopOrderingStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::self_loop_ordering_strategy::SelfLoopOrderingStrategy,
    "SelfLoopPlacementStrategy" => SelfLoopPlacementStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::self_loop_placement_strategy::SelfLoopPlacementStrategy,
    "NodePromotionStrategy" => NodePromotionStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::node_promotion_strategy::NodePromotionStrategy,
    "PortSortingStrategy" => PortSortingStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::port_sorting_strategy::PortSortingStrategy,
    "LongEdgeOrderingStrategy" => LongEdgeOrderingStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::long_edge_ordering_strategy::LongEdgeOrderingStrategy,
    "DirectionCongruency" => DirectionCongruency: upleft_elk::org::eclipse::elk::alg::layered::options::direction_congruency::DirectionCongruency,
    "InteractiveReferencePoint" => InteractiveReferencePoint: upleft_elk::org::eclipse::elk::alg::layered::options::interactive_reference_point::InteractiveReferencePoint,
    "GroupOrderStrategy" => GroupOrderStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::group_order_strategy::GroupOrderStrategy,
    "ValidifyStrategy" => ValidifyStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::validify_strategy::ValidifyStrategy,
    "CuttingStrategy" => CuttingStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::cutting_strategy::CuttingStrategy,
    "ConstraintCalculationStrategy" => ConstraintCalculationStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::constraint_calculation_strategy::ConstraintCalculationStrategy,
    "LayerUnzippingStrategy" => LayerUnzippingStrategy: upleft_elk::org::eclipse::elk::alg::layered::options::layer_unzipping_strategy::LayerUnzippingStrategy,
    "SplineRoutingMode" => SplineRoutingMode: upleft_elk::org::eclipse::elk::alg::layered::options::spline_routing_mode::SplineRoutingMode,
}

fn decode_raw(type_name: &str, raw: i64) -> Option<PropValue> {
    use upleft_elk::org::eclipse::elk::core::options::{
        content_alignment::ContentAlignment, node_label_placement::NodeLabelPlacement, port_label_placement::PortLabelPlacement,
        size_constraint::SizeConstraint, size_options::SizeOptions,
    };
    Some(match short_type(type_name) {
        "SizeConstraint" => PropValue::SizeConstraint(SizeConstraint::from_raw(raw)),
        "PortLabelPlacement" => PropValue::PortLabelPlacement(PortLabelPlacement::from_raw(raw)),
        "SizeOptions" => PropValue::SizeOptions(SizeOptions::from_raw(raw)),
        "NodeLabelPlacement" => PropValue::NodeLabelPlacement(NodeLabelPlacement::from_raw(raw)),
        "ContentAlignment" => PropValue::ContentAlignment(ContentAlignment::from_raw(raw)),
        _ => return None,
    })
}

fn encode_raw(v: &PropValue) -> Option<(&'static str, i64)> {
    Some(match v {
        PropValue::SizeConstraint(x) => ("SizeConstraint", x.raw()),
        PropValue::PortLabelPlacement(x) => ("PortLabelPlacement", x.raw()),
        PropValue::SizeOptions(x) => ("SizeOptions", x.raw()),
        PropValue::NodeLabelPlacement(x) => ("NodeLabelPlacement", x.raw()),
        PropValue::ContentAlignment(x) => ("ContentAlignment", x.raw()),
        _ => return None,
    })
}

fn node_type(raw: &str) -> NodeType {
    NodeType::ALL.iter().copied().find(|t| t.raw_value() == raw).unwrap_or_else(|| panic!("node type {raw}"))
}

fn port_side(s: &str) -> PortSide {
    PortSide::ALL.iter().copied().find(|p| norm(p.name()) == norm(s)).unwrap_or_else(|| panic!("port side {s}"))
}

struct Decoder<'a> {
    graphs: &'a [LGraphId],
    layers: &'a [LayerId],
    nodes: &'a [LNodeId],
    ports: &'a [LPortId],
    edges: &'a [LEdgeId],
    labels: &'a [LLabelId],
    kvectors: HashMap<i64, Rc<std::cell::RefCell<KVector>>>,
    chains: HashMap<i64, Rc<std::cell::RefCell<KVectorChain>>>,
    margins: HashMap<i64, Rc<std::cell::RefCell<ElkMargin>>>,
    paddings: HashMap<i64, Rc<std::cell::RefCell<ElkPadding>>>,
    objects: HashMap<i64, Rc<OpaqueObj>>,
    elk_types: ElkTypes,
}

impl<'a> Decoder<'a> {
    /// Returns the value and, for a `PORT_ANCHOR` that is a port's position, that port.
    fn value(&mut self, v: &Value) -> (Option<PropValue>, Option<LPortId>) {
        let t = v["t"].as_str().unwrap();
        let o = v.get("o").and_then(Value::as_i64);
        let pv = match t {
            "nil" => return (None, None),
            "b" => PropValue::Bool(v["v"].as_bool().unwrap()),
            "i" => PropValue::Int(v["v"].as_i64().unwrap()),
            "d" => PropValue::Double(dbl(&v["v"])),
            "s" => PropValue::from(v["v"].as_str().unwrap()),
            "kv" => {
                let k = vec2(&v["v"]);
                let r = self.kvectors.entry(o.unwrap()).or_insert_with(|| kvector_ref(k)).clone();
                let alias = v.get("aliasPort").and_then(Value::as_u64).map(|p| self.ports[p as usize]);
                return (Some(PropValue::KVector(r)), alias);
            }
            "kvc" => {
                let pts: Vec<KVector> = v["v"].as_array().unwrap().iter().map(vec2).collect();
                let r = self.chains.entry(o.unwrap()).or_insert_with(|| kvector_chain_ref(KVectorChain::from_vec(pts))).clone();
                PropValue::KVectorChain(r)
            }
            "margin" => {
                let (t, r, b, l) = spacing(&v["v"]);
                let m = self.margins.entry(o.unwrap()).or_insert_with(|| Rc::new(std::cell::RefCell::new(ElkMargin::new(t, r, b, l)))).clone();
                PropValue::ElkMargin(m)
            }
            "padding" => {
                let (t, r, b, l) = spacing(&v["v"]);
                let m = self.paddings.entry(o.unwrap()).or_insert_with(|| Rc::new(std::cell::RefCell::new(ElkPadding::new(t, r, b, l)))).clone();
                PropValue::ElkPadding(m)
            }
            "node" => PropValue::LNode(self.nodes[v["v"].as_u64().unwrap() as usize]),
            "port" => PropValue::LPort(self.ports[v["v"].as_u64().unwrap() as usize]),
            "edge" => PropValue::LEdge(self.edges[v["v"].as_u64().unwrap() as usize]),
            "label" => PropValue::LLabel(self.labels[v["v"].as_u64().unwrap() as usize]),
            "layer" => PropValue::Layer(self.layers[v["v"].as_u64().unwrap() as usize]),
            "graph" => PropValue::LGraph(self.graphs[v["v"].as_u64().unwrap() as usize]),
            "nodes" => PropValue::from(ids(&v["v"]).into_iter().map(|i| self.nodes[i as usize]).collect::<Vec<_>>()),
            "ports" => PropValue::from(ids(&v["v"]).into_iter().map(|i| self.ports[i as usize]).collect::<Vec<_>>()),
            "edges" => PropValue::from(ids(&v["v"]).into_iter().map(|i| self.edges[i as usize]).collect::<Vec<_>>()),
            "labels" => PropValue::from(ids(&v["v"]).into_iter().map(|i| self.labels[i as usize]).collect::<Vec<_>>()),
            "gprops" => {
                let mut set = EnumSet::<GraphProperties>::new();
                for n in v["v"].as_array().unwrap() {
                    let n = norm(n.as_str().unwrap());
                    let gp = GraphProperties::ALL.iter().copied().find(|g| norm(g.name()) == n).expect("graph property");
                    set.insert(gp);
                }
                PropValue::GraphPropertiesSet(set)
            }
            "enum" => match decode_enum(v["type"].as_str().unwrap(), v["v"].as_str().unwrap()) {
                Some(pv) => pv,
                None => PropValue::object(Rc::new(Opaque(v.clone()))),
            },
            "raw" => match decode_raw(v["type"].as_str().unwrap(), v["v"].as_i64().unwrap()) {
                Some(pv) => pv,
                None => PropValue::object(Rc::new(Opaque(v.clone()))),
            },
            "obj" => {
                let type_name = v["type"].as_str().unwrap().to_string();
                let id = o.unwrap();
                let key = |tag: u8| (tag, id as u32);
                if type_name.starts_with("ElkPort") {
                    self.elk_types.names.insert(key(0), type_name);
                    PropValue::ElkPort(ElkPortId(id as u32))
                } else if type_name.starts_with("ElkNode") {
                    self.elk_types.names.insert(key(1), type_name);
                    PropValue::ElkNode(ElkNodeId(id as u32))
                } else if type_name.starts_with("ElkEdge") && !type_name.contains("Section") {
                    self.elk_types.names.insert(key(2), type_name);
                    PropValue::ElkEdge(ElkEdgeId(id as u32))
                } else if type_name.starts_with("ElkLabel") {
                    self.elk_types.names.insert(key(3), type_name);
                    PropValue::ElkLabel(ElkLabelId(id as u32))
                } else {
                    let r = self.objects.entry(id).or_insert_with(|| Rc::new(OpaqueObj { type_name, id })).clone();
                    PropValue::Object(r)
                }
            }
            _ => PropValue::object(Rc::new(Opaque(v.clone()))),
        };
        (Some(pv), None)
    }

    fn props(&mut self, v: &Value) -> (PropertyMap, Option<LPortId>) {
        let mut map = PropertyMap::new();
        let mut alias = None;
        for (k, pv) in v.as_object().unwrap() {
            let (value, a) = self.value(pv);
            if k == "org.eclipse.elk.port.anchor" {
                alias = a;
            }
            map.set_by_id(k, value);
        }
        (map, alias)
    }
}

/// Rebuilds an arena from a snapshot. Graph 0 is the processed graph.
pub fn load(text: &str) -> Loaded {
    let snap: Value = serde_json::from_str(text).expect("snapshot json");
    let mut lg = LGraphArena::new();
    let graphs: Vec<LGraphId> = snap["graphs"].as_array().unwrap().iter().map(|_| lg.new_graph()).collect();
    let layers: Vec<LayerId> = snap["layers"].as_array().unwrap().iter().map(|l| lg.new_layer(graphs[l["owner"].as_u64().unwrap() as usize])).collect();
    let nodes: Vec<LNodeId> = snap["nodes"].as_array().unwrap().iter().map(|_| lg.new_node(None)).collect();
    let ports: Vec<LPortId> = snap["ports"].as_array().unwrap().iter().map(|_| lg.new_port()).collect();
    let edges: Vec<LEdgeId> = snap["edges"].as_array().unwrap().iter().map(|_| lg.new_edge()).collect();
    let labels: Vec<LLabelId> = snap["labels"].as_array().unwrap().iter().map(|l| lg.new_label(l["text"].as_str().unwrap())).collect();

    let mut dec = Decoder {
        graphs: &graphs,
        layers: &layers,
        nodes: &nodes,
        ports: &ports,
        edges: &edges,
        labels: &labels,
        kvectors: HashMap::new(),
        chains: HashMap::new(),
        margins: HashMap::new(),
        paddings: HashMap::new(),
        objects: HashMap::new(),
        elk_types: ElkTypes::default(),
    };

    let mut spacings_needed = false;
    for (i, g) in snap["graphs"].as_array().unwrap().iter().enumerate() {
        let id = graphs[i];
        lg[id].id = g["id"].as_i64().unwrap() as i32;
        lg[id].size = vec2(&g["size"]);
        let (t, r, b, l) = spacing(&g["padding"]);
        lg[id].padding.top = t;
        lg[id].padding.right = r;
        lg[id].padding.bottom = b;
        lg[id].padding.left = l;
        lg[id].offset = vec2(&g["offset"]);
        lg[id].parent_node = opt_id(&g["parentNode"]).map(|n| nodes[n as usize]);
        if i == 0 {
            lg[id].layers = ids(&g["layers"]).into_iter().map(|l| layers[l as usize]).collect();
            lg[id].layerless_nodes = ids(&g["layerless"]).into_iter().map(|n| nodes[n as usize]).collect();
            let (props, _) = dec.props(&g["props"]);
            spacings_needed = props.get_by_id("spacings").is_some();
            lg[id].props = props;
        }
    }
    for (i, l) in snap["layers"].as_array().unwrap().iter().enumerate() {
        let id = layers[i];
        lg[id].id = l["id"].as_i64().unwrap() as i32;
        lg[id].size = vec2(&l["size"]);
        lg[id].nodes = ids(&l["nodes"]).into_iter().map(|n| nodes[n as usize]).collect();
        lg[id].props = dec.props(&l["props"]).0;
    }
    for (i, n) in snap["nodes"].as_array().unwrap().iter().enumerate() {
        let id = nodes[i];
        lg[id].id = n["id"].as_i64().unwrap() as i32;
        lg[id].node_type = node_type(n["type"].as_str().unwrap());
        lg[id].position = vec2(&n["pos"]);
        lg[id].size = vec2(&n["size"]);
        let (t, r, b, l) = spacing(&n["margin"]);
        lg[id].margin = ElkMargin::new(t, r, b, l);
        let (t, r, b, l) = spacing(&n["padding"]);
        lg[id].padding = ElkPadding::new(t, r, b, l);
        lg[id].graph = opt_id(&n["graph"]).map(|g| graphs[g as usize]);
        lg[id].layer = opt_id(&n["layer"]).map(|l| layers[l as usize]);
        lg[id].nested_graph = opt_id(&n["nested"]).map(|g| graphs[g as usize]);
        lg[id].ports = ids(&n["ports"]).into_iter().map(|p| ports[p as usize]).collect();
        lg[id].labels = ids(&n["labels"]).into_iter().map(|l| labels[l as usize]).collect();
        let (props, alias) = dec.props(&n["props"]);
        lg[id].props = props;
        lg[id].port_anchor_alias = alias;
    }
    for (i, p) in snap["ports"].as_array().unwrap().iter().enumerate() {
        let id = ports[i];
        lg[id].id = p["id"].as_i64().unwrap() as i32;
        lg[id].side = port_side(p["side"].as_str().unwrap());
        lg[id].position = vec2(&p["pos"]);
        lg[id].size = vec2(&p["size"]);
        lg[id].anchor = vec2(&p["anchor"]);
        lg[id].explicitly_supplied_port_anchor = p["explicitAnchor"].as_bool().unwrap();
        lg[id].connected_to_external_nodes = p["ext"].as_bool().unwrap();
        let (t, r, b, l) = spacing(&p["margin"]);
        lg[id].margin = ElkMargin::new(t, r, b, l);
        lg[id].owner = opt_id(&p["owner"]).map(|n| nodes[n as usize]);
        lg[id].labels = ids(&p["labels"]).into_iter().map(|l| labels[l as usize]).collect();
        lg[id].incoming_edges = ids(&p["in"]).into_iter().map(|e| edges[e as usize]).collect();
        lg[id].outgoing_edges = ids(&p["out"]).into_iter().map(|e| edges[e as usize]).collect();
        lg[id].props = dec.props(&p["props"]).0;
    }
    for (i, e) in snap["edges"].as_array().unwrap().iter().enumerate() {
        let id = edges[i];
        lg[id].id = e["id"].as_i64().unwrap() as i32;
        lg[id].source = opt_id(&e["source"]).map(|p| ports[p as usize]);
        lg[id].target = opt_id(&e["target"]).map(|p| ports[p as usize]);
        lg[id].bend_points = KVectorChain::from_vec(e["bends"].as_array().unwrap().iter().map(vec2).collect());
        lg[id].labels = ids(&e["labels"]).into_iter().map(|l| labels[l as usize]).collect();
        lg[id].props = dec.props(&e["props"]).0;
    }
    for (i, l) in snap["labels"].as_array().unwrap().iter().enumerate() {
        let id = labels[i];
        lg[id].id = l["id"].as_i64().unwrap() as i32;
        lg[id].position = vec2(&l["pos"]);
        lg[id].size = vec2(&l["size"]);
        lg[id].props = dec.props(&l["props"]).0;
    }
    let elk_types = std::mem::take(&mut dec.elk_types);

    // The Swift `Spacings` object reads the graph's properties lazily; rebuild it.
    if spacings_needed {
        let g = graphs[0];
        let s = Spacings::new(&lg, g);
        lg[g].props.set(&InternalProperties::SPACINGS, PropValue::object(Rc::new(s)));
    }

    Loaded { lg, graph: graphs[0], elk_types }
}

// ---------------------------------------------------------------------------
// Dumping

#[derive(Default)]
struct Ids {
    graphs: Vec<LGraphId>,
    graph_ix: HashMap<LGraphId, usize>,
    layers: Vec<LayerId>,
    layer_ix: HashMap<LayerId, usize>,
    nodes: Vec<LNodeId>,
    node_ix: HashMap<LNodeId, usize>,
    ports: Vec<LPortId>,
    port_ix: HashMap<LPortId, usize>,
    edges: Vec<LEdgeId>,
    edge_ix: HashMap<LEdgeId, usize>,
    labels: Vec<LLabelId>,
    label_ix: HashMap<LLabelId, usize>,
    queue: Vec<(u8, usize)>,
}

macro_rules! visit_fn {
    ($name:ident, $ty:ty, $list:ident, $ix:ident, $kind:expr) => {
        fn $name(&mut self, x: $ty) -> usize {
            if let Some(&i) = self.$ix.get(&x) {
                return i;
            }
            let i = self.$list.len();
            self.$list.push(x);
            self.$ix.insert(x, i);
            if $kind != 255u8 {
                self.queue.push(($kind, i));
            }
            i
        }
    };
}

impl Ids {
    visit_fn!(graph, LGraphId, graphs, graph_ix, 255u8);
    visit_fn!(layer, LayerId, layers, layer_ix, 4u8);
    visit_fn!(node, LNodeId, nodes, node_ix, 0u8);
    visit_fn!(port, LPortId, ports, port_ix, 1u8);
    visit_fn!(edge, LEdgeId, edges, edge_ix, 2u8);
    visit_fn!(label, LLabelId, labels, label_ix, 3u8);

    fn visit_value(&mut self, v: &PropValue) {
        match v {
            PropValue::LNode(x) => {
                self.node(*x);
            }
            PropValue::LPort(x) => {
                self.port(*x);
            }
            PropValue::LEdge(x) => {
                self.edge(*x);
            }
            PropValue::LLabel(x) => {
                self.label(*x);
            }
            PropValue::Layer(x) => {
                self.layer(*x);
            }
            PropValue::LGraph(x) => {
                self.graph(*x);
            }
            PropValue::LNodes(xs) => {
                for &x in xs.iter() {
                    self.node(x);
                }
            }
            PropValue::LPorts(xs) => {
                for &x in xs.iter() {
                    self.port(x);
                }
            }
            PropValue::LEdges(xs) => {
                for &x in xs.iter() {
                    self.edge(x);
                }
            }
            PropValue::LLabels(xs) => {
                for &x in xs.iter() {
                    self.label(x);
                }
            }
            _ => {}
        }
    }

    fn visit_props(&mut self, props: &PropertyMap) {
        let mut entries: Vec<(&str, &PropValue)> = props.all().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        for (_, v) in entries {
            self.visit_value(v);
        }
    }
}

fn fmt_d(v: f64) -> String {
    format!("\"{}\"", swift::describe_double(v))
}

fn fmt_vec(v: KVector) -> String {
    format!("[{},{}]", fmt_d(v.x), fmt_d(v.y))
}

fn fmt_spacing(top: f64, right: f64, bottom: f64, left: f64) -> String {
    format!("[{},{},{},{}]", fmt_d(top), fmt_d(right), fmt_d(bottom), fmt_d(left))
}

fn esc(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// Object numbering by first emission, keyed by identity.
#[derive(Default)]
struct Objs {
    ix: HashMap<(u8, usize), usize>,
}

impl Objs {
    fn get(&mut self, key: (u8, usize)) -> usize {
        let n = self.ix.len();
        *self.ix.entry(key).or_insert(n)
    }
}

struct Dumper<'a> {
    lg: &'a LGraphArena,
    ids: Ids,
    objs: Objs,
    elk_types: &'a ElkTypes,
}

impl<'a> Dumper<'a> {
    fn list<T: Copy>(&self, xs: &[T], f: impl Fn(T) -> usize) -> String {
        xs.iter().map(|&x| f(x).to_string()).collect::<Vec<_>>().join(",")
    }

    fn value(&mut self, key: &str, v: &PropValue, owner: Option<LNodeId>) -> String {
        let lg = self.lg;
        match v {
            PropValue::Bool(x) => format!("{{\"t\":\"b\",\"v\":{x}}}"),
            PropValue::Int(x) => format!("{{\"t\":\"i\",\"v\":{x}}}"),
            PropValue::Double(x) => format!("{{\"t\":\"d\",\"v\":{}}}", fmt_d(*x)),
            PropValue::Str(s) => format!("{{\"t\":\"s\",\"v\":{}}}", esc(s)),
            PropValue::KVector(r) => {
                let alias = if key == "org.eclipse.elk.port.anchor" { owner.and_then(|n| lg[n].port_anchor_alias) } else { None };
                if let Some(p) = alias {
                    let o = self.objs.get((10, p.index()));
                    let pi = self.ids.port_ix[&p];
                    format!("{{\"t\":\"kv\",\"o\":{o},\"v\":{},\"aliasPort\":{pi}}}", fmt_vec(lg[p].position))
                } else {
                    let o = self.objs.get((1, Rc::as_ptr(r) as usize));
                    format!("{{\"t\":\"kv\",\"o\":{o},\"v\":{}}}", fmt_vec(*r.borrow()))
                }
            }
            PropValue::KVectorChain(r) => {
                let o = self.objs.get((2, Rc::as_ptr(r) as usize));
                let pts = r.borrow().iter().map(|&p| fmt_vec(p)).collect::<Vec<_>>().join(",");
                format!("{{\"t\":\"kvc\",\"o\":{o},\"v\":[{pts}]}}")
            }
            PropValue::ElkMargin(r) => {
                let o = self.objs.get((3, Rc::as_ptr(r) as usize));
                let m = r.borrow();
                format!("{{\"t\":\"margin\",\"o\":{o},\"v\":{}}}", fmt_spacing(m.top, m.right, m.bottom, m.left))
            }
            PropValue::ElkPadding(r) => {
                let o = self.objs.get((4, Rc::as_ptr(r) as usize));
                let m = r.borrow();
                format!("{{\"t\":\"padding\",\"o\":{o},\"v\":{}}}", fmt_spacing(m.top, m.right, m.bottom, m.left))
            }
            PropValue::LNode(x) => format!("{{\"t\":\"node\",\"v\":{}}}", self.ids.node_ix[x]),
            PropValue::LPort(x) => format!("{{\"t\":\"port\",\"v\":{}}}", self.ids.port_ix[x]),
            PropValue::LEdge(x) => format!("{{\"t\":\"edge\",\"v\":{}}}", self.ids.edge_ix[x]),
            PropValue::LLabel(x) => format!("{{\"t\":\"label\",\"v\":{}}}", self.ids.label_ix[x]),
            PropValue::Layer(x) => format!("{{\"t\":\"layer\",\"v\":{}}}", self.ids.layer_ix[x]),
            PropValue::LGraph(x) => format!("{{\"t\":\"graph\",\"v\":{}}}", self.ids.graph_ix[x]),
            PropValue::LNodes(xs) => format!("{{\"t\":\"nodes\",\"v\":[{}]}}", self.list(xs, |x| self.ids.node_ix[&x])),
            PropValue::LPorts(xs) => format!("{{\"t\":\"ports\",\"v\":[{}]}}", self.list(xs, |x| self.ids.port_ix[&x])),
            PropValue::LEdges(xs) => format!("{{\"t\":\"edges\",\"v\":[{}]}}", self.list(xs, |x| self.ids.edge_ix[&x])),
            PropValue::LLabels(xs) => format!("{{\"t\":\"labels\",\"v\":[{}]}}", self.list(xs, |x| self.ids.label_ix[&x])),
            PropValue::GraphPropertiesSet(set) => {
                let mut names: Vec<String> = set.iter().map(|g| esc(g.name())).collect();
                names.sort();
                format!("{{\"t\":\"gprops\",\"v\":[{}]}}", names.join(","))
            }
            PropValue::ElkPort(p) => self.elk_obj(0, p.0),
            PropValue::ElkNode(n) => self.elk_obj(1, n.0),
            PropValue::ElkEdge(e) => self.elk_obj(2, e.0),
            PropValue::ElkLabel(l) => self.elk_obj(3, l.0),
            PropValue::Object(o) => {
                if let Ok(op) = o.clone().downcast::<Opaque>() {
                    return op.0.to_string();
                }
                if let Ok(obj) = o.clone().downcast::<OpaqueObj>() {
                    let n = self.objs.get((20, obj.id as usize));
                    return format!("{{\"t\":\"obj\",\"type\":{},\"o\":{n}}}", esc(&obj.type_name));
                }
                if o.clone().downcast::<Spacings>().is_ok() {
                    let n = self.objs.get((21, Rc::as_ptr(o) as *const u8 as usize));
                    return format!("{{\"t\":\"obj\",\"type\":\"org_eclipse_elk_alg_layered_options_Spacings\",\"o\":{n}}}");
                }
                "{\"t\":\"opaque\",\"type\":\"RustObject\"}".to_string()
            }
            other => {
                if let Some((t, name)) = encode_enum(other) {
                    return format!("{{\"t\":\"enum\",\"type\":{},\"v\":{}}}", esc(t), esc(&name));
                }
                if let Some((t, raw)) = encode_raw(other) {
                    return format!("{{\"t\":\"raw\",\"type\":{},\"v\":{raw}}}", esc(t));
                }
                format!("{{\"t\":\"opaque\",\"type\":{}}}", esc(&format!("{other:?}")))
            }
        }
    }

    fn elk_obj(&mut self, tag: u8, id: u32) -> String {
        let type_name = self.elk_types.names.get(&(tag, id)).cloned().unwrap_or_else(|| "ElkObject".into());
        let n = self.objs.get((30 + tag, id as usize));
        format!("{{\"t\":\"obj\",\"type\":{},\"o\":{n}}}", esc(&type_name))
    }

    fn props(&mut self, props: &PropertyMap, owner: Option<LNodeId>) -> String {
        let mut entries: Vec<(&str, &PropValue)> = props.all().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let parts: Vec<String> = entries.into_iter().map(|(k, v)| format!("{}:{}", esc(k), self.value(k, v, owner))).collect();
        format!("{{{}}}", parts.join(","))
    }
}

fn opt<K: Eq + std::hash::Hash + Copy>(ix: &HashMap<K, usize>, x: Option<K>) -> String {
    x.and_then(|x| ix.get(&x).copied()).map_or("null".to_string(), |i| i.to_string())
}

/// Dumps the arena in the lab's snapshot format.
pub fn dump(lg: &LGraphArena, g: LGraphId, elk_types: &ElkTypes) -> String {
    let mut ids = Ids::default();
    ids.graph(g);
    for &l in &lg[g].layers {
        ids.layer(l);
    }
    for &n in &lg[g].layerless_nodes {
        ids.node(n);
    }
    ids.visit_props(&lg[g].props);
    let mut qi = 0;
    while qi < ids.queue.len() {
        let (k, i) = ids.queue[qi];
        qi += 1;
        match k {
            0 => {
                let n = ids.nodes[i];
                if let Some(gg) = lg[n].graph {
                    ids.graph(gg);
                }
                if let Some(l) = lg[n].layer {
                    ids.layer(l);
                }
                for &p in &lg[n].ports {
                    ids.port(p);
                }
                for &l in &lg[n].labels {
                    ids.label(l);
                }
                if let Some(ng) = lg[n].nested_graph {
                    ids.graph(ng);
                }
                ids.visit_props(&lg[n].props);
            }
            1 => {
                let p = ids.ports[i];
                if let Some(o) = lg[p].owner {
                    ids.node(o);
                }
                for &l in &lg[p].labels {
                    ids.label(l);
                }
                for &e in &lg[p].incoming_edges {
                    ids.edge(e);
                }
                for &e in &lg[p].outgoing_edges {
                    ids.edge(e);
                }
                ids.visit_props(&lg[p].props);
            }
            2 => {
                let e = ids.edges[i];
                if let Some(s) = lg[e].source {
                    ids.port(s);
                }
                if let Some(t) = lg[e].target {
                    ids.port(t);
                }
                for &l in &lg[e].labels {
                    ids.label(l);
                }
                ids.visit_props(&lg[e].props);
            }
            3 => {
                let l = ids.labels[i];
                ids.visit_props(&lg[l].props);
            }
            _ => {
                let l = ids.layers[i];
                ids.graph(lg[l].owner);
                for &n in &lg[l].nodes {
                    ids.node(n);
                }
                ids.visit_props(&lg[l].props);
            }
        }
    }

    let mut d = Dumper { lg, ids, objs: Objs::default(), elk_types };

    let mut out = String::from("{\n\"graphs\":[\n");
    let mut parts = Vec::new();
    for (i, &x) in d.ids.graphs.clone().iter().enumerate() {
        let gr = &lg[x];
        let mut s = format!(
            "{{\"id\":{},\"size\":{},\"padding\":{},\"offset\":{},\"parentNode\":{}",
            gr.id,
            fmt_vec(gr.size),
            fmt_spacing(gr.padding.top, gr.padding.right, gr.padding.bottom, gr.padding.left),
            fmt_vec(gr.offset),
            opt(&d.ids.node_ix, gr.parent_node)
        );
        if i == 0 {
            s += &format!(",\"layers\":[{}]", d.list(&gr.layers, |l| d.ids.layer_ix[&l]));
            s += &format!(",\"layerless\":[{}]", d.list(&gr.layerless_nodes, |n| d.ids.node_ix[&n]));
            let p = d.props(&gr.props, None);
            s += &format!(",\"props\":{p}");
        }
        parts.push(s + "}");
    }
    out += &parts.join(",\n");
    out += "\n],\n\"layers\":[\n";
    let mut parts = Vec::new();
    for &x in d.ids.layers.clone().iter() {
        let l = &lg[x];
        let nodes = d.list(&l.nodes, |n| d.ids.node_ix[&n]);
        let p = d.props(&l.props, None);
        parts.push(format!("{{\"id\":{},\"owner\":{},\"size\":{},\"nodes\":[{nodes}],\"props\":{p}}}", l.id, d.ids.graph_ix[&l.owner], fmt_vec(l.size)));
    }
    out += &parts.join(",\n");
    out += "\n],\n\"nodes\":[\n";
    let mut parts = Vec::new();
    for &x in d.ids.nodes.clone().iter() {
        let n = &lg[x];
        let mut s = format!("{{\"id\":{},\"type\":{},\"pos\":{},\"size\":{}", n.id, esc(n.node_type.raw_value()), fmt_vec(n.position), fmt_vec(n.size));
        s += &format!(
            ",\"margin\":{},\"padding\":{}",
            fmt_spacing(n.margin.top, n.margin.right, n.margin.bottom, n.margin.left),
            fmt_spacing(n.padding.top, n.padding.right, n.padding.bottom, n.padding.left)
        );
        s += &format!(",\"graph\":{},\"layer\":{},\"nested\":{}", opt(&d.ids.graph_ix, n.graph), opt(&d.ids.layer_ix, n.layer), opt(&d.ids.graph_ix, n.nested_graph));
        s += &format!(",\"ports\":[{}]", d.list(&n.ports, |p| d.ids.port_ix[&p]));
        s += &format!(",\"labels\":[{}]", d.list(&n.labels, |l| d.ids.label_ix[&l]));
        let p = d.props(&n.props, Some(x));
        s += &format!(",\"props\":{p}}}");
        parts.push(s);
    }
    out += &parts.join(",\n");
    out += "\n],\n\"ports\":[\n";
    let mut parts = Vec::new();
    for &x in d.ids.ports.clone().iter() {
        let p = &lg[x];
        let mut s = format!("{{\"id\":{},\"side\":{},\"pos\":{},\"size\":{},\"anchor\":{}", p.id, esc(p.side.name()), fmt_vec(p.position), fmt_vec(p.size), fmt_vec(p.anchor));
        s += &format!(
            ",\"explicitAnchor\":{},\"ext\":{},\"margin\":{}",
            p.explicitly_supplied_port_anchor,
            p.connected_to_external_nodes,
            fmt_spacing(p.margin.top, p.margin.right, p.margin.bottom, p.margin.left)
        );
        s += &format!(",\"owner\":{}", opt(&d.ids.node_ix, p.owner));
        s += &format!(",\"labels\":[{}]", d.list(&p.labels, |l| d.ids.label_ix[&l]));
        s += &format!(",\"in\":[{}]", d.list(&p.incoming_edges, |e| d.ids.edge_ix[&e]));
        s += &format!(",\"out\":[{}]", d.list(&p.outgoing_edges, |e| d.ids.edge_ix[&e]));
        let pr = d.props(&p.props, None);
        s += &format!(",\"props\":{pr}}}");
        parts.push(s);
    }
    out += &parts.join(",\n");
    out += "\n],\n\"edges\":[\n";
    let mut parts = Vec::new();
    for &x in d.ids.edges.clone().iter() {
        let e = &lg[x];
        let mut s = format!("{{\"id\":{},\"source\":{},\"target\":{}", e.id, opt(&d.ids.port_ix, e.source), opt(&d.ids.port_ix, e.target));
        s += &format!(",\"bends\":[{}]", e.bend_points.iter().map(|&b| fmt_vec(b)).collect::<Vec<_>>().join(","));
        s += &format!(",\"labels\":[{}]", d.list(&e.labels, |l| d.ids.label_ix[&l]));
        let pr = d.props(&e.props, None);
        s += &format!(",\"props\":{pr}}}");
        parts.push(s);
    }
    out += &parts.join(",\n");
    out += "\n],\n\"labels\":[\n";
    let mut parts = Vec::new();
    for &x in d.ids.labels.clone().iter() {
        let l = &lg[x];
        let pr = d.props(&l.props, None);
        parts.push(format!("{{\"id\":{},\"text\":{},\"pos\":{},\"size\":{},\"props\":{pr}}}", l.id, esc(&l.text), fmt_vec(l.position), fmt_vec(l.size)));
    }
    out += &parts.join(",\n");
    out += "\n]\n}\n";
    out
}

// ---------------------------------------------------------------------------
// Comparing

/// Normalises what legitimately differs between the Swift and Rust dumps:
/// enum case spelling (`center` vs `CENTER`) and type-name prefixes.
fn normalize(v: &mut Value) {
    match v {
        Value::Object(map) => {
            let is_enum = map.get("t").and_then(Value::as_str) == Some("enum");
            if is_enum {
                if let Some(Value::String(t)) = map.get_mut("type") {
                    *t = short_type(t).to_string();
                }
                if let Some(Value::String(c)) = map.get_mut("v") {
                    *c = norm(c);
                }
            }
            if map.get("t").and_then(Value::as_str) == Some("raw") {
                if let Some(Value::String(t)) = map.get_mut("type") {
                    *t = short_type(t).to_string();
                }
            }
            if map.get("t").and_then(Value::as_str) == Some("gprops") {
                if let Some(Value::Array(a)) = map.get_mut("v") {
                    for x in a.iter_mut() {
                        if let Value::String(s) = x {
                            *s = norm(s);
                        }
                    }
                    a.sort_by(|x, y| x.as_str().cmp(&y.as_str()));
                }
            }
            if let Some(Value::String(s)) = map.get_mut("side") {
                *s = norm(s);
            }
            for (_, x) in map.iter_mut() {
                normalize(x);
            }
        }
        Value::Array(a) => {
            for x in a.iter_mut() {
                normalize(x);
            }
        }
        _ => {}
    }
}

fn first_diff(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    if out.len() >= 20 {
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => first_diff(&format!("{path}.{k}"), p, q, out),
                    (p, q) => out.push(format!("{path}.{k}: swift={} rust={}", p.map_or("-".into(), |v| v.to_string()), q.map_or("-".into(), |v| v.to_string()))),
                }
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y.iter()).enumerate() {
                first_diff(&format!("{path}[{i}]"), p, q, out);
            }
        }
        _ => {
            if a != b {
                out.push(format!("{path}: swift={a} rust={b}"));
            }
        }
    }
}

/// Compares a Swift snapshot with a Rust dump; returns the differences.
pub fn compare(swift: &str, rust: &str) -> Vec<String> {
    let mut a: Value = serde_json::from_str(swift).unwrap();
    let mut b: Value = serde_json::from_str(rust).unwrap();
    normalize(&mut a);
    normalize(&mut b);
    let mut out = Vec::new();
    first_diff("", &a, &b, &mut out);
    out
}
