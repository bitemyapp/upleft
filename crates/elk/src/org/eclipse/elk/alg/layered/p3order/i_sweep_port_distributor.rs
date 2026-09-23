//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_ISweepPortDistributor.swift`.

use std::cell::RefCell;
use std::rc::Rc;

use super::abstract_barycenter_port_distributor::AbstractBarycenterPortDistributor;
use super::counting::i_initializable::IInitializable;
use super::greedy_port_distributor::GreedyPortDistributor;
use super::layer_sweep_crossing_minimizer::CrossMinType;
use super::layer_total_port_distributor::LayerTotalPortDistributor;
use super::node_relative_port_distributor::NodeRelativePortDistributor;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LNodeId};

pub trait ISweepPortDistributor: IInitializable {
    /// `distributePortsWhileSweeping(_:_:_:)`.
    fn distribute_ports_while_sweeping(&mut self, lg: &mut LGraphArena, order: &[Vec<LNodeId>], free_layer_index: usize, is_forward_sweep: bool) -> bool;
}

/// `any ISweepPortDistributor` as `GraphInfoHolder` holds it. A barycenter
/// distributor is shared with the `BarycenterHeuristic`.
#[derive(Clone, Debug)]
pub enum SweepPortDistributor {
    Barycenter(Rc<RefCell<AbstractBarycenterPortDistributor>>),
    Greedy(GreedyPortDistributor),
}

impl IInitializable for SweepPortDistributor {}

impl ISweepPortDistributor for SweepPortDistributor {
    fn distribute_ports_while_sweeping(&mut self, lg: &mut LGraphArena, order: &[Vec<LNodeId>], free_layer_index: usize, is_forward_sweep: bool) -> bool {
        match self {
            SweepPortDistributor::Barycenter(pd) => pd.borrow_mut().distribute_ports_while_sweeping(lg, order, free_layer_index, is_forward_sweep),
            SweepPortDistributor::Greedy(pd) => pd.distribute_ports_while_sweeping(lg, order, free_layer_index, is_forward_sweep),
        }
    }
}

impl SweepPortDistributor {
    /// `ISweepPortDistributor.create(_:_:_:)`. The Swift draws the choice
    /// with `Bool.random(using:)` from a `RandomNumberGenerator`; the draw is
    /// passed in as `random_bool`. (`GraphInfoHolder` does not use this.)
    pub fn create(cmt: CrossMinType, random_bool: impl FnOnce() -> bool, current_order: &[Vec<LNodeId>]) -> SweepPortDistributor {
        if cmt == CrossMinType::TWO_SIDED_GREEDY_SWITCH {
            SweepPortDistributor::Greedy(GreedyPortDistributor::new())
        } else if random_bool() {
            SweepPortDistributor::Barycenter(Rc::new(RefCell::new(NodeRelativePortDistributor::new(current_order.len() as i64))))
        } else {
            SweepPortDistributor::Barycenter(Rc::new(RefCell::new(LayerTotalPortDistributor::new(current_order.len() as i64))))
        }
    }
}
