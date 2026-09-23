//! Port of `alg/layered/GraphConfigurator.swift` (which also defines
//! `Random`, elk-swift's `java.util.Random`, and `LabelManagementOptions`).

use crate::org::eclipse::elk::graph::properties::keys;
use crate::org::eclipse::elk::graph::properties::property::Property;

/// `LabelManagementOptions` (a stub in elk-swift).
pub mod LabelManagementOptions {
    use super::*;
    pub static LABEL_MANAGER: Property = Property::new(keys::ELK_LABELS_LABEL_MANAGER);
}

/// `Random`: `java.util.Random`'s linear congruential generator, as elk-swift
/// implements it (including its `nextInt(bound)` and the unseeded constructor).
#[derive(Clone, Debug)]
pub struct Random {
    pub seed: u64,
}

impl Random {
    /// `Random()`: seeded from the clock. Only used for `randomSeed == 0`.
    pub fn new_unseeded() -> Random {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        Random { seed: (millis as i64 as u64) ^ 0x5DEECE66D }
    }

    /// `Random(seed:)`.
    pub fn with_seed(seed: i64) -> Random {
        Random { seed: ((seed as u64) ^ 0x5DEECE66D) & 0xFFFFFFFFFFFF }
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(0x5DEECE66D).wrapping_add(0xB)) & 0xFFFFFFFFFFFF;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i64 {
        self.next(32) as i64
    }

    /// `nextInt(_ bound:)`; `0` for a non-positive bound.
    pub fn next_int_bounded(&mut self, bound: i64) -> i64 {
        if bound <= 0 {
            return 0;
        }
        if bound & (bound - 1) == 0 {
            return ((bound.wrapping_mul(self.next(31) as i64)) >> 31) as i64;
        }
        let bound32 = bound as i32;
        loop {
            let bits = self.next(31);
            let val = bits % bound32;
            if bits.wrapping_sub(val).wrapping_add(bound32).wrapping_sub(1) >= 0 {
                return val as i64;
            }
        }
    }

    pub fn next_double(&mut self) -> f64 {
        (((self.next(26) as i64) << 27) + self.next(27) as i64) as f64 / (1i64 << 53) as f64
    }

    pub fn next_long(&mut self) -> i64 {
        ((self.next(32) as i64) << 32).wrapping_add(self.next(32) as i64)
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = ((seed as u64) ^ 0x5DEECE66D) & 0xFFFFFFFFFFFF;
    }
}

use std::cell::RefCell;
use std::rc::Rc;

use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LNodeId};
use crate::org::eclipse::elk::alg::layered::intermediate::intermediate_processor_strategy::IntermediateProcessorStrategy as IPS;
use crate::org::eclipse::elk::alg::layered::layered_phases::{LayeredPhases, PhaseFactory};
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::alg::layered::options::spacings::Spacings;
use crate::org::eclipse::elk::alg::layered::options::{
    crossing_minimization_strategy::CrossingMinimizationStrategy, cycle_breaking_strategy::CycleBreakingStrategy,
    graph_compaction_strategy::GraphCompactionStrategy, graph_properties::GraphProperties,
    greedy_switch_type::GreedySwitchType, layering_strategy::LayeringStrategy,
    node_placement_strategy::NodePlacementStrategy, node_promotion_strategy::NodePromotionStrategy,
    ordering_strategy::OrderingStrategy,
};
use crate::org::eclipse::elk::alg::layered::p5edges::edge_router_factory::EdgeRouterFactory;
use crate::org::eclipse::elk::core::alg::algorithm_assembler::AlgorithmAssembler;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ProcessorList;
use crate::org::eclipse::elk::core::alg::layout_processor_configuration::LayoutProcessorConfiguration;
use crate::org::eclipse::elk::core::options::direction::Direction;
use crate::org::eclipse::elk::core::options::edge_routing::EdgeRouting;
use crate::org::eclipse::elk::core::options::hierarchy_handling::HierarchyHandling;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::graph::properties::property::PropValue;

/// `GraphConfigurator.BASELINE_PROCESSING_CONFIGURATION`.
fn baseline_processing_configuration() -> LayoutProcessorConfiguration {
    let mut c = LayoutProcessorConfiguration::create();
    c.add_before(LayeredPhases::P4_NODE_PLACEMENT, IPS::INNERMOST_NODE_MARGIN_CALCULATOR)
        .add_before(LayeredPhases::P4_NODE_PLACEMENT, IPS::LABEL_AND_NODE_SIZE_PROCESSOR)
        .add_before(LayeredPhases::P5_EDGE_ROUTING, IPS::LAYER_SIZE_AND_GRAPH_HEIGHT_CALCULATOR)
        .add_after(LayeredPhases::P5_EDGE_ROUTING, IPS::END_LABEL_SORTER);
    c
}

/// `GraphConfigurator.LABEL_MANAGEMENT_ADDITIONS`.
fn label_management_additions() -> LayoutProcessorConfiguration {
    let mut c = LayoutProcessorConfiguration::create();
    c.add_before(LayeredPhases::P4_NODE_PLACEMENT, IPS::CENTER_LABEL_MANAGEMENT_PROCESSOR)
        .add_before(LayeredPhases::P4_NODE_PLACEMENT, IPS::END_NODE_PORT_LABEL_MANAGEMENT_PROCESSOR);
    c
}

/// `GraphConfigurator.HIERARCHICAL_ADDITIONS`.
fn hierarchical_additions() -> LayoutProcessorConfiguration {
    let mut c = LayoutProcessorConfiguration::create();
    c.add_after(LayeredPhases::P5_EDGE_ROUTING, IPS::HIERARCHICAL_NODE_RESIZER);
    c
}

#[derive(Default)]
pub struct GraphConfigurator {
    pub algorithm_assembler: AlgorithmAssembler,
}

impl GraphConfigurator {
    pub const MIN_EDGE_SPACING: f64 = 2.0;

    pub fn new() -> GraphConfigurator {
        GraphConfigurator::default()
    }

    /// `configureGraphProperties(_:)`.
    pub fn configure_graph_properties(&mut self, lg: &mut LGraphArena, lgraph: LGraphId) {
        let edge_spacing = lg[lgraph].props.get_as::<f64>(&LayeredOptions::SPACING_EDGE_EDGE).unwrap_or(0.0);
        if edge_spacing < Self::MIN_EDGE_SPACING {
            lg[lgraph].props.set(&LayeredOptions::SPACING_EDGE_EDGE, Self::MIN_EDGE_SPACING);
        }

        let direction = lg[lgraph].props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::UNDEFINED);
        if direction == Direction::UNDEFINED {
            let d = lg.get_direction(lgraph);
            lg[lgraph].props.set(&LayeredOptions::DIRECTION, d);
        }

        let random_seed: i64 = if let Some(i) = lg[lgraph].props.get_as::<i64>(&LayeredOptions::RANDOM_SEED) {
            i
        } else if let Some(d) = lg[lgraph].props.get_as::<f64>(&LayeredOptions::RANDOM_SEED) {
            d as i64
        } else {
            1
        };
        let random = if random_seed == 0 { Random::new_unseeded() } else { Random::with_seed(random_seed) };
        lg[lgraph].props.set(&InternalProperties::RANDOM, Rc::new(RefCell::new(random)));

        let favor_straightness = lg[lgraph].props.get_as::<bool>(&LayeredOptions::NODE_PLACEMENT_FAVOR_STRAIGHT_EDGES);
        if favor_straightness.is_none() {
            let edge_routing = Self::resolve_edge_routing(lg, lgraph);
            lg[lgraph].props.set(&LayeredOptions::NODE_PLACEMENT_FAVOR_STRAIGHT_EDGES, edge_routing == EdgeRouting::ORTHOGONAL);
        }

        self.copy_port_contraints(lg, lgraph);

        let spacings = Spacings::new(lg, lgraph);
        lg[lgraph].props.set(&InternalProperties::SPACINGS, PropValue::object(Rc::new(spacings)));
    }

    /// `copyPortContraints(_:)` (sic).
    pub fn copy_port_contraints(&mut self, lg: &mut LGraphArena, lgraph: LGraphId) {
        for lnode in lg[lgraph].layerless_nodes.clone() {
            self.copy_port_constraints(lg, lnode);
        }
        for layer in lg[lgraph].layers.clone() {
            for lnode in lg[layer].nodes.clone() {
                self.copy_port_constraints(lg, lnode);
            }
        }
    }

    pub fn copy_port_constraints(&mut self, lg: &mut LGraphArena, node: LNodeId) {
        let original = lg[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::FREE);
        lg[node].props.set(&InternalProperties::ORIGINAL_PORT_CONSTRAINTS, original);
        if let Some(nested) = lg[node].nested_graph {
            self.copy_port_contraints(lg, nested);
        }
    }

    /// `prepareGraphForLayout(_:)`: sets `PROCESSORS` on the graph.
    pub fn prepare_graph_for_layout(&mut self, lg: &mut LGraphArena, lgraph: LGraphId) {
        self.configure_graph_properties(lg, lgraph);
        self.algorithm_assembler.reset();

        let props = &lg[lgraph].props;
        let cb: CycleBreakingStrategy = props.get_typed(&LayeredOptions::CYCLE_BREAKING_STRATEGY).unwrap_or(CycleBreakingStrategy::GREEDY);
        let ls: LayeringStrategy = props.get_typed(&LayeredOptions::LAYERING_STRATEGY).unwrap_or(LayeringStrategy::NETWORK_SIMPLEX);
        let cm: CrossingMinimizationStrategy = props.get_typed(&LayeredOptions::CROSSING_MINIMIZATION_STRATEGY).unwrap_or(CrossingMinimizationStrategy::LAYER_SWEEP);
        let np: NodePlacementStrategy = props.get_typed(&LayeredOptions::NODE_PLACEMENT_STRATEGY).unwrap_or(NodePlacementStrategy::BRANDES_KOEPF);
        self.algorithm_assembler.set_phase(LayeredPhases::P1_CYCLE_BREAKING, PhaseFactory::CycleBreaking(cb));
        self.algorithm_assembler.set_phase(LayeredPhases::P2_LAYERING, PhaseFactory::Layering(ls));
        self.algorithm_assembler.set_phase(LayeredPhases::P3_NODE_ORDERING, PhaseFactory::CrossingMinimization(cm));
        self.algorithm_assembler.set_phase(LayeredPhases::P4_NODE_PLACEMENT, PhaseFactory::NodePlacement(np));
        let edge_routing = Self::resolve_edge_routing(lg, lgraph);
        self.algorithm_assembler.set_phase(LayeredPhases::P5_EDGE_ROUTING, PhaseFactory::EdgeRouting(EdgeRouterFactory::factory_for(edge_routing)));

        if let Some(config) = self.get_phase_independent_layout_processor_configuration(lg, lgraph) {
            self.algorithm_assembler.add_processor_configuration(&config);
        }

        let processors = self.algorithm_assembler.build(lg, lgraph);
        let list: ProcessorList = Rc::new(RefCell::new(processors));
        lg[lgraph].props.set(&InternalProperties::PROCESSORS, PropValue::object(list));
    }

    /// `getPhaseIndependentLayoutProcessorConfiguration(_:)`.
    pub fn get_phase_independent_layout_processor_configuration(&self, lg: &LGraphArena, lgraph: LGraphId) -> Option<LayoutProcessorConfiguration> {
        let props = &lg[lgraph].props;
        let graph_properties = props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES).unwrap_or_default();

        let mut configuration = LayoutProcessorConfiguration::create_from(&baseline_processing_configuration());

        let hierarchy_handling = props.get_as::<HierarchyHandling>(&LayeredOptions::HIERARCHY_HANDLING).unwrap_or(HierarchyHandling::INHERIT);
        if hierarchy_handling == HierarchyHandling::INCLUDE_CHILDREN {
            configuration.add_all(&hierarchical_additions());
        }

        if props.get_as::<bool>(&LayeredOptions::FEEDBACK_EDGES).unwrap_or(false) {
            configuration.add_before(LayeredPhases::P1_CYCLE_BREAKING, IPS::PORT_SIDE_PROCESSOR);
        } else {
            configuration.add_before(LayeredPhases::P3_NODE_ORDERING, IPS::PORT_SIDE_PROCESSOR);
        }

        if props.get(&LabelManagementOptions::LABEL_MANAGER).is_some() {
            configuration.add_all(&label_management_additions());
        }

        if props.get_as::<bool>(&LayeredOptions::INTERACTIVE_LAYOUT).unwrap_or(false)
            || props.get_as::<bool>(&LayeredOptions::GENERATE_POSITION_AND_LAYER_IDS).unwrap_or(false)
        {
            configuration.add_after(LayeredPhases::P5_EDGE_ROUTING, IPS::CONSTRAINTS_POSTPROCESSOR);
        }

        let direction = props.get_as::<Direction>(&LayeredOptions::DIRECTION).unwrap_or(Direction::RIGHT);
        match direction {
            Direction::LEFT | Direction::DOWN | Direction::UP => {
                configuration
                    .add_before(LayeredPhases::P1_CYCLE_BREAKING, IPS::DIRECTION_PREPROCESSOR)
                    .add_after(LayeredPhases::P5_EDGE_ROUTING, IPS::DIRECTION_POSTPROCESSOR);
            }
            _ => {}
        }

        if graph_properties.contains(GraphProperties::COMMENTS) {
            configuration
                .add_before(LayeredPhases::P1_CYCLE_BREAKING, IPS::COMMENT_PREPROCESSOR)
                .add_before(LayeredPhases::P4_NODE_PLACEMENT, IPS::COMMENT_NODE_MARGIN_CALCULATOR)
                .add_after(LayeredPhases::P5_EDGE_ROUTING, IPS::COMMENT_POSTPROCESSOR);
        }

        let node_promotion = props.get_as::<NodePromotionStrategy>(&LayeredOptions::LAYERING_NODE_PROMOTION_STRATEGY).unwrap_or(NodePromotionStrategy::NONE);
        if node_promotion != NodePromotionStrategy::NONE {
            configuration.add_before(LayeredPhases::P3_NODE_ORDERING, IPS::NODE_PROMOTION);
        }

        if graph_properties.contains(GraphProperties::PARTITIONS) {
            configuration.add_before(LayeredPhases::P1_CYCLE_BREAKING, IPS::PARTITION_PREPROCESSOR);
            configuration.add_before(LayeredPhases::P2_LAYERING, IPS::PARTITION_MIDPROCESSOR);
            configuration.add_before(LayeredPhases::P3_NODE_ORDERING, IPS::PARTITION_POSTPROCESSOR);
        }

        let compaction = props.get_as::<GraphCompactionStrategy>(&LayeredOptions::COMPACTION_POST_COMPACTION_STRATEGY).unwrap_or(GraphCompactionStrategy::NONE);
        let edge_routing = Self::resolve_edge_routing(lg, lgraph);
        if compaction != GraphCompactionStrategy::NONE && edge_routing != EdgeRouting::POLYLINE {
            configuration.add_after(LayeredPhases::P5_EDGE_ROUTING, IPS::HORIZONTAL_COMPACTOR);
        }

        if props.get_as::<bool>(&LayeredOptions::HIGH_DEGREE_NODES_TREATMENT).unwrap_or(false) {
            configuration.add_before(LayeredPhases::P3_NODE_ORDERING, IPS::HIGH_DEGREE_NODE_LAYER_PROCESSOR);
        }

        if props.get_as::<bool>(&LayeredOptions::CROSSING_MINIMIZATION_SEMI_INTERACTIVE).unwrap_or(false) {
            configuration.add_before(LayeredPhases::P3_NODE_ORDERING, IPS::SEMI_INTERACTIVE_CROSSMIN_PROCESSOR);
        }

        if Self::activate_greedy_switch_for(lg, lgraph) {
            let greedy_switch_type = if Self::is_hierarchical_layout(lg, lgraph) {
                props.get_as::<GreedySwitchType>(&LayeredOptions::CROSSING_MINIMIZATION_GREEDY_SWITCH_HIERARCHICAL_TYPE).unwrap_or(GreedySwitchType::OFF)
            } else {
                props.get_as::<GreedySwitchType>(&LayeredOptions::CROSSING_MINIMIZATION_GREEDY_SWITCH_TYPE).unwrap_or(GreedySwitchType::TWO_SIDED)
            };
            let internal = if greedy_switch_type == GreedySwitchType::ONE_SIDED { IPS::ONE_SIDED_GREEDY_SWITCH } else { IPS::TWO_SIDED_GREEDY_SWITCH };
            configuration.add_before(LayeredPhases::P4_NODE_PLACEMENT, internal);
        }

        // `LAYER_UNZIPPING_STRATEGY` and `WRAPPING_STRATEGY` are read `as? String`:
        // the importer never stores these as strings with the recognised
        // values ("ALTERNATING", "SINGLE_EDGE", "MULTI_EDGE" become other types
        // or stay unrecognised), so only a literal string matches.
        match props.get_as::<String>(&LayeredOptions::LAYER_UNZIPPING_STRATEGY).as_deref() {
            Some("ALTERNATING") => {
                configuration.add_before(LayeredPhases::P4_NODE_PLACEMENT, IPS::ALTERNATING_LAYER_UNZIPPER);
            }
            _ => {}
        }
        match props.get_as::<String>(&LayeredOptions::WRAPPING_STRATEGY).as_deref() {
            Some("SINGLE_EDGE") => {
                configuration.add_before(LayeredPhases::P4_NODE_PLACEMENT, IPS::SINGLE_EDGE_GRAPH_WRAPPER);
            }
            Some("MULTI_EDGE") => {
                configuration
                    .add_before(LayeredPhases::P3_NODE_ORDERING, IPS::BREAKING_POINT_INSERTER)
                    .add_before(LayeredPhases::P4_NODE_PLACEMENT, IPS::BREAKING_POINT_PROCESSOR)
                    .add_after(LayeredPhases::P5_EDGE_ROUTING, IPS::BREAKING_POINT_REMOVER);
            }
            _ => {}
        }

        if props.get_as::<OrderingStrategy>(&LayeredOptions::CONSIDER_MODEL_ORDER_STRATEGY).unwrap_or(OrderingStrategy::NONE) != OrderingStrategy::NONE {
            configuration.add_before(LayeredPhases::P3_NODE_ORDERING, IPS::SORT_BY_INPUT_ORDER_OF_MODEL);
        }

        Some(configuration)
    }

    /// `activateGreedySwitchFor(_:)`.
    pub fn activate_greedy_switch_for(lg: &LGraphArena, lgraph: LGraphId) -> bool {
        let props = &lg[lgraph].props;
        if Self::is_hierarchical_layout(lg, lgraph) {
            return lg[lgraph].parent_node.is_none()
                && props.get_as::<GreedySwitchType>(&LayeredOptions::CROSSING_MINIMIZATION_GREEDY_SWITCH_HIERARCHICAL_TYPE) != Some(GreedySwitchType::OFF);
        }
        let greedy_switch_type = props.get_as::<GreedySwitchType>(&LayeredOptions::CROSSING_MINIMIZATION_GREEDY_SWITCH_TYPE).unwrap_or(GreedySwitchType::TWO_SIDED);
        let interactive_cross_min = props.get_as::<bool>(&LayeredOptions::CROSSING_MINIMIZATION_SEMI_INTERACTIVE).unwrap_or(false)
            || props.get_as::<CrossingMinimizationStrategy>(&LayeredOptions::CROSSING_MINIMIZATION_STRATEGY) == Some(CrossingMinimizationStrategy::INTERACTIVE);
        let activation_threshold = props.get_as::<i64>(&LayeredOptions::CROSSING_MINIMIZATION_GREEDY_SWITCH_ACTIVATION_THRESHOLD).unwrap_or(0);
        let graph_size = lg[lgraph].layerless_nodes.len() as i64;
        !interactive_cross_min && greedy_switch_type != GreedySwitchType::OFF && (activation_threshold == 0 || activation_threshold > graph_size)
    }

    pub fn is_hierarchical_layout(lg: &LGraphArena, lgraph: LGraphId) -> bool {
        lg[lgraph].props.get_as::<HierarchyHandling>(&LayeredOptions::HIERARCHY_HANDLING) == Some(HierarchyHandling::INCLUDE_CHILDREN)
    }

    /// `resolveEdgeRouting(_:)`: an `EdgeRouting` value, else a raw string
    /// naming one, else `ORTHOGONAL`.
    pub fn resolve_edge_routing(lg: &LGraphArena, lgraph: LGraphId) -> EdgeRouting {
        if let Some(er) = lg[lgraph].props.get_as::<EdgeRouting>(&LayeredOptions::EDGE_ROUTING) {
            return er;
        }
        if let Some(s) = lg[lgraph].props.get_as::<String>(&LayeredOptions::EDGE_ROUTING) {
            if let Some(er) = EdgeRouting::from_raw(&s) {
                return er;
            }
        }
        EdgeRouting::ORTHOGONAL
    }
}
