//! Tests of the toolbar controls (`App/ToolbarControls.swift`),
//! `ToolbarGlassBand` and `DocumentWindow` that need no
//! `DocumentWindowController`.
//!
//! Ported from `Tests/DownrightAppTests/PresentationSwipeTests.swift` (same
//! names and assertions): `railTracksTheSwipeAndLandsWithoutSwitchingTwice`,
//! `railReturnsToWhereItStartedWhenTheSwipeIsAbandoned`. They live here, not
//! in `presentation_swipe_tests.rs`, because they test the rail alone, on a
//! windowless control, and that suite's other tests run over the gesture
//! stand-in.
//!
//! Upleft-only (no Swift original), marked `upleft_`: the controller-free
//! assertions of `WindowChromeTests.toolbarUsesNativeCenteredModeAndTrailingMenu`
//! (sizes, titles, spacing, the Find button's glass geometry, the overflow
//! menu), the Objective-C class names, the identity view's window
//! observations, the mode buttons' keyboard path, `ToolbarGlassBand`'s hit
//! testing, and `DocumentWindow`'s Escape and in-glass click routing and
//! `shouldDismissFloatingClick` geometry.
//!
//! Every window here is borderless, sits at (-30000, -30000) and is never
//! ordered in. Events are handed to `sendEvent:` only on paths that return
//! before `super` (nothing is dispatched, nothing activates).

mod main_thread;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSEvent, NSEventModifierFlags, NSEventType, NSLayoutAttribute, NSMenu, NSMenuItem, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use upleft_app::ai::markdown_document::{Phase, PresentationState};
use upleft_app::app::document_window::DocumentWindow;
use upleft_app::app::toolbar_controls::{
    ToolbarActionButton, ToolbarDocumentIdentityView, ToolbarMenuButton, ToolbarPresentationControl,
    ToolbarTrailingCluster,
};
use upleft_app::app::toolbar_glass_band::ToolbarGlassBand;
use upleft_app::panels::appkit_support::{RectExt, accessibility_label};
use upleft_render::theme::style_sheet::StyleSheet;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

fn frame(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

/// A borderless window at (-30000, -30000), never ordered in.
fn offscreen_window() -> Retained<NSWindow> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm()),
            frame(-30000.0, -30000.0, 400.0, 300.0),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window
}

/// A borderless `DocumentWindow` at (-30000, -30000), never ordered in.
fn offscreen_document_window() -> Retained<DocumentWindow> {
    let window = DocumentWindow::new(
        frame(-30000.0, -30000.0, 400.0, 300.0),
        NSWindowStyleMask::Borderless,
        NSBackingStoreType::Buffered,
        false,
        mtm(),
    );
    unsafe { window.setReleasedWhenClosed(false) };
    window
}

fn class_name(object: &AnyObject) -> String {
    object.class().name().to_string_lossy().into_owned()
}

fn as_object<T: Message>(value: &T) -> &AnyObject {
    // SAFETY: every `Message` type is an Objective-C object.
    unsafe { &*(value as *const T).cast::<AnyObject>() }
}

/// A content view that counts the mouse-downs AppKit delivers to it (and to
/// its subviews, through the responder chain).
struct RecordingViewIvars {
    mouse_downs: Cell<usize>,
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded after the ivars are set.
    #[unsafe(super(NSView, objc2_app_kit::NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftTestRecordingView"]
    #[ivars = RecordingViewIvars]
    struct RecordingView;

    impl RecordingView {
        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {
            self.ivars().mouse_downs.set(self.ivars().mouse_downs.get() + 1);
        }
    }
);

impl RecordingView {
    fn new(frame: NSRect) -> Retained<RecordingView> {
        let this = RecordingView::alloc(mtm()).set_ivars(RecordingViewIvars { mouse_downs: Cell::new(0) });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }
}

fn accessibility_value(view: &NSView) -> Option<String> {
    let value: Option<Retained<AnyObject>> = unsafe { msg_send![view, accessibilityValue] };
    value.map(|value| {
        let string: Retained<NSString> = unsafe { msg_send![&*value, description] };
        string.to_string()
    })
}

// MARK: - PresentationSwipeTests (rail)

fn rail_tracks_the_swipe_and_lands_without_switching_twice() {
    let changes: Rc<RefCell<Vec<isize>>> = Rc::new(RefCell::new(Vec::new()));
    let haptics = Rc::new(Cell::new(0));
    let recorded = changes.clone();
    let counted = haptics.clone();
    let control = ToolbarPresentationControl::new_with_haptics(
        move |segment| recorded.borrow_mut().push(segment),
        move || counted.set(counted.get() + 1),
        mtm(),
    );
    control.setFrame(frame(0.0, 0.0, 184.0, 34.0));
    control.layoutSubtreeIfNeeded();
    let document_center = control.selected_segment_center_for_testing();

    control.track_swipe(0.25);
    assert!(control.selection_indicator_frame_for_testing().mid_x() > document_center);
    assert_eq!(control.selected_segment(), 0);
    assert_eq!(haptics.get(), 0);

    control.track_swipe(1.0);
    assert_eq!(haptics.get(), 1);
    // The document has already switched behind the transition; the rail
    // must not switch it a second time on the way past.
    assert!(changes.borrow().is_empty());

    control.settle_swipe(1);
    assert_eq!(control.selected_segment(), 1);
    assert!(changes.borrow().is_empty());
    assert!(
        (control.selection_indicator_frame_for_testing().mid_x() - control.selected_segment_center_for_testing()).abs()
            < 0.01
    );
}

fn rail_returns_to_where_it_started_when_the_swipe_is_abandoned() {
    let control = ToolbarPresentationControl::new(|_| {}, mtm());
    control.setFrame(frame(0.0, 0.0, 184.0, 34.0));
    control.layoutSubtreeIfNeeded();
    let document_center = control.selected_segment_center_for_testing();

    control.track_swipe(0.8);
    assert!(control.selection_indicator_frame_for_testing().mid_x() > document_center);

    control.settle_swipe(0);
    assert_eq!(control.selected_segment(), 0);
    assert!((control.selection_indicator_frame_for_testing().mid_x() - document_center).abs() < 0.01);
}

// MARK: - Upleft-only

/// The controller-free half of `toolbarUsesNativeCenteredModeAndTrailingMenu`:
/// the controls built with the controller's arguments
/// (`DocumentWindowController+Actions.swift`, `toolbar(_:itemForItemIdentifier:…)`).
fn upleft_toolbar_controls_meet_the_centered_mode_and_trailing_menu_geometry() {
    let window = offscreen_window();
    let identity = ToolbarDocumentIdentityView::new(&window, mtm());
    assert_eq!(identity.intrinsicContentSize().width, 220.0);
    assert_eq!(identity.intrinsicContentSize().height, 36.0);

    let mode = ToolbarPresentationControl::new(|_| {}, mtm());
    mode.setHidden(false);
    assert!(!mode.isHidden());
    assert_eq!(mode.segment_titles(), vec!["Document".to_owned(), "Source".to_owned()]);
    assert_eq!(mode.selected_segment(), 0);
    assert_eq!(mode.intrinsicContentSize().width, 184.0);
    assert_eq!(mode.intrinsicContentSize().height, 34.0);

    let find = ToolbarActionButton::new(
        "magnifyingglass",
        "Find",
        "Find in this document",
        None,
        sel!(toolbarShowFind:),
        true,
        mtm(),
    );
    find.set_style_sheet(Rc::new(StyleSheet::current(mtm())));
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm()), &NSString::from_str(""));
    for title in ["Document Detail", "Source Focus"] {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm()),
                &NSString::from_str(title),
                None,
                &NSString::from_str(""),
            )
        };
        menu.addItem(&item);
    }
    let overflow = ToolbarMenuButton::new(&menu, mtm());
    let activity = NSView::new(mtm());
    let cluster = ToolbarTrailingCluster::new(&[&activity, &find, &overflow], mtm());

    assert_eq!(cluster.spacing(), 8.0);
    assert_eq!(cluster.alignment(), NSLayoutAttribute::CenterY);
    assert_eq!(accessibility_label(&*find).as_deref(), Some("Find"));
    assert_eq!(find.intrinsicContentSize().width, 34.0);
    assert_eq!(find.intrinsicContentSize().height, 34.0);
    assert!(find.uses_glass_surface_for_testing());
    assert_eq!(find.feedback_inset_x(), 0.0);
    assert_eq!(find.feedback_inset_y(), 0.0);
    assert_eq!(find.feedback_corner_radius(), 17.0);
    assert!(!find.is_on(), "find should rest unlit with its panel closed");
    assert_eq!(overflow.intrinsicContentSize().width, 34.0);
    assert_eq!(overflow.intrinsicContentSize().height, 34.0);
    assert!(overflow.popup_menu_items().iter().any(|item| item.title().to_string() == "Document Detail"));
    assert!(overflow.popup_menu_items().iter().any(|item| {
        let title = item.title().to_string();
        title == "Source Focus" || title == "Exit Source Focus"
    }));
    let arranged = cluster.arrangedSubviews();
    assert_eq!(arranged.len(), 3);
    assert!(std::ptr::eq(&*arranged.firstObject().unwrap(), &*activity));
    assert_eq!(class_name(as_object(&*arranged.lastObject().unwrap())), "ToolbarMenuButton");
    window.close();
}

fn upleft_objective_c_class_names_match_swift() {
    let window = offscreen_window();
    let identity = ToolbarDocumentIdentityView::new(&window, mtm());
    let mode = ToolbarPresentationControl::new(|_| {}, mtm());
    let [document, source] = mode.segment_buttons_for_testing();
    let find = ToolbarActionButton::new("magnifyingglass", "Find", "Find", None, sel!(toolbarShowFind:), false, mtm());
    let overflow = ToolbarMenuButton::new(&NSMenu::new(mtm()), mtm());
    let cluster = ToolbarTrailingCluster::new(&[], mtm());
    let band = ToolbarGlassBand::new(Rc::new(StyleSheet::current(mtm())), mtm());
    let document_window = offscreen_document_window();

    assert_eq!(class_name(as_object(&*identity)), "ToolbarDocumentIdentityView");
    assert_eq!(class_name(as_object(&*identity.proxy_button_for_testing())), "ToolbarInteractiveButton");
    assert_eq!(class_name(as_object(&*mode)), "ToolbarPresentationControl");
    assert_eq!(class_name(as_object(&*document)), "ToolbarModeButton");
    assert_eq!(class_name(as_object(&*source)), "ToolbarModeButton");
    assert_eq!(class_name(as_object(&*find)), "ToolbarActionButton");
    assert_eq!(class_name(as_object(&*overflow)), "ToolbarMenuButton");
    assert_eq!(class_name(as_object(&*cluster)), "ToolbarTrailingCluster");
    assert_eq!(class_name(as_object(&*band)), "ToolbarGlassBand");
    assert_eq!(class_name(as_object(&*document_window)), "DocumentWindow");
    document_window.close();
    window.close();
}

fn upleft_document_identity_follows_its_window() {
    let window = offscreen_window();
    window.setTitle(&NSString::from_str("plan.md"));
    let identity = ToolbarDocumentIdentityView::new(&window, mtm());
    assert_eq!(accessibility_label(&*identity).as_deref(), Some("plan.md"));
    assert_eq!(identity.toolTip().map(|tip| tip.to_string()).as_deref(), Some("plan.md"));

    window.setSubtitle(&NSString::from_str("Notes"));
    assert_eq!(accessibility_label(&*identity).as_deref(), Some("plan.md, Notes"));
    window.setTitle(&NSString::from_str("draft.md"));
    assert_eq!(accessibility_label(&*identity).as_deref(), Some("draft.md, Notes"));

    window.setDocumentEdited(true);
    assert_eq!(identity.document_state(), PresentationState::new(Phase::Edited, Some("Edit".into()), None));
    assert!(identity.is_edited());
    // An edited document is not exceptional: the label stays silent.
    assert_eq!(accessibility_label(&*identity).as_deref(), Some("draft.md, Notes"));
    window.setDocumentEdited(false);
    assert_eq!(identity.document_state(), PresentationState::NEUTRAL);

    identity.set_has_external_changes(true);
    assert_eq!(identity.document_state().phase, Phase::ChangedOnDisk);
    assert_eq!(accessibility_label(&*identity).as_deref(), Some("draft.md, Notes, Changed externally"));
    assert_eq!(identity.state_label_for_testing().stringValue().to_string(), "Changed externally");
    assert!(!identity.state_label_for_testing().isHidden());
    identity.set_document_state(PresentationState::new(Phase::Conflict, Some("Paste".into()), Some("Example".into())));
    assert_eq!(
        identity.toolTip().map(|tip| tip.to_string()).as_deref(),
        Some("draft.md, Notes — Conflict — Paste: Example")
    );

    // Dropping the view removes its observations; the window keeps working.
    drop(identity);
    window.setTitle(&NSString::from_str("after.md"));
    window.setDocumentEdited(true);
    window.close();
}

fn upleft_mode_buttons_follow_selection_and_arrow_keys() {
    let changes: Rc<RefCell<Vec<isize>>> = Rc::new(RefCell::new(Vec::new()));
    let recorded = changes.clone();
    let control = ToolbarPresentationControl::new(move |segment| recorded.borrow_mut().push(segment), mtm());
    let [document, source] = control.segment_buttons_for_testing();
    assert_eq!(accessibility_value(&control).as_deref(), Some("Document"));
    assert_eq!(accessibility_value(&document).as_deref(), Some("Selected"));
    assert_eq!(document.toolTip().map(|tip| tip.to_string()).as_deref(), Some("Rendered document"));
    assert_eq!(source.toolTip().map(|tip| tip.to_string()).as_deref(), Some("Source Focus — show raw Markdown"));
    assert_eq!(document.attributedTitle().string().to_string(), "Document");

    let right_arrow = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        &NSString::from_str("\u{F703}"),
        &NSString::from_str("\u{F703}"),
        false,
        124,
    )
    .expect("key event");
    document.keyDown(&right_arrow);
    assert_eq!(control.selected_segment(), 1);
    assert_eq!(*changes.borrow(), vec![1]);
    assert_eq!(accessibility_value(&control).as_deref(), Some("Source"));
    assert_eq!(accessibility_value(&source).as_deref(), Some("Selected"));
    assert_eq!(accessibility_value(&document).as_deref(), Some("Not selected"));
    // Already on Source: another press is not a change.
    source.keyDown(&right_arrow);
    assert_eq!(*changes.borrow(), vec![1]);

    control.set_selected_segment(7);
    assert_eq!(control.selected_segment(), 1, "segments clamp to the rail");
    assert_eq!(*changes.borrow(), vec![1], "setSelectedSegment never reports");
}

fn upleft_glass_band_passes_every_hit_through() {
    let band = ToolbarGlassBand::new(Rc::new(StyleSheet::current(mtm())), mtm());
    band.setFrame(frame(0.0, 0.0, 600.0, 52.0));
    band.layoutSubtreeIfNeeded();
    assert!(band.hitTest(NSPoint::new(10.0, 10.0)).is_none());
    assert!(band.hitTest(NSPoint::new(300.0, 26.0)).is_none());
    let glass = band.glass_for_testing();
    assert!(glass.passes_through_hits());
    assert_eq!(glass.frame(), band.bounds());
    let is_element: bool = unsafe { msg_send![&*band, isAccessibilityElement] };
    assert!(!is_element);
}

fn upleft_document_window_routes_escape_and_glass_clicks() {
    let window = offscreen_document_window();
    let content = RecordingView::new(frame(0.0, 0.0, 400.0, 300.0));
    window.setContentView(Some(&content));
    let surface = NSView::initWithFrame(NSView::alloc(mtm()), frame(300.0, 200.0, 80.0, 80.0));
    content.addSubview(&surface);

    let cancels = Rc::new(Cell::new(0));
    let outside = Rc::new(Cell::new(0));
    let counted_cancels = cancels.clone();
    let counted_outside = outside.clone();
    window.set_on_floating_cancel(Some(Rc::new(move || counted_cancels.set(counted_cancels.get() + 1))));
    window.set_on_floating_outside_mouse_down(Some(Rc::new(move || counted_outside.set(counted_outside.get() + 1))));
    window.set_floating_surface(Some(&surface));
    assert!(window.floating_surface().is_some());

    let escape = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        window.windowNumber(),
        None,
        &NSString::from_str("\u{1b}"),
        &NSString::from_str("\u{1b}"),
        false,
        53,
    )
    .expect("key event");
    window.sendEvent(&escape);
    assert_eq!(cancels.get(), 1);

    // A click on the surface's glass (not on a native control) is consumed:
    // it neither dismisses nor reaches the document.
    let click = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::LeftMouseDown,
        NSPoint::new(330.0, 230.0),
        NSEventModifierFlags::empty(),
        0.0,
        window.windowNumber(),
        None,
        0,
        1,
        1.0,
    )
    .expect("mouse event");
    window.sendEvent(&click);
    assert_eq!(outside.get(), 0);
    assert_eq!(cancels.get(), 1);
    assert_eq!(content.ivars().mouse_downs.get(), 0, "the document must not receive a click on the glass");

    // `shouldDismissFloatingClick`: inside the content and outside the
    // surface dismisses; on the surface, or outside the content, does not.
    assert!(DocumentWindow::should_dismiss_floating_click(NSPoint::new(50.0, 50.0), &content, &surface));
    assert!(!DocumentWindow::should_dismiss_floating_click(NSPoint::new(330.0, 230.0), &content, &surface));
    assert!(!DocumentWindow::should_dismiss_floating_click(NSPoint::new(-10.0, 50.0), &content, &surface));
    // A surface in no window, beside content in one, is never dismissed.
    let detached = NSView::initWithFrame(NSView::alloc(mtm()), frame(0.0, 0.0, 10.0, 10.0));
    assert!(!DocumentWindow::should_dismiss_floating_click(NSPoint::new(50.0, 50.0), &content, &detached));

    window.set_floating_surface(None);
    assert!(window.floating_surface().is_none());
    window.close();
}

fn main() {
    main_thread::run(&[
        ("rail_tracks_the_swipe_and_lands_without_switching_twice", rail_tracks_the_swipe_and_lands_without_switching_twice),
        (
            "rail_returns_to_where_it_started_when_the_swipe_is_abandoned",
            rail_returns_to_where_it_started_when_the_swipe_is_abandoned,
        ),
        (
            "upleft_toolbar_controls_meet_the_centered_mode_and_trailing_menu_geometry",
            upleft_toolbar_controls_meet_the_centered_mode_and_trailing_menu_geometry,
        ),
        ("upleft_objective_c_class_names_match_swift", upleft_objective_c_class_names_match_swift),
        ("upleft_document_identity_follows_its_window", upleft_document_identity_follows_its_window),
        ("upleft_mode_buttons_follow_selection_and_arrow_keys", upleft_mode_buttons_follow_selection_and_arrow_keys),
        ("upleft_glass_band_passes_every_hit_through", upleft_glass_band_passes_every_hit_through),
        ("upleft_document_window_routes_escape_and_glass_clicks", upleft_document_window_routes_escape_and_glass_clicks),
    ]);
}
