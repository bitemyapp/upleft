//! Rust counterpart of `oracle/app/Sources/downright-app-oracle/AppWindowCapture.swift`:
//! `app-window <scenario.json> <out.png> [--layout out.json]` and
//! `bench-app-window <scenario.json> <out.json>`.
//!
//! Builds one of the app's real windows the way the app builds it, in a fresh
//! sandbox, orders it in far outside every display (never activated, never
//! key), waits until three window-server captures agree, and writes the last
//! one. Every step mirrors the Swift file; see its header for the reasons
//! (`OffScreenWindows`, `WindowServerCapture`).

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, ProtocolObject, Sel};
use objc2::{AnyThread, ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType, NSBezelStyle, NSBitmapImageFileType,
    NSBitmapImageRep, NSButton, NSFont, NSScreen, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    NSDictionary, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSProcessInfo, NSRect, NSSize, NSString,
    NSUserDefaults,
};
use serde_json::Value;

use super::json::{self, Object};
use super::{Failure, Request};

/// `AppWindowScenario`.
pub(crate) struct Scenario {
    pub(crate) window: String,
    pub(crate) document: Option<String>,
    pub(crate) mode: String,
    pub(crate) dark: bool,
    pub(crate) size: Option<NSSize>,
    pub(crate) preferences: Option<Vec<u8>>,
    pub(crate) keybindings: Option<Vec<u8>>,
    pub(crate) pane: Option<String>,
    pub(crate) guide: String,
    /// Recent documents seeded into the sandbox (`sandbox::seed_recents`).
    pub(crate) recents: Vec<Value>,
    pub(crate) commands: Vec<String>,
    pub(crate) settle_timeout: Duration,
}

impl Scenario {
    pub(crate) fn load(path: &Path) -> Result<Scenario, Failure> {
        let text = std::fs::read_to_string(path)?;
        let object: Value = serde_json::from_str(&text).map_err(|error| Failure::Error(error.to_string()))?;
        if !object.is_object() {
            return Err(Failure::Error("a scenario is a JSON object".into()));
        }
        let window = object["window"].as_str().unwrap_or("").to_owned();
        let mode = object["mode"].as_str().unwrap_or("live").to_owned();
        if !matches!(mode.as_str(), "read" | "live" | "source") {
            return Err(Failure::Error(format!("unknown mode {mode}")));
        }
        let size = object["size"].as_array().and_then(|size| {
            (size.len() == 2).then(|| NSSize::new(size[0].as_f64().unwrap_or(0.0), size[1].as_f64().unwrap_or(0.0)))
        });
        // Written as the Swift side writes it (`.prettyPrinted, .sortedKeys`);
        // only the decoded values matter to Preferences.
        let preferences = match object.get("preferences") {
            Some(value) => Some(serde_json::to_vec_pretty(value).map_err(|error| Failure::Error(error.to_string()))?),
            None => None,
        };
        let keybindings = match object.get("keybindings") {
            Some(value) => Some(serde_json::to_vec_pretty(value).map_err(|error| Failure::Error(error.to_string()))?),
            None => None,
        };
        Ok(Scenario {
            window,
            keybindings,
            document: object["document"].as_str().map(str::to_owned),
            mode,
            dark: object["appearance"].as_str() == Some("dark"),
            size,
            preferences,
            pane: object["pane"].as_str().map(str::to_owned),
            guide: object["guide"].as_str().unwrap_or("unavailable").to_owned(),
            recents: object["recents"].as_array().cloned().unwrap_or_default(),
            commands: object["commands"]
                .as_array()
                .map(|commands| commands.iter().filter_map(|command| command.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            settle_timeout: Duration::from_secs_f64(object["settleTimeout"].as_f64().unwrap_or(10.0)),
        })
    }
}

/// Window kinds this oracle can build yet. Anything else is "not ported",
/// reported before the application starts.
fn is_ported(window: &str) -> bool {
    matches!(window, "probe" | "start" | "setup" | "preferences")
}

// MARK: - Sandbox

/// `AppWindowSandbox`.
pub(crate) mod sandbox {
    use super::*;

    pub fn prepare(scenario: &Scenario) -> Result<PathBuf, Failure> {
        let uuid = NSProcessInfo::processInfo().globallyUniqueString().to_string();
        let root = std::env::temp_dir().join(format!("upleft-app-window-{}-{uuid}", std::process::id()));
        let home = root.join("home");
        let support = root.join("support");
        std::fs::create_dir_all(&home)?;
        std::fs::create_dir_all(&support)?;
        // SAFETY: single-threaded at this point; nothing has read the
        // environment yet.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("CFFIXED_USER_HOME", &home);
            std::env::set_var("DOWNRIGHT_SUPPORT_DIRECTORY", &support);
        }
        if let Some(preferences) = &scenario.preferences {
            std::fs::write(support.join("preferences.json"), preferences)?;
        }
        if let Some(keybindings) = &scenario.keybindings {
            std::fs::write(support.join("keybindings.json"), keybindings)?;
        }
        seed_recents(&scenario.recents, &root, &support)?;
        clear_own_defaults();
        Ok(root)
    }

    /// `AppWindowSandbox.seedRecents`: each recent's file under
    /// `<root>/recents/` and `recents.json` in the support folder, in the
    /// scenario's order.
    fn seed_recents(recents: &[Value], root: &Path, support: &Path) -> Result<(), Failure> {
        if recents.is_empty() {
            return Ok(());
        }
        let folder = root.join("recents");
        let mut entries = Vec::new();
        for recent in recents {
            let (Some(relative), Some(opened)) = (recent["path"].as_str(), recent["opened"].as_str()) else {
                return Err(Failure::Error("a recent needs \"path\" and \"opened\"".into()));
            };
            let heading = recent["heading"].as_str().unwrap_or("");
            let file = folder.join(relative);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&file, format!("# {heading}\n"))?;
            entries.push(serde_json::json!({
                "path": file.to_string_lossy(),
                "displayName": file.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
                "firstHeading": heading,
                "lastOpened": opened,
                "wordCount": recent["words"].as_i64().unwrap_or(0),
            }));
        }
        let data = serde_json::to_vec(&Value::Array(entries)).map_err(|error| Failure::Error(error.to_string()))?;
        std::fs::write(support.join("recents.json"), data)?;
        Ok(())
    }

    /// This process's own defaults domain, never the user's app domain.
    pub fn clear_own_defaults() {
        let domain = objc2_foundation::NSBundle::mainBundle()
            .bundleIdentifier()
            .unwrap_or_else(|| NSProcessInfo::processInfo().processName());
        NSUserDefaults::standardUserDefaults().removePersistentDomainForName(&domain);
    }

    pub fn remove(root: &Path) {
        clear_own_defaults();
        let _ = std::fs::remove_dir_all(root);
    }
}

// MARK: - Off-screen windows

/// `OffScreenWindows`: `-[NSWindow constrainFrameRect:toScreen:]` becomes the
/// identity for the whole process, so a titled window ordered in at
/// (-30000, -30000) stays there instead of being pulled onto a display.
pub(crate) mod off_screen {
    use super::*;

    extern "C-unwind" fn identity(_this: &AnyObject, _cmd: Sel, rect: NSRect, _screen: *mut AnyObject) -> NSRect {
        rect
    }

    pub fn install() {
        let Some(method) = NSWindow::class().instance_method(sel!(constrainFrameRect:toScreen:)) else { return };
        // SAFETY: the replacement has the method's exact signature
        // (`- (NSRect)constrainFrameRect:(NSRect)r toScreen:(NSScreen *)s`).
        unsafe {
            let imp: Imp = std::mem::transmute::<
                extern "C-unwind" fn(&AnyObject, Sel, NSRect, *mut AnyObject) -> NSRect,
                Imp,
            >(identity);
            method.set_implementation(imp);
        }
    }

    /// Refuses to go on if a window touches any display: the windows the
    /// scene captures, and every other visible window of this process (an
    /// alert, a sheet, a panel some code path opens).
    pub fn verify(windows: &[Retained<NSWindow>], mtm: MainThreadMarker) {
        let mut windows: Vec<Retained<NSWindow>> = windows.to_vec();
        windows.extend(NSApplication::sharedApplication(mtm).windows().iter().filter(|window| window.isVisible()));
        for window in &windows {
            let frame = window.frame();
            let touches = NSScreen::screens(mtm).iter().any(|screen| intersects(screen.frame(), frame));
            if touches {
                for window in NSApplication::sharedApplication(mtm).windows().iter() {
                    window.orderOut(None);
                }
                eprintln!(
                    "app-window failed: a window reached a display at {{{{{}, {}}}, {{{}, {}}}}}",
                    frame.origin.x, frame.origin.y, frame.size.width, frame.size.height
                );
                std::process::exit(2);
            }
        }
    }

    fn intersects(a: NSRect, b: NSRect) -> bool {
        a.origin.x < b.origin.x + b.size.width
            && b.origin.x < a.origin.x + a.size.width
            && a.origin.y < b.origin.y + b.size.height
            && b.origin.y < a.origin.y + a.size.height
            && a.size.width > 0.0
            && a.size.height > 0.0
            && b.size.width > 0.0
            && b.size.height > 0.0
    }
}

// MARK: - Window-server capture

/// `WindowServerCapture`.
pub(crate) mod window_server {
    use super::*;

    type CreateImage = unsafe extern "C" fn(NSRect, u32, u32, u32) -> *mut CGImage;

    fn create_image() -> Option<CreateImage> {
        // SAFETY: dlopen/dlsym of a system framework symbol with its C signature.
        unsafe {
            let handle = libc::dlopen(c"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics".as_ptr(), libc::RTLD_NOW);
            if handle.is_null() {
                return None;
            }
            let symbol = libc::dlsym(handle, c"CGWindowListCreateImage".as_ptr());
            (!symbol.is_null()).then(|| std::mem::transmute::<*mut c_void, CreateImage>(symbol))
        }
    }

    pub fn image(window: &NSWindow) -> Result<CFRetained<CGImage>, String> {
        let create = create_image().ok_or("CGWindowListCreateImage is unavailable")?;
        let null = NSRect::new(NSPoint::new(f64::INFINITY, f64::INFINITY), NSSize::new(0.0, 0.0));
        // kCGWindowListOptionIncludingWindow; kCGWindowImageBoundsIgnoreFraming
        // | kCGWindowImageBestResolution.
        let image = unsafe { create(null, 1 << 3, window.windowNumber() as u32, (1 << 0) | (1 << 3)) };
        let image = std::ptr::NonNull::new(image)
            .ok_or_else(|| format!("the window server returned no image for window {}", window.windowNumber()))?;
        // SAFETY: a +1 "Create" result.
        Ok(unsafe { CFRetained::from_raw(image) })
    }

    pub fn png(windows: &[Retained<NSWindow>]) -> Result<Vec<u8>, String> {
        let images = windows.iter().map(|window| image(window)).collect::<Result<Vec<_>, _>>()?;
        let image = if images.len() == 1 { images[0].clone() } else { crate::capture::stack_images(&images)? };
        let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
        let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
            .ok_or("PNG encoding of the window capture failed")?;
        Ok(data.to_vec())
    }
}

/// The machine-wide window-capture lock (see `capture.rs`). Held until exit.
pub(crate) fn acquire_window_capture_lock() {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open("/tmp/upleft-window-capture.lock");
    match file.and_then(|file| file.lock().map(|()| file)) {
        Ok(file) => std::mem::forget(file),
        Err(error) => {
            eprintln!("app-window failed: cannot take /tmp/upleft-window-capture.lock: {error}");
            std::process::exit(2);
        }
    }
}

fn appearance(dark: bool) -> Retained<NSAppearance> {
    let name = unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } };
    NSAppearance::appearanceNamed(name).expect("system appearance")
}

/// `applyScenarioAppearance`: `AppDelegate.applySelectedTheme`, with the
/// system appearance taken from the scenario, and the bundle's icon as the
/// application icon.
pub(crate) fn apply_scenario_appearance(scenario: &Scenario, root: &Path, mtm: MainThreadMarker) {
    let icon_path = NSString::from_str(&root.join("vendor/downright/Resources/AppIcon.icns").to_string_lossy());
    let icon = objc2_app_kit::NSImage::initWithContentsOfFile(objc2_app_kit::NSImage::alloc(), &icon_path);
    unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(icon.as_deref()) };
    let appearance = appearance(scenario.dark);
    NSApplication::sharedApplication(mtm).setAppearance(Some(&appearance));
    let name = upleft_app::support::preferences::Preferences::shared().theme_name(&appearance);
    upleft_render::theme::theme_store::ThemeStore::shared().select(&name);
}

// MARK: - Scenes

/// `AppWindowScene`.
struct Scene {
    window: Retained<NSWindow>,
    pending_commands: Option<Vec<String>>,
    /// Keeps the window's controller alive (`retained` in Swift).
    _retained: Option<Retained<NSObject>>,
}

impl Scene {
    fn build(scenario: &Scenario, _root: &Path, mtm: MainThreadMarker) -> Result<Scene, Failure> {
        match scenario.window.as_str() {
            "probe" => {
                let probe = unsafe {
                    NSWindow::initWithContentRect_styleMask_backing_defer(
                        NSWindow::alloc(mtm),
                        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(480.0, 320.0)),
                        NSWindowStyleMask::Titled
                            | NSWindowStyleMask::Closable
                            | NSWindowStyleMask::Miniaturizable
                            | NSWindowStyleMask::Resizable,
                        NSBackingStoreType::Buffered,
                        false,
                    )
                };
                unsafe { probe.setReleasedWhenClosed(false) };
                probe.setTitle(&NSString::from_str("Probe"));
                let label = NSTextField::labelWithString(
                    &NSString::from_str("The quick brown fox jumps over the lazy dog."),
                    mtm,
                );
                label.setFrame(NSRect::new(NSPoint::new(24.0, 140.0), NSSize::new(432.0, 24.0)));
                label.setFont(Some(&NSFont::systemFontOfSize(17.0)));
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(&NSString::from_str("Continue"), None, None, mtm)
                };
                button.setBezelStyle(NSBezelStyle::Push);
                button.setFrame(NSRect::new(NSPoint::new(360.0, 20.0), NSSize::new(100.0, 32.0)));
                let content = probe.contentView().expect("content view");
                content.addSubview(&label);
                content.addSubview(&button);
                let scene = Scene { window: probe, pending_commands: None, _retained: None };
                scene.show(mtm);
                Ok(scene)
            }
            "start" => {
                use upleft_app::ai::document_state_store::DocumentStateStore;
                use upleft_app::app::start_window_controller::{StartGuideOffer, StartWindowController};
                let guide = match scenario.guide.as_str() {
                    "primary" => StartGuideOffer::Primary,
                    "secondary" => StartGuideOffer::Secondary,
                    _ => StartGuideOffer::Unavailable,
                };
                let recents = DocumentStateStore::shared().recents(StartWindowController::RECENT_DISPLAY_LIMIT);
                let controller = StartWindowController::new(recents, guide, mtm);
                let window = controller.window().ok_or_else(|| Failure::Error("start controller has no window".into()))?;
                let scene = Scene { window, pending_commands: None, _retained: Some(Retained::into_super(Retained::into_super(Retained::into_super(controller)))) };
                scene.show(mtm);
                Ok(scene)
            }
            "setup" => {
                use upleft_app::app::setup_window_controller::SetupWindowController;
                let controller = SetupWindowController::make_if_needed(mtm).ok_or_else(|| {
                    Failure::Error("SetupWindowController.makeIfNeeded() returned nil on this machine".into())
                })?;
                let window = controller.window().ok_or_else(|| Failure::Error("setup controller has no window".into()))?;
                let scene = Scene { window, pending_commands: None, _retained: Some(Retained::into_super(Retained::into_super(Retained::into_super(controller)))) };
                scene.show(mtm);
                Ok(scene)
            }
            "preferences" => {
                use upleft_app::app::preferences_window_controller::{PreferencesWindowController, SettingsPane};
                let controller = PreferencesWindowController::new(mtm);
                if let Some(name) = &scenario.pane {
                    let pane = SettingsPane::from_raw_value(name)
                        .ok_or_else(|| Failure::Error(format!("unknown pane {name}")))?;
                    controller.select(pane);
                }
                let window = controller.window().ok_or_else(|| Failure::Error("Settings has no window".into()))?;
                let scene = Scene { window, pending_commands: None, _retained: Some(Retained::into_super(Retained::into_super(Retained::into_super(controller)))) };
                scene.show(mtm);
                Ok(scene)
            }
            _ => Err(Failure::NotPorted),
        }
    }

    /// `showWindow(nil)` / `makeKeyAndOrderFront(nil)`, minus the key status
    /// and on no screen.
    fn show(&self, mtm: MainThreadMarker) {
        self.window.setFrameOrigin(NSPoint::new(-30000.0, -30000.0));
        self.window.orderFrontRegardless();
        off_screen::verify(std::slice::from_ref(&self.window), mtm);
    }

    fn perform_next_command(&mut self, scenario: &Scenario) -> Result<bool, Failure> {
        let commands = self.pending_commands.get_or_insert_with(|| scenario.commands.clone());
        if commands.is_empty() {
            return Ok(false);
        }
        let name = commands.remove(0);
        Err(Failure::Error(format!("commands need a document window (got {name})")))
    }

    fn captured_windows(&self) -> Vec<Retained<NSWindow>> {
        let mut windows = vec![self.window.clone()];
        if let Some(children) = self.window.childWindows() {
            windows.extend(children.iter().filter(|child| child.isVisible()));
        }
        windows
    }
}

// MARK: - Geometry

/// `AppWindowGeometry`.
mod geometry {
    use super::*;

    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
    }


    #[repr(C)]
    struct TypeNamePair {
        data: *const u8,
        length: usize,
    }

    type GetTypeName = unsafe extern "C" fn(*const c_void, bool) -> TypeNamePair;

    /// `String(describing: type(of:))`: an Objective-C class's own name; for
    /// a Swift class (a runtime name that is mangled, `_Tt…`, or
    /// module-qualified), the Swift runtime's unqualified type name
    /// (`swift_getTypeName(_, false)`, which `_typeName(_:qualified:)` calls).
    pub fn class_name(class: &AnyClass) -> String {
        let raw = class.name().to_string_lossy().into_owned();
        let name = raw.strip_prefix("NSKVONotifying_").map(str::to_owned).unwrap_or(raw);
        if !(name.starts_with("_Tt") || name.contains('.')) {
            return name;
        }
        swift_type_name(class).unwrap_or(name)
    }

    fn swift_type_name(class: &AnyClass) -> Option<String> {
        // SAFETY: `swift_getTypeName` from the Swift runtime, with its C
        // signature; a Swift class object is its own type metadata.
        unsafe {
            let handle = libc::dlopen(c"/usr/lib/swift/libswiftCore.dylib".as_ptr(), libc::RTLD_NOW);
            let symbol = dlsym(handle, c"swift_getTypeName".as_ptr());
            if symbol.is_null() {
                return None;
            }
            let get = std::mem::transmute::<*mut c_void, GetTypeName>(symbol);
            let pair = get((class as *const AnyClass).cast(), false);
            if pair.data.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(pair.data, pair.length);
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
    }

    pub fn rect(rect: NSRect) -> Value {
        Value::Array(vec![
            json::double(rect.origin.x),
            json::double(rect.origin.y),
            json::double(rect.size.width),
            json::double(rect.size.height),
        ])
    }

    /// An identifier AppKit makes from an object's address
    /// (`NSTabViewControllerToolbarUIProvider(0x…)`) differs per run.
    pub fn without_address(identifier: &str) -> String {
        match identifier.find("(0x") {
            Some(index) => format!("{}(0x…)", &identifier[..index]),
            None => identifier.to_owned(),
        }
    }

    /// See `AppWindowGeometry.view` in the Swift harness: a view with an
    /// ambiguous Auto Layout solution, and its subtree, report "ambiguous".
    pub fn view(view: &NSView) -> Value {
        view_in(view, false)
    }

    fn view_in(view: &NSView, ambiguous_ancestor: bool) -> Value {
        let ambiguous = ambiguous_ancestor || view.hasAmbiguousLayout();
        let geometry = |value: NSRect| if ambiguous { Value::String("ambiguous".into()) } else { rect(value) };
        let mut object = Object::new()
            .with("class", class_name(view.class()))
            .with("frame", geometry(view.frame()))
            .with("bounds", geometry(view.bounds()))
            .with("hidden", view.isHidden())
            .with("alpha", json::double(view.alphaValue()));
        if let Some(field) = view.downcast_ref::<NSTextField>() {
            object = object.with("text", field.stringValue().to_string());
        } else if let Some(button) = view.downcast_ref::<NSButton>() {
            object = object.with("title", button.title().to_string()).with("state", button.state());
        } else if let Some(text_view) = view.downcast_ref::<objc2_app_kit::NSTextView>()
            && let Some(layout) = unsafe { text_view.textLayoutManager() }
        {
            object = object.with("fragments", fragments(&layout));
        }
        let subviews: Vec<Value> = view.subviews().iter().map(|subview| view_in(&subview, ambiguous)).collect();
        object.with("subviews", Value::Array(subviews)).build()
    }

    /// See `AppWindowGeometry.fragments`: the fragments TextKit has made so
    /// far, without laying out anything more.
    fn fragments(layout: &objc2_app_kit::NSTextLayoutManager) -> Value {
        use objc2_app_kit::{NSTextElementProvider, NSTextLayoutFragment, NSTextLayoutFragmentEnumerationOptions};
        let Some(content) = (unsafe { layout.textContentManager() }) else { return Value::Null };
        let start = unsafe { content.documentRange() }.location();
        let fragments = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let collected = fragments.clone();
        let content_for_block = content.clone();
        let start_for_block = start.clone();
        let block = block2::RcBlock::new(move |fragment: std::ptr::NonNull<NSTextLayoutFragment>| -> objc2::runtime::Bool {
            let fragment = unsafe { fragment.as_ref() };
            let range = unsafe { fragment.rangeInElement() };
            let location = unsafe { content_for_block.offsetFromLocation_toLocation(&start_for_block, &range.location()) };
            let end = unsafe { content_for_block.offsetFromLocation_toLocation(&start_for_block, &range.endLocation()) };
            collected.borrow_mut().push(
                Object::new()
                    .with("class", class_name(fragment.class()))
                    .with("range", Value::Array(vec![location.into(), (end - location).into()]))
                    .with("frame", rect(unsafe { fragment.layoutFragmentFrame() }))
                    .with("state", unsafe { fragment.state() }.0 as i64)
                    .build(),
            );
            objc2::runtime::Bool::YES
        });
        unsafe {
            layout.enumerateTextLayoutFragmentsFromLocation_options_usingBlock(
                Some(&start),
                NSTextLayoutFragmentEnumerationOptions::empty(),
                &block,
            )
        };
        drop(block);
        let fragments = fragments.borrow().clone();
        Value::Array(fragments)
    }

    pub fn window(window: &NSWindow) -> Value {
        let mut object = Object::new()
            .with("class", class_name(window.class()))
            .with("frame", rect(window.frame()))
            .with("contentLayoutRect", rect(window.contentLayoutRect()))
            .with("title", window.title().to_string())
            .with("subtitle", window.subtitle().to_string())
            .with("styleMask", window.styleMask().0 as i64)
            .with("appearance", window.effectiveAppearance().name().to_string());
        if let Some(toolbar) = window.toolbar() {
            let items: Vec<Value> = toolbar
                .items()
                .iter()
                .map(|item| {
                    Object::new()
                        .with("identifier", item.itemIdentifier().to_string())
                        .with("viewClass", item.view().map_or(Value::Null, |view| Value::String(class_name(view.class()))))
                        .build()
                })
                .collect();
            object = object.with(
                "toolbar",
                Object::new().with("identifier", without_address(&toolbar.identifier().to_string())).with("items", Value::Array(items)),
            );
        }
        let root = window.contentView().map(|content| unsafe { content.superview() }.unwrap_or(content));
        object.with("views", root.map_or(Value::Null, |root| view(&root))).build()
    }
}

// MARK: - Session

struct Session {
    scenario: Scenario,
    output: PathBuf,
    layout: Option<PathBuf>,
    root: PathBuf,
    sandbox: PathBuf,
    scene: Option<Scene>,
    previous_capture: Option<Vec<u8>>,
    stable_captures: u32,
    deadline: Instant,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftAppWindowSession"]
    struct SessionDelegate;

    unsafe impl NSObjectProtocol for SessionDelegate {}

    unsafe impl NSApplicationDelegate for SessionDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            let mtm = self.mtm();
            let result = SESSION.with(|cell| -> Result<(), Failure> {
                let mut guard = cell.borrow_mut();
                let session = guard.as_mut().expect("session");
                apply_scenario_appearance(&session.scenario, &session.root, mtm);
                session.scene = Some(Scene::build(&session.scenario, &session.root, mtm)?);
                session.restart_settling();
                Ok(())
            });
            match result {
                Ok(()) => schedule_check(),
                Err(Failure::NotPorted) => fail("not ported", 3),
                Err(Failure::Error(message)) => fail(&message, 2),
            }
        }
    }
);

impl Session {
    fn restart_settling(&mut self) {
        self.previous_capture = None;
        self.stable_captures = 0;
        self.deadline = Instant::now() + self.scenario.settle_timeout;
    }
}

fn fail(message: &str, code: i32) -> ! {
    eprintln!("app-window failed: {message}");
    SESSION.with(|cell| {
        if let Ok(guard) = cell.try_borrow()
            && let Some(session) = guard.as_ref()
        {
            sandbox::remove(&session.sandbox);
        }
    });
    std::process::exit(code)
}

fn schedule_check() {
    let when = DispatchTime::try_from(Duration::from_millis(150)).expect("delay");
    let _ = DispatchQueue::main().after(when, check_settled);
}

/// Three byte-identical window-server captures in a row, 150 ms apart.
fn check_settled() {
    enum Next {
        Wait,
        Done(PathBuf),
    }
    let mtm = MainThreadMarker::new().expect("main thread");
    let next = SESSION.with(|cell| -> Result<Next, Failure> {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().expect("session");
        let windows = session.scene.as_ref().expect("scene").captured_windows();
        off_screen::verify(&windows, mtm);
        let png = window_server::png(&windows).map_err(|message| Failure::Error(format!("window capture failed: {message}")))?;
        if session.previous_capture.as_ref() == Some(&png) {
            session.stable_captures += 1;
        } else {
            session.stable_captures = 0;
            session.previous_capture = Some(png.clone());
        }
        if session.stable_captures < 2 && Instant::now() <= session.deadline {
            return Ok(Next::Wait);
        }
        if session.stable_captures < 2 {
            eprintln!("warning: window did not settle before the timeout");
        }
        let scenario = &session.scenario;
        if session.scene.as_mut().expect("scene").perform_next_command(scenario)? {
            session.restart_settling();
            return Ok(Next::Wait);
        }
        if let Some(layout) = &session.layout {
            let windows: Vec<Value> = windows.iter().map(|window| geometry::window(window)).collect();
            json::write(&Value::Array(windows), layout)?;
        }
        std::fs::write(&session.output, &png)?;
        Ok(Next::Done(session.sandbox.clone()))
    });
    match next {
        Ok(Next::Wait) => schedule_check(),
        Ok(Next::Done(sandbox)) => {
            sandbox::remove(&sandbox);
            std::process::exit(0)
        }
        Err(Failure::NotPorted) => fail("not ported", 3),
        Err(Failure::Error(message)) => fail(&message, 2),
    }
}

pub(crate) fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

/// `app-window`.
pub fn run(request: &Request) -> Result<(), Failure> {
    let scenario = Scenario::load(&request.input)?;
    if !is_ported(&scenario.window) {
        return Err(Failure::NotPorted);
    }
    let layout = request
        .flags
        .iter()
        .position(|flag| flag == "--layout")
        .and_then(|index| request.flags.get(index + 1))
        .map(PathBuf::from);
    let mtm = MainThreadMarker::new().ok_or_else(|| Failure::Error("app-window runs on the main thread".into()))?;
    acquire_window_capture_lock();
    off_screen::install();
    let sandbox = sandbox::prepare(&scenario)?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    SESSION.with(|cell| {
        *cell.borrow_mut() = Some(Session {
            scenario,
            output: request.output.clone(),
            layout,
            root: repository_root(),
            sandbox,
            scene: None,
            previous_capture: None,
            stable_captures: 0,
            deadline: Instant::now(),
        })
    });
    let delegate: Retained<SessionDelegate> = unsafe { msg_send![SessionDelegate::alloc(mtm), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    std::process::exit(0)
}


// MARK: - The main menu

/// `AppMenuDump`: `app-menu <scenario.json> <out.json>`.
pub fn menu(request: &Request) -> Result<(), Failure> {
    use objc2_app_kit::{NSMenu, NSMenuItem};
    let scenario = Scenario::load(&request.input)?;
    let mtm = MainThreadMarker::new().ok_or_else(|| Failure::Error("app-menu runs on the main thread".into()))?;
    let sandbox = sandbox::prepare(&scenario)?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    apply_scenario_appearance(&scenario, &repository_root(), mtm);
    let menu = upleft_app::app::main_menu::MainMenu::build(mtm);
    app.setMainMenu(Some(&menu));

    fn dump(menu: &NSMenu) -> Value {
        if let Some(delegate) = menu.delegate() {
            let responds: bool = unsafe { msg_send![&*delegate, respondsToSelector: sel!(menuNeedsUpdate:)] };
            if responds {
                let _: () = unsafe { msg_send![&*delegate, menuNeedsUpdate: menu] };
            }
        }
        let items: Vec<Value> = menu.itemArray().iter().map(|entry| dump_item(&entry)).collect();
        Object::new().with("title", menu.title().to_string()).with("items", Value::Array(items)).build()
    }

    fn dump_item(item: &NSMenuItem) -> Value {
        let represented = match item.representedObject() {
            None => Value::Null,
            Some(object) => match object.downcast::<NSString>() {
                Ok(string) => Value::String(string.to_string()),
                Err(other) => Value::String(format!("<{}>", geometry::class_name(other.class()))),
            },
        };
        let target = item.target();
        Object::new()
            .with("title", item.title().to_string())
            .with("separator", item.isSeparatorItem())
            .with("keyEquivalent", item.keyEquivalent().to_string())
            .with("modifiers", item.keyEquivalentModifierMask().0 as i64)
            .with("action", item.action().map_or(Value::Null, |action| Value::String(action.name().to_string_lossy().into_owned())))
            .with("target", target.map_or(Value::Null, |target| Value::String(geometry::class_name(target.class()))))
            .with("tag", item.tag() as i64)
            .with("representedObject", represented)
            .with("enabled", item.isEnabled())
            .with("state", item.state() as i64)
            .with("hidden", item.isHidden())
            .with("alternate", item.isAlternate())
            .with("indentation", item.indentationLevel() as i64)
            .with("image", item.image().is_some())
            .with("toolTip", item.toolTip().map_or(Value::Null, |tip| Value::String(tip.to_string())))
            .with("submenu", item.submenu().map_or(Value::Null, |submenu| dump(&submenu)))
            .build()
    }

    let value = dump(&menu);
    sandbox::remove(&sandbox);
    Ok(json::write(&value, &request.output)?)
}
