//! Port of `core/options/CoreOptions.swift`.
//!
//! Property declarations; defaults are the Swift ones (a `Property` without a
//! default reads as `nil` when unset).

use crate::org::eclipse::elk::graph::properties::keys;
use crate::org::eclipse::elk::graph::properties::property::Property;
use crate::org::eclipse::elk::graph::properties::property::PropValue;
use crate::org::eclipse::elk::core::math::{elk_margin::ElkMargin, elk_padding::ElkPadding, k_vector::KVector};
use crate::org::eclipse::elk::core::options::{alignment::Alignment, content_alignment::ContentAlignment, direction::Direction, edge_label_placement::EdgeLabelPlacement, edge_routing::EdgeRouting, edge_type::EdgeType, hierarchy_handling::HierarchyHandling, node_label_placement::NodeLabelPlacement, port_alignment::PortAlignment, port_constraints::PortConstraints, port_label_placement::PortLabelPlacement, port_side::PortSide, size_constraint::SizeConstraint, size_options::SizeOptions};

/// `org.eclipse.elk.algorithm` (`Property<String>`)
pub static ALGORITHM: Property = Property::new(keys::ELK_ALGORITHM);
/// `org.eclipse.elk.resolvedAlgorithm` (`Property<LayoutAlgorithmData>`)
pub static RESOLVED_ALGORITHM: Property = Property::new(keys::ELK_RESOLVED_ALGORITHM);
/// `org.eclipse.elk.direction` (`Property<Direction>`)
pub static DIRECTION: Property = Property::with_default(keys::ELK_DIRECTION, || PropValue::Direction(Direction::UNDEFINED));
/// `org.eclipse.elk.alignment` (`Property<Alignment>`)
pub static ALIGNMENT: Property = Property::with_default(keys::ELK_ALIGNMENT, || PropValue::Alignment(Alignment::AUTOMATIC));
/// `org.eclipse.elk.aspectRatio` (`Property<Double>`)
pub static ASPECT_RATIO: Property = Property::with_default(keys::ELK_ASPECT_RATIO, || PropValue::Double(0.0));
/// `org.eclipse.elk.noLayout` (`Property<Bool>`)
pub static NO_LAYOUT: Property = Property::with_default(keys::ELK_NO_LAYOUT, || PropValue::Bool(false));
/// `org.eclipse.elk.scaleFactor` (`Property<Double>`)
pub static SCALE_FACTOR: Property = Property::with_default(keys::ELK_SCALE_FACTOR, || PropValue::Double(1.0));
/// `org.eclipse.elk.animate` (`Property<Bool>`)
pub static ANIMATE: Property = Property::with_default(keys::ELK_ANIMATE, || PropValue::Bool(true));
/// `org.eclipse.elk.progressBar` (`Property<Bool>`)
pub static PROGRESS_BAR: Property = Property::with_default(keys::ELK_PROGRESS_BAR, || PropValue::Bool(false));
/// `org.eclipse.elk.zoomToFit` (`Property<Bool>`)
pub static ZOOM_TO_FIT: Property = Property::with_default(keys::ELK_ZOOM_TO_FIT, || PropValue::Bool(false));
/// `org.eclipse.elk.layoutAncestors` (`Property<Bool>`)
pub static LAYOUT_ANCESTORS: Property = Property::with_default(keys::ELK_LAYOUT_ANCESTORS, || PropValue::Bool(false));
/// `org.eclipse.elk.interactive` (`Property<Bool>`)
pub static INTERACTIVE_LAYOUT: Property = Property::with_default(keys::ELK_INTERACTIVE, || PropValue::Bool(false));
/// `org.eclipse.elk.hierarchyHandling` (`Property<HierarchyHandling>`)
pub static HIERARCHY_HANDLING: Property = Property::with_default(keys::ELK_HIERARCHY_HANDLING, || PropValue::HierarchyHandling(HierarchyHandling::INHERIT));
/// `org.eclipse.elk.separateConnectedComponents` (`Property<Bool>`)
pub static SEPARATE_CONNECTED_COMPONENTS: Property = Property::with_default(keys::ELK_SEPARATE_CONNECTED_COMPONENTS, || PropValue::Bool(true));
/// `org.eclipse.elk.padding` (`Property<ElkPadding>`)
pub static PADDING: Property = Property::with_default(keys::ELK_PADDING, || PropValue::elk_padding(ElkPadding::default()));
/// `org.eclipse.elk.spacing.nodeNode` (`Property<Double>`)
pub static SPACING_NODE_NODE: Property = Property::with_default(keys::ELK_SPACING_NODE_NODE, || PropValue::Double(20.0));
/// `org.eclipse.elk.spacing.edgeEdge` (`Property<Double>`)
pub static SPACING_EDGE_EDGE: Property = Property::with_default(keys::ELK_SPACING_EDGE_EDGE, || PropValue::Double(10.0));
/// `org.eclipse.elk.spacing.edgeNode` (`Property<Double>`)
pub static SPACING_EDGE_NODE: Property = Property::with_default(keys::ELK_SPACING_EDGE_NODE, || PropValue::Double(10.0));
/// `org.eclipse.elk.spacing.portPort` (`Property<Double>`)
pub static SPACING_PORT_PORT: Property = Property::with_default(keys::ELK_SPACING_PORT_PORT, || PropValue::Double(10.0));
/// `org.eclipse.elk.spacing.labelLabel` (`Property<Double>`)
pub static SPACING_LABEL_LABEL: Property = Property::with_default(keys::ELK_SPACING_LABEL_LABEL, || PropValue::Double(0.0));
/// `org.eclipse.elk.spacing.labelNode` (`Property<Double>`)
pub static SPACING_LABEL_NODE: Property = Property::with_default(keys::ELK_SPACING_LABEL_NODE, || PropValue::Double(5.0));
/// `org.eclipse.elk.spacing.labelPortHorizontal` (`Property<Double>`)
pub static SPACING_LABEL_PORT_HORIZONTAL: Property = Property::with_default(keys::ELK_SPACING_LABEL_PORT_HORIZONTAL, || PropValue::Double(1.0));
/// `org.eclipse.elk.spacing.labelPortVertical` (`Property<Double>`)
pub static SPACING_LABEL_PORT_VERTICAL: Property = Property::with_default(keys::ELK_SPACING_LABEL_PORT_VERTICAL, || PropValue::Double(1.0));
/// `org.eclipse.elk.spacing.portsSurrounding` (`Property<ElkMargin>`)
pub static SPACING_PORTS_SURROUNDING: Property = Property::new(keys::ELK_SPACING_PORTS_SURROUNDING);
/// `org.eclipse.elk.spacing.individual` (`Property<Any>`)
pub static SPACING_INDIVIDUAL: Property = Property::new(keys::ELK_SPACING_INDIVIDUAL);
/// `org.eclipse.elk.nodeSize.constraints` (`Property<SizeConstraint>`)
pub static NODE_SIZE_CONSTRAINTS: Property = Property::with_default(keys::ELK_NODE_SIZE_CONSTRAINTS, || PropValue::SizeConstraint(SizeConstraint::empty()));
/// `org.eclipse.elk.nodeSize.minimum` (`Property<KVector>`)
pub static NODE_SIZE_MINIMUM: Property = Property::with_default(keys::ELK_NODE_SIZE_MINIMUM, || PropValue::kvector(KVector::default()));
/// `org.eclipse.elk.nodeSize.options` (`Property<SizeOptions>`)
pub static NODE_SIZE_OPTIONS: Property = Property::with_default(keys::ELK_NODE_SIZE_OPTIONS, || PropValue::SizeOptions(SizeOptions::DEFAULT_MINIMUM_SIZE));
/// `org.eclipse.elk.nodeSize.fixedGraphSize` (`Property<Bool>`)
pub static NODE_SIZE_FIXED_GRAPH_SIZE: Property = Property::with_default(keys::ELK_NODE_SIZE_FIXED_GRAPH_SIZE, || PropValue::Bool(false));
/// `org.eclipse.elk.nodeLabels.placement` (`Property<NodeLabelPlacement>`)
pub static NODE_LABELS_PLACEMENT: Property = Property::with_default(keys::ELK_NODE_LABELS_PLACEMENT, || PropValue::NodeLabelPlacement(NodeLabelPlacement::empty()));
/// `org.eclipse.elk.nodeLabels.padding` (`Property<ElkPadding>`)
pub static NODE_LABELS_PADDING: Property = Property::with_default(keys::ELK_NODE_LABELS_PADDING, || PropValue::elk_padding(ElkPadding::uniform(5.0)));
/// `org.eclipse.elk.portConstraints` (`Property<PortConstraints>`)
pub static PORT_CONSTRAINTS: Property = Property::with_default(keys::ELK_PORT_CONSTRAINTS, || PropValue::PortConstraints(PortConstraints::UNDEFINED));
/// `org.eclipse.elk.port.side` (`Property<PortSide>`)
pub static PORT_SIDE: Property = Property::with_default(keys::ELK_PORT_SIDE, || PropValue::PortSide(PortSide::UNDEFINED));
/// `org.eclipse.elk.port.borderOffset` (`Property<Double>`)
pub static PORT_BORDER_OFFSET: Property = Property::with_default(keys::ELK_PORT_BORDER_OFFSET, || PropValue::Double(0.0));
/// `org.eclipse.elk.port.index` (`Property<Int>`)
pub static PORT_INDEX: Property = Property::with_default(keys::ELK_PORT_INDEX, || PropValue::Int(0));
/// `org.eclipse.elk.port.anchor` (`Property<KVector>`)
pub static PORT_ANCHOR: Property = Property::new(keys::ELK_PORT_ANCHOR);
/// `org.eclipse.elk.portLabels.placement` (`Property<PortLabelPlacement>`)
pub static PORT_LABELS_PLACEMENT: Property = Property::with_default(keys::ELK_PORT_LABELS_PLACEMENT, || PropValue::PortLabelPlacement(PortLabelPlacement::empty()));
/// `org.eclipse.elk.portLabels.nextToPortIfPossible` (`Property<Bool>`)
pub static PORT_LABELS_NEXT_TO_PORT_IF_POSSIBLE: Property = Property::with_default(keys::ELK_PORT_LABELS_NEXT_TO_PORT_IF_POSSIBLE, || PropValue::Bool(false));
/// `org.eclipse.elk.portLabels.treatAsGroup` (`Property<Bool>`)
pub static PORT_LABELS_TREAT_AS_GROUP: Property = Property::with_default(keys::ELK_PORT_LABELS_TREAT_AS_GROUP, || PropValue::Bool(true));
/// `org.eclipse.elk.portAlignment.default` (`Property<PortAlignment>`)
pub static PORT_ALIGNMENT_DEFAULT: Property = Property::with_default(keys::ELK_PORT_ALIGNMENT_DEFAULT, || PropValue::PortAlignment(PortAlignment::DISTRIBUTED));
/// `org.eclipse.elk.portAlignment.north` (`Property<PortAlignment>`)
pub static PORT_ALIGNMENT_NORTH: Property = Property::new(keys::ELK_PORT_ALIGNMENT_NORTH);
/// `org.eclipse.elk.portAlignment.south` (`Property<PortAlignment>`)
pub static PORT_ALIGNMENT_SOUTH: Property = Property::new(keys::ELK_PORT_ALIGNMENT_SOUTH);
/// `org.eclipse.elk.portAlignment.east` (`Property<PortAlignment>`)
pub static PORT_ALIGNMENT_EAST: Property = Property::new(keys::ELK_PORT_ALIGNMENT_EAST);
/// `org.eclipse.elk.portAlignment.west` (`Property<PortAlignment>`)
pub static PORT_ALIGNMENT_WEST: Property = Property::new(keys::ELK_PORT_ALIGNMENT_WEST);
/// `org.eclipse.elk.edgeRouting` (`Property<EdgeRouting>`)
pub static EDGE_ROUTING: Property = Property::with_default(keys::ELK_EDGE_ROUTING, || PropValue::EdgeRouting(EdgeRouting::UNDEFINED));
/// `org.eclipse.elk.edgeType` (`Property<EdgeType>`)
pub static EDGE_TYPE: Property = Property::with_default(keys::ELK_EDGE_TYPE, || PropValue::EdgeType(EdgeType::NONE));
/// `org.eclipse.elk.edgeLabels.placement` (`Property<EdgeLabelPlacement>`)
pub static EDGE_LABELS_PLACEMENT: Property = Property::with_default(keys::ELK_EDGE_LABELS_PLACEMENT, || PropValue::EdgeLabelPlacement(EdgeLabelPlacement::CENTER));
/// `org.eclipse.elk.edgeLabels.inline` (`Property<Bool>`)
pub static EDGE_LABELS_INLINE: Property = Property::with_default(keys::ELK_EDGE_LABELS_INLINE, || PropValue::Bool(false));
/// `org.eclipse.elk.junctionPoints` (`Property<KVectorChain>`)
pub static JUNCTION_POINTS: Property = Property::new(keys::ELK_JUNCTION_POINTS);
/// `org.eclipse.elk.commentBox` (`Property<Bool>`)
pub static COMMENT_BOX: Property = Property::with_default(keys::ELK_COMMENT_BOX, || PropValue::Bool(false));
/// `org.eclipse.elk.hypernode` (`Property<Bool>`)
pub static HYPERNODE: Property = Property::with_default(keys::ELK_HYPERNODE, || PropValue::Bool(false));
/// `org.eclipse.elk.margins` (`Property<ElkMargin>`)
pub static MARGINS: Property = Property::with_default(keys::ELK_MARGINS, || PropValue::elk_margin(ElkMargin::default()));
/// `org.eclipse.elk.insideSelfLoops.activate` (`Property<Bool>`)
pub static INSIDE_SELF_LOOPS_ACTIVATE: Property = Property::with_default(keys::ELK_INSIDE_SELF_LOOPS_ACTIVATE, || PropValue::Bool(false));
/// `org.eclipse.elk.insideSelfLoops.yo` (`Property<Bool>`)
pub static INSIDE_SELF_LOOPS_YO: Property = Property::with_default(keys::ELK_INSIDE_SELF_LOOPS_YO, || PropValue::Bool(false));
/// `org.eclipse.elk.contentAlignment` (`Property<ContentAlignment>`)
pub static CONTENT_ALIGNMENT: Property = Property::with_default(keys::ELK_CONTENT_ALIGNMENT, || PropValue::ContentAlignment(ContentAlignment::empty()));
/// `org.eclipse.elk.json.edgeCoords` (`Property<EdgeCoords>`)
pub static JSON_EDGE_COORDS: Property = Property::new(keys::ELK_JSON_EDGE_COORDS);
/// `org.eclipse.elk.json.shapeCoords` (`Property<ShapeCoords>`)
pub static JSON_SHAPE_COORDS: Property = Property::new(keys::ELK_JSON_SHAPE_COORDS);
/// `org.eclipse.elk.topdownLayout` (`Property<Bool>`)
pub static TOPDOWN_LAYOUT: Property = Property::with_default(keys::ELK_TOPDOWN_LAYOUT, || PropValue::Bool(false));
/// `org.eclipse.elk.topdown.nodeType` (`Property<TopdownNodeTypes>`)
pub static TOPDOWN_NODE_TYPE: Property = Property::new(keys::ELK_TOPDOWN_NODE_TYPE);
/// `org.eclipse.elk.topdown.scaleFactor` (`Property<Double>`)
pub static TOPDOWN_SCALE_FACTOR: Property = Property::with_default(keys::ELK_TOPDOWN_SCALE_FACTOR, || PropValue::Double(1.0));
/// `org.eclipse.elk.topdown.scaleCap` (`Property<Double>`)
pub static TOPDOWN_SCALE_CAP: Property = Property::with_default(keys::ELK_TOPDOWN_SCALE_CAP, || PropValue::Double(f64::MAX));
/// `org.eclipse.elk.topdown.hierarchicalNodeWidth` (`Property<Double>`)
pub static TOPDOWN_HIERARCHICAL_NODE_WIDTH: Property = Property::with_default(keys::ELK_TOPDOWN_HIERARCHICAL_NODE_WIDTH, || PropValue::Double(200.0));
/// `org.eclipse.elk.topdown.hierarchicalNodeAspectRatio` (`Property<Double>`)
pub static TOPDOWN_HIERARCHICAL_NODE_ASPECT_RATIO: Property = Property::with_default(keys::ELK_TOPDOWN_HIERARCHICAL_NODE_ASPECT_RATIO, || PropValue::Double(1.4142135623730951));
/// `org.eclipse.elk.topdown.sizeApproximator` (`Property<Any>`)
pub static TOPDOWN_SIZE_APPROXIMATOR: Property = Property::new(keys::ELK_TOPDOWN_SIZE_APPROXIMATOR);
/// `org.eclipse.elk.topdown.sizeCategories` (`Property<Int>`)
pub static TOPDOWN_SIZE_CATEGORIES: Property = Property::with_default(keys::ELK_TOPDOWN_SIZE_CATEGORIES, || PropValue::Int(4));
/// `org.eclipse.elk.topdown.sizeCategories.hierarchicalNodeWeight` (`Property<Int>`)
pub static TOPDOWN_SIZE_CATEGORIES_HIERARCHICAL_NODE_WEIGHT: Property = Property::with_default(keys::ELK_TOPDOWN_SIZE_CATEGORIES_HIERARCHICAL_NODE_WEIGHT, || PropValue::Int(50));
/// `org.eclipse.elk.childAreaWidth` (`Property<Double>`)
pub static CHILD_AREA_WIDTH: Property = Property::with_default(keys::ELK_CHILD_AREA_WIDTH, || PropValue::Double(0.0));
/// `org.eclipse.elk.childAreaHeight` (`Property<Double>`)
pub static CHILD_AREA_HEIGHT: Property = Property::with_default(keys::ELK_CHILD_AREA_HEIGHT, || PropValue::Double(0.0));
