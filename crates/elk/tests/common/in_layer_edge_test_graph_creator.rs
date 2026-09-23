//! Port of `Tests/ElkSwiftTests/TestHelpers/InLayerEdgeTestGraphCreator.swift`
//! (a `TestGraphCreator` subclass in Swift; extra methods here).

use upleft_elk::prelude::*;

use super::test_graph_creator::TestGraphCreator;

/// `InLayerEdgeTestGraphCreator` adds graph factories to `TestGraphCreator`.
pub type InLayerEdgeTestGraphCreator = TestGraphCreator;

impl TestGraphCreator {
    pub fn get_in_layer_edges_graph_with_crossings_to_between_layer_edge_with_fixed_port_order(&mut self) -> LGraphId {
        let layers = self.make_layers(2);
        let left_node = self.add_node_to_layer(layers[0]);
        let right_nodes = self.add_nodes_to_layer(2, layers[1]);
        self.set_port_order_fixed(right_nodes[0]);
        self.east_west_edge_from_to(left_node, right_nodes[0]);
        self.add_in_layer_edge(right_nodes[0], right_nodes[1], PortSide::WEST);
        self.east_west_edge_from_to(left_node, right_nodes[0]);
        self.east_west_edge_from_to(left_node, right_nodes[0]);
        self.get_graph()
    }

    pub fn get_in_layer_edges_with_fixed_port_order_and_normal_edge_crossings(&mut self) -> LGraphId {
        let layer = self.make_layers(2);
        let left_node = self.add_node_to_layer(layer[0]);
        let right_nodes = self.add_nodes_to_layer(3, layer[1]);
        self.set_fixed_order_constraint(right_nodes[0]);
        self.east_west_edge_from_to(left_node, right_nodes[0]);
        self.add_in_layer_edge(right_nodes[0], right_nodes[2], PortSide::WEST);
        self.east_west_edge_from_to(left_node, right_nodes[1]);
        self.get_graph()
    }

    pub fn get_in_layer_edges_crossings_but_no_fixed_order(&mut self) -> LGraphId {
        let layer = self.make_layers(2);
        let left_nodes = self.add_nodes_to_layer(2, layer[0]);
        let right_nodes = self.add_nodes_to_layer(2, layer[1]);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[0]);
        self.add_in_layer_edge(right_nodes[0], right_nodes[1], PortSide::WEST);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[1]);
        self.get_graph()
    }

    pub fn get_in_layer_edges_crossings_no_fixed_order_no_edge_between_upper_and_lower(&mut self) -> LGraphId {
        let layer = self.make_layers(2);
        let left_nodes = self.add_nodes_to_layer(2, layer[0]);
        let right_nodes = self.add_nodes_to_layer(3, layer[1]);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[1]);
        self.add_in_layer_edge(right_nodes[0], right_nodes[2], PortSide::WEST);
        self.add_in_layer_edge(right_nodes[0], right_nodes[2], PortSide::WEST);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[2]);
        self.get_graph()
    }

    pub fn get_in_layer_edges_crossings_no_fixed_order_no_edge_between_upper_and_lower_upside_down(&mut self) -> LGraphId {
        let layer = self.make_layers(2);
        let left_nodes = self.add_nodes_to_layer(2, layer[0]);
        let right_nodes = self.add_nodes_to_layer(4, layer[1]);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
        self.add_in_layer_edge(right_nodes[0], right_nodes[1], PortSide::WEST);
        self.add_in_layer_edge(right_nodes[1], right_nodes[3], PortSide::WEST);
        self.add_in_layer_edge(right_nodes[1], right_nodes[3], PortSide::WEST);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[2]);
        self.get_graph()
    }

    pub fn get_in_layer_crossings_on_both_sides(&mut self) -> LGraphId {
        let layers = self.make_layers(3);
        let left_node = self.add_node_to_layer(layers[0]);
        let middle_nodes = self.add_nodes_to_layer(3, layers[1]);
        let right_node = self.add_node_to_layer(layers[2]);
        self.add_in_layer_edge(middle_nodes[0], middle_nodes[2], PortSide::EAST);
        self.add_in_layer_edge(middle_nodes[0], middle_nodes[2], PortSide::WEST);
        self.east_west_edge_from_to(middle_nodes[1], right_node);
        self.east_west_edge_from_to(left_node, middle_nodes[1]);
        self.get_graph()
    }

    pub fn get_in_layer_edges_fixed_port_order_in_layer_and_in_between_layer_crossing(&mut self) -> LGraphId {
        let layers = self.make_layers(2);
        let left_node = self.add_node_to_layer(layers[0]);
        let right_nodes = self.add_nodes_to_layer(3, layers[1]);
        self.set_fixed_order_constraint(right_nodes[1]);
        self.east_west_edge_from_to(left_node, right_nodes[1]);
        self.add_in_layer_edge(right_nodes[0], right_nodes[1], PortSide::WEST);
        self.add_in_layer_edge(right_nodes[1], right_nodes[2], PortSide::WEST);
        self.get_graph()
    }

    pub fn get_in_layer_edges_fixed_port_order_in_layer_crossing(&mut self) -> LGraphId {
        let g1 = self.get_graph(); let l1 = self.make_layer_in(g1);
        let nodes = self.add_nodes_to_layer(3, l1);
        self.set_fixed_order_constraint(nodes[1]);
        self.add_in_layer_edge(nodes[0], nodes[1], PortSide::WEST);
        self.add_in_layer_edge(nodes[1], nodes[2], PortSide::WEST);
        self.get_graph()
    }

    pub fn get_fixed_port_order_two_in_layer_edges_cross_each_other(&mut self) -> LGraphId {
        let g2 = self.get_graph(); let l2 = self.make_layer_in(g2);
        let nodes = self.add_nodes_to_layer(3, l2);
        self.set_fixed_order_constraint(nodes[0]);
        self.add_in_layer_edge(nodes[0], nodes[2], PortSide::WEST);
        self.add_in_layer_edge(nodes[0], nodes[1], PortSide::WEST);
        self.get_graph()
    }

    pub fn get_in_layer_edges_downward_graph_no_fixed_order(&mut self) -> LGraphId {
        let layers = self.make_layers(2);
        let left_node = self.add_node_to_layer(layers[0]);
        let right_nodes = self.add_nodes_to_layer(3, layers[1]);
        self.east_west_edge_from_to(left_node, right_nodes[1]);
        self.add_in_layer_edge(right_nodes[0], right_nodes[1], PortSide::WEST);
        self.add_in_layer_edge(right_nodes[1], right_nodes[2], PortSide::WEST);
        self.get_graph()
    }

    pub fn get_in_layer_edges_multiple_edges_into_single_port(&mut self) -> LGraphId {
        let g3 = self.get_graph(); let l3 = self.make_layer_in(g3);
        let layer_two = l3;
        let left_node = self.add_node_to_layer(layer_two);
        let g4 = self.get_graph(); let l4 = self.make_layer_in(g4);
        let layer_one = l4;
        let right_nodes = self.add_nodes_to_layer(4, layer_one);
        self.add_in_layer_edge(right_nodes[1], right_nodes[3], PortSide::WEST);
        let left_port = self.add_port_on_side(left_node, PortSide::EAST);
        let right_top_port = self.add_port_on_side(right_nodes[0], PortSide::WEST);
        let right_middle_port = self.add_port_on_side(right_nodes[2], PortSide::WEST);
        self.add_edge_between_ports(left_port, right_middle_port);
        self.add_edge_between_ports(right_top_port, right_middle_port);
        self.get_graph()
    }

    pub fn get_one_layer_with_in_layer_crossings(&mut self) -> LGraphId {
        let g5 = self.get_graph(); let l5 = self.make_layer_in(g5);
        let layer = l5;
        let nodes = self.add_nodes_to_layer(4, layer);
        self.add_in_layer_edge(nodes[0], nodes[2], PortSide::WEST);
        self.add_in_layer_edge(nodes[1], nodes[3], PortSide::WEST);
        self.get_graph()
    }

    pub fn get_in_layer_one_layer_no_crossings(&mut self) -> LGraphId {
        let g6 = self.get_graph(); let l6 = self.make_layer_in(g6);
        let layer = l6;
        let nodes = self.add_nodes_to_layer(4, layer);
        self.add_in_layer_edge(nodes[0], nodes[3], PortSide::WEST);
        self.add_in_layer_edge(nodes[1], nodes[2], PortSide::WEST);
        self.get_graph()
    }

}
