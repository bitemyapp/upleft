//! The panel suites (`panel`, `panel-model`): the Rust side of
//! `oracle/app/Sources/downright-app-oracle/Panels/PanelHarness.swift`.
//! Every AppKit call mirrors the Swift harness call for call, so the harness
//! is never a source of differences.
//!
//!   panel        <scenario.json> <out.png> [--layout <out.json>]
//!   panel-model  <scenario.json> <out.json>
//!
//! See the Swift file for the scenario format. One scene module per Swift
//! panel under `scenes/`.

pub mod scenes;
pub mod tree;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType, NSBitmapImageFileType, NSBitmapImageRep,
    NSColorSpace, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSDictionary, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

use super::{Failure, Request};

/// `PanelScenario`: the envelope every panel shares, plus the panel's own
/// `state` object.
#[derive(Debug, Clone)]
pub struct PanelScenario {
    pub panel: String,
    pub theme: String,
    pub dark: bool,
    pub width: f64,
    pub height: f64,
    /// Path from the repository root, if the panel is attached to a document.
    pub document_path: Option<String>,
    pub state: Map<String, Value>,
}

impl PanelScenario {
    pub fn new(json: &Map<String, Value>, state: Option<Map<String, Value>>) -> Result<PanelScenario, String> {
        let panel = json.get("panel").and_then(Value::as_str).ok_or("scenario has no panel")?.to_owned();
        Ok(PanelScenario {
            panel,
            theme: json.get("theme").and_then(Value::as_str).unwrap_or("Paper Light").to_owned(),
            dark: json.get("dark").and_then(Value::as_bool).unwrap_or(false),
            width: json.get("width").and_then(Value::as_f64).unwrap_or(336.0),
            height: json.get("height").and_then(Value::as_f64).unwrap_or(480.0),
            document_path: json.get("document").and_then(Value::as_str).map(str::to_owned),
            state: state.unwrap_or_else(|| json.get("state").and_then(Value::as_object).cloned().unwrap_or_default()),
        })
    }

    /// The attached document's text (UTF-8, as the app reads a file).
    pub fn document_text(&self) -> Result<String, String> {
        let Some(path) = &self.document_path else { return Ok(String::new()) };
        let url = repository_root().join(path);
        std::fs::read_to_string(&url).map_err(|error| format!("{}: {error}", url.display()))
    }

    pub fn string(&self, key: &str) -> Option<String> {
        self.state.get(key).and_then(Value::as_str).map(str::to_owned)
    }

    pub fn string_or(&self, key: &str, fallback: &str) -> String {
        self.string(key).unwrap_or_else(|| fallback.to_owned())
    }

    /// `(state[key] as? NSNumber)?.intValue`: a JSON number truncated.
    pub fn int(&self, key: &str) -> Option<i64> {
        self.state.get(key).and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
    }

    pub fn int_or(&self, key: &str, fallback: i64) -> i64 {
        self.int(key).unwrap_or(fallback)
    }

    pub fn double(&self, key: &str) -> Option<f64> {
        self.state.get(key).and_then(Value::as_f64)
    }

    pub fn double_or(&self, key: &str, fallback: f64) -> f64 {
        self.double(key).unwrap_or(fallback)
    }

    /// `(state[key] as? NSNumber)?.boolValue ?? false`.
    pub fn bool(&self, key: &str) -> bool {
        match self.state.get(key) {
            Some(Value::Bool(value)) => *value,
            Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
            _ => false,
        }
    }

    pub fn array(&self, key: &str) -> Vec<Value> {
        self.state.get(key).and_then(Value::as_array).cloned().unwrap_or_default()
    }

    pub fn object(&self, key: &str) -> Map<String, Value> {
        self.state.get(key).and_then(Value::as_object).cloned().unwrap_or_default()
    }

    pub fn strings(&self, key: &str) -> Vec<String> {
        self.array(key).iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()
    }
}

/// `repositoryRoot` in the Swift oracle.
pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

/// What a panel scene provides (`PanelScene` in Swift).
pub trait PanelScene {
    /// Builds the panel and applies the scenario's state.
    fn build(&mut self, scenario: &PanelScenario, style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker)
    -> Result<Retained<NSView>, Failure>;

    /// Puts the panel in the harness window (default: the window's content
    /// view, at the scenario's size).
    fn host(&mut self, panel: &NSView, window: &NSWindow, scenario: &PanelScenario) {
        panel.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)));
        window.setContentView(Some(panel));
    }

    /// A window the scene owns (titled: ScreenCaptureKit; borderless:
    /// `cacheDisplay`). `None`: the harness window.
    fn own_window(&mut self, _panel: &NSView) -> Option<Retained<NSWindow>> {
        None
    }

    /// Runs once, after the window is ordered in (off-screen).
    fn after_show(&mut self, _window: &NSWindow, _scenario: &PanelScenario) {}

    /// Runs before every settle check.
    fn before_settle_check(&mut self) {}

    /// Panel-specific derived values for the dump.
    fn model(&self) -> Value {
        Value::Null
    }
}

fn appearance(dark: bool) -> Retained<NSAppearance> {
    let name = unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } };
    NSAppearance::appearanceNamed(name).expect("system appearance")
}

/// `panelStyleSheet(_:)`: the scenario's theme against its appearance,
/// Reduce Motion forced on as in the render harness.
pub fn panel_style_sheet(scenario: &PanelScenario) -> Result<(Rc<StyleSheet>, Retained<NSAppearance>), Failure> {
    let appearance = appearance(scenario.dark);
    let theme = ThemeStore::shared()
        .themes()
        .into_iter()
        .find(|theme| theme.name == scenario.theme)
        .ok_or_else(|| Failure::Error(format!("unknown theme {}", scenario.theme)))?;
    Ok((Rc::new(StyleSheet::new(theme, &appearance, Some(true))), appearance))
}

fn read_scenario_json(path: &Path) -> Result<Map<String, Value>, Failure> {
    let text = std::fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text).map_err(|error| Failure::Error(error.to_string()))?;
    value.as_object().cloned().ok_or_else(|| Failure::Error("scenario is not a JSON object".into()))
}

// MARK: - panel (windowed, off-screen)

struct Session {
    scenario: PanelScenario,
    output_png: PathBuf,
    output_layout: Option<PathBuf>,
    scene: Box<dyn PanelScene>,
    window: Option<Retained<NSWindow>>,
    settle_view: Option<Retained<NSView>>,
    previous_capture: Option<Vec<u8>>,
    stable_captures: u32,
    previous_server_capture: Option<Vec<u8>>,
    deadline: Option<Instant>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftPanelCaptureSession"]
    struct PanelCaptureDelegate;

    unsafe impl NSObjectProtocol for PanelCaptureDelegate {}

    unsafe impl NSApplicationDelegate for PanelCaptureDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            if let Err(message) = start(self.mtm()) {
                fail(&message);
            }
        }
    }
);

fn fail(message: &str) -> ! {
    eprintln!("panel failed: {message}");
    std::process::exit(2)
}

/// `PanelCaptureSession.run(input:output:flags:)`.
pub fn run_capture(request: &Request) -> Result<(), Failure> {
    let json = read_scenario_json(&request.input)?;
    let scenario = PanelScenario::new(&json, None).map_err(Failure::Error)?;
    let mut layout = None;
    let mut index = 0;
    let flags = &request.flags;
    while index < flags.len() {
        if flags[index] == "--layout" && index + 1 < flags.len() {
            layout = Some(PathBuf::from(&flags[index + 1]));
            index += 2;
        } else {
            return Err(Failure::Error(format!("unknown flag {}", flags[index])));
        }
    }
    let scene = scenes::make(&scenario.panel)?;
    acquire_window_capture_lock();
    crate::dump::app_window::off_screen::install();
    let mtm = MainThreadMarker::new().expect("panel capture runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    SESSION.with(|session| {
        *session.borrow_mut() = Some(Session {
            scenario,
            output_png: request.output.clone(),
            output_layout: layout,
            scene,
            window: None,
            settle_view: None,
            previous_capture: None,
            stable_captures: 0,
            previous_server_capture: None,
            deadline: None,
        })
    });
    let delegate: Retained<PanelCaptureDelegate> = unsafe { msg_send![PanelCaptureDelegate::alloc(mtm), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    std::process::exit(0)
}

/// Same lock and file as `capture.rs`.
fn acquire_window_capture_lock() {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open("/tmp/upleft-window-capture.lock")
        .unwrap_or_else(|error| fail(&format!("cannot open /tmp/upleft-window-capture.lock: {error}")));
    if let Err(error) = file.lock() {
        fail(&format!("cannot take /tmp/upleft-window-capture.lock: {error}"));
    }
    std::mem::forget(file);
}

fn start(mtm: MainThreadMarker) -> Result<(), String> {
    SESSION.with(|cell| -> Result<(), String> {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().expect("session");
        let scenario = session.scenario.clone();
        let (style_sheet, appearance) = panel_style_sheet(&scenario).map_err(failure_text)?;
        NSApplication::sharedApplication(mtm).setAppearance(Some(&appearance));
        let panel = session.scene.build(&scenario, style_sheet, mtm).map_err(failure_text)?;
        let window = match session.scene.own_window(&panel) {
            Some(own) => own,
            None => {
                let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height));
                let window = unsafe {
                    NSWindow::initWithContentRect_styleMask_backing_defer(
                        NSWindow::alloc(mtm),
                        frame,
                        NSWindowStyleMask::Borderless,
                        NSBackingStoreType::Buffered,
                        false,
                    )
                };
                unsafe { window.setReleasedWhenClosed(false) };
                window.setAppearance(Some(&appearance));
                window.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
                session.scene.host(&panel, &window, &scenario);
                window
            }
        };
        window.setFrameOrigin(NSPoint::new(-30000.0, -30000.0));
        window.orderFrontRegardless();
        crate::dump::app_window::off_screen::verify(std::slice::from_ref(&window), mtm);
        window.layoutIfNeeded();
        session.scene.after_show(&window, &scenario);
        session.settle_view = window.contentView();
        session.deadline = Some(Instant::now() + Duration::from_secs(8));
        session.window = Some(window);
        Ok(())
    })?;
    schedule_check();
    Ok(())
}

fn failure_text(failure: Failure) -> String {
    match failure {
        Failure::NotPorted => "not ported".to_owned(),
        Failure::Error(message) => message,
    }
}

fn schedule_check() {
    let when = DispatchTime::try_from(Duration::from_millis(150)).expect("delay");
    let _ = DispatchQueue::main().after(when, check_settled);
}

fn png_data(rep: &NSBitmapImageRep) -> Option<Vec<u8>> {
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
    Some(data.to_vec())
}

fn check_settled() {
    enum Next {
        Wait,
        Written,
    }
    let next = SESSION.with(|cell| -> Result<Next, String> {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().expect("session");
        session.scene.before_settle_check();
        let window = session.window.clone().expect("window");
        window.layoutIfNeeded();
        let view = session.settle_view.clone().ok_or("the window has no content view")?;
        view.displayIfNeeded();
        let bounds = view.bounds();
        let rep = view.bitmapImageRepForCachingDisplayInRect(bounds).ok_or("no bitmap representation")?;
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        let png = png_data(&rep).ok_or("PNG encoding failed")?;
        if session.previous_capture.as_ref() == Some(&png) {
            session.stable_captures += 1;
        } else {
            session.stable_captures = 0;
            session.previous_capture = Some(png.clone());
        }
        let expired = session.deadline.is_some_and(|deadline| Instant::now() > deadline);
        if session.stable_captures < 2 && !expired {
            return Ok(Next::Wait);
        }
        if session.stable_captures < 2 {
            eprintln!("warning: panel did not settle before the timeout");
        }
        // The window server composites glass and materials on its own
        // clock, after the view tree has settled: capture it until two
        // consecutive captures agree.
        let server = crate::dump::app_window::window_server::png(std::slice::from_ref(&window))?;
        let expired = session.deadline.is_some_and(|deadline| Instant::now() > deadline);
        if session.previous_server_capture.as_ref() != Some(&server) && !expired {
            session.previous_server_capture = Some(server);
            return Ok(Next::Wait);
        }
        if session.previous_server_capture.as_ref() != Some(&server) {
            eprintln!("warning: the window server capture did not settle before the timeout");
        }
        if let Some(layout) = &session.output_layout {
            let mut object = Map::new();
            object.insert("window".into(), tree::window(&window));
            object.insert("tree".into(), tree::dump(&view));
            object.insert("model".into(), session.scene.model());
            std::fs::write(layout, serde_json::to_string(&Value::Object(object)).expect("serialisable"))
                .map_err(|error| format!("write failed: {error}"))?;
        }
        crate::dump::app_window::off_screen::verify(std::slice::from_ref(&window), window.mtm());
        // The window server's composite of the off-screen window (glass,
        // materials and layers included); see `WindowServerCapture`.
        std::fs::write(&session.output_png, &server).map_err(|error| format!("write failed: {error}"))?;
        Ok(Next::Written)
    });
    match next {
        Err(message) => fail(&message),
        Ok(Next::Wait) => schedule_check(),
        Ok(Next::Written) => std::process::exit(0),
    }
}

// MARK: - panel-model (windowless)

/// `PanelModelDump.run(input:flags:)`.
pub fn run_model(request: &Request) -> Result<(), Failure> {
    let json = read_scenario_json(&request.input)?;
    let mtm = MainThreadMarker::new().expect("panel-model runs on the main thread");
    let _ = NSApplication::sharedApplication(mtm);
    let base = json.get("state").and_then(Value::as_object).cloned().unwrap_or_default();
    let states: Vec<Map<String, Value>> = json
        .get("states")
        .and_then(Value::as_array)
        .map(|states| states.iter().map(|state| state.as_object().cloned().unwrap_or_default()).collect())
        .unwrap_or_else(|| vec![Map::new()]);
    let mut results = Vec::new();
    for entry in states {
        let mut merged = base.clone();
        for (key, value) in &entry {
            merged.insert(key.clone(), value.clone());
        }
        let scenario = PanelScenario::new(&json, Some(merged)).map_err(Failure::Error)?;
        let (style_sheet, appearance) = panel_style_sheet(&scenario)?;
        NSApplication::sharedApplication(mtm).setAppearance(Some(&appearance));
        let mut scene = scenes::make(&scenario.panel)?;
        let panel = scene.build(&scenario, style_sheet, mtm)?;
        panel.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)));
        panel.layoutSubtreeIfNeeded();
        let mut object = Map::new();
        object.insert("name".into(), Value::String(entry.get("name").and_then(Value::as_str).unwrap_or("").to_owned()));
        object.insert("fittingSize".into(), tree::size(panel.fittingSize()));
        object.insert("tree".into(), tree::dump(&panel));
        object.insert("model".into(), scene.model());
        results.push(Value::Object(object));
    }
    let mut out = Map::new();
    out.insert("panel".into(), Value::String(json.get("panel").and_then(Value::as_str).unwrap_or("").to_owned()));
    out.insert("states".into(), Value::Array(results));
    Ok(super::json::write(&Value::Object(out), &request.output)?)
}
