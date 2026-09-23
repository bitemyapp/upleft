//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_SweepCopy.swift`.
//!
//! A snapshot of a node order and of every node's port list, and the code
//! that writes such a snapshot back into the layered graph.
//!
//! Both fields are immutable after construction, so sharing a Swift
//! instance and copying it are indistinguishable; the copy initializer shares
//! `portOrders` (as the Swift arrays share storage) and copies the node order.

use std::rc::Rc;

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::options::port_side::PortSide;
use crate::org::eclipse::elk::graph::properties::keys;
use crate::org::eclipse::elk::graph::properties::property::Property;
use crate::swift;

/// `PORT_CONSTRAINTS_KEY`: `Property<PortConstraints>("org.eclipse.elk.portConstraints")`
/// declared here without a default.
pub static PORT_CONSTRAINTS_KEY: Property = Property::new(keys::ELK_PORT_CONSTRAINTS);

#[derive(Clone, Debug)]
pub struct SweepCopy {
    /// Java: `LNode[][]`.
    pub node_order: Option<Vec<Vec<LNodeId>>>,
    /// Java: `List<List<List<LPort>>>`: every node's port list, per layer.
    pub port_orders: Rc<Vec<Vec<Vec<LPortId>>>>,
}

impl SweepCopy {
    /// `SweepCopy(_ nodeOrderIn: [[LNode]]?)`: copies the order and records
    /// each node's current port list. (The Swift also builds debug strings
    /// here that it never uses.)
    pub fn new(lg: &LGraphArena, node_order_in: Option<&[Vec<LNodeId>]>) -> SweepCopy {
        let node_order = Self::deep_copy(node_order_in);
        let mut new_port_orders: Vec<Vec<Vec<LPortId>>> = Vec::new();
        if let Some(node_order_in) = node_order_in {
            for layer_nodes in node_order_in {
                let layer_ports: Vec<Vec<LPortId>> = layer_nodes.iter().map(|&node| lg[node].ports.clone()).collect();
                new_port_orders.push(layer_ports);
            }
        }
        SweepCopy { node_order, port_orders: Rc::new(new_port_orders) }
    }

    /// `SweepCopy(_ sc: SweepCopy)`.
    pub fn from_copy(sc: &SweepCopy) -> SweepCopy {
        SweepCopy { node_order: Self::deep_copy(sc.node_order.as_deref()), port_orders: sc.port_orders.clone() }
    }

    /// `deepCopy(_:)`.
    pub fn deep_copy(current_best_node_order: Option<&[Vec<LNodeId>]>) -> Option<Vec<Vec<LNodeId>>> {
        current_best_node_order.map(|order| order.to_vec())
    }

    /// `nodes()`.
    pub fn nodes(&self) -> Option<&Vec<Vec<LNodeId>>> {
        self.node_order.as_ref()
    }

    /// `transferNodeAndPortOrdersToGraph(_:_:)`.
    pub fn transfer_node_and_port_orders_to_graph(&self, lg: &mut LGraphArena, lgraph: LGraphId, set_port_contstraints: bool) {
        let Some(node_order) = &self.node_order else { return };

        // NONDETERMINISTIC IN SWIFT: `updatePortOrder` is a dictionary keyed
        // by `ObjectIdentifier` and its values are iterated in hash order.
        // Each iteration only sorts and rewrites that one node's own port
        // list, so the order is unobservable; the port uses insertion order.
        let mut update_port_order: Vec<LNodeId> = Vec::new();
        let layers = lg[lgraph].layers.clone();
        let layer_limit = swift::min(layers.len(), node_order.len());

        for i in 0..layer_limit {
            let layer = layers[i];
            let mut graph_layer_nodes = lg[layer].nodes.clone();
            let ordered_layer_nodes = &node_order[i];
            let mut north_south_port_dummies: Vec<LNodeId> = Vec::new();

            let node_limit = swift::min(graph_layer_nodes.len(), ordered_layer_nodes.len());
            for j in 0..node_limit {
                let node = ordered_layer_nodes[j];
                lg[node].id = j as i32;
                if lg[node].node_type == NodeType::NORTH_SOUTH_PORT {
                    north_south_port_dummies.push(node);
                }

                graph_layer_nodes[j] = node;

                if i < self.port_orders.len() && j < self.port_orders[i].len() {
                    Self::apply_port_order(lg, &self.port_orders[i][j], node);
                }

                if set_port_contstraints {
                    let existing = lg[node].props.get_as::<PortConstraints>(&PORT_CONSTRAINTS_KEY);
                    let constraints = existing.unwrap_or(PortConstraints::UNDEFINED);
                    if !constraints.is_order_fixed() {
                        lg[node].props.set(&PORT_CONSTRAINTS_KEY, PortConstraints::FIXED_ORDER);
                    }
                }
            }

            lg.layer_set_nodes(layer, graph_layer_nodes);

            for dummy in north_south_port_dummies {
                if let Some(origin) = Self::assert_correct_port_sides(lg, dummy) {
                    if !update_port_order.contains(&origin) {
                        update_port_order.push(origin);
                    }
                    if !update_port_order.contains(&dummy) {
                        update_port_order.push(dummy);
                    }
                }
            }
        }

        for node in update_port_order {
            let ports: Vec<(usize, LPortId)> = lg[node].ports.iter().copied().enumerate().collect();
            let sorted: Vec<LPortId> = swift::sorted_by(ports, |lhs, rhs| {
                let cmp = cmp_combined(lg, lhs.1, rhs.1);
                if cmp != 0 {
                    return cmp < 0;
                }
                // Keep Java's stable-sort behavior for equal comparator values.
                lhs.0 < rhs.0
            })
            .into_iter()
            .map(|(_, p)| p)
            .collect();
            Self::apply_port_order(lg, &sorted, node);
            lg.node_cache_port_sides(node);
        }
    }

    /// `assertCorrectPortSides(_:)`: flips a north/south port whose dummy
    /// ended up on the other side of its node; returns the dummy's node.
    pub fn assert_correct_port_sides(lg: &mut LGraphArena, dummy: LNodeId) -> Option<LNodeId> {
        if lg[dummy].node_type != NodeType::NORTH_SOUTH_PORT {
            return None;
        }

        let origin: LNodeId = lg[dummy].props.get_typed::<LNodeId>(&InternalProperties::IN_LAYER_LAYOUT_UNIT)?;

        let Some(&dummy_port) = lg[dummy].ports.first() else { return Some(origin) };

        let Some(represented_port) = lg[dummy_port].props.get_typed::<LPortId>(&InternalProperties::ORIGIN) else {
            return Some(origin);
        };

        for port in lg[origin].ports.clone() {
            if port != represented_port {
                continue;
            }
            if lg[port].side == PortSide::NORTH && lg[dummy].id > lg[origin].id {
                lg.port_set_side(port, PortSide::SOUTH);
                if lg[port].explicitly_supplied_port_anchor {
                    let port_height = lg[port].size.y;
                    let anchor_y = lg[port].anchor.y;
                    lg[port].anchor.y = port_height - anchor_y;
                }
            } else if lg[port].side == PortSide::SOUTH && lg[origin].id > lg[dummy].id {
                lg.port_set_side(port, PortSide::NORTH);
                if lg[port].explicitly_supplied_port_anchor {
                    let port_height = lg[port].size.y;
                    let anchor_y = lg[port].anchor.y;
                    lg[port].anchor.y = -(port_height - anchor_y);
                }
            }
            break;
        }

        Some(origin)
    }

    /// `applyPortOrder(_:to:)`: detaches every current port of `node`, then
    /// attaches `ordered_ports` in order (ports missing from the list stay
    /// detached; ports owned elsewhere move to `node`).
    pub fn apply_port_order(lg: &mut LGraphArena, ordered_ports: &[LPortId], node: LNodeId) {
        for port in lg[node].ports.clone() {
            lg.port_set_node(port, None);
        }
        for &port in ordered_ports {
            lg.port_set_node(port, Some(node));
        }
    }
}

/// `PortListSorter.CMP_COMBINED`.
fn cmp_combined(lg: &LGraphArena, p1: LPortId, p2: LPortId) -> i64 {
    crate::org::eclipse::elk::alg::layered::intermediate::port_list_sorter::PortListSorter::cmp_combined(lg, p1, p2)
}
