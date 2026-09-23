//! Port of `alg/layered/options/NodeFlexibility.swift`.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};
use crate::org::eclipse::elk::graph::properties::keys;
use crate::org::eclipse::elk::graph::properties::property::Property;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum NodeFlexibility {
    NONE,
    PORT_POSITION,
    NODE_SIZE_WHERE_SPACE_PERMITS,
    NODE_SIZE,
}

pub static NODE_FLEXIBILITY: Property = Property::new(keys::ELK_LAYERED_NODE_PLACEMENT_NETWORK_SIMPLEX_NODE_FLEXIBILITY);
pub static NODE_FLEXIBILITY_DEFAULT: Property = Property::new(keys::ELK_LAYERED_NODE_PLACEMENT_NETWORK_SIMPLEX_NODE_FLEXIBILITY_DEFAULT);

impl NodeFlexibility {
    pub fn is_flexible_size(self) -> bool {
        self == NodeFlexibility::NODE_SIZE
    }

    pub fn is_flexible_size_where_space_permits(self) -> bool {
        self == NodeFlexibility::NODE_SIZE_WHERE_SPACE_PERMITS || self == NodeFlexibility::NODE_SIZE
    }

    pub fn is_flexible_ports(self) -> bool {
        matches!(self, NodeFlexibility::PORT_POSITION | NodeFlexibility::NODE_SIZE_WHERE_SPACE_PERMITS | NodeFlexibility::NODE_SIZE)
    }

    pub fn is_at_least(self, nf: NodeFlexibility) -> bool {
        match self {
            NodeFlexibility::NODE_SIZE => nf.is_flexible_size(),
            NodeFlexibility::NODE_SIZE_WHERE_SPACE_PERMITS | NodeFlexibility::PORT_POSITION => nf.is_flexible_ports(),
            NodeFlexibility::NONE => true,
        }
    }

    pub fn get_node_flexibility(lg: &LGraphArena, node: LNodeId) -> NodeFlexibility {
        if lg[node].props.has(&NODE_FLEXIBILITY) {
            return lg[node].props.get_as(&NODE_FLEXIBILITY).unwrap_or(NodeFlexibility::NONE);
        }
        match lg.node_graph(node) {
            Some(g) => lg[g].props.get_as(&NODE_FLEXIBILITY_DEFAULT).unwrap_or(NodeFlexibility::NONE),
            None => NodeFlexibility::NONE,
        }
    }
}
