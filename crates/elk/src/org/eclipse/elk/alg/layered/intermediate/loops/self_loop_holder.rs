//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/org_eclipse_elk_alg_layered_intermediate_loops_SelfLoopHolder.swift`.
//!
//! The holder owns every self loop object of its node: the self loop ports
//! (`slPortsList`, in insertion order), the self loop edges, and the hyper
//! loops (`slHyperLoops`). They reference each other by index
//! ([`SlPortId`], [`SlEdgeId`], [`SlLoopId`]).
//!
//! Swift stores the holder (a class) as the node's `SELF_LOOP_HOLDER`
//! property; here it is a shared `Rc<RefCell<SelfLoopHolder>>` in a
//! `PropValue::Object`, read back with [`SelfLoopHolder::of`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::self_hyper_loop::{SelfHyperLoop, SlLoopId};
use super::self_loop_edge::{SelfLoopEdge, SlEdgeId};
use super::self_loop_port::{SelfLoopPort, SlPortId};
use std::collections::VecDeque;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId, LPortId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::graph::properties::property::PropValue;

pub type SelfLoopHolderRef = Rc<RefCell<SelfLoopHolder>>;

#[derive(Debug)]
pub struct SelfLoopHolder {
    l_node: LNodeId,
    /// `slHyperLoops` (every hyper loop the holder creates is in it).
    pub(crate) sl_hyper_loops: Vec<SelfHyperLoop>,
    /// `slPortsList` (LinkedHashMap values order); also the port arena.
    pub(crate) sl_ports_list: Vec<SelfLoopPort>,
    sl_ports_map: HashMap<LPortId, SlPortId>,
    /// Every `SelfLoopEdge` created by `initialize()`.
    pub(crate) sl_edges: Vec<SelfLoopEdge>,

    ports_hidden_value: bool,
    /// `routingSlotCountArray` (a `RoutingSlotCountArray`, indexed by
    /// `PortSide` ordinal).
    pub(crate) routing_slot_count: [i64; 5],
}

const UNVISITED: i32 = 0;
const VISITED: i32 = 1;

impl SelfLoopHolder {
    fn new(node: LNodeId) -> SelfLoopHolder {
        SelfLoopHolder {
            l_node: node,
            sl_hyper_loops: Vec::new(),
            sl_ports_list: Vec::new(),
            sl_ports_map: HashMap::new(),
            sl_edges: Vec::new(),
            ports_hidden_value: false,
            routing_slot_count: [0; 5],
        }
    }

    // MARK: - Creation

    /// `install(_:)`: creates the holder, stores it on the node, and
    /// initializes it.
    pub fn install(lg: &mut LGraphArena, l_node: LNodeId) -> SelfLoopHolderRef {
        let holder: SelfLoopHolderRef = Rc::new(RefCell::new(SelfLoopHolder::new(l_node)));
        lg[l_node].props.set(&InternalProperties::SELF_LOOP_HOLDER, PropValue::object(holder.clone()));
        holder.borrow_mut().initialize(lg);
        holder
    }

    /// `lNode.getProperty(SELF_LOOP_HOLDER) as? SelfLoopHolder`.
    pub fn of(lg: &LGraphArena, l_node: LNodeId) -> Option<SelfLoopHolderRef> {
        lg[l_node].props.get_object::<RefCell<SelfLoopHolder>>(&InternalProperties::SELF_LOOP_HOLDER)
    }

    pub fn needs_self_loop_processing(lg: &LGraphArena, l_node: LNodeId) -> bool {
        if lg[l_node].node_type != NodeType::NORMAL {
            return false;
        }
        lg.node_outgoing_edges(l_node).into_iter().any(|e| lg.edge_is_self_loop(e))
    }

    // MARK: - Initialization

    fn initialize(&mut self, lg: &mut LGraphArena) {
        for l_edge in lg.node_outgoing_edges(self.l_node) {
            if lg.edge_is_self_loop(l_edge) {
                let (Some(edge_source), Some(edge_target)) = (lg[l_edge].source, lg[l_edge].target) else { continue };
                let sl_source = self.self_loop_port_for(lg, edge_source);
                let sl_target = self.self_loop_port_for(lg, edge_target);
                self.new_sl_edge(l_edge, sl_source, sl_target);
            }
        }

        // Reset port IDs for BFS
        for sl_port in &self.sl_ports_list {
            lg[sl_port.get_l_port()].id = UNVISITED;
        }

        // Run BFS at every port to gather edges into hyperloops
        for i in 0..self.sl_ports_list.len() {
            let sl_port = SlPortId(i as u32);
            if lg[self.sl_port(sl_port).get_l_port()].id == UNVISITED {
                self.initialize_hyper_loop(lg, sl_port);
            }
        }
    }

    fn self_loop_port_for(&mut self, lg: &LGraphArena, lport: LPortId) -> SlPortId {
        if let Some(&existing) = self.sl_ports_map.get(&lport) {
            return existing;
        }
        let sl_port = SlPortId(self.sl_ports_list.len() as u32);
        self.sl_ports_map.insert(lport, sl_port);
        self.sl_ports_list.push(SelfLoopPort::new(lg, lport));
        sl_port
    }

    fn initialize_hyper_loop(&mut self, lg: &mut LGraphArena, sl_port: SlPortId) -> SlLoopId {
        let sl_loop = self.new_sl_loop();

        let mut bfs_queue: VecDeque<SlPortId> = VecDeque::new();
        bfs_queue.push_back(sl_port);

        while let Some(current_sl_port) = bfs_queue.pop_front() {
            let l_port = self.sl_port(current_sl_port).get_l_port();
            lg[l_port].id = VISITED;

            for sl_edge in self.sl_port(current_sl_port).get_outgoing_sl_edges().to_vec() {
                self.add_self_loop_edge(lg, sl_loop, sl_edge);
                let sl_target_port = self.sl_edge(sl_edge).get_sl_target();
                if lg[self.sl_port(sl_target_port).get_l_port()].id == UNVISITED {
                    bfs_queue.push_back(sl_target_port);
                }
            }

            for sl_edge in self.sl_port(current_sl_port).get_incoming_sl_edges().to_vec() {
                self.add_self_loop_edge(lg, sl_loop, sl_edge);
                let sl_source_port = self.sl_edge(sl_edge).get_sl_source();
                if lg[self.sl_port(sl_source_port).get_l_port()].id == UNVISITED {
                    bfs_queue.push_back(sl_source_port);
                }
            }
        }

        sl_loop
    }

    // MARK: - Accessors

    pub fn get_l_node(&self) -> LNodeId {
        self.l_node
    }

    /// `getSLHyperLoops()`.
    pub fn get_sl_hyper_loops(&self) -> &[SelfHyperLoop] {
        &self.sl_hyper_loops
    }

    /// The ids of `getSLHyperLoops()`, in order.
    pub fn sl_loop_ids(&self) -> impl Iterator<Item = SlLoopId> + 'static {
        (0..self.sl_hyper_loops.len() as u32).map(SlLoopId)
    }

    pub fn sl_loop(&self, id: SlLoopId) -> &SelfHyperLoop {
        &self.sl_hyper_loops[id.index()]
    }

    pub fn sl_loop_mut(&mut self, id: SlLoopId) -> &mut SelfHyperLoop {
        &mut self.sl_hyper_loops[id.index()]
    }

    pub fn sl_port(&self, id: SlPortId) -> &SelfLoopPort {
        &self.sl_ports_list[id.index()]
    }

    pub fn sl_port_mut(&mut self, id: SlPortId) -> &mut SelfLoopPort {
        &mut self.sl_ports_list[id.index()]
    }

    pub fn sl_edge(&self, id: SlEdgeId) -> &SelfLoopEdge {
        &self.sl_edges[id.index()]
    }

    /// `getSLPortMap()` lookup.
    pub fn sl_port_for_l_port(&self, lport: LPortId) -> Option<SlPortId> {
        self.sl_ports_map.get(&lport).copied()
    }

    /// `getSLPortValues()`: the ports in insertion order (as their ids).
    pub fn get_sl_port_values(&self) -> impl Iterator<Item = SlPortId> + 'static {
        (0..self.sl_ports_list.len() as u32).map(SlPortId)
    }

    pub fn are_ports_hidden(&self) -> bool {
        self.ports_hidden_value
    }

    pub fn set_ports_hidden(&mut self, hidden: bool) {
        self.ports_hidden_value = hidden;
    }

    /// `getRoutingSlotCount()[side.ordinal]`.
    pub fn get_routing_slot_count(&self) -> &[i64; 5] {
        &self.routing_slot_count
    }
}
