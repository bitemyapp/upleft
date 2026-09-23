//! Port of `alg/layered/intermediate/NorthSouthPortPreprocessor.swift`.
//!
//! Inserts dummy nodes to cope with northern and southern ports: each such
//! port (with edges) gets a `NORTH_SOUTH_PORT` dummy above or below its node
//! in the same layer, and the edges are rerouted through it. Runs before
//! phase 3.

use crate::org::eclipse::elk::alg::layered::options::ordering_strategy::OrderingStrategy;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

const USE_NEW_APPROACH: bool = true;

#[derive(Default)]
pub struct NorthSouthPortPreprocessor;

impl NorthSouthPortPreprocessor {
    pub fn new() -> NorthSouthPortPreprocessor {
        NorthSouthPortPreprocessor
    }

    /// `node.getGraph()?.getProperty(CONSIDER_MODEL_ORDER_STRATEGY) as? OrderingStrategy`.
    fn model_order_strategy(lg: &LGraphArena, node: LNodeId) -> Option<OrderingStrategy> {
        lg.node_graph(node).and_then(|g| lg[g].props.get_as::<OrderingStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_STRATEGY))
    }

    // MARK: - Port List Sorting

    /// `sortPortList(_:)`: renumbers the northern/southern ports and sorts the
    /// node's port list (sides in ordinal order; inputs, then in/outs, then
    /// outputs on the north side, the reverse on the south side).
    fn sort_port_list(lg: &mut LGraphArena, node: LNodeId) {
        let port_count = lg[node].ports.len() as i32;

        let mut in_ports_id = 0;
        let mut in_out_ports_id = port_count;
        let mut out_ports_id = 2 * port_count;

        for port in lg[node].ports.clone() {
            match lg[port].side {
                PortSide::EAST | PortSide::WEST => lg[port].id = -1,
                PortSide::NORTH | PortSide::SOUTH => {
                    let incoming = lg[port].incoming_edges.len();
                    let outgoing = lg[port].outgoing_edges.len();

                    if incoming > 0 && outgoing > 0 {
                        lg[port].id = in_out_ports_id;
                        in_out_ports_id += 1;
                    } else if incoming > 0 {
                        lg[port].id = in_ports_id;
                        in_ports_id += 1;
                    } else if outgoing > 0 {
                        lg[port].id = out_ports_id;
                        out_ports_id += 1;
                    } else {
                        lg[port].id = in_ports_id;
                        in_ports_id += 1;
                    }
                }
                _ => {}
            }
        }

        let ports = lg[node].ports.clone();
        let sorted = swift::sorted_by(ports, |&port1, &port2| {
            let side1 = lg[port1].side;
            let side2 = lg[port2].side;

            if side1 != side2 {
                side1.ordinal() < side2.ordinal()
            } else {
                let (id1, id2) = (lg[port1].id, lg[port2].id);
                if id1 == id2 {
                    return false;
                }
                if side1 == PortSide::NORTH {
                    id1 < id2
                } else {
                    id2 < id1
                }
            }
        });
        // Replace the port list contents
        lg[node].ports = sorted;
    }

    /// `modelOrderNorthSouthInputReversing(_:_:)`.
    fn model_order_north_south_input_reversing(lg: &LGraphArena, port_list: &[LPortId], _node: LNodeId) -> Vec<LPortId> {
        let mut incoming: Vec<LPortId> = Vec::new();
        let mut outgoing: Vec<LPortId> = Vec::new();
        for &port in port_list {
            if !lg[port].incoming_edges.is_empty() {
                incoming.push(port);
            } else {
                outgoing.push(port);
            }
        }
        incoming.reverse();
        incoming.extend(outgoing);
        incoming
    }

    // MARK: - Dummy Node Creation

    /// `createDummyNodes(_:_:_:_:_:)`.
    fn create_dummy_nodes(
        lg: &mut LGraphArena,
        layered_graph: LGraphId,
        ports: &[LPortId],
        dummy_nodes: &mut Vec<LNodeId>,
        opposing_side_dummy_nodes: &mut Vec<LNodeId>,
        barycenter_associates: &mut Vec<LNodeId>,
    ) {
        let mut same_side_self_loop_edges: Vec<LEdgeId> = Vec::new();
        let mut north_south_self_loop_edges: Vec<LEdgeId> = Vec::new();

        for &port in ports {
            for &edge in &lg[port].outgoing_edges {
                if lg.edge_source_node(edge) == lg.edge_target_node(edge) {
                    let target_side = lg[edge].target.map(|t| lg[t].side);
                    if Some(lg[port].side) == target_side {
                        same_side_self_loop_edges.push(edge);
                        continue;
                    } else if lg[port].side == PortSide::NORTH && target_side == Some(PortSide::SOUTH) {
                        north_south_self_loop_edges.push(edge);
                        continue;
                    }
                }
            }
        }

        // Create north->south self-loop dummies
        for edge in north_south_self_loop_edges {
            Self::create_north_south_self_loop_dummy_nodes(lg, layered_graph, edge, dummy_nodes, opposing_side_dummy_nodes, PortSide::EAST);
        }

        // Create same-side self-loop dummies
        for edge in same_side_self_loop_edges {
            Self::create_same_side_self_loop_dummy_node(lg, layered_graph, edge, dummy_nodes);
        }

        Self::classify_and_create_dummies(lg, layered_graph, ports, dummy_nodes, barycenter_associates);
    }

    /// Same as `createDummyNodes` but without opposing side dummies (used for southern ports).
    fn create_dummy_nodes_for_south(lg: &mut LGraphArena, layered_graph: LGraphId, ports: &[LPortId], dummy_nodes: &mut Vec<LNodeId>, barycenter_associates: &mut Vec<LNodeId>) {
        let mut same_side_self_loop_edges: Vec<LEdgeId> = Vec::new();

        for &port in ports {
            for &edge in &lg[port].outgoing_edges {
                if lg.edge_source_node(edge) == lg.edge_target_node(edge) {
                    let target_side = lg[edge].target.map(|t| lg[t].side);
                    if Some(lg[port].side) == target_side {
                        same_side_self_loop_edges.push(edge);
                    }
                }
            }
        }

        for edge in same_side_self_loop_edges {
            Self::create_same_side_self_loop_dummy_node(lg, layered_graph, edge, dummy_nodes);
        }

        Self::classify_and_create_dummies(lg, layered_graph, ports, dummy_nodes, barycenter_associates);
    }

    /// `classifyAndCreateDummies(_:_:_:_:)`.
    fn classify_and_create_dummies(lg: &mut LGraphArena, layered_graph: LGraphId, ports: &[LPortId], dummy_nodes: &mut Vec<LNodeId>, barycenter_associates: &mut Vec<LNodeId>) {
        let mut in_ports: Vec<LPortId> = Vec::new();
        let mut out_ports: Vec<LPortId> = Vec::new();
        let mut in_out_ports: Vec<LPortId> = Vec::new();

        for &port in ports {
            let has_in = !lg[port].incoming_edges.is_empty();
            let has_out = !lg[port].outgoing_edges.is_empty();

            if has_in && has_out {
                in_out_ports.push(port);
            } else if has_in {
                in_ports.push(port);
            } else if has_out {
                out_ports.push(port);
            }
        }

        if USE_NEW_APPROACH {
            for &in_port in &in_ports {
                let d = Self::create_dummy_node(lg, layered_graph, Some(in_port), None, dummy_nodes);
                barycenter_associates.push(d);
            }
            for &out_port in &out_ports {
                let d = Self::create_dummy_node(lg, layered_graph, None, Some(out_port), dummy_nodes);
                barycenter_associates.push(d);
            }
        } else {
            let mut in_ports_index: usize = 0;
            let mut out_ports_index: i64 = out_ports.len() as i64 - 1;

            while in_ports_index < in_ports.len() && out_ports_index >= 0 {
                let in_port = in_ports[in_ports_index];
                let out_port = out_ports[out_ports_index as usize];

                let (Some(out_idx), Some(in_idx)) = (ports.iter().position(|&p| p == out_port), ports.iter().position(|&p| p == in_port)) else { break };
                if out_idx < in_idx {
                    break;
                }

                let d = Self::create_dummy_node(lg, layered_graph, Some(in_port), Some(out_port), dummy_nodes);
                barycenter_associates.push(d);
                in_ports_index += 1;
                out_ports_index -= 1;
            }

            while in_ports_index < in_ports.len() {
                let d = Self::create_dummy_node(lg, layered_graph, Some(in_ports[in_ports_index]), None, dummy_nodes);
                barycenter_associates.push(d);
                in_ports_index += 1;
            }

            while out_ports_index >= 0 {
                let d = Self::create_dummy_node(lg, layered_graph, None, Some(out_ports[out_ports_index as usize]), dummy_nodes);
                barycenter_associates.push(d);
                out_ports_index -= 1;
            }
        }

        for &in_out_port in &in_out_ports {
            let d = Self::create_dummy_node(lg, layered_graph, Some(in_out_port), Some(in_out_port), dummy_nodes);
            barycenter_associates.push(d);
        }
    }

    /// `createDummyNode(_:_:_:_:)`.
    fn create_dummy_node(lg: &mut LGraphArena, layered_graph: LGraphId, in_port: Option<LPortId>, out_port: Option<LPortId>, dummy_nodes: &mut Vec<LNodeId>) -> LNodeId {
        let dummy = lg.new_node(Some(layered_graph));
        lg[dummy].node_type = NodeType::NORTH_SOUTH_PORT;
        lg[dummy].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);

        let mut crossing_hint: i64 = 0;

        if let Some(in_port) = in_port {
            let dummy_input_port = lg.new_port();
            lg[dummy_input_port].props.set(&InternalProperties::ORIGIN, PropValue::LPort(in_port));
            // `inPort.getNode() as Any`: an optional stored as `Any`; the
            // ports here always have an owner.
            let in_owner = lg[in_port].owner;
            lg[dummy].props.set_opt(&InternalProperties::ORIGIN, in_owner.map(PropValue::LNode));
            lg.port_set_side(dummy_input_port, PortSide::WEST);
            lg.port_set_node(dummy_input_port, Some(dummy));

            for edge in lg[in_port].incoming_edges.clone() {
                lg.edge_set_target(edge, Some(dummy_input_port));
            }

            lg[in_port].props.set(&InternalProperties::PORT_DUMMY, PropValue::LNode(dummy));
            crossing_hint += 1;
        }

        if let Some(out_port) = out_port {
            let dummy_output_port = lg.new_port();
            let out_owner = lg[out_port].owner;
            lg[dummy].props.set_opt(&InternalProperties::ORIGIN, out_owner.map(PropValue::LNode));
            lg[dummy_output_port].props.set(&InternalProperties::ORIGIN, PropValue::LPort(out_port));
            lg.port_set_side(dummy_output_port, PortSide::EAST);
            lg.port_set_node(dummy_output_port, Some(dummy));

            for edge in lg[out_port].outgoing_edges.clone() {
                lg.edge_set_source(edge, Some(dummy_output_port));
            }

            lg[out_port].props.set(&InternalProperties::PORT_DUMMY, PropValue::LNode(dummy));
            crossing_hint += 1;
        }

        lg[dummy].props.set(&InternalProperties::CROSSING_HINT, crossing_hint);
        dummy_nodes.push(dummy);
        dummy
    }

    /// `createSameSideSelfLoopDummyNode(_:_:_:)`.
    fn create_same_side_self_loop_dummy_node(lg: &mut LGraphArena, layered_graph: LGraphId, self_loop: LEdgeId, dummy_nodes: &mut Vec<LNodeId>) {
        let dummy = lg.new_node(Some(layered_graph));
        lg[dummy].node_type = NodeType::NORTH_SOUTH_PORT;
        lg[dummy].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
        lg[dummy].props.set(&InternalProperties::ORIGIN, PropValue::LEdge(self_loop));

        let self_loop_source = lg[self_loop].source;
        let self_loop_target = lg[self_loop].target;

        let dummy_input_port = lg.new_port();
        lg[dummy_input_port].props.set_opt(&InternalProperties::ORIGIN, self_loop_target.map(PropValue::LPort));
        lg.port_set_side(dummy_input_port, PortSide::WEST);
        lg.port_set_node(dummy_input_port, Some(dummy));

        let dummy_output_port = lg.new_port();
        lg[dummy_output_port].props.set_opt(&InternalProperties::ORIGIN, self_loop_source.map(PropValue::LPort));
        lg.port_set_side(dummy_output_port, PortSide::EAST);
        lg.port_set_node(dummy_output_port, Some(dummy));

        if let Some(s) = self_loop_source {
            lg[s].props.set(&InternalProperties::PORT_DUMMY, PropValue::LNode(dummy));
        }
        if let Some(t) = self_loop_target {
            lg[t].props.set(&InternalProperties::PORT_DUMMY, PropValue::LNode(dummy));
        }

        lg.edge_set_source(self_loop, None);
        lg.edge_set_target(self_loop, None);

        dummy_nodes.push(dummy);
        lg[dummy].props.set(&InternalProperties::CROSSING_HINT, 2i64);
    }

    /// `createNorthSouthSelfLoopDummyNodes(_:_:_:_:_:)`.
    fn create_north_south_self_loop_dummy_nodes(
        lg: &mut LGraphArena,
        layered_graph: LGraphId,
        self_loop: LEdgeId,
        north_dummy_nodes: &mut Vec<LNodeId>,
        south_dummy_nodes: &mut Vec<LNodeId>,
        port_side: PortSide,
    ) {
        let self_loop_source = lg[self_loop].source;
        let self_loop_target = lg[self_loop].target;

        // North dummy
        let north_dummy = lg.new_node(Some(layered_graph));
        lg[north_dummy].node_type = NodeType::NORTH_SOUTH_PORT;
        lg[north_dummy].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
        let source_node = lg.edge_source_node(self_loop);
        lg[north_dummy].props.set_opt(&InternalProperties::ORIGIN, source_node.map(PropValue::LNode));

        let north_dummy_output_port = lg.new_port();
        lg[north_dummy_output_port].props.set_opt(&InternalProperties::ORIGIN, self_loop_source.map(PropValue::LPort));
        lg.port_set_side(north_dummy_output_port, port_side);
        lg.port_set_node(north_dummy_output_port, Some(north_dummy));

        if let Some(s) = self_loop_source {
            lg[s].props.set(&InternalProperties::PORT_DUMMY, PropValue::LNode(north_dummy));
        }

        // South dummy
        let south_dummy = lg.new_node(Some(layered_graph));
        lg[south_dummy].node_type = NodeType::NORTH_SOUTH_PORT;
        lg[south_dummy].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
        let target_node = lg.edge_target_node(self_loop);
        lg[south_dummy].props.set_opt(&InternalProperties::ORIGIN, target_node.map(PropValue::LNode));

        let south_dummy_input_port = lg.new_port();
        lg[south_dummy_input_port].props.set_opt(&InternalProperties::ORIGIN, self_loop_target.map(PropValue::LPort));
        lg.port_set_side(south_dummy_input_port, port_side);
        lg.port_set_node(south_dummy_input_port, Some(south_dummy));

        if let Some(t) = self_loop_target {
            lg[t].props.set(&InternalProperties::PORT_DUMMY, PropValue::LNode(south_dummy));
        }

        // Reroute the edge
        lg.edge_set_source(self_loop, Some(north_dummy_output_port));
        lg.edge_set_target(self_loop, Some(south_dummy_input_port));

        north_dummy_nodes.insert(0, north_dummy);
        south_dummy_nodes.push(south_dummy);

        lg[north_dummy].props.set(&InternalProperties::CROSSING_HINT, 1i64);
        lg[south_dummy].props.set(&InternalProperties::CROSSING_HINT, 1i64);
    }
}

impl ILayoutProcessor for NorthSouthPortPreprocessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Odd port side processing", 1.0);

        let mut pointer: i64;
        let mut north_dummy_nodes: Vec<LNodeId> = Vec::new();
        let mut south_dummy_nodes: Vec<LNodeId> = Vec::new();

        for layer in lg[layered_graph].layers.clone() {
            pointer = -1;

            let node_array = lg[layer].nodes.clone();
            for node in node_array {
                pointer += 1;

                // We only care about non-dummy nodes with fixed port sides
                let port_constraints = lg[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS);
                if !(lg[node].node_type == NodeType::NORMAL && port_constraints.is_some_and(|pc| pc.is_side_fixed())) {
                    continue;
                }

                // Sort the port list if we have control over the port order
                if !port_constraints.is_some_and(|pc| pc.is_order_fixed()) {
                    // `let strategy = node.getGraph()?.getProperty(...)` (untyped: the `Any?` overload)
                    let strategy = lg.node_graph(node).and_then(|g| lg[g].props.get(&LayeredOptions::CONSIDER_MODEL_ORDER_STRATEGY));
                    if strategy.is_none() || strategy.and_then(|s| s.cast::<OrderingStrategy>()) == Some(OrderingStrategy::NONE) {
                        Self::sort_port_list(lg, node);
                    }
                }

                // Nodes form their own layout unit
                lg[node].props.set(&InternalProperties::IN_LAYER_LAYOUT_UNIT, PropValue::LNode(node));

                north_dummy_nodes.clear();
                south_dummy_nodes.clear();

                let mut barycenter_associates: Vec<LNodeId> = Vec::new();

                // Prepare ports on the northern side
                let mut port_list: Vec<LPortId> = lg.node_ports_on_side(node, PortSide::NORTH);

                if let Some(strategy) = Self::model_order_strategy(lg, node) {
                    if strategy != OrderingStrategy::NONE {
                        port_list = Self::model_order_north_south_input_reversing(lg, &port_list, node);
                    }
                }

                Self::create_dummy_nodes(lg, layered_graph, &port_list, &mut north_dummy_nodes, &mut south_dummy_nodes, &mut barycenter_associates);

                let insert_point = pointer;
                let successor = node;
                for &dummy in &north_dummy_nodes {
                    lg.node_set_layer_at(dummy, insert_point as usize, layer);
                    pointer += 1;

                    lg[dummy].props.set(&InternalProperties::IN_LAYER_LAYOUT_UNIT, PropValue::LNode(node));

                    // assert(dummy.getPorts().count >= 1): a no-op; `[0]` still traps
                    let dummy_port = lg[dummy].ports[0];
                    if let Some(origin_port) = lg[dummy_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) {
                        if lg[origin_port].props.get_typed::<bool>(&LayeredOptions::ALLOW_NON_FLOW_PORTS_TO_SWITCH_SIDES) != Some(true) {
                            let mut constraints: Vec<LNodeId> = lg[dummy].props.get_as::<Vec<LNodeId>>(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS).unwrap_or_default();
                            constraints.push(successor);
                            lg[dummy].props.set(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS, constraints);
                        }
                    }

                    // (USE_NEW_APPROACH: the successor stays the node)
                }

                // Southern ports - listed right to left, so reverse
                let mut south_port_list: Vec<LPortId> = lg.node_ports_on_side(node, PortSide::SOUTH);
                south_port_list.reverse();

                if let Some(strategy) = Self::model_order_strategy(lg, node) {
                    if strategy != OrderingStrategy::NONE {
                        south_port_list = Self::model_order_north_south_input_reversing(lg, &south_port_list, node);
                    }
                }

                Self::create_dummy_nodes_for_south(lg, layered_graph, &south_port_list, &mut south_dummy_nodes, &mut barycenter_associates);

                let predecessor = node;
                for &dummy in &south_dummy_nodes {
                    pointer += 1;
                    lg.node_set_layer_at(dummy, pointer as usize, layer);

                    lg[dummy].props.set(&InternalProperties::IN_LAYER_LAYOUT_UNIT, PropValue::LNode(node));

                    let dummy_port = lg[dummy].ports[0];
                    if let Some(origin_port) = lg[dummy_port].props.get_as::<LPortId>(&InternalProperties::ORIGIN) {
                        if lg[origin_port].props.get_typed::<bool>(&LayeredOptions::ALLOW_NON_FLOW_PORTS_TO_SWITCH_SIDES) != Some(true) {
                            let mut constraints: Vec<LNodeId> = lg[predecessor].props.get_as::<Vec<LNodeId>>(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS).unwrap_or_default();
                            constraints.push(dummy);
                            lg[predecessor].props.set(&InternalProperties::IN_LAYER_SUCCESSOR_CONSTRAINTS, constraints);
                        }
                    }

                    // (USE_NEW_APPROACH: the predecessor stays the node)
                }

                if !barycenter_associates.is_empty() {
                    lg[node].props.set(&InternalProperties::BARYCENTER_ASSOCIATES, barycenter_associates);
                }
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "NorthSouthPortPreprocessor"
    }
}
