//! Port of `alg/layered/options/Spacings.swift`.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::options::core_options as CoreOptions;
use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;
use crate::org::eclipse::elk::graph::properties::property::{PropValue, Property};
use crate::swift;

const N: usize = 8;

/// Spacing lookups between node types. Keeps the graph it was built for and
/// reads that graph's (and the nodes' graphs') properties at query time.
pub struct Spacings {
    graph: LGraphId,
    horizontal: [Option<&'static Property>; N * N],
    vertical: [Option<&'static Property>; N * N],
}

fn idx(t1: usize, t2: usize) -> usize {
    t1 * N + t2
}

/// `asDouble(_:)`.
fn as_double(value: Option<PropValue>) -> f64 {
    match value {
        Some(PropValue::Double(d)) => d,
        Some(PropValue::Int(i)) => i as f64,
        Some(PropValue::Str(s)) => swift::parse_double(&s).unwrap_or(0.0),
        _ => 0.0,
    }
}

impl Spacings {
    pub fn new(_lg: &LGraphArena, graph: LGraphId) -> Spacings {
        let mut s = Spacings { graph, horizontal: [None; N * N], vertical: [None; N * N] };
        s.precalculate_node_type_spacings();
        s
    }

    fn precalculate_node_type_spacings(&mut self) {
        use NodeType::*;
        self.nt1_vh(NORMAL, &LayeredOptions::SPACING_NODE_NODE, &LayeredOptions::SPACING_NODE_NODE_BETWEEN_LAYERS);
        self.nt2_vh(NORMAL, LONG_EDGE, &LayeredOptions::SPACING_EDGE_NODE, &LayeredOptions::SPACING_EDGE_NODE_BETWEEN_LAYERS);
        self.nt2(NORMAL, NORTH_SOUTH_PORT, &LayeredOptions::SPACING_EDGE_NODE);
        self.nt2(NORMAL, EXTERNAL_PORT, &LayeredOptions::SPACING_EDGE_NODE);
        self.nt2_vh(NORMAL, LABEL, &LayeredOptions::SPACING_NODE_NODE, &LayeredOptions::SPACING_NODE_NODE_BETWEEN_LAYERS);
        self.nt1_vh(LONG_EDGE, &LayeredOptions::SPACING_EDGE_EDGE, &LayeredOptions::SPACING_EDGE_EDGE_BETWEEN_LAYERS);
        self.nt2(LONG_EDGE, NORTH_SOUTH_PORT, &LayeredOptions::SPACING_EDGE_EDGE);
        self.nt2(LONG_EDGE, EXTERNAL_PORT, &LayeredOptions::SPACING_EDGE_EDGE);
        self.nt2_vh(LONG_EDGE, LABEL, &LayeredOptions::SPACING_EDGE_NODE, &LayeredOptions::SPACING_EDGE_NODE_BETWEEN_LAYERS);
        self.nt1(NORTH_SOUTH_PORT, &LayeredOptions::SPACING_EDGE_EDGE);
        self.nt2(NORTH_SOUTH_PORT, EXTERNAL_PORT, &LayeredOptions::SPACING_EDGE_EDGE);
        self.nt2(NORTH_SOUTH_PORT, LABEL, &LayeredOptions::SPACING_LABEL_NODE);
        self.nt1(EXTERNAL_PORT, &LayeredOptions::SPACING_PORT_PORT);
        self.nt2_vh(EXTERNAL_PORT, LABEL, &LayeredOptions::SPACING_LABEL_PORT_VERTICAL, &LayeredOptions::SPACING_LABEL_PORT_HORIZONTAL);
        self.nt1_vh(LABEL, &LayeredOptions::SPACING_EDGE_EDGE, &LayeredOptions::SPACING_EDGE_EDGE);
        self.nt1_vh(BREAKING_POINT, &LayeredOptions::SPACING_EDGE_EDGE, &LayeredOptions::SPACING_EDGE_EDGE_BETWEEN_LAYERS);
        self.nt2_vh(BREAKING_POINT, NORMAL, &LayeredOptions::SPACING_EDGE_NODE, &LayeredOptions::SPACING_EDGE_NODE_BETWEEN_LAYERS);
        self.nt2_vh(BREAKING_POINT, LABEL, &LayeredOptions::SPACING_EDGE_NODE, &LayeredOptions::SPACING_EDGE_NODE_BETWEEN_LAYERS);
        self.nt2_vh(BREAKING_POINT, LONG_EDGE, &LayeredOptions::SPACING_EDGE_NODE, &LayeredOptions::SPACING_EDGE_NODE_BETWEEN_LAYERS);
    }

    fn nt1(&mut self, nt: NodeType, spacing: &'static Property) {
        let o = nt.ordinal();
        self.vertical[idx(o, o)] = Some(spacing);
    }

    fn nt1_vh(&mut self, nt: NodeType, vert: &'static Property, horz: &'static Property) {
        let o = nt.ordinal();
        self.vertical[idx(o, o)] = Some(vert);
        self.horizontal[idx(o, o)] = Some(horz);
    }

    fn nt2(&mut self, n1: NodeType, n2: NodeType, spacing: &'static Property) {
        let (o1, o2) = (n1.ordinal(), n2.ordinal());
        self.vertical[idx(o1, o2)] = Some(spacing);
        self.vertical[idx(o2, o1)] = Some(spacing);
    }

    fn nt2_vh(&mut self, n1: NodeType, n2: NodeType, vert: &'static Property, horz: &'static Property) {
        let (o1, o2) = (n1.ordinal(), n2.ordinal());
        self.vertical[idx(o1, o2)] = Some(vert);
        self.vertical[idx(o2, o1)] = Some(vert);
        self.horizontal[idx(o1, o2)] = Some(horz);
        self.horizontal[idx(o2, o1)] = Some(horz);
    }

    pub fn get_horizontal_spacing(&self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> f64 {
        Self::local_spacing(lg, n1, n2, &self.horizontal)
    }

    pub fn get_horizontal_spacing_types(&self, lg: &LGraphArena, nt1: NodeType, nt2: NodeType) -> f64 {
        self.local_spacing_types(lg, nt1, nt2, &self.horizontal)
    }

    pub fn get_vertical_spacing(&self, lg: &LGraphArena, n1: LNodeId, n2: LNodeId) -> f64 {
        Self::local_spacing(lg, n1, n2, &self.vertical)
    }

    pub fn get_vertical_spacing_types(&self, lg: &LGraphArena, nt1: NodeType, nt2: NodeType) -> f64 {
        self.local_spacing_types(lg, nt1, nt2, &self.vertical)
    }

    fn local_spacing(lg: &LGraphArena, n1: LNodeId, n2: LNodeId, mapping: &[Option<&'static Property>; N * N]) -> f64 {
        let i = idx(lg[n1].node_type.ordinal(), lg[n2].node_type.ordinal());
        let Some(layout_option) = mapping[i] else { return 0.0 };
        let s1 = Self::get_individual_or_default(lg, n1, layout_option);
        let s2 = Self::get_individual_or_default(lg, n2, layout_option);
        swift::max(s1, s2)
    }

    fn local_spacing_types(&self, lg: &LGraphArena, nt1: NodeType, nt2: NodeType, mapping: &[Option<&'static Property>; N * N]) -> f64 {
        let i = idx(nt1.ordinal(), nt2.ordinal());
        let Some(layout_option) = mapping[i] else { return 0.0 };
        as_double(lg[self.graph].props.get(layout_option))
    }

    /// `getIndividualOrDefault(_:_:)`.
    pub fn get_individual_or_default(lg: &LGraphArena, node: LNodeId, property: &Property) -> f64 {
        if lg[node].props.has(&CoreOptions::SPACING_INDIVIDUAL) {
            if let Some(individual) = lg[node].props.get_object::<PropertyMap>(&CoreOptions::SPACING_INDIVIDUAL) {
                if individual.has(property) {
                    return as_double(individual.get(property));
                }
            }
        }
        as_double(lg.node_graph(node).and_then(|g| lg[g].props.get(property)))
    }
}
