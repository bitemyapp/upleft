//! A stand-in for the parts of `DocumentWindowController` the gesture tests
//! use, until the window controller is ported.
//!
//! The Swift gesture suites build a real `DocumentWindowController` and use it
//! for three things: its panes (`documentPanes`, `primaryContainer`), its
//! style sheet (`activeStyleSheet`, with Reduce Motion overridden), and — for
//! the presentation swipe — the presentation segment that the swipe's host
//! reads and writes (`presentationSegment`, `changePresentation(to:)`,
//! `setPresentationSegment(_:)`, `documentLineCount`). This module supplies
//! exactly those, over a real `MarkdownContainerView` in a real window:
//!
//! - the window is borderless, sits at (-30000, -30000) and is never ordered
//!   in, so nothing reaches a screen and nothing takes focus;
//! - the presentation host records the segment instead of rebuilding the text
//!   view in Source mode, and its rail callbacks do nothing (the rail,
//!   `ToolbarPresentationControl`, is not ported yet).
//!
//! Tests whose assertions are about the window controller's own behaviour
//! (the rail, window resizing, split view, jump history, the commands, the
//! text view's mode) are not run against this stand-in; each test file lists
//! them as skipped.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{NSAppearanceCustomization, NSBackingStoreType, NSTextStorage, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use upleft_app::app::presentation_drag::PresentationSwitchBudget;
use upleft_app::app::presentation_swipe::{Host as PresentationSwipeHost, PresentationSwipeCoordinator};
use upleft_core::DirtySet;
use upleft_core::parser::MarkdownParser;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::markdown_container_view::MarkdownContainerView;

/// The document window's stand-in.
pub struct DocumentStandIn {
    pub window: Retained<NSWindow>,
    pub primary_container: Retained<MarkdownContainerView>,
    pub storage: Retained<NSTextStorage>,
    active_style_sheet: Rc<StyleSheet>,
    document_line_count: isize,
    segment: Rc<Cell<isize>>,
    commits: Rc<RefCell<Vec<isize>>>,
    presentation_swipe: RefCell<Option<Rc<PresentationSwipeCoordinator>>>,
}

impl DocumentStandIn {
    /// `DocumentWindowController()` with `text` open, its window framed to
    /// `width`×`height` and laid out, and — when `reduce_motion` is given —
    /// `activeStyleSheet = StyleSheet(theme: activeStyleSheet.theme,
    /// appearance: window.effectiveAppearance, reduceMotionOverride:)`.
    pub fn new(text: &str, width: f64, height: f64, reduce_motion: Option<bool>, mtm: MainThreadMarker) -> DocumentStandIn {
        let storage: Retained<NSTextStorage> =
            unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*NSString::from_str(text)] };
        let initial = Rc::new(StyleSheet::current(mtm));
        let container = MarkdownContainerView::new(&storage, initial.clone(), mtm);
        let parsed = MarkdownParser::parse(text);
        let document_line_count = parsed.line_starts.len() as isize;
        container.text_view().update(parsed, &DirtySet::wholesale(), true);

        let frame = NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(width, height));
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
        window.setContentView(Some(&container));
        window.setFrame_display(frame, false);
        window.layoutIfNeeded();
        container.layoutSubtreeIfNeeded();

        let active_style_sheet = match reduce_motion {
            Some(reduce_motion) => {
                let sheet = Rc::new(StyleSheet::new(initial.theme.clone(), &window.effectiveAppearance(), Some(reduce_motion)));
                container.set_style_sheet(sheet.clone());
                sheet
            }
            None => initial,
        };

        DocumentStandIn {
            window,
            primary_container: container,
            storage,
            active_style_sheet,
            document_line_count,
            segment: Rc::new(Cell::new(0)),
            commits: Rc::new(RefCell::new(Vec::new())),
            presentation_swipe: RefCell::new(None),
        }
    }

    /// `sizedController(reduceMotion:)`: an empty document in a 900×700 window.
    pub fn sized(reduce_motion: bool, mtm: MainThreadMarker) -> DocumentStandIn {
        DocumentStandIn::new("", 900.0, 700.0, Some(reduce_motion), mtm)
    }

    /// `documentPanes`: no split, so the primary pane alone.
    pub fn document_panes(&self) -> Vec<Retained<MarkdownContainerView>> {
        vec![self.primary_container.clone()]
    }

    /// `activeStyleSheet`.
    pub fn active_style_sheet(&self) -> Rc<StyleSheet> {
        self.active_style_sheet.clone()
    }

    /// `documentLineCount`: `parsed.lineStarts.count`.
    pub fn document_line_count(&self) -> isize {
        self.document_line_count
    }

    /// `presentationSegment`: `0` Document, `1` Source.
    pub fn presentation_segment(&self) -> isize {
        self.segment.get()
    }

    /// `changePresentation(to:)`, recorded.
    pub fn change_presentation(&self, segment: isize) {
        self.commits.borrow_mut().push(segment);
        self.segment.set(segment);
    }

    /// Every `commitSegment` the swipe asked for.
    pub fn commits(&self) -> Vec<isize> {
        self.commits.borrow().clone()
    }

    /// `presentationSwipe`: built on first use, as the Swift `lazy var` is,
    /// with the window controller's host wiring minus the rail.
    pub fn presentation_swipe(&self) -> Rc<PresentationSwipeCoordinator> {
        if let Some(swipe) = self.presentation_swipe.borrow().as_ref() {
            return swipe.clone();
        }
        let container = self.primary_container.clone();
        let style_sheet = self.active_style_sheet.clone();
        let selected = self.segment.clone();
        let committed = self.segment.clone();
        let commits = self.commits.clone();
        let lines = self.document_line_count;
        let set = self.segment.clone();
        let swipe = PresentationSwipeCoordinator::new(PresentationSwipeHost {
            panes: Box::new(move || vec![container.clone()]),
            style_sheet: Box::new(move || style_sheet.clone()),
            selected_segment: Box::new(move || selected.get()),
            commit_segment: Box::new(move |segment| {
                commits.borrow_mut().push(segment);
                committed.set(segment);
            }),
            track_rail: Box::new(|_| {}),
            settle_rail: Box::new(|_| {}),
            document_lines: Box::new(move || lines),
            set_segment: Box::new(move |segment| set.set(segment)),
        });
        *self.presentation_swipe.borrow_mut() = Some(swipe.clone());
        swipe
    }

    /// `close()`.
    pub fn close(&self) {
        self.window.close();
    }
}

impl Drop for DocumentStandIn {
    /// `defer { controller.close() }`.
    fn drop(&mut self) {
        self.close();
    }
}

/// `scrollLayer.transform.m41` of a pane, if it has a layer.
pub fn translation_x(container: &MarkdownContainerView) -> Option<f64> {
    container.scroll_view().layer().map(|layer| layer.transform().m41)
}

/// `defer { PresentationSwitchBudget.resetCalibrationForTesting() }`.
pub struct ResetCalibration;

impl Drop for ResetCalibration {
    fn drop(&mut self) {
        PresentationSwitchBudget::reset_calibration_for_testing();
    }
}
