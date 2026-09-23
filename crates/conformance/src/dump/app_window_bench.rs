//! `bench-app-window <scenario.json> <out.json>`: the Rust side of
//! `AppWindowBench` in `oracle/app/Sources/downright-app-oracle/AppWindowCapture.swift`.
//!
//! Document open to the first displayed frame, and a Live → Source → Live
//! mode switch to its next frame, over the scenario's document, with the same
//! steps in the same order as the Swift. Windows are off-screen, as in
//! `app-window`.

use std::time::Instant;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSResponder, NSWindow};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRunLoop};
use serde_json::Value;
use upleft_app::app::document_window_controller::DocumentWindowController;
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::RenderMode;

use super::app_window::{self, Scenario, off_screen, sandbox};
use super::json::{self, Object};
use super::{Failure, Request};

const WARMUP: usize = 3;
const RUNS: usize = 15;

fn run_loop_once(seconds: f64) {
    let date = NSDate::dateWithTimeIntervalSinceNow(seconds);
    unsafe { NSRunLoop::mainRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &date) };
}

fn is_first_responder(window: &NSWindow, controller: &DocumentWindowController) -> bool {
    let text_view = controller.primary_container().text_view().clone();
    let responder: Retained<NSResponder> = Retained::into_super(Retained::into_super(Retained::into_super(text_view)));
    window.firstResponder().is_some_and(|first| std::ptr::eq(&*first, &*responder))
}

pub fn run(request: &Request) -> Result<(), Failure> {
    let scenario = Scenario::load(&request.input)?;
    let mtm = MainThreadMarker::new().ok_or_else(|| Failure::Error("bench-app-window runs on the main thread".into()))?;
    app_window::acquire_window_capture_lock();
    off_screen::install();
    let sandbox_root = sandbox::prepare(&scenario)?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let root = app_window::repository_root();
    app_window::apply_scenario_appearance(&scenario, &root, mtm);
    let path = scenario.document.clone().ok_or_else(|| Failure::Error("bench needs a document".into()))?;
    let url = FileUrl::from_path(&root.join(path).to_string_lossy());

    let mut open = Vec::new();
    let mut to_source = Vec::new();
    let mut to_live = Vec::new();
    for iteration in 0..(WARMUP + RUNS) {
        let start = Instant::now();
        let controller = DocumentWindowController::new(mtm);
        if let (Some(size), Some(window)) = (scenario.size, controller.window()) {
            window.setContentSize(size);
        }
        controller.open(&url, RenderMode::Live).map_err(|error| Failure::Error(error.localized_description()))?;
        let window = controller.window().ok_or_else(|| Failure::Error("the controller has no window".into()))?;
        window.setFrameOrigin(NSPoint::new(-30000.0, -30000.0));
        window.orderFrontRegardless();
        off_screen::verify(std::slice::from_ref(&window), mtm);
        // The first frame is the one the deferred restore paints: it makes
        // the document's text view first responder.
        while !is_first_responder(&window, &controller) {
            run_loop_once(0.001);
        }
        window.displayIfNeeded();
        let opened = Instant::now();

        controller.apply_mode(RenderMode::Source);
        window.displayIfNeeded();
        let sourced = Instant::now();
        controller.apply_mode(RenderMode::Live);
        window.displayIfNeeded();
        let lived = Instant::now();

        if iteration >= WARMUP {
            open.push((opened - start).as_secs_f64() * 1e3);
            to_source.push((sourced - opened).as_secs_f64() * 1e3);
            to_live.push((lived - sourced).as_secs_f64() * 1e3);
        }
        controller.close();
        run_loop_once(0.05);
    }
    sandbox::remove(&sandbox_root);
    let stage = |name: &str, mut samples: Vec<f64>| -> Value {
        samples.sort_by(f64::total_cmp);
        Object::new()
            .with("stage", name)
            .with("p50", json::double(samples[samples.len() / 2]))
            .with("min", json::double(samples[0]))
            .with("runs", samples.len())
            .build()
    };
    let value = Value::Array(vec![
        stage("open to first frame", open),
        stage("mode switch Live to Source", to_source),
        stage("mode switch Source to Live", to_live),
    ]);
    json::write(&value, &request.output)?;
    std::process::exit(0)
}
