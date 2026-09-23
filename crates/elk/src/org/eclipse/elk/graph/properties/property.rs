//! Port of `graph/properties/Property.swift` and the dynamic value model of
//! elk-swift's property maps.
//!
//! elk-swift stores property values as `Any` in `[String: Any]` maps and reads
//! them back with dynamic casts. Two read forms are common and they differ when
//! the stored value has an unexpected type (for example the importer stores
//! `"8"` as a `Double` while the reader expects an `Int`):
//!
//! * `holder.getProperty(P) as? T` — the stored value (or, if none is stored,
//!   `P`'s default) cast to `T`; a stored value of another type gives `nil`.
//!   Port: [`PropertyMap::get_as`](super::map_property_holder::PropertyMap::get_as).
//! * `let v: T? = holder.getProperty(P)` (the generic overload, also used by
//!   `getProperty(P) ?? fallback` in a typed context) — the stored value cast
//!   to `T`, and if that fails `P`'s default cast to `T`.
//!   Port: [`PropertyMap::get_typed`](super::map_property_holder::PropertyMap::get_typed).
//!
//! Values keep their Swift dynamic type: `Int` and `Double` are different
//! variants, as are the enums. Swift classes stored as values are shared
//! references (`Rc<RefCell<_>>`) so mutation through one holder is visible
//! through every holder that copied the property, exactly as in Swift.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use super::keys::{self, PropKey};
use crate::bridge::elk_graph_impl::{ElkEdgeId, ElkEdgeSectionId, ElkLabelId, ElkNodeId, ElkPortId};
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::components::component_ordering_strategy::ComponentOrderingStrategy;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LEdgeId, LGraphId, LLabelId, LNodeId, LPortId, LayerId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::graph_configurator::Random;
use crate::org::eclipse::elk::alg::layered::options::{
    center_edge_label_placement_strategy::CenterEdgeLabelPlacementStrategy,
    constraint_calculation_strategy::ConstraintCalculationStrategy,
    crossing_minimization_strategy::CrossingMinimizationStrategy, cutting_strategy::CuttingStrategy,
    cycle_breaking_strategy::CycleBreakingStrategy, direction_congruency::DirectionCongruency,
    edge_constraint::EdgeConstraint, edge_label_side_selection::EdgeLabelSideSelection,
    edge_straightening_strategy::EdgeStraighteningStrategy, fixed_alignment::FixedAlignment,
    graph_compaction_strategy::GraphCompactionStrategy, graph_properties::GraphProperties,
    greedy_switch_type::GreedySwitchType, group_order_strategy::GroupOrderStrategy,
    in_layer_constraint::InLayerConstraint, interactive_reference_point::InteractiveReferencePoint,
    layer_constraint::LayerConstraint, layer_unzipping_strategy::LayerUnzippingStrategy,
    layering_strategy::LayeringStrategy, long_edge_ordering_strategy::LongEdgeOrderingStrategy,
    node_flexibility::NodeFlexibility, node_placement_strategy::NodePlacementStrategy,
    node_promotion_strategy::NodePromotionStrategy, ordering_strategy::OrderingStrategy,
    port_sorting_strategy::PortSortingStrategy, port_type::PortType,
    self_loop_distribution_strategy::SelfLoopDistributionStrategy,
    self_loop_ordering_strategy::SelfLoopOrderingStrategy,
    self_loop_placement_strategy::SelfLoopPlacementStrategy, spline_routing_mode::SplineRoutingMode,
    validify_strategy::ValidifyStrategy, wrapping_strategy::WrappingStrategy,
};
use crate::org::eclipse::elk::core::math::elk_margin::ElkMargin;
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChain;
use crate::org::eclipse::elk::core::options::{
    alignment::Alignment, content_alignment::ContentAlignment, direction::Direction, edge_coords::EdgeCoords,
    edge_label_placement::EdgeLabelPlacement, edge_routing::EdgeRouting, edge_type::EdgeType,
    hierarchy_handling::HierarchyHandling, label_side::LabelSide, node_label_placement::NodeLabelPlacement,
    port_alignment::PortAlignment, port_constraints::PortConstraints, port_label_placement::PortLabelPlacement,
    port_side::PortSide, shape_coords::ShapeCoords, size_constraint::SizeConstraint, size_options::SizeOptions,
    topdown_node_types::TopdownNodeTypes,
};

/// A property identifier with its default value (`Property<T>`).
///
/// Several Swift `Property` objects share an id but have different defaults
/// (`LayeredOptions.NODE_SIZE_MINIMUM` has none, `CoreOptions.NODE_SIZE_MINIMUM`
/// defaults to `KVector()`), so the default belongs to the `Property`, the
/// storage slot to the key.
///
/// A default is created afresh on every read. Swift hands out one shared
/// default instance for class-typed defaults; the port relies on no reachable
/// code mutating a default it did not store (noted where it matters).
pub struct Property {
    pub key: PropKey,
    pub default: Option<fn() -> PropValue>,
}

impl Property {
    pub const fn new(key: PropKey) -> Property {
        Property { key, default: None }
    }

    pub const fn with_default(key: PropKey, default: fn() -> PropValue) -> Property {
        Property { key, default: Some(default) }
    }

    pub fn id(&self) -> &'static str {
        self.key.name()
    }

    pub fn default_value(&self) -> Option<PropValue> {
        self.default.map(|d| d())
    }
}

impl fmt::Debug for Property {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

macro_rules! prop_values {
    (copy { $($cv:ident($ct:ty)),* $(,)? } shared { $($sv:ident($st:ty)),* $(,)? } lists { $($lv:ident($lt:ty)),* $(,)? }) => {
        /// A dynamically typed property value (Swift `Any`).
        #[derive(Clone)]
        pub enum PropValue {
            Bool(bool),
            /// Swift `Int`.
            Int(i64),
            /// Swift `Double`.
            Double(f64),
            Str(Rc<str>),
            $($cv($ct),)*
            $($sv(Rc<RefCell<$st>>),)*
            $($lv(Rc<Vec<$lt>>),)*
            /// Any other Swift object or value. Downcast with
            /// [`PropValue::downcast`].
            Object(Rc<dyn Any>),
        }

        impl fmt::Debug for PropValue {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    PropValue::Bool(v) => write!(f, "Bool({v})"),
                    PropValue::Int(v) => write!(f, "Int({v})"),
                    PropValue::Double(v) => write!(f, "Double({v})"),
                    PropValue::Str(v) => write!(f, "Str({v:?})"),
                    $(PropValue::$cv(v) => write!(f, "{}({:?})", stringify!($cv), v),)*
                    $(PropValue::$sv(v) => write!(f, "{}({:?})", stringify!($sv), v.borrow()),)*
                    $(PropValue::$lv(v) => write!(f, "{}({:?})", stringify!($lv), v),)*
                    PropValue::Object(_) => write!(f, "Object"),
                }
            }
        }

        $(
            impl PropCast for $ct {
                fn from_value(v: &PropValue) -> Option<Self> {
                    if let PropValue::$cv(x) = v { Some(x.clone()) } else { None }
                }
            }
            impl From<$ct> for PropValue {
                fn from(v: $ct) -> PropValue {
                    PropValue::$cv(v)
                }
            }
        )*
        $(
            impl PropCast for Rc<RefCell<$st>> {
                fn from_value(v: &PropValue) -> Option<Self> {
                    if let PropValue::$sv(x) = v { Some(x.clone()) } else { None }
                }
            }
            impl From<Rc<RefCell<$st>>> for PropValue {
                fn from(v: Rc<RefCell<$st>>) -> PropValue {
                    PropValue::$sv(v)
                }
            }
        )*
        $(
            impl PropCast for Rc<Vec<$lt>> {
                fn from_value(v: &PropValue) -> Option<Self> {
                    if let PropValue::$lv(x) = v { Some(x.clone()) } else { None }
                }
            }
            impl PropCast for Vec<$lt> {
                fn from_value(v: &PropValue) -> Option<Self> {
                    if let PropValue::$lv(x) = v { Some((**x).clone()) } else { None }
                }
            }
            impl From<Vec<$lt>> for PropValue {
                fn from(v: Vec<$lt>) -> PropValue {
                    PropValue::$lv(Rc::new(v))
                }
            }
        )*
    };
}

prop_values! {
    copy {
        Direction(Direction),
        PortSide(PortSide),
        EdgeRouting(EdgeRouting),
        HierarchyHandling(HierarchyHandling),
        PortConstraints(PortConstraints),
        EdgeLabelPlacement(EdgeLabelPlacement),
        Alignment(Alignment),
        PortAlignment(PortAlignment),
        EdgeType(EdgeType),
        LabelSide(LabelSide),
        EdgeCoords(EdgeCoords),
        ShapeCoords(ShapeCoords),
        TopdownNodeTypes(TopdownNodeTypes),
        ContentAlignment(ContentAlignment),
        SizeConstraint(SizeConstraint),
        SizeOptions(SizeOptions),
        NodeLabelPlacement(NodeLabelPlacement),
        PortLabelPlacement(PortLabelPlacement),
        CycleBreakingStrategy(CycleBreakingStrategy),
        LayeringStrategy(LayeringStrategy),
        CrossingMinimizationStrategy(CrossingMinimizationStrategy),
        NodePlacementStrategy(NodePlacementStrategy),
        EdgeStraighteningStrategy(EdgeStraighteningStrategy),
        FixedAlignment(FixedAlignment),
        GreedySwitchType(GreedySwitchType),
        GraphCompactionStrategy(GraphCompactionStrategy),
        WrappingStrategy(WrappingStrategy),
        OrderingStrategy(OrderingStrategy),
        LayerConstraint(LayerConstraint),
        InLayerConstraint(InLayerConstraint),
        EdgeConstraint(EdgeConstraint),
        PortType(PortType),
        EdgeLabelSideSelection(EdgeLabelSideSelection),
        CenterEdgeLabelPlacementStrategy(CenterEdgeLabelPlacementStrategy),
        SelfLoopDistributionStrategy(SelfLoopDistributionStrategy),
        SelfLoopOrderingStrategy(SelfLoopOrderingStrategy),
        SelfLoopPlacementStrategy(SelfLoopPlacementStrategy),
        NodePromotionStrategy(NodePromotionStrategy),
        PortSortingStrategy(PortSortingStrategy),
        LongEdgeOrderingStrategy(LongEdgeOrderingStrategy),
        DirectionCongruency(DirectionCongruency),
        InteractiveReferencePoint(InteractiveReferencePoint),
        GroupOrderStrategy(GroupOrderStrategy),
        ValidifyStrategy(ValidifyStrategy),
        CuttingStrategy(CuttingStrategy),
        ConstraintCalculationStrategy(ConstraintCalculationStrategy),
        LayerUnzippingStrategy(LayerUnzippingStrategy),
        SplineRoutingMode(SplineRoutingMode),
        NodeFlexibility(NodeFlexibility),
        ComponentOrderingStrategy(ComponentOrderingStrategy),
        NodeType(NodeType),
        GraphPropertiesSet(EnumSet<GraphProperties>),
        PortSideSet(EnumSet<PortSide>),
        LNode(LNodeId),
        LPort(LPortId),
        LEdge(LEdgeId),
        LLabel(LLabelId),
        LGraph(LGraphId),
        Layer(LayerId),
        ElkNode(ElkNodeId),
        ElkPort(ElkPortId),
        ElkEdge(ElkEdgeId),
        ElkLabel(ElkLabelId),
        ElkEdgeSection(ElkEdgeSectionId),
    }
    shared {
        KVector(KVector),
        KVectorChain(KVectorChain),
        ElkPadding(ElkPadding),
        ElkMargin(ElkMargin),
        Random(Random),
    }
    lists {
        LNodes(LNodeId),
        LEdges(LEdgeId),
        LPorts(LPortId),
        LLabels(LLabelId),
    }
}

/// A Swift dynamic cast (`value as? T`).
pub trait PropCast: Sized {
    fn from_value(v: &PropValue) -> Option<Self>;
}

impl PropCast for bool {
    fn from_value(v: &PropValue) -> Option<Self> {
        if let PropValue::Bool(x) = v { Some(*x) } else { None }
    }
}

impl PropCast for i64 {
    fn from_value(v: &PropValue) -> Option<Self> {
        if let PropValue::Int(x) = v { Some(*x) } else { None }
    }
}

impl PropCast for f64 {
    fn from_value(v: &PropValue) -> Option<Self> {
        if let PropValue::Double(x) = v { Some(*x) } else { None }
    }
}

impl PropCast for Rc<str> {
    fn from_value(v: &PropValue) -> Option<Self> {
        if let PropValue::Str(x) = v { Some(x.clone()) } else { None }
    }
}

impl PropCast for String {
    fn from_value(v: &PropValue) -> Option<Self> {
        if let PropValue::Str(x) = v { Some(x.to_string()) } else { None }
    }
}

impl PropCast for PropValue {
    fn from_value(v: &PropValue) -> Option<Self> {
        Some(v.clone())
    }
}

impl From<bool> for PropValue {
    fn from(v: bool) -> PropValue {
        PropValue::Bool(v)
    }
}

/// A Swift `Int` literal or value.
impl From<i64> for PropValue {
    fn from(v: i64) -> PropValue {
        PropValue::Int(v)
    }
}

impl From<f64> for PropValue {
    fn from(v: f64) -> PropValue {
        PropValue::Double(v)
    }
}

impl From<&str> for PropValue {
    fn from(v: &str) -> PropValue {
        PropValue::Str(Rc::from(v))
    }
}

impl From<String> for PropValue {
    fn from(v: String) -> PropValue {
        PropValue::Str(Rc::from(v))
    }
}

impl PropValue {
    /// Wraps any other Swift object or value.
    pub fn object<T: Any>(value: Rc<T>) -> PropValue {
        PropValue::Object(value)
    }

    pub fn kvector(v: KVector) -> PropValue {
        PropValue::KVector(Rc::new(RefCell::new(v)))
    }

    pub fn kvector_chain(v: KVectorChain) -> PropValue {
        PropValue::KVectorChain(Rc::new(RefCell::new(v)))
    }

    pub fn elk_padding(v: ElkPadding) -> PropValue {
        PropValue::ElkPadding(Rc::new(RefCell::new(v)))
    }

    pub fn elk_margin(v: ElkMargin) -> PropValue {
        PropValue::ElkMargin(Rc::new(RefCell::new(v)))
    }

    pub fn cast<T: PropCast>(&self) -> Option<T> {
        T::from_value(self)
    }

    /// Downcasts a [`PropValue::Object`].
    pub fn downcast<T: Any>(&self) -> Option<Rc<T>> {
        if let PropValue::Object(o) = self { o.clone().downcast::<T>().ok() } else { None }
    }

    /// The value as the JSON exporter keeps it (`String`, `Double`, `Int`,
    /// `Bool`); everything else is skipped.
    pub fn exportable(&self) -> Option<serde_json::Value> {
        match self {
            PropValue::Str(s) => Some(serde_json::Value::String(s.to_string())),
            PropValue::Double(d) => serde_json::Number::from_f64(*d).map(serde_json::Value::Number),
            PropValue::Int(i) => Some(serde_json::Value::from(*i)),
            PropValue::Bool(b) => Some(serde_json::Value::Bool(*b)),
            _ => None,
        }
    }
}

/// Casts `Any?` with Swift `as? T` semantics.
pub fn cast<T: PropCast>(value: Option<PropValue>) -> Option<T> {
    value.as_ref().and_then(T::from_value)
}

/// The key for an id string, if any declared property uses it.
pub fn key_for(id: &str) -> Option<PropKey> {
    keys::lookup(id)
}
