//! Port of `alg/layered/intermediate/IntermediateProcessorStrategy.swift`.

use super::compaction::horizontal_graph_compactor::HorizontalGraphCompactor;
use super::graph_transformer::{GraphTransformer, Mode};
use super::unzipping::alternating_layer_unzipper::AlternatingLayerUnzipper;
use super::wrapping::{
    breaking_point_inserter::BreakingPointInserter, breaking_point_processor::BreakingPointProcessor,
    breaking_point_remover::BreakingPointRemover, single_edge_graph_wrapper::SingleEdgeGraphWrapper,
};
use super::*;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::{CrossMinType, LayerSweepCrossingMinimizer};
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

/// The intermediate processors, in declaration order (the order they run in
/// when several share a slot).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum IntermediateProcessorStrategy {
    DIRECTION_PREPROCESSOR,
    COMMENT_PREPROCESSOR,
    EDGE_AND_LAYER_CONSTRAINT_EDGE_REVERSER,
    INTERACTIVE_EXTERNAL_PORT_POSITIONER,
    PARTITION_PREPROCESSOR,
    LABEL_DUMMY_INSERTER,
    SELF_LOOP_PREPROCESSOR,
    LAYER_CONSTRAINT_PREPROCESSOR,
    PARTITION_MIDPROCESSOR,
    HIGH_DEGREE_NODE_LAYER_PROCESSOR,
    NODE_PROMOTION,
    LAYER_CONSTRAINT_POSTPROCESSOR,
    PARTITION_POSTPROCESSOR,
    HIERARCHICAL_PORT_CONSTRAINT_PROCESSOR,
    SEMI_INTERACTIVE_CROSSMIN_PROCESSOR,
    BREAKING_POINT_INSERTER,
    LONG_EDGE_SPLITTER,
    PORT_SIDE_PROCESSOR,
    INVERTED_PORT_PROCESSOR,
    PORT_LIST_SORTER,
    SORT_BY_INPUT_ORDER_OF_MODEL,
    NORTH_SOUTH_PORT_PREPROCESSOR,
    BREAKING_POINT_PROCESSOR,
    ONE_SIDED_GREEDY_SWITCH,
    TWO_SIDED_GREEDY_SWITCH,
    SELF_LOOP_PORT_RESTORER,
    ALTERNATING_LAYER_UNZIPPER,
    SINGLE_EDGE_GRAPH_WRAPPER,
    IN_LAYER_CONSTRAINT_PROCESSOR,
    END_NODE_PORT_LABEL_MANAGEMENT_PROCESSOR,
    LABEL_AND_NODE_SIZE_PROCESSOR,
    INNERMOST_NODE_MARGIN_CALCULATOR,
    SELF_LOOP_ROUTER,
    COMMENT_NODE_MARGIN_CALCULATOR,
    END_LABEL_PREPROCESSOR,
    LABEL_DUMMY_SWITCHER,
    CENTER_LABEL_MANAGEMENT_PROCESSOR,
    LABEL_SIDE_SELECTOR,
    HYPEREDGE_DUMMY_MERGER,
    HIERARCHICAL_PORT_DUMMY_SIZE_PROCESSOR,
    LAYER_SIZE_AND_GRAPH_HEIGHT_CALCULATOR,
    HIERARCHICAL_PORT_POSITION_PROCESSOR,
    CONSTRAINTS_POSTPROCESSOR,
    COMMENT_POSTPROCESSOR,
    HYPERNODE_PROCESSOR,
    HIERARCHICAL_PORT_ORTHOGONAL_EDGE_ROUTER,
    LONG_EDGE_JOINER,
    SELF_LOOP_POSTPROCESSOR,
    BREAKING_POINT_REMOVER,
    NORTH_SOUTH_PORT_POSTPROCESSOR,
    HORIZONTAL_COMPACTOR,
    LABEL_DUMMY_REMOVER,
    FINAL_SPLINE_BENDPOINTS_CALCULATOR,
    END_LABEL_SORTER,
    REVERSED_EDGE_RESTORER,
    END_LABEL_POSTPROCESSOR,
    HIERARCHICAL_NODE_RESIZER,
    DIRECTION_POSTPROCESSOR,
}

/// `_NoOpProcessor` (spline routing is not supported).
struct NoOpProcessor;

impl ILayoutProcessor for NoOpProcessor {
    fn process(&mut self, _lg: &mut LGraphArena, _graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {}

    fn name(&self) -> &'static str {
        "NoOpProcessor"
    }
}

impl IntermediateProcessorStrategy {
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn create(self) -> Box<dyn ILayoutProcessor> {
        use IntermediateProcessorStrategy::*;
        match self {
            BREAKING_POINT_INSERTER => Box::new(BreakingPointInserter::new()),
            BREAKING_POINT_PROCESSOR => Box::new(BreakingPointProcessor::new()),
            BREAKING_POINT_REMOVER => Box::new(BreakingPointRemover::new()),
            CENTER_LABEL_MANAGEMENT_PROCESSOR => Box::new(label_management_processor::LabelManagementProcessor::new(true)),
            COMMENT_NODE_MARGIN_CALCULATOR => Box::new(comment_node_margin_calculator::CommentNodeMarginCalculator::new()),
            COMMENT_POSTPROCESSOR => Box::new(comment_postprocessor::CommentPostprocessor::new()),
            COMMENT_PREPROCESSOR => Box::new(comment_preprocessor::CommentPreprocessor::new()),
            CONSTRAINTS_POSTPROCESSOR => Box::new(constraints_postprocessor::ConstraintsPostprocessor::new()),
            DIRECTION_POSTPROCESSOR => Box::new(GraphTransformer::new(Mode::TO_INTERNAL_LTR)),
            DIRECTION_PREPROCESSOR => Box::new(GraphTransformer::new(Mode::TO_INPUT_DIRECTION)),
            EDGE_AND_LAYER_CONSTRAINT_EDGE_REVERSER => Box::new(edge_and_layer_constraint_edge_reverser::EdgeAndLayerConstraintEdgeReverser::new()),
            END_LABEL_POSTPROCESSOR => Box::new(end_label_postprocessor::EndLabelPostprocessor::new()),
            END_LABEL_PREPROCESSOR => Box::new(end_label_preprocessor::EndLabelPreprocessor::new()),
            END_NODE_PORT_LABEL_MANAGEMENT_PROCESSOR => Box::new(label_management_processor::LabelManagementProcessor::new(false)),
            FINAL_SPLINE_BENDPOINTS_CALCULATOR => Box::new(NoOpProcessor),
            HIERARCHICAL_NODE_RESIZER => Box::new(hierarchical_node_resizing_processor::HierarchicalNodeResizingProcessor::new()),
            HIERARCHICAL_PORT_CONSTRAINT_PROCESSOR => Box::new(hierarchical_port_constraint_processor::HierarchicalPortConstraintProcessor::new()),
            HIERARCHICAL_PORT_DUMMY_SIZE_PROCESSOR => Box::new(hierarchical_port_dummy_size_processor::HierarchicalPortDummySizeProcessor::new()),
            HIERARCHICAL_PORT_ORTHOGONAL_EDGE_ROUTER => Box::new(hierarchical_port_orthogonal_edge_router::HierarchicalPortOrthogonalEdgeRouter::new()),
            HIERARCHICAL_PORT_POSITION_PROCESSOR => Box::new(hierarchical_port_position_processor::HierarchicalPortPositionProcessor::new()),
            HIGH_DEGREE_NODE_LAYER_PROCESSOR => Box::new(high_degree_node_layering_processor::HighDegreeNodeLayeringProcessor::new()),
            HORIZONTAL_COMPACTOR => Box::new(HorizontalGraphCompactor::new()),
            HYPEREDGE_DUMMY_MERGER => Box::new(hyperedge_dummy_merger::HyperedgeDummyMerger::new()),
            HYPERNODE_PROCESSOR => Box::new(hypernodes_processor::HypernodesProcessor::new()),
            IN_LAYER_CONSTRAINT_PROCESSOR => Box::new(in_layer_constraint_processor::InLayerConstraintProcessor::new()),
            INNERMOST_NODE_MARGIN_CALCULATOR => Box::new(innermost_node_margin_calculator::InnermostNodeMarginCalculator::new()),
            INTERACTIVE_EXTERNAL_PORT_POSITIONER => Box::new(interactive_external_port_positioner::InteractiveExternalPortPositioner::new()),
            INVERTED_PORT_PROCESSOR => Box::new(inverted_port_processor::InvertedPortProcessor::new()),
            LABEL_AND_NODE_SIZE_PROCESSOR => Box::new(label_and_node_size_processor::LabelAndNodeSizeProcessor::new()),
            LABEL_DUMMY_INSERTER => Box::new(label_dummy_inserter::LabelDummyInserter::new()),
            LABEL_DUMMY_REMOVER => Box::new(label_dummy_remover::LabelDummyRemover::new()),
            LABEL_DUMMY_SWITCHER => Box::new(label_dummy_switcher::LabelDummySwitcher::new()),
            LABEL_SIDE_SELECTOR => Box::new(label_side_selector::LabelSideSelector::new()),
            END_LABEL_SORTER => Box::new(end_label_sorter::EndLabelSorter::new()),
            LAYER_CONSTRAINT_POSTPROCESSOR => Box::new(layer_constraint_postprocessor::LayerConstraintPostprocessor::new()),
            LAYER_CONSTRAINT_PREPROCESSOR => Box::new(layer_constraint_preprocessor::LayerConstraintPreprocessor::new()),
            LAYER_SIZE_AND_GRAPH_HEIGHT_CALCULATOR => Box::new(layer_size_and_graph_height_calculator::LayerSizeAndGraphHeightCalculator::new()),
            LONG_EDGE_JOINER => Box::new(long_edge_joiner::LongEdgeJoiner::new()),
            LONG_EDGE_SPLITTER => Box::new(long_edge_splitter::LongEdgeSplitter::new()),
            NODE_PROMOTION => Box::new(node_promotion::NodePromotion::new()),
            NORTH_SOUTH_PORT_POSTPROCESSOR => Box::new(north_south_port_postprocessor::NorthSouthPortPostprocessor::new()),
            NORTH_SOUTH_PORT_PREPROCESSOR => Box::new(north_south_port_preprocessor::NorthSouthPortPreprocessor::new()),
            ONE_SIDED_GREEDY_SWITCH => Box::new(LayerSweepCrossingMinimizer::new(CrossMinType::ONE_SIDED_GREEDY_SWITCH)),
            PARTITION_MIDPROCESSOR => Box::new(partition_midprocessor::PartitionMidprocessor::new()),
            PARTITION_POSTPROCESSOR => Box::new(partition_postprocessor::PartitionPostprocessor::new()),
            PARTITION_PREPROCESSOR => Box::new(partition_preprocessor::PartitionPreprocessor::new()),
            PORT_LIST_SORTER => Box::new(port_list_sorter::PortListSorter::new()),
            PORT_SIDE_PROCESSOR => Box::new(port_side_processor::PortSideProcessor::new()),
            REVERSED_EDGE_RESTORER => Box::new(reversed_edge_restorer::ReversedEdgeRestorer::new()),
            SELF_LOOP_PREPROCESSOR => Box::new(self_loop_pre_processor::SelfLoopPreProcessor::new()),
            SELF_LOOP_PORT_RESTORER => Box::new(self_loop_port_restorer::SelfLoopPortRestorer::new()),
            ALTERNATING_LAYER_UNZIPPER => Box::new(AlternatingLayerUnzipper::new()),
            SELF_LOOP_POSTPROCESSOR => Box::new(self_loop_post_processor::SelfLoopPostProcessor::new()),
            SELF_LOOP_ROUTER => Box::new(self_loop_router::SelfLoopRouter::new()),
            SEMI_INTERACTIVE_CROSSMIN_PROCESSOR => Box::new(semi_interactive_cross_min_processor::SemiInteractiveCrossMinProcessor::new()),
            SINGLE_EDGE_GRAPH_WRAPPER => Box::new(SingleEdgeGraphWrapper::new()),
            SORT_BY_INPUT_ORDER_OF_MODEL => Box::new(sort_by_input_model_processor::SortByInputModelProcessor::new()),
            TWO_SIDED_GREEDY_SWITCH => Box::new(LayerSweepCrossingMinimizer::new(CrossMinType::TWO_SIDED_GREEDY_SWITCH)),
        }
    }
}
