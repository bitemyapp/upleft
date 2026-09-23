//! Port of `core/alg/LayoutProcessorConfiguration.swift`.

use crate::org::eclipse::elk::alg::layered::intermediate::intermediate_processor_strategy::IntermediateProcessorStrategy;
use crate::org::eclipse::elk::alg::layered::layered_phases::LayeredPhases;

/// Slots of intermediate processor factories: slot `2·p` runs before phase
/// `p`, slot `2·p + 1` after it. The assembler only ever reads the "before"
/// slots and the slot after the last phase, so processors added after an
/// earlier phase are silently dropped — as in elk-swift.
#[derive(Clone, Debug, Default)]
pub struct LayoutProcessorConfiguration {
    pub processor_lists: Vec<Vec<IntermediateProcessorStrategy>>,
    pub current_index: i32,
}

impl LayoutProcessorConfiguration {
    pub fn create() -> LayoutProcessorConfiguration {
        LayoutProcessorConfiguration { processor_lists: Vec::new(), current_index: -1 }
    }

    pub fn create_from(source: &LayoutProcessorConfiguration) -> LayoutProcessorConfiguration {
        source.clone()
    }

    fn slot_before(phase: LayeredPhases) -> usize {
        phase.ordinal() * 2
    }

    fn slot_after(phase: LayeredPhases) -> usize {
        phase.ordinal() * 2 + 1
    }

    pub fn clear(&mut self) -> &mut Self {
        self.processor_lists.clear();
        self.current_index = -1;
        self
    }

    fn do_add(&mut self, index: usize, processor: IntermediateProcessorStrategy) {
        while self.processor_lists.len() <= index {
            self.processor_lists.push(Vec::new());
        }
        self.processor_lists[index].push(processor);
    }

    pub fn add_before(&mut self, phase: LayeredPhases, processor: IntermediateProcessorStrategy) -> &mut Self {
        self.current_index = -1;
        self.do_add(Self::slot_before(phase), processor);
        self
    }

    pub fn add_after(&mut self, phase: LayeredPhases, processor: IntermediateProcessorStrategy) -> &mut Self {
        self.current_index = -1;
        self.do_add(Self::slot_after(phase), processor);
        self
    }

    pub fn add_all(&mut self, configuration: &LayoutProcessorConfiguration) -> &mut Self {
        for i in 0..configuration.processor_lists.len() {
            while self.processor_lists.len() <= i {
                self.processor_lists.push(Vec::new());
            }
            self.processor_lists[i].extend_from_slice(&configuration.processor_lists[i]);
        }
        self
    }

    pub fn processors_before(&self, phase: LayeredPhases) -> Vec<IntermediateProcessorStrategy> {
        self.processors_at(Self::slot_before(phase))
    }

    pub fn processors_after(&self, phase: LayeredPhases) -> Vec<IntermediateProcessorStrategy> {
        self.processors_at(Self::slot_after(phase))
    }

    fn processors_at(&self, index: usize) -> Vec<IntermediateProcessorStrategy> {
        self.processor_lists.get(index).cloned().unwrap_or_default()
    }
}
