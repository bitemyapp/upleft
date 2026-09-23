//! Port of `core/alg/AlgorithmAssembler.swift`.
//!
//! elk-swift never reads its processor cache: every `build` creates fresh
//! phase and processor instances.

use super::i_layout_phase::ILayoutPhase;
use super::i_layout_processor::ILayoutProcessor;
use super::layout_processor_configuration::LayoutProcessorConfiguration;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::alg::layered::intermediate::intermediate_processor_strategy::IntermediateProcessorStrategy;
use crate::org::eclipse::elk::alg::layered::layered_phases::{LayeredPhases, PhaseFactory};
use crate::swift;

pub struct AlgorithmAssembler {
    phase_factory_list: [Option<PhaseFactory>; 5],
    additional_processors: LayoutProcessorConfiguration,
}

impl Default for AlgorithmAssembler {
    fn default() -> Self {
        AlgorithmAssembler { phase_factory_list: [None; 5], additional_processors: LayoutProcessorConfiguration::create() }
    }
}

impl AlgorithmAssembler {
    pub fn create() -> AlgorithmAssembler {
        AlgorithmAssembler::default()
    }

    pub fn reset(&mut self) -> &mut Self {
        self.phase_factory_list = [None; 5];
        self.additional_processors.clear();
        self
    }

    pub fn set_phase(&mut self, phase: LayeredPhases, factory: PhaseFactory) -> &mut Self {
        self.phase_factory_list[phase.ordinal()] = Some(factory);
        self
    }

    pub fn add_processor_configuration(&mut self, config: &LayoutProcessorConfiguration) -> &mut Self {
        self.additional_processors.add_all(config);
        self
    }

    pub fn build(&mut self, lg: &LGraphArena, graph: LGraphId) -> Vec<Box<dyn ILayoutProcessor>> {
        let mut phase_implementations: Vec<Option<Box<dyn ILayoutPhase>>> = Vec::new();
        for phase in LayeredPhases::ALL {
            phase_implementations.push(self.phase_factory_list[phase.ordinal()].map(|f| f.create()));
        }

        let mut processor_configuration = LayoutProcessorConfiguration::create();
        for phase in phase_implementations.iter().flatten() {
            if let Some(config) = phase.get_layout_processor_configuration(lg, graph) {
                processor_configuration.add_all(&config);
            }
        }
        processor_configuration.add_all(&self.additional_processors);

        let mut algorithm: Vec<Box<dyn ILayoutProcessor>> = Vec::new();
        let mut phase_implementations = phase_implementations.into_iter();
        for phase in LayeredPhases::ALL {
            algorithm.extend(Self::retrieve_processors_from_factories(processor_configuration.processors_before(phase)));
            if let Some(Some(phase_impl)) = phase_implementations.next() {
                algorithm.push(phase_impl);
            }
        }
        algorithm.extend(Self::retrieve_processors_from_factories(processor_configuration.processors_after(LayeredPhases::P5_EDGE_ROUTING)));
        algorithm
    }

    /// Sorted by ordinal with Swift's stable sort, then instantiated.
    pub fn retrieve_processors_from_factories(factories: Vec<IntermediateProcessorStrategy>) -> Vec<Box<dyn ILayoutProcessor>> {
        let sorted = swift::sorted_by(factories, |a, b| a.ordinal() < b.ordinal());
        sorted.into_iter().map(|f| f.create()).collect()
    }
}
