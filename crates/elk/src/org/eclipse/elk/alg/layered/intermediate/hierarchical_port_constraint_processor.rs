//! Port of `alg/layered/intermediate/HierarchicalPortConstraintProcessor.swift`.
//!
//! Processes constraints imposed on hierarchical node dummies.
//!
//! Eastern and western ports cannot be ordered arbitrarily by the crossing
//! minimizer if the port order is fixed. Thus, this processor inserts
//! appropriate in-layer successor constraints to restrict the node ordering.
//!
//! Northern and southern external ports are replaced by new external port
//! dummies. For each node connected to a northern or southern hierarchical
//! port dummy, a new dummy is placed in the adjacent layer, rerouting the
//! edges appropriately. The original dummies are removed, to be reinserted
//! later by `HierarchicalPortOrthogonalEdgeRouter`.
//!
//! Runs before phase 3.

use std::collections::HashMap;

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::alignment::Alignment;
use crate::prelude::*;

/// Index of the input port in the list of ports of newly created north / south port dummy nodes.
const DUMMY_INPUT_PORT: usize = 0;

/// Index of the output port in the list of ports of newly created north / south port dummy nodes.
const DUMMY_OUTPUT_PORT: usize = 1;

/// `ObjectIdentifier(value as AnyObject)` for an external port dummy's
/// `ORIGIN`: the identity of the referenced object. A missing value becomes
/// the `NSNull` singleton (one shared identity); any other value type is
/// boxed afresh by `as AnyObject`, so it gets an identity of its own every
/// time (`fresh` counts those).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum OriginKey {
    Null,
    Object(u8, u32),
    Boxed(u64),
}

fn origin_key(v: Option<PropValue>, fresh: &mut u64) -> OriginKey {
    match v {
        None => OriginKey::Null,
        Some(PropValue::ElkPort(p)) => OriginKey::Object(0, p.0),
        Some(PropValue::ElkNode(n)) => OriginKey::Object(1, n.0),
        Some(PropValue::ElkEdge(e)) => OriginKey::Object(2, e.0),
        Some(PropValue::ElkLabel(l)) => OriginKey::Object(3, l.0),
        Some(PropValue::ElkEdgeSection(s)) => OriginKey::Object(4, s.0),
        Some(PropValue::LPort(p)) => OriginKey::Object(5, p.0),
        Some(PropValue::LNode(n)) => OriginKey::Object(6, n.0),
        Some(PropValue::LEdge(e)) => OriginKey::Object(7, e.0),
        Some(PropValue::LLabel(l)) => OriginKey::Object(8, l.0),
        Some(PropValue::LGraph(g)) => OriginKey::Object(9, g.0),
        Some(PropValue::Layer(l)) => OriginKey::Object(10, l.0),
        Some(_) => {
            *fresh += 1;
            OriginKey::Boxed(*fresh)
        }
    }
}

#[derive(Default)]
pub struct HierarchicalPortConstraintProcessor;

impl HierarchicalPortConstraintProcessor {
    pub fn new() -> HierarchicalPortConstraintProcessor {
        HierarchicalPortConstraintProcessor
    }

    // MARK: - East / West Hierarchical Port Dummies

    /// `processEasternAndWesternPortDummies(_:)`.
    fn process_eastern_and_western_port_dummies(lg: &mut LGraphArena, layered_graph: LGraphId) {
        // If the port constraints are not at least FIXED_ORDER, there's nothing to be done here
        let port_constraints = lg[layered_graph].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
        if !port_constraints.is_order_fixed() {
            return;
        }

        let layers = lg[layered_graph].layers.clone();

        // This affects the first and last layer
        Self::process_eastern_and_western_port_dummies_in_layer(lg, layers[0]);
        Self::process_eastern_and_western_port_dummies_in_layer(lg, layers[layers.len() - 1]);
    }

    /// `processEasternAndWesternPortDummiesInLayer(_:)`.
    fn process_eastern_and_western_port_dummies_in_layer(lg: &mut LGraphArena, layer: LayerId) {
        // Put the nodes into an array
        let mut nodes = lg[layer].nodes.clone();

        // Sort the array; hierarchical port dummies are at the top, sorted by
        // position or ratio in ascending order (the sorted copy is only used
        // to add constraints; the layer keeps its order)
        {
            let lgr: &LGraphArena = lg;
            swift::sort_by(&mut nodes, |&node1, &node2| {
                let node_type1 = lgr[node1].node_type;
                let node_pos1 = lgr[node1].props.get_as::<f64>(&InternalProperties::PORT_RATIO_OR_POSITION).unwrap_or(0.0);
                let node_type2 = lgr[node2].node_type;
                let node_pos2 = lgr[node2].props.get_as::<f64>(&InternalProperties::PORT_RATIO_OR_POSITION).unwrap_or(0.0);

                if node_type2 != NodeType::EXTERNAL_PORT {
                    true
                } else if node_type1 != NodeType::EXTERNAL_PORT {
                    false
                } else {
                    node_pos1 < node_pos2
                }
            });
        }

        // Insert in-layer successor constraints where appropriate
        let mut last_hierarchical_dummy: Option<LNodeId> = None;

        for node in nodes {
            if lg[node].node_type != NodeType::EXTERNAL_PORT {
                // No hierarchical port dummy nodes any more
                break;
            }

            // Only process dummies created for eastern or western external ports
            let external_port_side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE);
            if external_port_side != Some(PortSide::WEST) && external_port_side != Some(PortSide::EAST) {
                continue;
            }

            if let Some(last_dummy) = last_hierarchical_dummy {
                let mut constraints: Vec<LNodeId> = lg[last_dummy].props.get_as::<Vec<LNodeId>>(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS).unwrap_or_default();
                constraints.push(node);
                lg[last_dummy].props.set(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS, constraints);
            }

            last_hierarchical_dummy = Some(node);
        }
    }

    // MARK: - North / South Hierarchical Port Dummies

    /// `processNorthernAndSouthernPortDummies(_:)`.
    fn process_northern_and_southern_port_dummies(lg: &mut LGraphArena, layered_graph: LGraphId) {
        // If the port constraints are not at least FIXED_SIDE, there's nothing to do here
        let port_constraints = lg[layered_graph].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
        if !port_constraints.is_side_fixed() {
            return;
        }

        let layers = lg[layered_graph].layers.clone();
        let layer_count = layers.len();
        let mut fresh = 0u64;

        // For each layer, we keep a map of dummy nodes created for a given original external port
        // dummy. Index i belongs to layer i - 1 (index 0 to a new first layer, the last index to
        // a new last layer). The maps are only looked up, never iterated.
        let mut ext_port_to_dummy_node_map: Vec<HashMap<OriginKey, LNodeId>> = vec![HashMap::new(), HashMap::new()];
        let mut new_dummy_nodes: Vec<Vec<LNodeId>> = vec![Vec::new(), Vec::new()];

        // We remember each original external port dummy we encounter
        let mut original_external_port_dummies: Vec<LNodeId> = Vec::new();

        // Iterate through each layer
        for curr_layer_idx in 0..layer_count {
            let current_layer = layers[curr_layer_idx];

            // Dummy node maps and lists for the previous and next layer (Swift
            // copies them out and writes them back at the end of the layer)
            let mut prev_ext_port_to_dummy_nodes_map = std::mem::take(&mut ext_port_to_dummy_node_map[curr_layer_idx]);
            let mut next_ext_port_to_dummy_nodes_map: HashMap<OriginKey, LNodeId> = HashMap::new();
            ext_port_to_dummy_node_map.push(HashMap::new());

            let mut prev_new_dummy_nodes = std::mem::take(&mut new_dummy_nodes[curr_layer_idx]);
            let mut next_new_dummy_nodes: Vec<LNodeId> = Vec::new();
            new_dummy_nodes.push(Vec::new());

            // Iterate through the layer's nodes, looking for normal nodes connected to
            // northern / southern hierarchical port dummies
            for current_node in lg[current_layer].nodes.clone() {
                if Self::is_northern_or_southern_dummy(lg, current_node) {
                    // It's a northern or southern external port dummy. Schedule for removal.
                    original_external_port_dummies.push(current_node);
                    continue;
                }

                // Iterate over the node's incoming edges
                for edge in lg.node_incoming_edges(current_node) {
                    let Some(source_node) = lg.edge_source_node(edge) else { continue };

                    // Check if it's a northern / southern dummy node
                    if !Self::is_northern_or_southern_dummy(lg, source_node) {
                        continue;
                    }

                    // See if a dummy has already been created for the previous layer
                    let origin = origin_key(lg[source_node].props.get(&InternalProperties::ORIGIN), &mut fresh);
                    let prev_layer_dummy = match prev_ext_port_to_dummy_nodes_map.get(&origin) {
                        Some(&existing) => existing,
                        None => {
                            // No. Create one.
                            let d = Self::create_dummy(lg, layered_graph, source_node);
                            prev_ext_port_to_dummy_nodes_map.insert(origin, d);
                            prev_new_dummy_nodes.push(d);
                            d
                        }
                    };

                    // Reroute the edge
                    let port = lg[prev_layer_dummy].ports[DUMMY_OUTPUT_PORT];
                    lg.edge_set_source(edge, Some(port));
                }

                // Iterate over the node's outgoing edges
                for edge in lg.node_outgoing_edges(current_node) {
                    let Some(target_node) = lg.edge_target_node(edge) else { continue };

                    // Check if it's a northern / southern dummy node
                    if !Self::is_northern_or_southern_dummy(lg, target_node) {
                        continue;
                    }

                    // See if a dummy has already been created for the next layer
                    let origin = origin_key(lg[target_node].props.get(&InternalProperties::ORIGIN), &mut fresh);
                    let next_layer_dummy = match next_ext_port_to_dummy_nodes_map.get(&origin) {
                        Some(&existing) => existing,
                        None => {
                            // No. Create one.
                            let d = Self::create_dummy(lg, layered_graph, target_node);
                            next_ext_port_to_dummy_nodes_map.insert(origin, d);
                            next_new_dummy_nodes.push(d);
                            d
                        }
                    };

                    // Reroute the edge
                    let port = lg[next_layer_dummy].ports[DUMMY_INPUT_PORT];
                    lg.edge_set_target(edge, Some(port));
                }
            }

            // Write back modified maps/lists
            ext_port_to_dummy_node_map[curr_layer_idx] = prev_ext_port_to_dummy_nodes_map;
            ext_port_to_dummy_node_map[curr_layer_idx + 2] = next_ext_port_to_dummy_nodes_map;
            new_dummy_nodes[curr_layer_idx] = prev_new_dummy_nodes;
            new_dummy_nodes[curr_layer_idx + 2] = next_new_dummy_nodes;
        }

        // Add the newly created dummy nodes
        for i in 0..new_dummy_nodes.len() {
            if new_dummy_nodes[i].is_empty() {
                continue;
            }

            // Find the layer the dummy nodes should be added to. (After a new
            // first layer has been inserted, `layers[i - 1]` is off by one —
            // as in the Swift.)
            let layer = if i == 0 {
                // A new first layer must be created
                let layer = lg.new_layer(layered_graph);
                lg[layered_graph].layers.insert(0, layer);
                layer
            } else if i == ext_port_to_dummy_node_map.len() - 1 {
                // A new last layer must be created
                let layer = lg.new_layer(layered_graph);
                lg[layered_graph].layers.push(layer);
                layer
            } else {
                lg[layered_graph].layers[i - 1]
            };

            for &dummy in &new_dummy_nodes[i] {
                lg.node_set_layer(dummy, Some(layer));
            }
        }

        // Iterate through the hierarchical port dummies and remove them
        for &original_dummy in &original_external_port_dummies {
            lg.node_set_layer(original_dummy, None);
        }

        // Remember the original external port dummies in the graph
        lg[layered_graph].props.set(&InternalProperties::EXT_PORT_REPLACED_DUMMIES, original_external_port_dummies);
    }

    /// `isNorthernOrSouthernDummy(_:)`.
    fn is_northern_or_southern_dummy(lg: &LGraphArena, node: LNodeId) -> bool {
        if lg[node].node_type == NodeType::EXTERNAL_PORT {
            let port_side = lg[node].props.get_as::<PortSide>(&InternalProperties::EXT_PORT_SIDE);
            return port_side == Some(PortSide::NORTH) || port_side == Some(PortSide::SOUTH);
        }
        false
    }

    /// `createDummy(_:_:)`: a dummy for the given original dummy, whose
    /// `ORIGIN` is the original dummy's origin and whose
    /// `EXT_PORT_REPLACED_DUMMY` is the original dummy.
    ///
    /// `copyProperties` shares the original's `PORT_ANCHOR` vector, which in
    /// Swift is the original dummy port's `position` object. The new dummy
    /// therefore gets the original's anchor alias: its `PORT_ANCHOR` reads
    /// the original dummy port's position, as in Swift.
    fn create_dummy(lg: &mut LGraphArena, layered_graph: LGraphId, original_dummy: LNodeId) -> LNodeId {
        let new_dummy = lg.new_node(Some(layered_graph));
        let props = lg[original_dummy].props.clone();
        lg[new_dummy].props.copy_properties(&props);
        lg[new_dummy].port_anchor_alias = lg[original_dummy].port_anchor_alias;
        lg[new_dummy].props.set(&InternalProperties::EXT_PORT_REPLACED_DUMMY, PropValue::LNode(original_dummy));
        lg[new_dummy].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
        lg[new_dummy].props.set(&LayeredOptions::ALIGNMENT, Alignment::CENTER);
        lg[new_dummy].node_type = NodeType::EXTERNAL_PORT;

        let input_port = lg.new_port();
        lg.port_set_node(input_port, Some(new_dummy));
        lg.port_set_side(input_port, PortSide::WEST);

        let output_port = lg.new_port();
        lg.port_set_node(output_port, Some(new_dummy));
        lg.port_set_side(output_port, PortSide::EAST);

        new_dummy
    }
}

impl ILayoutProcessor for HierarchicalPortConstraintProcessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Hierarchical port constraint processing", 1.0);

        Self::process_eastern_and_western_port_dummies(lg, layered_graph);
        Self::process_northern_and_southern_port_dummies(lg, layered_graph);

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "HierarchicalPortConstraintProcessor"
    }
}
