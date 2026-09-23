//! Short names for the modules and types almost every ported file uses.
//! Not an elk-swift file. `LayeredOptions::X`, `CoreOptions::X` and
//! `InternalProperties::X` read exactly like the Swift they port.

pub use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
pub use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
pub use crate::org::eclipse::elk::core::options::core_options as CoreOptions;

pub use crate::bridge::java_compat::{EnumOrdinal, EnumSet};
pub use crate::org::eclipse::elk::alg::layered::graph::l_graph::{
    LEdgeId, LGraphArena, LGraphId, LLabelId, LNodeId, LPortId, LayerId,
};
pub use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
pub use crate::org::eclipse::elk::alg::layered::options::graph_properties::GraphProperties;
pub use crate::org::eclipse::elk::alg::layered::options::port_type::PortType;
pub use crate::org::eclipse::elk::core::math::k_vector::KVector;
pub use crate::org::eclipse::elk::core::math::k_vector_chain::KVectorChain;
pub use crate::org::eclipse::elk::core::math::spacing::Spacing;
pub use crate::org::eclipse::elk::core::options::direction::Direction;
pub use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
pub use crate::org::eclipse::elk::core::options::port_side::PortSide;
pub use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;
pub use crate::org::eclipse::elk::graph::properties::map_property_holder::PropertyMap;
pub use crate::org::eclipse::elk::graph::properties::property::{PropCast, PropValue, Property};
pub use crate::swift;
