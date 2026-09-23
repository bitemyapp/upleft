//! Port of `core/util/BasicProgressMonitor.swift` (including the
//! `TimeoutProgressMonitor` subclass `ELK.layout` uses).
//!
//! Sub-monitors are plain `BasicProgressMonitor`s, so only the top-level
//! monitor handed to the recursive layout engine can ever report
//! cancellation — the layered algorithm itself never sees a timeout.

use std::time::{Duration, Instant};

use super::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct BasicProgressMonitor {
    task_name: String,
    closed: bool,
    total_work: f32,
    completed_work: f32,
    current_child_work: f32,
    /// `TimeoutProgressMonitor`'s deadline.
    deadline: Option<Instant>,
}

impl BasicProgressMonitor {
    pub const UNKNOWN_WORK: f32 = -1.0;

    pub fn new() -> BasicProgressMonitor {
        BasicProgressMonitor { current_child_work: -1.0, ..Default::default() }
    }

    /// `TimeoutProgressMonitor(timeout:)`.
    pub fn with_timeout(timeout: Duration) -> BasicProgressMonitor {
        BasicProgressMonitor { current_child_work: -1.0, deadline: Some(Instant::now() + timeout), ..Default::default() }
    }

    pub fn task_name(&self) -> &str {
        &self.task_name
    }
}

impl IElkProgressMonitor for BasicProgressMonitor {
    fn begin(&mut self, name: &str, total_work: f32) -> bool {
        if self.closed {
            return false;
        }
        if !self.task_name.is_empty() {
            return false;
        }
        self.task_name = name.to_string();
        self.total_work = total_work;
        true
    }

    fn worked(&mut self, work: f32) {
        if work > 0.0 && !self.closed && self.total_work > 0.0 && self.completed_work < self.total_work {
            self.completed_work += work;
        }
    }

    fn done(&mut self) {
        if self.task_name.is_empty() {
            return;
        }
        if self.closed {
            return;
        }
        if self.completed_work < self.total_work {
            let remaining = self.total_work - self.completed_work;
            self.worked(remaining);
        }
        self.closed = true;
    }

    fn is_running(&self) -> bool {
        !self.task_name.is_empty() && !self.closed
    }

    fn is_canceled(&self) -> bool {
        match self.deadline {
            Some(deadline) => Instant::now() > deadline,
            None => false,
        }
    }

    fn sub_task(&mut self, work: f32) -> Option<Box<dyn IElkProgressMonitor>> {
        if self.closed {
            return None;
        }
        self.current_child_work = work;
        Some(Box::new(BasicProgressMonitor::new()))
    }
}
