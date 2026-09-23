//! Port of `alg/layered/p5edges/orthogonal/HyperEdgeSegmentDependency.swift`.
//!
//! Dependencies live in the [`HyperEdgeSegmentGraph`] arena next to the
//! segments; the Swift instance methods that relink segments
//! (`setSource`, `setTarget`, `remove`, `reverse`) are arena methods.

use super::hyper_edge_segment::{DependencyId, HyperEdgeSegmentGraph, SegmentId};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DependencyType {
    REGULAR,
    CRITICAL,
}

impl DependencyType {
    pub fn raw_value(self) -> &'static str {
        match self {
            DependencyType::REGULAR => "REGULAR",
            DependencyType::CRITICAL => "CRITICAL",
        }
    }
}

pub const CRITICAL_DEPENDENCY_WEIGHT: i64 = 1;

#[derive(Clone, Debug)]
pub struct HyperEdgeSegmentDependency {
    pub dep_type: DependencyType,
    pub source: Option<SegmentId>,
    pub target: Option<SegmentId>,
    pub weight: i64,
}

impl HyperEdgeSegmentDependency {
    /// `init(_:_:_:_:)`: creates the dependency and links it into both segments.
    fn create(graph: &mut HyperEdgeSegmentGraph, dep_type: DependencyType, source: SegmentId, target: SegmentId, weight: i64) -> DependencyId {
        let id = DependencyId(graph.dependencies.len() as u32);
        graph.dependencies.push(HyperEdgeSegmentDependency { dep_type, source: None, target: None, weight });
        graph.dependency_set_source(id, Some(source));
        graph.dependency_set_target(id, Some(target));
        id
    }

    /// `createAndAddRegular(_:_:_:)`.
    pub fn create_and_add_regular(graph: &mut HyperEdgeSegmentGraph, source: SegmentId, target: SegmentId, weight: i64) -> DependencyId {
        Self::create(graph, DependencyType::REGULAR, source, target, weight)
    }

    /// `createAndAddCritical(_:_:)`.
    pub fn create_and_add_critical(graph: &mut HyperEdgeSegmentGraph, source: SegmentId, target: SegmentId) -> DependencyId {
        Self::create(graph, DependencyType::CRITICAL, source, target, CRITICAL_DEPENDENCY_WEIGHT)
    }

    pub fn get_type(&self) -> DependencyType {
        self.dep_type
    }

    pub fn get_source(&self) -> Option<SegmentId> {
        self.source
    }

    pub fn get_target(&self) -> Option<SegmentId> {
        self.target
    }

    pub fn get_weight(&self) -> i64 {
        self.weight
    }
}

impl HyperEdgeSegmentGraph {
    /// `dependency.remove()`.
    pub fn dependency_remove(&mut self, dep: DependencyId) {
        self.dependency_set_source(dep, None);
        self.dependency_set_target(dep, None);
    }

    /// `dependency.reverse()`.
    pub fn dependency_reverse(&mut self, dep: DependencyId) {
        let old_source = self[dep].source;
        let old_target = self[dep].target;
        self.dependency_set_source(dep, old_target);
        self.dependency_set_target(dep, old_source);
    }

    /// `dependency.setSource(_:)`.
    pub fn dependency_set_source(&mut self, dep: DependencyId, new_source: Option<SegmentId>) {
        if let Some(source) = self[dep].source {
            self.remove_outgoing_segment_dependency(source, dep);
        }

        self[dep].source = new_source;

        if let Some(source) = new_source {
            self[source].outgoing_segment_deps.push(dep);
        }
    }

    /// `dependency.setTarget(_:)`.
    pub fn dependency_set_target(&mut self, dep: DependencyId, new_target: Option<SegmentId>) {
        if let Some(target) = self[dep].target {
            self.remove_incoming_segment_dependency(target, dep);
        }

        self[dep].target = new_target;

        if let Some(target) = new_target {
            self[target].incoming_segment_deps.push(dep);
        }
    }
}
