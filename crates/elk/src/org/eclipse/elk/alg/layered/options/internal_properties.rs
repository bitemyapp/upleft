//! Port of `alg/layered/options/InternalProperties.swift`.
//!
//! Property declarations; defaults are the Swift ones (a `Property` without a
//! default reads as `nil` when unset).

use crate::org::eclipse::elk::graph::properties::keys;
use crate::org::eclipse::elk::graph::properties::property::Property;

pub const FUZZINESS: f64 = 0.0001;

/// `origin` (`Property<Any>`)
pub static ORIGIN: Property = Property::new(keys::ORIGIN);
/// `originalBendpoints` (`Property<Any>`)
pub static ORIGINAL_BENDPOINTS: Property = Property::new(keys::ORIGINAL_BENDPOINTS);
/// `originalDummyNodePosition` (`Property<Any>`)
pub static ORIGINAL_DUMMY_NODE_POSITION: Property = Property::new(keys::ORIGINAL_DUMMY_NODE_POSITION);
/// `originalPortConstraints` (`Property<Any>`)
pub static ORIGINAL_PORT_CONSTRAINTS: Property = Property::new(keys::ORIGINAL_PORT_CONSTRAINTS);
/// `longEdgeSource` (`Property<Any>`)
pub static LONG_EDGE_SOURCE: Property = Property::new(keys::LONG_EDGE_SOURCE);
/// `longEdgeTarget` (`Property<Any>`)
pub static LONG_EDGE_TARGET: Property = Property::new(keys::LONG_EDGE_TARGET);
/// `longEdgeTargetNode` (`Property<Any>`)
pub static LONG_EDGE_TARGET_NODE: Property = Property::new(keys::LONG_EDGE_TARGET_NODE);
/// `longEdgeHasLabelDummies` (`Property<Any>`)
pub static LONG_EDGE_HAS_LABEL_DUMMIES: Property = Property::new(keys::LONG_EDGE_HAS_LABEL_DUMMIES);
/// `inLayerLayoutUnit` (`Property<Any>`)
pub static IN_LAYER_LAYOUT_UNIT: Property = Property::new(keys::IN_LAYER_LAYOUT_UNIT);
/// `inLayerSuccessorConstraint` (`Property<Any>`)
pub static IN_LAYER_SUCCESSOR_CONSTRAINTS: Property = Property::new(keys::IN_LAYER_SUCCESSOR_CONSTRAINT);
/// `inLayerConstraint` (`Property<Any>`)
pub static IN_LAYER_CONSTRAINT: Property = Property::new(keys::IN_LAYER_CONSTRAINT);
/// `graphProperties` (`Property<Any>`)
pub static GRAPH_PROPERTIES: Property = Property::new(keys::GRAPH_PROPERTIES);
/// `barycenterAssociates` (`Property<Any>`)
pub static BARYCENTER_ASSOCIATES: Property = Property::new(keys::BARYCENTER_ASSOCIATES);
/// `modelOrder` (`Property<Int>`)
pub static MODEL_ORDER: Property = Property::new(keys::MODEL_ORDER);
/// `modelOrder.maximum` (`Property<Int>`)
pub static MAX_MODEL_ORDER_NODES: Property = Property::new(keys::MODEL_ORDER_MAXIMUM);
/// `modelOrderGroups.cb.number` (`Property<Int>`)
pub static CB_NUM_MODEL_ORDER_GROUPS: Property = Property::new(keys::MODEL_ORDER_GROUPS_CB_NUMBER);
/// `targetNode.modelOrder` (`Property<Int>`)
pub static TARGET_NODE_MODEL_ORDER: Property = Property::new(keys::TARGET_NODE_MODEL_ORDER);
/// `cyclic` (`Property<Bool>`)
pub static CYCLIC: Property = Property::new(keys::CYCLIC);
/// `reversed` (`Property<Bool>`)
pub static REVERSED: Property = Property::new(keys::REVERSED);
/// `isPartOfCycle` (`Property<Bool>`)
pub static IS_PART_OF_CYCLE: Property = Property::new(keys::IS_PART_OF_CYCLE);
/// `inputCollect` (`Property<Bool>`)
pub static INPUT_COLLECT: Property = Property::new(keys::INPUT_COLLECT);
/// `outputCollect` (`Property<Bool>`)
pub static OUTPUT_COLLECT: Property = Property::new(keys::OUTPUT_COLLECT);
/// `spacings` (`Property<Any>`)
pub static SPACINGS: Property = Property::new(keys::SPACINGS);
/// `portDummy` (`Property<Any>`)
pub static PORT_DUMMY: Property = Property::new(keys::PORT_DUMMY);
/// `processors` (`Property<[AnyGraphProcessor]>`)
pub static PROCESSORS: Property = Property::new(keys::PROCESSORS);
/// `tarjan.lowlink` (`Property<Int>`)
pub static TARJAN_LOWLINK: Property = Property::new(keys::TARJAN_LOWLINK);
/// `tarjan.id` (`Property<Int>`)
pub static TARJAN_ID: Property = Property::new(keys::TARJAN_ID);
/// `tarjan.onStack` (`Property<Bool>`)
pub static TARJAN_ON_STACK: Property = Property::new(keys::TARJAN_ON_STACK);
/// `extPort.side` (`Property<PortSide>`)
pub static EXT_PORT_SIDE: Property = Property::new(keys::EXT_PORT_SIDE);
/// `extPort.connections` (`Property<Set<PortSide>>`)
pub static EXT_PORT_CONNECTIONS: Property = Property::new(keys::EXT_PORT_CONNECTIONS);
/// `extPort.size` (`Property<Any>`)
pub static EXT_PORT_SIZE: Property = Property::new(keys::EXT_PORT_SIZE);
/// `endLabel.edge` (`Property<Any>`)
pub static END_LABEL_EDGE: Property = Property::new(keys::END_LABEL_EDGE);
/// `endLabels` (`Property<Any>`)
pub static END_LABELS: Property = Property::new(keys::END_LABELS);
/// `edgeConstraint` (`Property<Any>`)
pub static EDGE_CONSTRAINT: Property = Property::new(keys::EDGE_CONSTRAINT);
/// `topComments` (`Property<Any>`)
pub static TOP_COMMENTS: Property = Property::new(keys::TOP_COMMENTS);
/// `bottomComments` (`Property<Any>`)
pub static BOTTOM_COMMENTS: Property = Property::new(keys::BOTTOM_COMMENTS);
/// `commentConnPort` (`Property<Any>`)
pub static COMMENT_CONN_PORT: Property = Property::new(keys::COMMENT_CONN_PORT);
/// `spline.route.start` (`Property<Any>`)
pub static SPLINE_ROUTE_START: Property = Property::new(keys::SPLINE_ROUTE_START);
/// `spline.edgeChain` (`Property<Any>`)
pub static SPLINE_EDGE_CHAIN: Property = Property::new(keys::SPLINE_EDGE_CHAIN);
/// `spline.nsPortY` (`Property<Double>`)
pub static SPLINE_NS_PORT_Y_COORD: Property = Property::new(keys::SPLINE_NS_PORT_Y);
/// `spline.survivingEdge` (`Property<Any>`)
pub static SPLINE_SURVIVING_EDGE: Property = Property::new(keys::SPLINE_SURVIVING_EDGE);
/// `dummy` (`Property<Bool>`)
pub static DUMMY: Property = Property::new(keys::DUMMY);
/// `compoundNode` (`Property<Bool>`)
pub static COMPOUND_NODE: Property = Property::new(keys::COMPOUND_NODE);
/// `crossHierarchyMap` (`Property<Any>`)
pub static CROSS_HIERARCHY_MAP: Property = Property::new(keys::CROSS_HIERARCHY_MAP);
/// `insideConnections` (`Property<Bool>`)
pub static INSIDE_CONNECTIONS: Property = Property::new(keys::INSIDE_CONNECTIONS);
/// `coordinateSystemOrigin` (`Property<Any>`)
pub static COORDINATE_SYSTEM_ORIGIN: Property = Property::new(keys::COORDINATE_SYSTEM_ORIGIN);
/// `bb.upLeft` (`Property<Any>`)
pub static BB_UPLEFT: Property = Property::new(keys::BB_UP_LEFT);
/// `bb.lowRight` (`Property<Any>`)
pub static BB_LOWRIGHT: Property = Property::new(keys::BB_LOW_RIGHT);
/// `random` (`Property<Any>`)
pub static RANDOM: Property = Property::new(keys::RANDOM);
/// `labelSide` (`Property<Any>`)
pub static LABEL_SIDE: Property = Property::new(keys::LABEL_SIDE);
/// `maxEdgeThickness` (`Property<Double>`)
pub static MAX_EDGE_THICKNESS: Property = Property::new(keys::MAX_EDGE_THICKNESS);
/// `portRatioOrPosition` (`Property<Double>`)
pub static PORT_RATIO_OR_POSITION: Property = Property::new(keys::PORT_RATIO_OR_POSITION);
/// `targetOffset` (`Property<Any>`)
pub static TARGET_OFFSET: Property = Property::new(keys::TARGET_OFFSET);
/// `unnecessaryBendpoints` (`Property<Bool>`)
pub static UNNECESSARY_BENDPOINTS: Property = Property::new(keys::UNNECESSARY_BENDPOINTS);
/// `originalLabelEdge` (`Property<Any>`)
pub static ORIGINAL_LABEL_EDGE: Property = Property::new(keys::ORIGINAL_LABEL_EDGE);
/// `representedLabels` (`Property<Any>`)
pub static REPRESENTED_LABELS: Property = Property::new(keys::REPRESENTED_LABELS);
/// `hiddenNodes` (`Property<Any>`)
pub static HIDDEN_NODES: Property = Property::new(keys::HIDDEN_NODES);
/// `originalOppositePort` (`Property<Any>`)
pub static ORIGINAL_OPPOSITE_PORT: Property = Property::new(keys::ORIGINAL_OPPOSITE_PORT);
/// `longEdgeBeforeLabelDummy` (`Property<Bool>`)
pub static LONG_EDGE_BEFORE_LABEL_DUMMY: Property = Property::new(keys::LONG_EDGE_BEFORE_LABEL_DUMMY);
/// `extPort.replacedDummies` (`Property<[LNode]>`)
pub static EXT_PORT_REPLACED_DUMMIES: Property = Property::new(keys::EXT_PORT_REPLACED_DUMMIES);
/// `extPort.replacedDummy` (`Property<LNode>`)
pub static EXT_PORT_REPLACED_DUMMY: Property = Property::new(keys::EXT_PORT_REPLACED_DUMMY);
/// `crossingHint` (`Property<Int>`)
pub static CROSSING_HINT: Property = Property::new(keys::CROSSING_HINT);
/// `selfLoopHolder` (`Property<Any>`)
pub static SELF_LOOP_HOLDER: Property = Property::new(keys::SELF_LOOP_HOLDER);
