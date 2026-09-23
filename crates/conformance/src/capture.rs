//! The Rust counterpart of `CaptureSession` in
//! `oracle/Sources/downright-oracle/RenderCapture.swift`: show a scene in an
//! on-screen borderless window of an activated app, wait until three
//! consecutive `cacheDisplay` captures are byte-identical, then capture the
//! composited window through ScreenCaptureKit.
//!
//! Every AppKit call mirrors the Swift session call for call, because the
//! harness itself must not be a source of pixel differences — the `probe`
//! suite proves that with a stock text view before any ported view is judged.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType, NSBitmapImageFileType,
    NSBitmapImageRep, NSColorSpace, NSFont, NSTextView, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_core_graphics::{CGBitmapContextCreate, CGBitmapContextCreateImage, CGContext, CGImage, CGImageAlphaInfo};
use objc2_foundation::{NSDictionary, NSError, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use objc2_screen_capture_kit::{
    SCCaptureResolutionType, SCContentFilter, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
};

/// Matches `RenderRequest` in the Swift oracle.
#[derive(Debug, Clone)]
pub struct CaptureRequest {
    pub input: PathBuf,
    pub output_png: PathBuf,
    pub output_layout: Option<PathBuf>,
    pub dark: bool,
    pub width: f64,
    pub height: f64,
    pub settle_timeout: Duration,
    pub capture_from_screen: bool,
}

/// Matches the Swift `CaptureScene` protocol.
pub trait CaptureScene {
    /// Builds the content in `window` and returns the view whose cached
    /// display decides when the scene has settled.
    fn build(&mut self, window: &NSWindow, request: &CaptureRequest, mtm: MainThreadMarker) -> Result<Retained<NSView>, String>;
    /// Runs once, after the window is ordered on screen.
    fn after_show(&mut self, window: &NSWindow);
    /// Runs before every settle check.
    fn before_settle_check(&mut self);
    /// Writes any extra outputs once the scene has settled.
    fn write_extras(&mut self, bitmap: &NSBitmapImageRep, request: &CaptureRequest) -> Result<(), String>;
    /// False while the scene is still driving itself into the state to
    /// capture; the session only counts stable captures once it is ready.
    fn is_ready(&self) -> bool {
        true
    }
    /// Further windows (child windows, panels) captured below the main
    /// window, top to bottom, in the same PNG.
    fn extra_windows(&self) -> Vec<Retained<NSWindow>> {
        Vec::new()
    }
}

struct Session {
    request: CaptureRequest,
    scene: Box<dyn CaptureScene>,
    window: Option<Retained<NSWindow>>,
    settle_view: Option<Retained<NSView>>,
    previous_capture: Option<Vec<u8>>,
    stable_captures: u32,
    deadline: Option<Instant>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftCaptureSession"]
    struct CaptureDelegate;

    unsafe impl NSObjectProtocol for CaptureDelegate {}

    unsafe impl NSApplicationDelegate for CaptureDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            if let Err(message) = start(self.mtm()) {
                fail(&message);
            }
        }
    }
);

impl CaptureDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

fn fail(message: &str) -> ! {
    eprintln!("render failed: {message}");
    std::process::exit(2)
}

/// Runs the capture to completion; the process exits from inside.
pub fn run(request: CaptureRequest, scene: Box<dyn CaptureScene>) -> ! {
    acquire_window_capture_lock();
    let mtm = MainThreadMarker::new().expect("capture runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    SESSION.with(|session| {
        *session.borrow_mut() = Some(Session {
            request,
            scene,
            window: None,
            settle_view: None,
            previous_capture: None,
            stable_captures: 0,
            deadline: None,
        })
    });
    let delegate = CaptureDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    std::process::exit(0)
}

/// One window capture at a time on this machine; see `acquireWindowCaptureLock`
/// in the Swift oracle, which takes the same lock. Held until the process exits.
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

pub fn appearance(dark: bool) -> Retained<NSAppearance> {
    let name = unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } };
    NSAppearance::appearanceNamed(name).expect("system appearance")
}

fn start(mtm: MainThreadMarker) -> Result<(), String> {
    SESSION.with(|cell| -> Result<(), String> {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().expect("session");
        let request = session.request.clone();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(request.width, request.height));
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
        window.setAppearance(Some(&appearance(request.dark)));
        window.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
        let settle_view = session.scene.build(&window, &request, mtm)?;

        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        window.orderFrontRegardless();
        session.scene.after_show(&window);
        session.deadline = Some(Instant::now() + request.settle_timeout);
        session.window = Some(window);
        session.settle_view = Some(settle_view);
        Ok(())
    })?;
    schedule_check();
    Ok(())
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
        Capture { window_numbers: Vec<u32>, output: PathBuf },
    }
    let next = SESSION.with(|cell| -> Result<Next, String> {
        let mut guard = cell.borrow_mut();
        let session = guard.as_mut().expect("session");
        session.scene.before_settle_check();
        let view = session.settle_view.clone().expect("settle view");
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
        if !session.scene.is_ready() {
            session.stable_captures = 0;
        }
        let expired = session.deadline.is_some_and(|deadline| Instant::now() > deadline);
        if session.stable_captures < 2 && !expired {
            return Ok(Next::Wait);
        }
        if session.stable_captures < 2 {
            eprintln!("warning: render did not settle before the timeout");
        }
        let request = session.request.clone();
        session.scene.write_extras(&rep, &request)?;
        if !request.capture_from_screen {
            let data = view_capture(&*session.scene, &rep, png)?;
            std::fs::write(&request.output_png, &data).map_err(|error| format!("write failed: {error}"))?;
            return Ok(Next::Written);
        }
        let window = session.window.as_ref().expect("window");
        let mut window_numbers = vec![window.windowNumber() as u32];
        window_numbers.extend(session.scene.extra_windows().iter().map(|extra| extra.windowNumber() as u32));
        Ok(Next::Capture { window_numbers, output: request.output_png })
    });
    match next {
        Err(message) => fail(&message),
        Ok(Next::Wait) => schedule_check(),
        Ok(Next::Written) => std::process::exit(0),
        Ok(Next::Capture { window_numbers, output }) => capture_window_from_screen(window_numbers, output),
    }
}

/// See `viewCapture` in the Swift oracle: `--capture view` with each extra
/// window's content view cached and stacked beneath the settle view.
fn view_capture(scene: &dyn CaptureScene, main: &NSBitmapImageRep, png: Vec<u8>) -> Result<Vec<u8>, String> {
    let extras = scene.extra_windows();
    let Some(first) = main.CGImage() else { return Ok(png) };
    if extras.is_empty() {
        return Ok(png);
    }
    let mut images = vec![first];
    for extra in &extras {
        let view = extra.contentView().ok_or("no bitmap representation for an extra window")?;
        let bounds = view.bounds();
        let rep = view.bitmapImageRepForCachingDisplayInRect(bounds).ok_or("no bitmap representation for an extra window")?;
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        images.push(rep.CGImage().ok_or("no image for an extra window")?);
    }
    let stacked = stack_images(&images)?;
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &stacked);
    png_data(&rep).ok_or_else(|| "PNG encoding of the stacked capture failed".to_owned())
}

/// See `captureWindowFromScreen` in the Swift oracle: the main window, then
/// each extra window, stacked when there is more than one.
fn capture_window_from_screen(window_numbers: Vec<u32>, output: PathBuf) {
    let on_content = RcBlock::new(move |content: *mut SCShareableContent, error: *mut NSError| {
        let Some(content) = (unsafe { content.as_ref() }) else {
            let message = unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string());
            fail(&format!("window capture failed: {}", message.unwrap_or_default()));
        };
        let windows = unsafe { content.windows() };
        let mut filters = Vec::new();
        for number in &window_numbers {
            let Some(sc_window) = windows.iter().find(|window| unsafe { window.windowID() } == *number) else {
                fail("window capture failed: ScreenCaptureKit does not list the render window");
            };
            filters.push(unsafe { SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &sc_window) });
        }
        capture_next(Arc::new(filters), Arc::new(Mutex::new(Vec::new())), output.clone());
    });
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            false, true, &on_content,
        )
    };
}

type Captured = Arc<Mutex<Vec<objc2_core_foundation::CFRetained<CGImage>>>>;

fn capture_next(filters: Arc<Vec<Retained<SCContentFilter>>>, images: Captured, output: PathBuf) {
    let index = images.lock().unwrap().len();
    if index == filters.len() {
        let images = images.lock().unwrap();
        let image = if images.len() == 1 {
            images[0].clone()
        } else {
            stack_images(&images).unwrap_or_else(|message| fail(&format!("window capture failed: {message}")))
        };
        let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
        let Some(png) = png_data(&rep) else {
            fail("window capture failed: PNG encoding of the window capture failed");
        };
        if let Err(error) = std::fs::write(&output, png) {
            fail(&format!("window capture failed: {error}"));
        }
        std::process::exit(0);
    }
    let filter = &filters[index];
    let configuration = unsafe { SCStreamConfiguration::new() };
    unsafe {
        let rect = filter.contentRect();
        let scale = f64::from(filter.pointPixelScale());
        configuration.setWidth((rect.size.width * scale) as usize);
        configuration.setHeight((rect.size.height * scale) as usize);
        configuration.setShowsCursor(false);
        configuration.setIgnoreShadowsSingleWindow(true);
        configuration.setCaptureResolution(SCCaptureResolutionType::Best);
    }
    let next_filters = filters.clone();
    let on_image = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        let Some(image) = (unsafe { image.as_ref() }) else {
            let message = unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string());
            fail(&format!("window capture failed: {}", message.unwrap_or_default()));
        };
        images.lock().unwrap().push(objc2_core_foundation::Type::retain(image));
        capture_next(next_filters.clone(), images.clone(), output.clone());
    });
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(filter, &configuration, Some(&on_image))
    };
}

/// See `stackImages` in the Swift oracle: the captures one above the other,
/// left-aligned, in the first capture's colour space.
fn stack_images<T: std::ops::Deref<Target = CGImage>>(images: &[T]) -> Result<objc2_core_foundation::CFRetained<CGImage>, String> {
    let width = images.iter().map(|image| CGImage::width(Some(image))).max().unwrap_or(0);
    let height: usize = images.iter().map(|image| CGImage::height(Some(image))).sum();
    let space = images.first().and_then(|image| CGImage::color_space(Some(image))).ok_or("cannot make the stacking context")?;
    // SAFETY: a fresh context that owns its own buffer.
    let context = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            width,
            height,
            8,
            0,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }
    .ok_or("cannot make the stacking context")?;
    let mut top = height;
    for image in images {
        let image_height = CGImage::height(Some(image));
        top -= image_height;
        CGContext::draw_image(
            Some(&context),
            NSRect::new(
                NSPoint::new(0.0, top as f64),
                NSSize::new(CGImage::width(Some(image)) as f64, image_height as f64),
            ),
            Some(image),
        );
    }
    CGBitmapContextCreateImage(Some(&context)).ok_or_else(|| "stacking failed".to_owned())
}

/// See `ProbeScene` in the Swift oracle: a stock TextKit 2 text view showing
/// the input verbatim, to prove the harness before any port is judged by it.
pub struct ProbeScene;

impl CaptureScene for ProbeScene {
    fn build(&mut self, window: &NSWindow, request: &CaptureRequest, mtm: MainThreadMarker) -> Result<Retained<NSView>, String> {
        let text = crate::dump::markup::read_text(&request.input).map_err(|error| format!("{error:?}"))?;
        let scroll_view = NSTextView::scrollableTextView(mtm);
        scroll_view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(request.width, request.height)));
        let text_view = scroll_view
            .documentView()
            .and_then(|view| view.downcast::<NSTextView>().ok())
            .ok_or("scrollableTextView has no text view")?;
        text_view.setFont(Some(&NSFont::systemFontOfSize(15.0)));
        text_view.setTextContainerInset(NSSize::new(24.0, 24.0));
        text_view.setString(&NSString::from_str(&text));
        window.setContentView(Some(&scroll_view));
        Ok(Retained::into_super(scroll_view))
    }

    fn after_show(&mut self, window: &NSWindow) {
        window.layoutIfNeeded();
    }

    fn before_settle_check(&mut self) {}

    fn write_extras(&mut self, _bitmap: &NSBitmapImageRep, _request: &CaptureRequest) -> Result<(), String> {
        Ok(())
    }
}
