//! Port of `alg/layered/options/LayeredOptions.swift`.
//!
//! Property declarations; defaults are the Swift ones (a `Property` without a
//! default reads as `nil` when unset).

use crate::org::eclipse::elk::graph::properties::keys;
use crate::org::eclipse::elk::graph::properties::property::Property;
use crate::org::eclipse::elk::graph::properties::property::PropValue;
use crate::org::eclipse::elk::alg::layered::options::{self_loop_distribution_strategy::SelfLoopDistributionStrategy, self_loop_ordering_strategy::SelfLoopOrderingStrategy};

pub const ALGORITHM_ID: &str = "org.eclipse.elk.layered";

/// `org.eclipse.elk.layered.layering.layerConstraint` (`Property<Any>`)
pub static LAYERING_LAYER_CONSTRAINT: Property = Property::new(keys::ELK_LAYERED_LAYERING_LAYER_CONSTRAINT);
/// `org.eclipse.elk.edgeLabels.placement` (`Property<Any>`)
pub static EDGE_LABELS_PLACEMENT: Property = Property::new(keys::ELK_EDGE_LABELS_PLACEMENT);
/// `org.eclipse.elk.nodeLabels.placement` (`Property<Any>`)
pub static NODE_LABELS_PLACEMENT: Property = Property::new(keys::ELK_NODE_LABELS_PLACEMENT);
/// `org.eclipse.elk.portConstraints` (`Property<Any>`)
pub static PORT_CONSTRAINTS: Property = Property::new(keys::ELK_PORT_CONSTRAINTS);
/// `org.eclipse.elk.layered.thoroughness` (`Property<Int>`)
pub static THOROUGHNESS: Property = Property::new(keys::ELK_LAYERED_THOROUGHNESS);
/// `org.eclipse.elk.spacing.edgeEdge` (`Property<Double>`)
pub static SPACING_EDGE_EDGE: Property = Property::with_default(keys::ELK_SPACING_EDGE_EDGE, || PropValue::Double(10.0));
/// `org.eclipse.elk.spacing.nodeNode` (`Property<Double>`)
pub static SPACING_NODE_NODE: Property = Property::with_default(keys::ELK_SPACING_NODE_NODE, || PropValue::Double(20.0));
/// `org.eclipse.elk.spacing.portPort` (`Property<Double>`)
pub static SPACING_PORT_PORT: Property = Property::with_default(keys::ELK_SPACING_PORT_PORT, || PropValue::Double(10.0));
/// `org.eclipse.elk.spacing.portsSurrounding` (`Property<Any>`)
pub static SPACING_PORTS_SURROUNDING: Property = Property::new(keys::ELK_SPACING_PORTS_SURROUNDING);
/// `org.eclipse.elk.spacing.edgeNode` (`Property<Double>`)
pub static SPACING_EDGE_NODE: Property = Property::with_default(keys::ELK_SPACING_EDGE_NODE, || PropValue::Double(10.0));
/// `org.eclipse.elk.spacing.edgeLabel` (`Property<Double>`)
pub static SPACING_EDGE_LABEL: Property = Property::with_default(keys::ELK_SPACING_EDGE_LABEL, || PropValue::Double(2.0));
/// `org.eclipse.elk.spacing.labelLabel` (`Property<Double>`)
pub static SPACING_LABEL_LABEL: Property = Property::with_default(keys::ELK_SPACING_LABEL_LABEL, || PropValue::Double(0.0));
/// `org.eclipse.elk.spacing.labelPort` (`Property<Double>`)
pub static SPACING_LABEL_PORT: Property = Property::with_default(keys::ELK_SPACING_LABEL_PORT, || PropValue::Double(5.0));
/// `org.eclipse.elk.spacing.labelNode` (`Property<Double>`)
pub static SPACING_LABEL_NODE: Property = Property::with_default(keys::ELK_SPACING_LABEL_NODE, || PropValue::Double(5.0));
/// `org.eclipse.elk.spacing.labelPortHorizontal` (`Property<Double>`)
pub static SPACING_LABEL_PORT_HORIZONTAL: Property = Property::with_default(keys::ELK_SPACING_LABEL_PORT_HORIZONTAL, || PropValue::Double(1.0));
/// `org.eclipse.elk.spacing.labelPortVertical` (`Property<Double>`)
pub static SPACING_LABEL_PORT_VERTICAL: Property = Property::with_default(keys::ELK_SPACING_LABEL_PORT_VERTICAL, || PropValue::Double(1.0));
/// `org.eclipse.elk.layered.spacing.edgeEdgeBetweenLayers` (`Property<Double>`)
pub static SPACING_EDGE_EDGE_BETWEEN_LAYERS: Property = Property::with_default(keys::ELK_LAYERED_SPACING_EDGE_EDGE_BETWEEN_LAYERS, || PropValue::Double(10.0));
/// `org.eclipse.elk.layered.spacing.edgeNodeBetweenLayers` (`Property<Double>`)
pub static SPACING_EDGE_NODE_BETWEEN_LAYERS: Property = Property::with_default(keys::ELK_LAYERED_SPACING_EDGE_NODE_BETWEEN_LAYERS, || PropValue::Double(10.0));
/// `org.eclipse.elk.layered.spacing.nodeNodeBetweenLayers` (`Property<Double>`)
pub static SPACING_NODE_NODE_BETWEEN_LAYERS: Property = Property::with_default(keys::ELK_LAYERED_SPACING_NODE_NODE_BETWEEN_LAYERS, || PropValue::Double(20.0));
/// `org.eclipse.elk.spacing.commentComment` (`Property<Double>`)
pub static SPACING_COMMENT_COMMENT: Property = Property::with_default(keys::ELK_SPACING_COMMENT_COMMENT, || PropValue::Double(10.0));
/// `org.eclipse.elk.spacing.commentNode` (`Property<Double>`)
pub static SPACING_COMMENT_NODE: Property = Property::with_default(keys::ELK_SPACING_COMMENT_NODE, || PropValue::Double(10.0));
/// `org.eclipse.elk.spacing.componentComponent` (`Property<Double>`)
pub static SPACING_COMPONENT_COMPONENT: Property = Property::with_default(keys::ELK_SPACING_COMPONENT_COMPONENT, || PropValue::Double(20.0));
/// `org.eclipse.elk.spacing.baseValue` (`Property<Double>`)
pub static SPACING_BASE_VALUE: Property = Property::with_default(keys::ELK_SPACING_BASE_VALUE, || PropValue::Double(0.0));
/// `org.eclipse.elk.priority` (`Property<Int>`)
pub static PRIORITY: Property = Property::new(keys::ELK_PRIORITY);
/// `org.eclipse.elk.layered.priority.direction` (`Property<Int>`)
pub static PRIORITY_DIRECTION: Property = Property::new(keys::ELK_LAYERED_PRIORITY_DIRECTION);
/// `org.eclipse.elk.layered.priority.shortness` (`Property<Int>`)
pub static PRIORITY_SHORTNESS: Property = Property::new(keys::ELK_LAYERED_PRIORITY_SHORTNESS);
/// `org.eclipse.elk.layered.priority.straightness` (`Property<Int>`)
pub static PRIORITY_STRAIGHTNESS: Property = Property::new(keys::ELK_LAYERED_PRIORITY_STRAIGHTNESS);
/// `org.eclipse.elk.layered.interactiveReferencePoint` (`Property<Any>`)
pub static INTERACTIVE_REFERENCE_POINT: Property = Property::new(keys::ELK_LAYERED_INTERACTIVE_REFERENCE_POINT);
/// `org.eclipse.elk.layered.nodePlacement.favorStraightEdges` (`Property<Bool>`)
pub static NODE_PLACEMENT_FAVOR_STRAIGHT_EDGES: Property = Property::new(keys::ELK_LAYERED_NODE_PLACEMENT_FAVOR_STRAIGHT_EDGES);
/// `org.eclipse.elk.layered.nodePlacement.bk.edgeStraightening` (`Property<Any>`)
pub static NODE_PLACEMENT_BK_EDGE_STRAIGHTENING: Property = Property::new(keys::ELK_LAYERED_NODE_PLACEMENT_BK_EDGE_STRAIGHTENING);
/// `org.eclipse.elk.layered.nodePlacement.bk.fixedAlignment` (`Property<Any>`)
pub static NODE_PLACEMENT_BK_FIXED_ALIGNMENT: Property = Property::new(keys::ELK_LAYERED_NODE_PLACEMENT_BK_FIXED_ALIGNMENT);
/// `org.eclipse.elk.layered.nodePlacement.linearSegments.deflectionDampening` (`Property<Double>`)
pub static NODE_PLACEMENT_LINEAR_SEGMENTS_DEFLECTION_DAMPENING: Property = Property::new(keys::ELK_LAYERED_NODE_PLACEMENT_LINEAR_SEGMENTS_DEFLECTION_DAMPENING);
/// `org.eclipse.elk.layered.nodePlacement.strategy` (`Property<Any>`)
pub static NODE_PLACEMENT_STRATEGY: Property = Property::new(keys::ELK_LAYERED_NODE_PLACEMENT_STRATEGY);
/// `org.eclipse.elk.layered.considerModelOrder.groupModelOrder.cycleBreakingId` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CYCLE_BREAKING_ID: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CYCLE_BREAKING_ID);
/// `org.eclipse.elk.layered.considerModelOrder.groupModelOrder.crossingMinimizationId` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CROSSING_MINIMIZATION_ID: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CROSSING_MINIMIZATION_ID);
/// `org.eclipse.elk.layered.considerModelOrder.groupModelOrder.componentGroupId` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_COMPONENT_GROUP_ID: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_COMPONENT_GROUP_ID);
/// `org.eclipse.elk.layered.considerModelOrder.groupModelOrder.cbGroupOrderStrategy` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CB_GROUP_ORDER_STRATEGY: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CB_GROUP_ORDER_STRATEGY);
/// `org.eclipse.elk.layered.considerModelOrder.groupModelOrder.cmGroupOrderStrategy` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CM_GROUP_ORDER_STRATEGY: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CM_GROUP_ORDER_STRATEGY);
/// `org.eclipse.elk.layered.considerModelOrder.groupModelOrder.cmEnforcedGroupOrders` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CM_ENFORCED_GROUP_ORDERS: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CM_ENFORCED_GROUP_ORDERS);
/// `org.eclipse.elk.layered.considerModelOrder.groupModelOrder.cbPreferredSourceId` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CB_PREFERRED_SOURCE_ID: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CB_PREFERRED_SOURCE_ID);
/// `org.eclipse.elk.layered.considerModelOrder.groupModelOrder.cbPreferredTargetId` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CB_PREFERRED_TARGET_ID: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_GROUP_MODEL_ORDER_CB_PREFERRED_TARGET_ID);
/// `org.eclipse.elk.direction` (`Property<Direction>`)
pub static DIRECTION: Property = Property::new(keys::ELK_DIRECTION);
/// `org.eclipse.elk.layered.layering.layerId` (`Property<Int>`)
pub static LAYERING_LAYER_ID: Property = Property::new(keys::ELK_LAYERED_LAYERING_LAYER_ID);
/// `org.eclipse.elk.layered.crossingMinimization.inLayerPredOf` (`Property<Any>`)
pub static CROSSING_MINIMIZATION_IN_LAYER_PRED_OF: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_IN_LAYER_PRED_OF);
/// `org.eclipse.elk.layered.crossingMinimization.inLayerSuccOf` (`Property<Any>`)
pub static CROSSING_MINIMIZATION_IN_LAYER_SUCC_OF: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_IN_LAYER_SUCC_OF);
/// `org.eclipse.elk.nodeSize.constraints` (`Property<Any>`)
pub static NODE_SIZE_CONSTRAINTS: Property = Property::new(keys::ELK_NODE_SIZE_CONSTRAINTS);
/// `org.eclipse.elk.nodeSize.options` (`Property<Any>`)
pub static NODE_SIZE_OPTIONS: Property = Property::new(keys::ELK_NODE_SIZE_OPTIONS);
/// `org.eclipse.elk.nodeSize.minimum` (`Property<Any>`)
pub static NODE_SIZE_MINIMUM: Property = Property::new(keys::ELK_NODE_SIZE_MINIMUM);
/// `org.eclipse.elk.nodeSize.fixedGraphSize` (`Property<Bool>`)
pub static NODE_SIZE_FIXED_GRAPH_SIZE: Property = Property::new(keys::ELK_NODE_SIZE_FIXED_GRAPH_SIZE);
/// `org.eclipse.elk.noLayout` (`Property<Bool>`)
pub static NO_LAYOUT: Property = Property::new(keys::ELK_NO_LAYOUT);
/// `org.eclipse.elk.layered.layering.layerChoiceConstraint` (`Property<Int>`)
pub static LAYERING_LAYER_CHOICE_CONSTRAINT: Property = Property::new(keys::ELK_LAYERED_LAYERING_LAYER_CHOICE_CONSTRAINT);
/// `org.eclipse.elk.portLabels.placement` (`Property<Any>`)
pub static PORT_LABELS_PLACEMENT: Property = Property::new(keys::ELK_PORT_LABELS_PLACEMENT);
/// `org.eclipse.elk.portLabels.nextToPortIfPossible` (`Property<Bool>`)
pub static PORT_LABELS_NEXT_TO_PORT_IF_POSSIBLE: Property = Property::new(keys::ELK_PORT_LABELS_NEXT_TO_PORT_IF_POSSIBLE);
/// `org.eclipse.elk.portAlignment.default` (`Property<Any>`)
pub static PORT_ALIGNMENT_DEFAULT: Property = Property::new(keys::ELK_PORT_ALIGNMENT_DEFAULT);
/// `org.eclipse.elk.port.side` (`Property<Any>`)
pub static PORT_SIDE: Property = Property::new(keys::ELK_PORT_SIDE);
/// `org.eclipse.elk.port.borderOffset` (`Property<Double>`)
pub static PORT_BORDER_OFFSET: Property = Property::new(keys::ELK_PORT_BORDER_OFFSET);
/// `org.eclipse.elk.port.anchor` (`Property<KVector>`)
pub static PORT_ANCHOR: Property = Property::new(keys::ELK_PORT_ANCHOR);
/// `org.eclipse.elk.port.index` (`Property<Int>`)
pub static PORT_INDEX: Property = Property::new(keys::ELK_PORT_INDEX);
/// `org.eclipse.elk.padding` (`Property<Any>`)
pub static PADDING: Property = Property::new(keys::ELK_PADDING);
/// `org.eclipse.elk.alignment` (`Property<Any>`)
pub static ALIGNMENT: Property = Property::new(keys::ELK_ALIGNMENT);
/// `org.eclipse.elk.nodeLabels.padding` (`Property<Any>`)
pub static NODE_LABELS_PADDING: Property = Property::new(keys::ELK_NODE_LABELS_PADDING);
/// `org.eclipse.elk.aspectRatio` (`Property<Double>`)
pub static ASPECT_RATIO: Property = Property::new(keys::ELK_ASPECT_RATIO);
/// `org.eclipse.elk.position` (`Property<Any>`)
pub static POSITION: Property = Property::new(keys::ELK_POSITION);
/// `org.eclipse.elk.position` (`Property<Any>`)
pub static DESIRED_POSITION: Property = Property::new(keys::ELK_POSITION);
/// `org.eclipse.elk.insideSelfLoops.activate` (`Property<Bool>`)
pub static INSIDE_SELF_LOOPS_ACTIVATE: Property = Property::new(keys::ELK_INSIDE_SELF_LOOPS_ACTIVATE);
/// `org.eclipse.elk.insideSelfLoops.yo` (`Property<Bool>`)
pub static INSIDE_SELF_LOOPS_YO: Property = Property::new(keys::ELK_INSIDE_SELF_LOOPS_YO);
/// `org.eclipse.elk.separateConnectedComponents` (`Property<Bool>`)
pub static SEPARATE_CONNECTED_COMPONENTS: Property = Property::new(keys::ELK_SEPARATE_CONNECTED_COMPONENTS);
/// `org.eclipse.elk.contentAlignment` (`Property<Any>`)
pub static CONTENT_ALIGNMENT: Property = Property::new(keys::ELK_CONTENT_ALIGNMENT);
/// `org.eclipse.elk.edgeRouting` (`Property<Any>`)
pub static EDGE_ROUTING: Property = Property::new(keys::ELK_EDGE_ROUTING);
/// `org.eclipse.elk.edgeRouting.splines.mode` (`Property<Any>`)
pub static EDGE_ROUTING_SPLINES_MODE: Property = Property::new(keys::ELK_EDGE_ROUTING_SPLINES_MODE);
/// `org.eclipse.elk.layered.edgeRouting.selfLoopDistribution` (`Property<SelfLoopDistributionStrategy>`)
pub static EDGE_ROUTING_SELF_LOOP_DISTRIBUTION: Property = Property::with_default(keys::ELK_LAYERED_EDGE_ROUTING_SELF_LOOP_DISTRIBUTION, || PropValue::SelfLoopDistributionStrategy(SelfLoopDistributionStrategy::NORTH));
/// `org.eclipse.elk.layered.edgeRouting.selfLoopOrdering` (`Property<SelfLoopOrderingStrategy>`)
pub static EDGE_ROUTING_SELF_LOOP_ORDERING: Property = Property::with_default(keys::ELK_LAYERED_EDGE_ROUTING_SELF_LOOP_ORDERING, || PropValue::SelfLoopOrderingStrategy(SelfLoopOrderingStrategy::STACKED));
/// `org.eclipse.elk.layered.spacing.nodeSelfLoop` (`Property<Double>`)
pub static SPACING_NODE_SELF_LOOP: Property = Property::with_default(keys::ELK_LAYERED_SPACING_NODE_SELF_LOOP, || PropValue::Double(10.0));
/// `org.eclipse.elk.layered.edgeRouting.polyline.slopedEdgeZoneWidth` (`Property<Double>`)
pub static EDGE_ROUTING_POLYLINE_SLOPED_EDGE_ZONE_WIDTH: Property = Property::new(keys::ELK_LAYERED_EDGE_ROUTING_POLYLINE_SLOPED_EDGE_ZONE_WIDTH);
/// `org.eclipse.elk.edge.thickness` (`Property<Double>`)
pub static EDGE_THICKNESS: Property = Property::with_default(keys::ELK_EDGE_THICKNESS, || PropValue::Double(1.0));
/// `org.eclipse.elk.junctionPoints` (`Property<KVectorChain>`)
pub static JUNCTION_POINTS: Property = Property::new(keys::ELK_JUNCTION_POINTS);
/// `org.eclipse.elk.commentBox` (`Property<Bool>`)
pub static COMMENT_BOX: Property = Property::new(keys::ELK_COMMENT_BOX);
/// `org.eclipse.elk.hypernode` (`Property<Bool>`)
pub static HYPERNODE: Property = Property::new(keys::ELK_HYPERNODE);
/// `org.eclipse.elk.hierarchyHandling` (`Property<Any>`)
pub static HIERARCHY_HANDLING: Property = Property::new(keys::ELK_HIERARCHY_HANDLING);
/// `org.eclipse.elk.interactive` (`Property<Bool>`)
pub static INTERACTIVE_LAYOUT: Property = Property::new(keys::ELK_INTERACTIVE);
/// `org.eclipse.elk.layered.edgeLabels.sideSelection` (`Property<Any>`)
pub static EDGE_LABELS_SIDE_SELECTION: Property = Property::new(keys::ELK_LAYERED_EDGE_LABELS_SIDE_SELECTION);
/// `org.eclipse.elk.edgeLabels.inline` (`Property<Bool>`)
pub static EDGE_LABELS_INLINE: Property = Property::new(keys::ELK_EDGE_LABELS_INLINE);
/// `org.eclipse.elk.layered.unnecessaryBendpoints` (`Property<Bool>`)
pub static UNNECESSARY_BENDPOINTS: Property = Property::new(keys::ELK_LAYERED_UNNECESSARY_BENDPOINTS);
/// `org.eclipse.elk.layered.directionCongruency` (`Property<Any>`)
pub static DIRECTION_CONGRUENCY: Property = Property::new(keys::ELK_LAYERED_DIRECTION_CONGRUENCY);
/// `org.eclipse.elk.layered.feedbackEdges` (`Property<Bool>`)
pub static FEEDBACK_EDGES: Property = Property::new(keys::ELK_LAYERED_FEEDBACK_EDGES);
/// `org.eclipse.elk.layered.mergeEdges` (`Property<Bool>`)
pub static MERGE_EDGES: Property = Property::new(keys::ELK_LAYERED_MERGE_EDGES);
/// `org.eclipse.elk.layered.mergeHierarchyEdges` (`Property<Bool>`)
pub static MERGE_HIERARCHY_EDGES: Property = Property::new(keys::ELK_LAYERED_MERGE_HIERARCHY_EDGES);
/// `org.eclipse.elk.randomSeed` (`Property<Int>`)
pub static RANDOM_SEED: Property = Property::new(keys::ELK_RANDOM_SEED);
/// `org.eclipse.elk.layered.minWidth` (`Property<Double>`)
pub static MIN_WIDTH: Property = Property::new(keys::ELK_LAYERED_MIN_WIDTH);
/// `org.eclipse.elk.layered.minHeight` (`Property<Double>`)
pub static MIN_HEIGHT: Property = Property::new(keys::ELK_LAYERED_MIN_HEIGHT);
/// `org.eclipse.elk.layered.crossingMinimization.strategy` (`Property<Any>`)
pub static CROSSING_MINIMIZATION_STRATEGY: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_STRATEGY);
/// `org.eclipse.elk.layered.crossingMinimization.greedySwitchActivationThreshold` (`Property<Int>`)
pub static CROSSING_MINIMIZATION_GREEDY_SWITCH_ACTIVATION_THRESHOLD: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_GREEDY_SWITCH_ACTIVATION_THRESHOLD);
/// `org.eclipse.elk.layered.crossingMinimization.greedySwitchType` (`Property<Any>`)
pub static CROSSING_MINIMIZATION_GREEDY_SWITCH_TYPE: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_GREEDY_SWITCH_TYPE);
/// `org.eclipse.elk.layered.crossingMinimization.greedySwitchHierarchicalType` (`Property<Any>`)
pub static CROSSING_MINIMIZATION_GREEDY_SWITCH_HIERARCHICAL_TYPE: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_GREEDY_SWITCH_HIERARCHICAL_TYPE);
/// `org.eclipse.elk.layered.crossingMinimization.semiInteractive` (`Property<Bool>`)
pub static CROSSING_MINIMIZATION_SEMI_INTERACTIVE: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_SEMI_INTERACTIVE);
/// `org.eclipse.elk.layered.crossingMinimization.forceNodeModelOrder` (`Property<Bool>`)
pub static CROSSING_MINIMIZATION_FORCE_NODE_MODEL_ORDER: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_FORCE_NODE_MODEL_ORDER);
/// `org.eclipse.elk.layered.crossingMinimization.hierarchicalSweepiness` (`Property<Double>`)
pub static CROSSING_MINIMIZATION_HIERARCHICAL_SWEEPINESS: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_HIERARCHICAL_SWEEPINESS);
/// `org.eclipse.elk.layered.crossingMinimization.positionChoiceConstraint` (`Property<Int>`)
pub static CROSSING_MINIMIZATION_POSITION_CHOICE_CONSTRAINT: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_POSITION_CHOICE_CONSTRAINT);
/// `org.eclipse.elk.layered.crossingMinimization.positionId` (`Property<Int>`)
pub static CROSSING_MINIMIZATION_POSITION_ID: Property = Property::new(keys::ELK_LAYERED_CROSSING_MINIMIZATION_POSITION_ID);
/// `org.eclipse.elk.layered.considerModelOrder.strategy` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_STRATEGY: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_STRATEGY);
/// `org.eclipse.elk.layered.considerModelOrder.crossingCounterNodeInfluence` (`Property<Double>`)
pub static CONSIDER_MODEL_ORDER_CROSSING_COUNTER_NODE_INFLUENCE: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_CROSSING_COUNTER_NODE_INFLUENCE);
/// `org.eclipse.elk.layered.considerModelOrder.crossingCounterPortInfluence` (`Property<Double>`)
pub static CONSIDER_MODEL_ORDER_CROSSING_COUNTER_PORT_INFLUENCE: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_CROSSING_COUNTER_PORT_INFLUENCE);
/// `org.eclipse.elk.layered.considerModelOrder.noModelOrder` (`Property<Bool>`)
pub static CONSIDER_MODEL_ORDER_NO_MODEL_ORDER: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_NO_MODEL_ORDER);
/// `org.eclipse.elk.layered.considerModelOrder.components` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_COMPONENTS: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_COMPONENTS);
/// `org.eclipse.elk.layered.considerModelOrder.longEdgeStrategy` (`Property<Any>`)
pub static CONSIDER_MODEL_ORDER_LONG_EDGE_STRATEGY: Property = Property::new(keys::ELK_LAYERED_CONSIDER_MODEL_ORDER_LONG_EDGE_STRATEGY);
/// `org.eclipse.elk.layered.layering.strategy` (`Property<Any>`)
pub static LAYERING_STRATEGY: Property = Property::new(keys::ELK_LAYERED_LAYERING_STRATEGY);
/// `org.eclipse.elk.layered.layering.nodePromotion.strategy` (`Property<Any>`)
pub static LAYERING_NODE_PROMOTION_STRATEGY: Property = Property::new(keys::ELK_LAYERED_LAYERING_NODE_PROMOTION_STRATEGY);
/// `org.eclipse.elk.layered.layering.coffmanGraham.layerBound` (`Property<Int>`)
pub static LAYERING_COFFMAN_GRAHAM_LAYER_BOUND: Property = Property::new(keys::ELK_LAYERED_LAYERING_COFFMAN_GRAHAM_LAYER_BOUND);
/// `org.eclipse.elk.layered.layerUnzippingStrategy` (`Property<Any>`)
pub static LAYER_UNZIPPING_STRATEGY: Property = Property::new(keys::ELK_LAYERED_LAYER_UNZIPPING_STRATEGY);
/// `org.eclipse.elk.layered.cycleBreaking.strategy` (`Property<Any>`)
pub static CYCLE_BREAKING_STRATEGY: Property = Property::new(keys::ELK_LAYERED_CYCLE_BREAKING_STRATEGY);
/// `org.eclipse.elk.layered.allowNonFlowPortsToSwitchSides` (`Property<Bool>`)
pub static ALLOW_NON_FLOW_PORTS_TO_SWITCH_SIDES: Property = Property::new(keys::ELK_LAYERED_ALLOW_NON_FLOW_PORTS_TO_SWITCH_SIDES);
/// `org.eclipse.elk.layered.compaction.postCompaction.strategy` (`Property<Any>`)
pub static COMPACTION_POST_COMPACTION_STRATEGY: Property = Property::new(keys::ELK_LAYERED_COMPACTION_POST_COMPACTION_STRATEGY);
/// `org.eclipse.elk.layered.compaction.postCompaction.constraints` (`Property<Any>`)
pub static COMPACTION_POST_COMPACTION_CONSTRAINTS: Property = Property::new(keys::ELK_LAYERED_COMPACTION_POST_COMPACTION_CONSTRAINTS);
/// `org.eclipse.elk.layered.compaction.connectedComponents` (`Property<Bool>`)
pub static COMPACTION_CONNECTED_COMPONENTS: Property = Property::new(keys::ELK_LAYERED_COMPACTION_CONNECTED_COMPONENTS);
/// `org.eclipse.elk.layered.highDegreeNodes.treatment` (`Property<Bool>`)
pub static HIGH_DEGREE_NODES_TREATMENT: Property = Property::new(keys::ELK_LAYERED_HIGH_DEGREE_NODES_TREATMENT);
/// `org.eclipse.elk.layered.highDegreeNodes.threshold` (`Property<Int>`)
pub static HIGH_DEGREE_NODES_THRESHOLD: Property = Property::new(keys::ELK_LAYERED_HIGH_DEGREE_NODES_THRESHOLD);
/// `org.eclipse.elk.layered.highDegreeNodes.treeHeight` (`Property<Int>`)
pub static HIGH_DEGREE_NODES_TREE_HEIGHT: Property = Property::new(keys::ELK_LAYERED_HIGH_DEGREE_NODES_TREE_HEIGHT);
/// `org.eclipse.elk.partitioning.activate` (`Property<Bool>`)
pub static PARTITIONING_ACTIVATE: Property = Property::new(keys::ELK_PARTITIONING_ACTIVATE);
/// `org.eclipse.elk.layered.generatePositionAndLayerIds` (`Property<Bool>`)
pub static GENERATE_POSITION_AND_LAYER_IDS: Property = Property::new(keys::ELK_LAYERED_GENERATE_POSITION_AND_LAYER_IDS);
/// `org.eclipse.elk.layered.portSortingStrategy` (`Property<Any>`)
pub static PORT_SORTING_STRATEGY: Property = Property::new(keys::ELK_LAYERED_PORT_SORTING_STRATEGY);
/// `org.eclipse.elk.layered.edgeLabels.centerLabelPlacementStrategy` (`Property<Any>`)
pub static EDGE_LABELS_CENTER_LABEL_PLACEMENT_STRATEGY: Property = Property::new(keys::ELK_LAYERED_EDGE_LABELS_CENTER_LABEL_PLACEMENT_STRATEGY);
/// `org.eclipse.elk.layered.wrapping.strategy` (`Property<Any>`)
pub static WRAPPING_STRATEGY: Property = Property::new(keys::ELK_LAYERED_WRAPPING_STRATEGY);
/// `org.eclipse.elk.layered.wrapping.additionalEdgeSpacing` (`Property<Double>`)
pub static WRAPPING_ADDITIONAL_EDGE_SPACING: Property = Property::new(keys::ELK_LAYERED_WRAPPING_ADDITIONAL_EDGE_SPACING);
/// `org.eclipse.elk.layered.wrapping.correctionFactor` (`Property<Double>`)
pub static WRAPPING_CORRECTION_FACTOR: Property = Property::new(keys::ELK_LAYERED_WRAPPING_CORRECTION_FACTOR);
/// `org.eclipse.elk.layered.wrapping.cutting.strategy` (`Property<Any>`)
pub static WRAPPING_CUTTING_STRATEGY: Property = Property::new(keys::ELK_LAYERED_WRAPPING_CUTTING_STRATEGY);
/// `org.eclipse.elk.layered.wrapping.cutting.cuts` (`Property<Any>`)
pub static WRAPPING_CUTTING_CUTS: Property = Property::new(keys::ELK_LAYERED_WRAPPING_CUTTING_CUTS);
/// `org.eclipse.elk.layered.wrapping.cutting.msd.freedom` (`Property<Int>`)
pub static WRAPPING_CUTTING_CUTS_MSD_FREEDOM: Property = Property::new(keys::ELK_LAYERED_WRAPPING_CUTTING_MSD_FREEDOM);
/// `org.eclipse.elk.layered.wrapping.validify.strategy` (`Property<Any>`)
pub static WRAPPING_VALIDIFY_STRATEGY: Property = Property::new(keys::ELK_LAYERED_WRAPPING_VALIDIFY_STRATEGY);
/// `org.eclipse.elk.layered.wrapping.validify.forbiddenIndices` (`Property<Int>`)
pub static WRAPPING_VALIDIFY_FORBID_SELF_CROSSING_REDUCE_COUNTER: Property = Property::new(keys::ELK_LAYERED_WRAPPING_VALIDIFY_FORBIDDEN_INDICES);
/// `org.eclipse.elk.layered.wrapping.multiEdge.improveCuts` (`Property<Bool>`)
pub static WRAPPING_MULTI_EDGE_IMPROVE_CUTS: Property = Property::new(keys::ELK_LAYERED_WRAPPING_MULTI_EDGE_IMPROVE_CUTS);
/// `org.eclipse.elk.layered.wrapping.multiEdge.improveWrappedEdges` (`Property<Bool>`)
pub static WRAPPING_MULTI_EDGE_IMPROVE_WRAPPED_EDGES: Property = Property::new(keys::ELK_LAYERED_WRAPPING_MULTI_EDGE_IMPROVE_WRAPPED_EDGES);
/// `org.eclipse.elk.layered.wrapping.multiEdge.distancePenalty` (`Property<Double>`)
pub static WRAPPING_MULTI_EDGE_DISTANCE_PENALTY: Property = Property::new(keys::ELK_LAYERED_WRAPPING_MULTI_EDGE_DISTANCE_PENALTY);
