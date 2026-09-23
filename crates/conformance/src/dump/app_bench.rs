//! Rust counterpart of `oracle/app/Sources/downright-app-oracle/AppBench.swift`
//! (`bench-export`, `bench-workspace`, `bench-find`): the same stages over the
//! same inputs, with the same harness (one warm-up, N runs, nearest-rank
//! p50/p95). `scripts/app-bench-compare.py` runs both oracles and compares.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use serde_json::Value;
use upleft_app::export::html_exporter::{HTMLExporter, NativeFragmentImageProvider};
use upleft_app::support::find_engine::{FindEngine, FindQuery, FindSession};
use upleft_app::workspace::workspace_index::{WorkspaceIndex, WorkspaceIndexPolicy, WorkspaceIndexSnapshot};
use upleft_app::workspace::workspace_link_graph::WorkspaceLinkGraphBuilder;
use upleft_app::workspace::workspace_search::{WorkspaceSearch, WorkspaceSearchQuery};
use upleft_core::document_io::DocumentIO;
use upleft_core::parser::MarkdownParser;
use upleft_core::ParsedDocument;
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::Theme;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

use super::json::{self, Object};
use super::style_sheet::appearance_named;
use super::{Failure, Request};

fn percentile(ascending: &[f64], p: f64) -> f64 {
    let rank = (p * ascending.len() as f64).ceil() as usize;
    ascending[(ascending.len() - 1).min(rank.max(1) - 1)]
}

/// Each run drains its own autorelease pool inside the timed region, as the
/// Swift side does.
fn measure(label: String, runs: usize, mut body: impl FnMut()) -> (String, Value) {
    objc2::rc::autoreleasepool(|_| body());
    let mut samples: Vec<f64> = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        objc2::rc::autoreleasepool(|_| body());
        samples.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (p50, p95) = (percentile(&samples, 0.50), percentile(&samples, 0.95));
    let max = *samples.last().unwrap();
    println!("  {label:<52}  p50 {p50:8.3} ms   p95 {p95:8.3} ms   max {max:8.3} ms (n={runs})");
    let value = Object::new()
        .with("p50", json::double(p50))
        .with("p95", json::double(p95))
        .with("max", json::double(max))
        .with("runs", runs as i64)
        .build();
    (label, value)
}

fn write(results: Vec<(String, Value)>, output: &Path) -> Result<(), Failure> {
    let mut object = Object::new();
    for (label, value) in results {
        object = object.with(&label, value);
    }
    Ok(json::write(&object.build(), output)?)
}

fn style_sheet() -> StyleSheet {
    let theme = ThemeStore::shared()
        .themes()
        .into_iter()
        .find(|theme| theme.name == "Paper Light")
        .unwrap_or_else(Theme::fallback);
    StyleSheet::new(theme, &appearance_named(false), Some(true))
}

fn read(url: &FileUrl) -> Result<String, Failure> {
    DocumentIO::read(Path::new(&url.path())).map(|(text, _)| text).map_err(|error| Failure::Error(error.to_string()))
}

pub fn export(request: &Request) -> Result<(), Failure> {
    let url = FileUrl::from_path(&request.input.to_string_lossy()).standardized_file_url();
    let text = read(&url)?;
    let name = url.last_path_component();
    let title = url.deleting_path_extension().last_path_component();
    let base = url.deleting_last_path_component();
    let sheet = style_sheet();
    let exporter = |document: std::sync::Arc<ParsedDocument>| {
        let sheet = sheet.clone();
        HTMLExporter::new(
            document,
            sheet.theme.clone(),
            title.clone(),
            Some(base.clone()),
            Some(Box::new(NativeFragmentImageProvider::new(sheet))),
        )
    };
    let document = MarkdownParser::parse(&text);
    let mut sink = 0usize;
    let mut results = Vec::new();
    results.push(measure(format!("HTMLExporter.html, {name}"), 25, || {
        sink = sink.wrapping_add(exporter(document.clone()).html().len())
    }));
    results.push(measure(format!("parse + HTMLExporter.html, {name}"), 10, || {
        sink = sink.wrapping_add(exporter(MarkdownParser::parse(&text)).html().len())
    }));
    results.push(measure(format!("HTMLExporter.html forPrint, {name}"), 25, || {
        let mut paper = exporter(document.clone());
        paper.for_print = true;
        sink = sink.wrapping_add(paper.html().len())
    }));
    if sink == 42 {
        println!();
    }
    write(results, &request.output)
}

unsafe extern "C" {
    fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
}

pub fn workspace(request: &Request) -> Result<(), Failure> {
    let root = FileUrl::from_path(&request.input.to_string_lossy()).standardized_file_url();
    let output = std::path::absolute(&request.output)?;
    std::env::set_current_dir("/").map_err(|error| Failure::Error(format!("cannot move to /: {error}")))?;
    let name = root.last_path_component();
    let snapshot = Rc::new(RefCell::new(WorkspaceIndexSnapshot::empty()));
    let scan = || {
        let index = WorkspaceIndex::new(WorkspaceIndexPolicy::default());
        let published: Rc<RefCell<Option<WorkspaceIndexSnapshot>>> = Rc::new(RefCell::new(None));
        let sink = published.clone();
        index.set_on_update(Some(Box::new(move |update: &WorkspaceIndexSnapshot| {
            *sink.borrow_mut() = Some(update.clone())
        })));
        index.start(&root);
        while published.borrow().is_none() {
            // SAFETY: runs the current (main) thread's run loop briefly.
            unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.001, 0) };
        }
        *snapshot.borrow_mut() = published.borrow_mut().take().expect("published");
    };
    let mut sink = 0usize;
    let mut results = Vec::new();
    results.push(measure(format!("WorkspaceIndex scan, {name}"), 10, || {
        scan();
        sink = sink.wrapping_add(snapshot.borrow().entries.len())
    }));
    let snapshot = snapshot.borrow().clone();
    results.push(measure(format!("WorkspaceLinkGraphBuilder.build, {name}"), 25, || {
        sink = sink.wrapping_add(WorkspaceLinkGraphBuilder::build(&snapshot).outgoing.len())
    }));
    results.push(measure(format!("WorkspaceSearch.search \"release\", {name}"), 10, || {
        sink = sink.wrapping_add(WorkspaceSearch::search(&WorkspaceSearchQuery::new("release"), &snapshot).len())
    }));
    let whole_word =
        WorkspaceSearchQuery { text: "(?:link|task)s?".into(), is_regex: true, whole_word: true, ..Default::default() };
    results.push(measure(format!("WorkspaceSearch.search regex whole word, {name}"), 10, || {
        sink = sink.wrapping_add(WorkspaceSearch::search(&whole_word, &snapshot).len())
    }));
    if sink == 42 {
        println!();
    }
    write(results, &output)
}

pub fn find(request: &Request) -> Result<(), Failure> {
    let url = FileUrl::from_path(&request.input.to_string_lossy()).standardized_file_url();
    let text = read(&url)?;
    let name = request.input.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let mut sink = 0usize;
    let mut results = Vec::new();
    let literal = FindQuery::new("section");
    results.push(measure(format!("FindEngine.matches \"section\", {name}"), 25, || {
        sink = sink.wrapping_add(FindEngine::matches(&text, &literal).len())
    }));
    let whole_word = FindQuery { case_sensitive: true, whole_word: true, ..FindQuery::new("Section") };
    results.push(measure(format!("FindEngine.matches case, whole word, {name}"), 25, || {
        sink = sink.wrapping_add(FindEngine::matches(&text, &whole_word).len())
    }));
    let regex = FindQuery::regex("(?m)^## (.*)$");
    results.push(measure(format!("FindEngine.matches regex, {name}"), 25, || {
        sink = sink.wrapping_add(FindEngine::matches(&text, &regex).len())
    }));
    let capture = FindQuery::regex(r"Section (\d+)");
    results.push(measure(format!("FindEngine.replaceAllEdits regex, {name}"), 10, || {
        sink = sink.wrapping_add(FindEngine::replace_all_edits(&text, &capture, "Part $1").len())
    }));
    let task = FindQuery::new("task");
    let caret = text.encode_utf16().count() as isize / 2;
    results.push(measure(format!("FindSession.update + advance, {name}"), 25, || {
        let mut session = FindSession::new();
        session.update(task.clone(), &text, caret);
        sink = sink.wrapping_add(session.advance(true).map_or(0, |range| range.location as usize))
    }));
    if sink == 42 {
        println!();
    }
    write(results, &request.output)
}
