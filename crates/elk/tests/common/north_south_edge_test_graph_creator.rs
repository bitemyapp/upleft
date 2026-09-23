//! Port of `Tests/ElkSwiftTests/TestHelpers/NorthSouthEdgeTestGraphCreator.swift`
//! (a `TestGraphCreator` subclass in Swift; extra methods here).

use upleft_elk::prelude::*;

use super::test_graph_creator::TestGraphCreator;

/// `NorthSouthEdgeTestGraphCreator` adds graph factories to `TestGraphCreator`.
pub type NorthSouthEdgeTestGraphCreator = TestGraphCreator;

impl TestGraphCreator {
    pub fn get_north_south_upward_crossing_graph(&mut self) -> LGraphId {
        let l1 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(3, l1);
        let l2 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(2, l2);
        self.add_north_south_edge(PortSide::NORTH, left_nodes[2], left_nodes[1], right_nodes[1], false);
        self.add_north_south_edge(PortSide::NORTH, left_nodes[2], left_nodes[0], right_nodes[0], false);
        self.set_fixed_order_constraint(left_nodes[2]);
        self.get_graph()
    }

    pub fn get_north_south_upward_multiple_crossing_graph(&mut self) -> LGraphId {
        let l3 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(4, l3);
        let l4 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(3, l4);
        self.add_north_south_edge(PortSide::NORTH, left_nodes[3], left_nodes[2], right_nodes[2], false);
        self.add_north_south_edge(PortSide::NORTH, left_nodes[3], left_nodes[1], right_nodes[1], false);
        self.add_north_south_edge(PortSide::NORTH, left_nodes[3], left_nodes[0], right_nodes[0], false);
        self.set_fixed_order_constraint(left_nodes[3]);
        self.get_graph()
    }

    pub fn get_three_layer_north_south_crossing_graph(&mut self) -> LGraphId {
        let l5 = self.make_layer();
        let left_node = self.add_node_to_layer(l5);
        let l6 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l6);
        let l7 = self.make_layer();
        let right_node = self.add_node_to_layer(l7);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.set_fixed_order_constraint(right_node);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], right_node, false);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[1], right_node, false);
        self.east_west_edge_from_to(left_node, middle_nodes[0]);
        self.get_graph()
    }

    pub fn get_north_south_downward_crossing_graph(&mut self) -> LGraphId {
        let l8 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(3, l8);
        let l9 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(2, l9);
        self.add_north_south_edge(PortSide::SOUTH, left_nodes[0], left_nodes[2], right_nodes[1], false);
        self.add_north_south_edge(PortSide::SOUTH, left_nodes[0], left_nodes[1], right_nodes[0], false);
        self.set_fixed_order_constraint(left_nodes[0]);
        self.get_graph()
    }

    pub fn get_north_south_downward_multiple_crossing_graph(&mut self) -> LGraphId {
        let l10 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(4, l10);
        let l11 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(3, l11);
        self.add_north_south_edge(PortSide::SOUTH, left_nodes[0], left_nodes[3], right_nodes[2], false);
        self.add_north_south_edge(PortSide::SOUTH, left_nodes[0], left_nodes[2], right_nodes[1], false);
        self.add_north_south_edge(PortSide::SOUTH, left_nodes[0], left_nodes[1], right_nodes[0], false);
        self.set_fixed_order_constraint(left_nodes[0]);
        self.get_graph()
    }

    pub fn get_southern_north_south_dummy_edge_crossing_graph(&mut self) -> LGraphId {
        let l12 = self.make_layer();
        let left_node = self.add_node_to_layer(l12);
        let l13 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l13);
        let l14 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(2, l14);
        self.east_west_edge_from_to(left_node, middle_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[0]);
        self.set_as_long_edge_dummy(middle_nodes[1]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], right_nodes[1], true);
        self.get_graph()
    }

    pub fn get_southern_north_south_dummy_edge_two_crossing_graph(&mut self) -> LGraphId {
        let l15 = self.make_layer();
        let left_node = self.add_node_to_layer(l15);
        let l16 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(4, l16);
        let l17 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(3, l17);
        self.east_west_edge_from_to(left_node, middle_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[0]);
        self.set_as_long_edge_dummy(middle_nodes[1]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], right_nodes[1], true);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[3], right_nodes[2], true);
        self.get_graph()
    }

    pub fn get_southern_two_dummy_edge_and_north_south_crossing_graph(&mut self) -> LGraphId {
        let l18 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, l18);
        let l19 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(5, l19);
        let l20 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(4, l20);
        self.east_west_edge_from_to(left_nodes[0], middle_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[0]);
        self.set_as_long_edge_dummy(middle_nodes[1]);
        self.east_west_edge_from_to(left_nodes[1], middle_nodes[3]);
        self.east_west_edge_from_to(middle_nodes[3], right_nodes[2]);
        self.set_as_long_edge_dummy(middle_nodes[3]);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[4], right_nodes[3], true);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], right_nodes[1], true);
        self.get_graph()
    }

    pub fn get_northern_north_south_dummy_edge_crossing_graph(&mut self) -> LGraphId {
        let l21 = self.make_layer();
        let left_node = self.add_node_to_layer(l21);
        let l22 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l22);
        let l23 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(2, l23);
        self.east_west_edge_from_to(left_node, middle_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[1]);
        self.set_as_long_edge_dummy(middle_nodes[1]);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[0], right_nodes[0], true);
        self.get_graph()
    }

    pub fn get_south_port_on_normal_node_below_long_edge_dummy(&mut self) -> LGraphId {
        let l24 = self.make_layer();
        let left_node = self.add_node_to_layer(l24);
        let l25 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l25);
        let l26 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(2, l26);
        self.east_west_edge_from_to(left_node, middle_nodes[0]);
        self.east_west_edge_from_to(middle_nodes[0], right_nodes[0]);
        self.lg[middle_nodes[0]].node_type = NodeType::LONG_EDGE;
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[1], middle_nodes[2], right_nodes[1], false);
        self.get_graph()
    }

    pub fn get_north_port_ond_normal_node_above_long_edge_dummy(&mut self) -> LGraphId {
        let l27 = self.make_layer();
        let left_node = self.add_node_to_layer(l27);
        let l28 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l28);
        let l29 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(2, l29);
        self.east_west_edge_from_to(left_node, middle_nodes[2]);
        self.east_west_edge_from_to(middle_nodes[2], right_nodes[1]);
        self.lg[middle_nodes[2]].node_type = NodeType::LONG_EDGE;
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[1], middle_nodes[0], right_nodes[0], false);
        self.get_graph()
    }

    pub fn get_long_edge_dummy_and_normal_node_with_unused_ports_on_southern_side(&mut self) -> LGraphId {
        let l30 = self.make_layer();
        let left_node = self.add_node_to_layer(l30);
        let l31 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(2, l31);
        let l32 = self.make_layer();
        let right_node = self.add_node_to_layer(l32);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.east_west_edge_from_to(left_node, middle_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[1], right_node);
        self.set_as_long_edge_dummy(middle_nodes[1]);
        self.add_port_on_side(middle_nodes[0], PortSide::SOUTH);
        self.add_port_on_side(middle_nodes[0], PortSide::SOUTH);
        self.get_graph()
    }

    pub fn get_long_edge_dummy_and_normal_node_with_unused_ports_on_northern_side(&mut self) -> LGraphId {
        let l33 = self.make_layer();
        let left_node = self.add_node_to_layer(l33);
        let l34 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(2, l34);
        let l35 = self.make_layer();
        let right_node = self.add_node_to_layer(l35);
        self.east_west_edge_from_to(left_node, middle_nodes[0]);
        self.east_west_edge_from_to(middle_nodes[0], right_node);
        self.lg[middle_nodes[0]].node_type = NodeType::LONG_EDGE;
        self.add_port_on_side(middle_nodes[1], PortSide::NORTH);
        self.add_port_on_side(middle_nodes[1], PortSide::NORTH);
        self.get_graph()
    }

    pub fn get_multiple_north_south_and_long_edge_dummies_on_both_sides(&mut self) -> LGraphId {
        let l36 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, l36);
        let l37 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(7, l37);
        let l38 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(6, l38);
        self.east_west_edge_from_to(left_nodes[0], middle_nodes[2]);
        self.east_west_edge_from_to(middle_nodes[2], right_nodes[2]);
        self.east_west_edge_from_to(left_nodes[1], middle_nodes[4]);
        self.east_west_edge_from_to(middle_nodes[4], right_nodes[4]);
        self.set_as_long_edge_dummy(middle_nodes[2]);
        self.set_as_long_edge_dummy(middle_nodes[4]);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[3], middle_nodes[0], right_nodes[0], false);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[3], middle_nodes[1], right_nodes[1], false);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[3], middle_nodes[5], right_nodes[4], false);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[3], middle_nodes[6], right_nodes[5], false);
        self.get_graph()
    }

    pub fn get_southern_north_south_graph_edges_from_east_and_west_no_crossings(&mut self) -> LGraphId {
        let l39 = self.make_layer();
        let left_node = self.add_node_to_layer(l39);
        let l40 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l40);
        let l41 = self.make_layer();
        let right_node = self.add_node_to_layer(l41);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[1], right_node, false);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], left_node, true);
        self.get_graph()
    }

    pub fn get_northern_north_south_graph_edges_from_east_and_west_no_crossings(&mut self) -> LGraphId {
        let l42 = self.make_layer();
        let left_node = self.add_node_to_layer(l42);
        let l43 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l43);
        let l44 = self.make_layer();
        let right_node = self.add_node_to_layer(l44);
        self.set_fixed_order_constraint(middle_nodes[2]);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[0], left_node, true);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[1], right_node, false);
        self.get_graph()
    }

    pub fn get_northern_north_south_graph_edges_from_east_and_west_no_crossings_upper_edge_east(&mut self) -> LGraphId {
        let l45 = self.make_layer();
        let left_node = self.add_node_to_layer(l45);
        let l46 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l46);
        let l47 = self.make_layer();
        let right_node = self.add_node_to_layer(l47);
        self.set_fixed_order_constraint(middle_nodes[2]);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[1], left_node, true);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[0], right_node, false);
        self.get_graph()
    }

    pub fn get_north_south_edges_from_east_and_west_and_cross(&mut self) -> LGraphId {
        let l48 = self.make_layer();
        let left_node = self.add_node_to_layer(l48);
        let l49 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l49);
        let l50 = self.make_layer();
        let right_node = self.add_node_to_layer(l50);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[1], left_node, true);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], right_node, false);
        self.get_graph()
    }

    pub fn get_southern_north_south_edges_both_to_east(&mut self) -> LGraphId {
        let l51 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l51);
        let l52 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(2, l52);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[1], right_nodes[0], false);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], right_nodes[1], false);
        self.get_graph()
    }

    pub fn get_north_south_southern_two_western_edges(&mut self) -> LGraphId {
        let l53 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, l53);
        let l54 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l54);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[1], left_nodes[0], true);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], left_nodes[1], true);
        self.get_graph()
    }

    pub fn get_north_south_southern_three_western_edges(&mut self) -> LGraphId {
        let l55 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(3, l55);
        let l56 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(4, l56);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[1], left_nodes[0], true);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], left_nodes[1], true);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[3], left_nodes[2], true);
        self.get_graph()
    }

    pub fn get_north_south_northern_western_edges(&mut self) -> LGraphId {
        let l57 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(2, l57);
        let l58 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l58);
        self.set_fixed_order_constraint(middle_nodes[2]);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[1], left_nodes[1], true);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[0], left_nodes[0], true);
        self.get_graph()
    }

    pub fn get_north_south_northern_eastern_port_to_west_western_port_to_east(&mut self) -> LGraphId {
        let l59 = self.make_layer();
        let left_node = self.add_node_to_layer(l59);
        let l60 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l60);
        let l61 = self.make_layer();
        let right_node = self.add_node_to_layer(l61);
        self.set_fixed_order_constraint(middle_nodes[2]);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[1], right_node, false);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[2], middle_nodes[0], left_node, true);
        self.get_graph()
    }

    pub fn get_north_south_all_sides_multiple_crossings(&mut self) -> LGraphId {
        let l62 = self.make_layer();
        let left_node = self.add_node_to_layer(l62);
        let l63 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(7, l63);
        let l64 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(5, l64);
        self.set_fixed_order_constraint(middle_nodes[3]);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[3], middle_nodes[1], right_nodes[1], false);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[3], middle_nodes[2], left_node, true);
        self.add_north_south_edge(PortSide::NORTH, middle_nodes[3], middle_nodes[0], right_nodes[0], false);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[3], middle_nodes[6], right_nodes[4], false);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[3], middle_nodes[4], right_nodes[2], false);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[3], middle_nodes[5], right_nodes[3], false);
        self.get_graph()
    }

    pub fn get_north_south_southern_western_port_to_east_and_eastern_port_to_west(&mut self) -> LGraphId {
        let l65 = self.make_layer();
        let left_node = self.add_node_to_layer(l65);
        let l66 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(3, l66);
        let l67 = self.make_layer();
        let right_node = self.add_node_to_layer(l67);
        self.set_fixed_order_constraint(middle_nodes[0]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[2], left_node, true);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[0], middle_nodes[1], right_node, false);
        self.get_graph()
    }

    pub fn get_graph_where_layout_unit_prevents_switch(&mut self) -> LGraphId {
        let l68 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(4, l68);
        let l69 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(2, l69);
        self.set_fixed_order_constraint(left_nodes[0]);
        self.set_fixed_order_constraint(left_nodes[3]);
        self.add_north_south_edge(PortSide::SOUTH, left_nodes[0], left_nodes[1], right_nodes[1], false);
        self.add_north_south_edge(PortSide::NORTH, left_nodes[3], left_nodes[2], right_nodes[0], false);
        self.get_graph()
    }

    pub fn get_graph_layout_unit_prevents_switch_with_node_with_node_with_northern_edges(&mut self) -> LGraphId {
        let l70 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(3, l70);
        let l71 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(3, l71);
        self.add_north_south_edge(PortSide::NORTH, left_nodes[1], left_nodes[0], right_nodes[0], false);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[2]);
        self.east_west_edge_from_to(left_nodes[2], right_nodes[1]);
        self.get_graph()
    }

    pub fn get_graph_layout_unit_prevents_switch_with_node_with_node_with_southern_edges(&mut self) -> LGraphId {
        let l72 = self.make_layer();
        let left_nodes = self.add_nodes_to_layer(4, l72);
        let l73 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(3, l73);
        self.east_west_edge_from_to(left_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(left_nodes[1], right_nodes[0]);
        self.add_north_south_edge(PortSide::SOUTH, left_nodes[1], left_nodes[2], right_nodes[2], false);
        self.get_graph()
    }

    pub fn get_graph_layout_unit_does_not_prevent_switch_with_long_edge_dummy(&mut self) -> LGraphId {
        let l74 = self.make_layer();
        let left_node = self.add_node_to_layer(l74);
        let l75 = self.make_layer();
        let middle_nodes = self.add_nodes_to_layer(4, l75);
        let l76 = self.make_layer();
        let right_nodes = self.add_nodes_to_layer(3, l76);
        self.set_as_long_edge_dummy(middle_nodes[0]);
        self.east_west_edge_from_to(left_node, middle_nodes[0]);
        self.east_west_edge_from_to(middle_nodes[0], right_nodes[1]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[0]);
        self.east_west_edge_from_to(middle_nodes[1], right_nodes[0]);
        self.add_north_south_edge(PortSide::SOUTH, middle_nodes[1], middle_nodes[2], right_nodes[2], false);
        self.get_graph()
    }

}
