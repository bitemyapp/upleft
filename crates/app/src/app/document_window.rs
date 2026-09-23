//! Port of `App/DocumentWindow.swift`.
//!
//! Document windows route the one event that a floating surface cannot
//! receive itself: a left click on the document behind it. This is cheaper
//! than an `NSEvent` local monitor per controller and preserves the original
//! event so the click that dismisses the panel can still place the caret.
//!
//! Objective-C name: `DocumentWindow`. Swift declares no initialiser, so the
//! controller calls `NSWindow`'s designated one; [`DocumentWindow::new`] is
//! `DocumentWindow(contentRect:styleMask:backing:defer:)`.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSEvent, NSEventModifierFlags, NSEventType, NSResponder, NSTextField, NSTextView, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::panels::appkit_support::{RectExt, downcast, object};
use crate::panels::floating_panel_surface::FloatingPanelSurface;

pub struct DocumentWindowIvars {
    on_floating_outside_mouse_down: RefCell<Option<Rc<dyn Fn()>>>,
    on_floating_cancel: RefCell<Option<Rc<dyn Fn()>>>,
    floating_surface: RefCell<ObjcWeak<NSView>>,
}

define_class!(
    /// A document window that routes clicks and Escape around its floating
    /// surface.
    // SAFETY: `initWithContentRect:styleMask:backing:defer:` is forwarded in
    // `new` after the ivars are set; `sendEvent:` keeps AppKit's signature.
    #[unsafe(super(NSWindow, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DocumentWindow"]
    #[ivars = DocumentWindowIvars]
    pub struct DocumentWindow;

    unsafe impl NSObjectProtocol for DocumentWindow {}

    impl DocumentWindow {
        #[unsafe(method(sendEvent:))]
        fn __send_event(&self, event: &NSEvent) {
            self.send_event(event);
        }
    }
);

/// `NSRect(origin: point, size: .zero)`.
fn point_rect(point: NSPoint) -> NSRect {
    NSRect::new(point, NSSize::new(0.0, 0.0))
}

/// Swift's `a === b` on two optional windows.
fn same_window(a: Option<&NSWindow>, b: Option<&NSWindow>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => std::ptr::eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `(surface as? FloatingPanelSurface)?.visibleBodyBoundsForHitTesting ??
/// surface.bounds`.
fn visible_bounds(surface: &NSView) -> NSRect {
    match downcast::<FloatingPanelSurface>(object(surface)) {
        Some(floating) => floating.visible_body_bounds_for_hit_testing(),
        None => surface.bounds(),
    }
}

impl DocumentWindow {
    /// `DocumentWindow(contentRect:styleMask:backing:defer:)`.
    pub fn new(
        content_rect: NSRect,
        style: NSWindowStyleMask,
        backing: NSBackingStoreType,
        defer: bool,
        mtm: MainThreadMarker,
    ) -> Retained<DocumentWindow> {
        let this = Self::alloc(mtm).set_ivars(DocumentWindowIvars {
            on_floating_outside_mouse_down: RefCell::new(None),
            on_floating_cancel: RefCell::new(None),
            floating_surface: RefCell::new(ObjcWeak::default()),
        });
        unsafe {
            msg_send![
                super(this),
                initWithContentRect: content_rect,
                styleMask: style,
                backing: backing,
                defer: defer
            ]
        }
    }

    /// `onFloatingOutsideMouseDown`.
    pub fn set_on_floating_outside_mouse_down(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_floating_outside_mouse_down.borrow_mut() = handler;
    }

    /// `onFloatingCancel`.
    pub fn set_on_floating_cancel(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_floating_cancel.borrow_mut() = handler;
    }

    /// `floatingSurface` (weak).
    pub fn floating_surface(&self) -> Option<Retained<NSView>> {
        self.ivars().floating_surface.borrow().load()
    }

    /// `floatingSurface = surface`.
    pub fn set_floating_surface(&self, surface: Option<&NSView>) {
        *self.ivars().floating_surface.borrow_mut() = surface.map(ObjcWeak::from).unwrap_or_default();
    }

    /// `shouldDismissFloatingClick(at:content:surface:)`.
    pub fn should_dismiss_floating_click(location_in_window: NSPoint, content: &NSView, surface: &NSView) -> bool {
        let content_point = content.convertPoint_fromView(location_in_window, None);
        let content_window = content.window();
        let surface_window = surface.window();
        let surface_point = if same_window(surface_window.as_deref(), content_window.as_deref()) {
            surface.convertPoint_fromView(location_in_window, None)
        } else if let (Some(source_window), Some(surface_window)) = (content_window, surface_window) {
            let screen_point = source_window.convertRectToScreen(point_rect(location_in_window)).origin;
            let child_point = surface_window.convertRectFromScreen(point_rect(screen_point)).origin;
            surface.convertPoint_fromView(child_point, None)
        } else {
            return false;
        };
        let visible_bounds = visible_bounds(surface);
        content.bounds().contains_point(content_point) && !visible_bounds.contains_point(surface_point)
    }

    fn send_event(&self, event: &NSEvent) {
        let kind = event.r#type();
        if kind == NSEventType::KeyDown
            && event.keyCode() == 53
            && self.floating_surface().is_some()
            && event.modifierFlags().intersection(NSEventModifierFlags::DeviceIndependentFlagsMask).is_empty()
        {
            let text_view = self.firstResponder().and_then(|responder| downcast::<NSTextView>(object(&*responder)));
            if let Some(text_view) = text_view {
                let has_marked_text: bool = unsafe { msg_send![&*text_view, hasMarkedText] };
                if has_marked_text {
                    let _: () = unsafe { msg_send![super(self), sendEvent: event] };
                    return;
                }
            }
            let handler = self.ivars().on_floating_cancel.borrow().clone();
            if let Some(handler) = handler {
                handler();
            }
            return;
        }
        if kind == NSEventType::LeftMouseDown
            && let Some(surface) = self.floating_surface()
            && let Some(surface_window) = surface.window()
            && (std::ptr::eq(&*surface_window, &**self)
                || self
                    .childWindows()
                    .is_some_and(|children| children.iter().any(|child| std::ptr::eq(&*child, &*surface_window))))
            && let Some(content) = self.contentView()
        {
            // `event.locationInWindow` belongs to this document window,
            // while the surface now lives in a detached child window.
            // Convert through screen space before asking the panel for local
            // hit geometry.
            let screen_point = self.convertRectToScreen(point_rect(event.locationInWindow())).origin;
            let surface_window_point = surface_window.convertRectFromScreen(point_rect(screen_point)).origin;
            let surface_point = surface.convertPoint_fromView(surface_window_point, None);
            let floating = downcast::<FloatingPanelSurface>(object(&*surface));
            let visible_bounds = match &floating {
                Some(floating) => floating.visible_body_bounds_for_hit_testing(),
                None => surface.bounds(),
            };
            if visible_bounds.contains_point(surface_point)
                && let Some(native_control) =
                    floating.as_ref().and_then(|floating| floating.native_control(surface_window_point))
            {
                if let Some(field) = downcast::<NSTextField>(object(&*native_control))
                    && field.isEditable()
                {
                    self.makeFirstResponder(Some(&field));
                    // SAFETY: `selectText:` takes any sender, including nil.
                    unsafe { field.selectText(None) };
                    return;
                }
                native_control.mouseDown(event);
                return;
            }
            if visible_bounds.contains_point(surface_point) {
                // The click is inside glass but not on a control. Consume it;
                // the document beneath must never receive a caret/selection
                // event through the panel.
                return;
            }
            if Self::should_dismiss_floating_click(event.locationInWindow(), &content, &surface) {
                // AppKit's menu tracking and context menus do not enter this
                // left-button path. The event remains untouched so the
                // document receives the same click after dismissal.
                let handler = self.ivars().on_floating_outside_mouse_down.borrow().clone();
                if let Some(handler) = handler {
                    handler();
                }
            }
        }
        let _: () = unsafe { msg_send![super(self), sendEvent: event] };
    }
}
