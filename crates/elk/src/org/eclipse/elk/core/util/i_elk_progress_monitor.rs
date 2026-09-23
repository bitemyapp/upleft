//! Port of `core/util/IElkProgressMonitor.swift`.
//!
//! Only the parts that influence a layout are modelled: `begin`/`done` state
//! (`isRunning`, and `subTask` returning `nil` once closed) and cancellation.
//! elk-swift never enables logging or execution-time measurement.

pub trait IElkProgressMonitor {
    fn begin(&mut self, name: &str, total_work: f32) -> bool;
    fn worked(&mut self, _work: f32) {}
    fn done(&mut self);
    fn is_running(&self) -> bool;
    fn is_canceled(&self) -> bool;
    fn sub_task(&mut self, work: f32) -> Option<Box<dyn IElkProgressMonitor>>;
    fn is_logging_enabled(&self) -> bool {
        false
    }
    fn log(&mut self, _message: &str) {}
}
