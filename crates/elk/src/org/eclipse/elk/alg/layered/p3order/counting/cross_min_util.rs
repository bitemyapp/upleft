//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/counting/org_eclipse_elk_alg_layered_p3order_counting_CrossMinUtil.swift`.
//!
//! Also holds [`port_side_view`], the read-only form of `LNode.getPortSideView`
//! every crossing-minimisation module uses.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::core::options::port_side::PortSide;

/// `node.getPortSideView(side)` without the arena borrow: when the node has
/// not cached its port sides, Swift recomputes the indices on every call
/// (`findPortIndices`) and the stored indices are never read, so computing
/// them here is equivalent. Returns the slice of `node.ports` Swift copies.
pub fn port_side_view(lg: &LGraphArena, node: LNodeId, side: PortSide) -> &[LPortId] {
    let n = &lg[node];
    let range = if n.port_sides_cached {
        n.port_side_indices.and_then(|i| i[side.ordinal()])
    } else {
        find_port_indices(lg, &n.ports)[side.ordinal()]
    };
    match range {
        Some((a, b)) => &n.ports[a..b],
        None => &[],
    }
}

/// `LNode.findPortIndices()` (as a value).
fn find_port_indices(lg: &LGraphArena, ports: &[LPortId]) -> [Option<(usize, usize)>; 5] {
    let mut indices: [Option<(usize, usize)>; 5] = [None; 5];
    let mut first_index_for_current_side = 0;
    let mut current_side = PortSide::NORTH;
    let mut current_index = 0;
    for &port in ports {
        let side = lg[port].side;
        if side != current_side {
            if first_index_for_current_side != current_index {
                indices[current_side.ordinal()] = Some((first_index_for_current_side, current_index));
            }
            current_side = side;
            first_index_for_current_side = current_index;
        }
        current_index += 1;
    }
    indices[current_side.ordinal()] = Some((first_index_for_current_side, current_index));
    indices
}

/// The ports of one side in the order `inNorthSouthEastWestOrder` yields
/// them, without allocating.
pub enum SideOrder<'a> {
    Forward(std::slice::Iter<'a, LPortId>),
    Reversed(std::iter::Rev<std::slice::Iter<'a, LPortId>>),
}

impl<'a> Iterator for SideOrder<'a> {
    type Item = LPortId;
    #[inline]
    fn next(&mut self) -> Option<LPortId> {
        match self {
            SideOrder::Forward(i) => i.next().copied(),
            SideOrder::Reversed(i) => i.next().copied(),
        }
    }
}

pub struct CrossMinUtil;

impl CrossMinUtil {
    /// `inNorthSouthEastWestOrder(_:_:)`: EAST and NORTH ports in list
    /// order, SOUTH and WEST ports reversed, nothing for `UNDEFINED`.
    pub fn in_north_south_east_west_order(lg: &LGraphArena, node: LNodeId, side: PortSide) -> Vec<LPortId> {
        Self::in_north_south_east_west_order_iter(lg, node, side).collect()
    }

    /// [`Self::in_north_south_east_west_order`] as an iterator.
    pub fn in_north_south_east_west_order_iter(lg: &LGraphArena, node: LNodeId, side: PortSide) -> SideOrder<'_> {
        match side {
            PortSide::EAST | PortSide::NORTH => SideOrder::Forward(port_side_view(lg, node, side).iter()),
            PortSide::SOUTH | PortSide::WEST => SideOrder::Reversed(port_side_view(lg, node, side).iter().rev()),
            _ => SideOrder::Forward([].iter()),
        }
    }
}
