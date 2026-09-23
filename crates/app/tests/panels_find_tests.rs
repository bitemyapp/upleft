//! View-level tests for FindBarView, ChangeSummaryBarView, SearchResultsPanelView, SearchInspectorView
//! (ported from DownrightAppTests). Runs on the main thread; any window is
//! off-screen and never activated.

#[path = "main_thread/mod.rs"]
mod main_thread;

fn main() {
    main_thread::run(&[]);
}
