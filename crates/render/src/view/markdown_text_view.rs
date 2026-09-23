//! Port of `View/MarkdownTextView.swift`: one text surface with an editable
//! document view and explicit source view.
//!
//! A single `NSTextView` on TextKit 2 over a single `NSTextStorage`, with
//! three decoration policies. Switching modes preserves scroll and selection
//! because nothing is rebuilt — the same layout manager keeps laying out the
//! same bytes with a different set of substitutions.
//!
//! Three rules run through the whole file:
//!
//!  * **Source offsets are the only truth.** Everything public speaks them;
//!    TextKit's hybrid offsets are converted at the boundary via `DisplayMap`.
//!  * **The AppKit layer is owned directly** (§14).
//!  * **Read mode has no insertion caret but every pointer interaction stays
//!    live** (§3.2, §5).
//!
//! The pointer, editing, clipboard, drag and Quick Look half of the class is
//! `markdown_text_view_interaction` (`MarkdownTextView+Interaction.swift`);
//! its Objective-C overrides are registered here, because a class is defined
//! in one place.
//!
//! State lives in `Cell`/`RefCell` ivars that are borrowed only for the
//! statement that reads or writes them: AppKit re-enters this class from
//! inside its own calls (a `super` selection change, a storage edit, a
//! layout pass), exactly as it re-enters the Swift one.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityCustomAction, NSAccessibilityElement, NSAttributedStringNSStringDrawing, NSBezierPath,
    NSClipView, NSColor, NSDragOperation, NSDraggingInfo, NSEvent, NSEventPhase, NSFont,
    NSFontWeightMedium, NSGraphicsContext, NSMenu, NSMutableParagraphStyle, NSParagraphStyle, NSPasteboard,
    NSResponder, NSScrollView, NSSelectionAffinity, NSText, NSTextAlignment,
    NSTextContainer, NSTextLayoutManager, NSTextRange,
    NSTextStorage, NSTextStorageObserving, NSTextTab, NSTextView,
    NSTrackingArea, NSTrackingAreaOptions, NSUnderlineStyle, NSView, NSWindowDidResignKeyNotification,
    NSViewBoundsDidChangeNotification, NSLineBreakMode,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGContext;
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSNotification,
    NSNotificationCenter, NSNumber, NSOperationQueue, NSPoint, NSRect, NSSize, NSString, NSValue,
};
use upleft_core::{BlockContent, DirtySet, NSRange, ParsedDocument, PathToken, TextEdit, ZoomLevel};

use objc2_app_kit::{NSTextElementProvider, NSTextSelectionDataSource};
use crate::appkit_compat::{
    RECT_ZERO, RectExt, WorkItem, attribute_value, attributed_string, enumerate_attribute, from_ns, keys, main_async,
    ns, rect,
};
use crate::core_types::ChangeKind;
use crate::engine::decoration_engine::DecorationEngine;
use crate::engine::display_map::{DisplayMap, DisplaySubstitution, ParagraphIndex, RangeSet, SourceEditProjection};
use crate::engine::elision_plan::ElisionPlan;
use crate::engine::marker_policy::MarkerPolicy;
use crate::engine::render_metrics;
use crate::fragments::fragment_base::{CheckboxPulse, FragmentContext, cf_absolute_time_get_current};
use crate::fragments::inline_math_display::InlineMathDisplay;
use crate::motion::{self, SpringDriver, SpringScalar};
use crate::render_contracts::{
    DecorationPolicy, FragmentPayload, MarkdownRenderConfiguration, MarkdownRevealPolicy, RenderMode, SourceFocus,
    attribute_keys,
};
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::StyleSheet;
use crate::view::base_display_map::{self, BaseDisplayMapInputs, WordJoinerRuns};
use crate::view::fragment_provider::FragmentProvider;
use crate::view::gutter_rail_view::GutterRailView;
use crate::view::markdown_content_storage::MarkdownContentStorage;
use crate::view::markdown_text_view_delegate::{MarkdownTextViewDelegate, ScrollPosition};
use crate::view::paragraph_substitution::ParagraphSubstitution;
use crate::view::tracking_area::refresh_tracking_area;

// MARK: - FragmentAccessibilityElement

pub struct FragmentAccessibilityElementIvars {
    text_view: ObjcWeak<MarkdownTextView>,
    source_offset: isize,
    on_press: RefCell<Option<Box<dyn Fn() -> bool>>>,
}

define_class!(
    /// Accessibility geometry is demand-driven (`FragmentAccessibilityElement`).
    // SAFETY: NSAccessibilityElement's initialiser is `init`; overrides keep
    // AppKit's signatures. No Drop impl.
    #[unsafe(super(NSAccessibilityElement))]
    #[thread_kind = MainThreadOnly]
    #[name = "FragmentAccessibilityElement"]
    #[ivars = FragmentAccessibilityElementIvars]
    struct FragmentAccessibilityElement;

    impl FragmentAccessibilityElement {
        #[unsafe(method(accessibilityFrameInParentSpace))]
        fn accessibility_frame_in_parent_space(&self) -> NSRect {
            self.ivars()
                .text_view
                .load()
                .and_then(|view| view.rect_for_offset(self.ivars().source_offset))
                .unwrap_or(RECT_ZERO)
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            self.ivars().on_press.borrow().as_ref().is_some_and(|press| press())
        }
    }
);

impl FragmentAccessibilityElement {
    fn new(text_view: &MarkdownTextView, source_offset: isize) -> Retained<Self> {
        let this = Self::alloc(text_view.mtm()).set_ivars(FragmentAccessibilityElementIvars {
            text_view: ObjcWeak::from(text_view),
            source_offset,
            on_press: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }
}

// MARK: - Content resize policy

/// A height update request. Structural updates keep the document height
/// correct, but semantic parse results wait for an idle gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentResizeRequest {
    Semantic,
    LineCount,
    ScrollRepair,
    Viewport,
    Immediate,
}

pub struct ContentResizePolicy;

impl ContentResizePolicy {
    pub const SEMANTIC_IDLE_DELAY: f64 = 0.080;
    pub const LINE_COUNT_IDLE_DELAY: f64 = 0.040;

    fn rank(request: ContentResizeRequest) -> u8 {
        match request {
            ContentResizeRequest::Semantic => 0,
            ContentResizeRequest::ScrollRepair => 1,
            ContentResizeRequest::LineCount => 2,
            ContentResizeRequest::Viewport => 3,
            ContentResizeRequest::Immediate => 4,
        }
    }

    pub fn merge(current: Option<ContentResizeRequest>, next: ContentResizeRequest) -> ContentResizeRequest {
        let Some(current) = current else { return next };
        if Self::rank(next) >= Self::rank(current) { next } else { current }
    }

    pub fn idle_delay(request: ContentResizeRequest) -> f64 {
        match request {
            ContentResizeRequest::Semantic => Self::SEMANTIC_IDLE_DELAY,
            ContentResizeRequest::LineCount | ContentResizeRequest::ScrollRepair => Self::LINE_COUNT_IDLE_DELAY,
            ContentResizeRequest::Viewport | ContentResizeRequest::Immediate => 0.0,
        }
    }
}

// MARK: - Public value types

/// One external change, as the document surface needs to draw it (§8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeMark {
    pub kind: ChangeKind,
    pub range: NSRange,
    pub words: Vec<NSRange>,
    pub visited: bool,
    pub deleted_text: String,
}

impl ChangeMark {
    pub fn new(kind: ChangeKind, range: NSRange, words: Vec<NSRange>) -> ChangeMark {
        ChangeMark { kind, range, words, visited: false, deleted_text: String::new() }
    }
}

/// Where the reader is looking, in a form that survives a relayout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportAnchor {
    pub offset: isize,
    /// Signed distance from the anchor line's top edge down to the
    /// viewport's top edge, in document coordinates.
    pub gap: CGFloat,
}

/// The only thing that varies between source paragraphs.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SourceParagraphSpacing {
    before: CGFloat,
    after: CGFloat,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentLayoutScope {
    Viewport,
    Document,
}

/// Host hook for the command/keybinding layer.
pub type KeyEventHandler = Rc<dyn Fn(&NSEvent) -> bool>;

// MARK: - Ivars

pub struct MarkdownTextViewIvars {
    // Public surface
    markdown_delegate: RefCell<Option<Weak<dyn MarkdownTextViewDelegate>>>,
    text_magnification_accumulator: Cell<CGFloat>,
    key_event_handler: RefCell<Option<KeyEventHandler>>,
    parsed_document: RefCell<Arc<ParsedDocument>>,
    mode: Cell<RenderMode>,
    configuration: RefCell<MarkdownRenderConfiguration>,
    source_focus: Cell<SourceFocus>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    zoom_level: Cell<ZoomLevel>,
    folded_heading_slugs: RefCell<HashSet<String>>,
    search_hits: RefCell<Vec<NSRange>>,
    current_search_hit: Cell<Option<NSRange>>,
    speech_highlight: Cell<Option<NSRange>>,
    change_marks: RefCell<Vec<ChangeMark>>,
    document_url: RefCell<Option<String>>,

    // Internals
    pub(crate) engine: RefCell<DecorationEngine>,
    pub(crate) fragment_context: Rc<FragmentContext>,
    substitution: Retained<ParagraphSubstitution>,
    fragment_provider: RefCell<Option<Retained<FragmentProvider>>>,
    content_storage: Retained<MarkdownContentStorage>,
    markdown_layout_manager: Retained<NSTextLayoutManager>,

    base_hidden_ranges: RefCell<Vec<NSRange>>,
    base_display_map: RefCell<DisplayMap>,
    base_layout_map: RefCell<DisplayMap>,
    paragraph_index: RefCell<ParagraphIndex>,
    display_map: RefCell<DisplayMap>,
    hard_wrap_ranges: RefCell<Vec<NSRange>>,
    hard_wrap_substitutions: RefCell<Vec<DisplaySubstitution>>,
    elision: RefCell<ElisionPlan>,
    expanded_elision_ranges: RefCell<Vec<NSRange>>,

    is_applying_selection: Cell<bool>,
    is_performing_source_edit: Cell<bool>,
    last_mutation_provenance: RefCell<String>,
    pub(crate) should_follow_caret_after_local_edit: Cell<bool>,
    pub(crate) local_edit_viewport_anchor: Cell<Option<ViewportAnchor>>,
    pub(crate) is_tracking_mouse_selection: Cell<bool>,
    pub(crate) suppresses_caret_reveal: Cell<bool>,
    anchored_paragraph: Cell<Option<NSRange>>,
    reveal_paragraph: Cell<Option<NSRange>>,
    elision_was_identity: Cell<bool>,
    overlay_ranges: RefCell<Vec<NSRange>>,
    object_change_marks: RefCell<Vec<ChangeMark>>,
    fragment_accessibility_elements: RefCell<Vec<Retained<FragmentAccessibilityElement>>>,
    path_existence: RefCell<HashMap<PathToken, bool>>,
    path_refresh_generation: Cell<u64>,
    code_collapse_overrides: RefCell<HashMap<isize, bool>>,
    invisibles_applied: Cell<bool>,
    hover_tracking: RefCell<Option<Retained<NSTrackingArea>>>,
    scroll_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    resign_key_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    pending_resize_request: Cell<Option<ContentResizeRequest>>,
    resize_work_item: RefCell<Option<WorkItem>>,
    pub(crate) copied_code_feedback_work_item: RefCell<Option<WorkItem>>,
    motion_driver: RefCell<Option<SpringDriver>>,
    resize_generation: Cell<u64>,
    viewport_repair_generation: Cell<u64>,
    pending_resize_anchor: Cell<Option<ViewportAnchor>>,
    pending_resize_viewport_y: Cell<Option<CGFloat>>,
    next_document_update_viewport_y: Cell<Option<CGFloat>>,
    resize_needs_repair: Cell<bool>,
    applied_source_focus: Cell<SourceFocus>,
    pending_shrink_repair: Cell<bool>,
    pending_shrink_repair_work_item: RefCell<Option<WorkItem>>,
    applying_pending_shrink_repair: Cell<bool>,
    scroll_coalesce_work_item: RefCell<Option<WorkItem>>,
    scroll_coalesce_generation: Cell<u64>,

    last_fragment_invalidation_range_for_testing: Cell<Option<Option<NSRange>>>,
    pub(crate) hovered_heading_index: Cell<Option<usize>>,
    pub(crate) hovered_link_range: Cell<Option<NSRange>>,
    pub(crate) drop_insertion_offset: Cell<Option<isize>>,
    pub(crate) claims_active_drag: Cell<bool>,
    pub(crate) composing_paragraph: Cell<Option<NSRange>>,
    update_generation: Cell<isize>,
    gutter_rail: RefCell<ObjcWeak<GutterRailView>>,
    word_joiner_runs: RefCell<WordJoinerRuns>,

    scroll_spring: Cell<SpringScalar>,
    scroll_spring_is_active: Cell<bool>,
    scroll_spring_clip: RefCell<ObjcWeak<NSClipView>>,
    pending_scroll_y: Cell<Option<CGFloat>>,
    pending_motion_invalidation: Cell<Option<NSRect>>,
}

impl Drop for MarkdownTextViewIvars {
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        if let Some(observer) = self.scroll_observer.get_mut().take() {
            unsafe { center.removeObserver(observer.as_ref()) };
        }
        if let Some(observer) = self.resign_key_observer.get_mut().take() {
            unsafe { center.removeObserver(observer.as_ref()) };
        }
        for item in [
            self.resize_work_item.get_mut().take(),
            self.pending_shrink_repair_work_item.get_mut().take(),
            self.scroll_coalesce_work_item.get_mut().take(),
            self.copied_code_feedback_work_item.get_mut().take(),
        ]
        .into_iter()
        .flatten()
        {
            item.cancel();
        }
        if let Some(driver) = self.motion_driver.get_mut().take() {
            driver.park();
        }
    }
}

// MARK: - The class

define_class!(
    /// `MarkdownTextView`: the document surface.
    // SAFETY: NSTextView's designated initialiser is
    // `initWithFrame:textContainer:`, forwarded in `new` after the ivars are
    // set. Every override keeps AppKit's signature and calls `super` where
    // Swift does. Drop is implemented on the ivars only.
    #[unsafe(super(NSTextView, NSText, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "MarkdownTextView"]
    #[ivars = MarkdownTextViewIvars]
    pub struct MarkdownTextView;

    unsafe impl NSObjectProtocol for MarkdownTextView {}

    impl MarkdownTextView {
        #[unsafe(method(setSelectedRanges:affinity:stillSelecting:))]
        fn __set_selected_ranges(&self, ranges: &NSArray<NSValue>, affinity: NSSelectionAffinity, still_selecting: bool) {
            let _: () = unsafe {
                msg_send![super(self), setSelectedRanges: ranges, affinity: affinity, stillSelecting: still_selecting]
            };
            let ivars = self.ivars();
            if ivars.is_applying_selection.get()
                || ivars.is_performing_source_edit.get()
                || ivars.is_tracking_mouse_selection.get()
                || still_selecting
            {
                return;
            }
            self.handle_selection_changed(true, None);
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            self.key_down(event);
        }

        #[unsafe(method(characterIndexForInsertionAtPoint:))]
        fn __character_index_for_insertion(&self, point: NSPoint) -> usize {
            self.character_index_for_insertion(point)
        }

        #[unsafe(method(scrollWheel:))]
        fn __scroll_wheel(&self, event: &NSEvent) {
            self.interrupt_animated_scroll();
            if self.delegate().is_some_and(|delegate| delegate.should_claim_scroll_gesture(self, event)) {
                return;
            }
            let _: () = unsafe { msg_send![super(self), scrollWheel: event] };
        }

        #[unsafe(method(viewWillStartLiveResize))]
        fn __view_will_start_live_resize(&self) {
            let _: () = unsafe { msg_send![super(self), viewWillStartLiveResize] };
            self.interrupt_animated_scroll();
        }

        #[unsafe(method(magnifyWithEvent:))]
        fn __magnify(&self, event: &NSEvent) {
            self.magnify(event);
        }

        #[unsafe(method(smartMagnifyWithEvent:))]
        fn __smart_magnify(&self, _event: &NSEvent) {
            self.interrupt_animated_scroll();
            if let Some(delegate) = self.delegate() {
                delegate.did_request_smart_text_zoom(self);
            }
        }

        #[unsafe(method(swipeWithEvent:))]
        fn __swipe(&self, event: &NSEvent) {
            self.interrupt_animated_scroll();
            let _: () = unsafe { msg_send![super(self), swipeWithEvent: event] };
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            let _: () = unsafe { msg_send![super(self), drawRect: dirty_rect] };
            self.draw_deleted_change_marks(dirty_rect);
            self.draw_object_change_marks(dirty_rect);
            self.draw_hovered_link_underline(dirty_rect);
            self.draw_drop_insertion_caret(dirty_rect);
        }

        #[unsafe(method(drawBackgroundInRect:))]
        fn __draw_background(&self, rect: NSRect) {
            let _: () = unsafe { msg_send![super(self), drawBackgroundInRect: rect] };
            self.draw_scoped_source_background(rect);
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            self.view_did_move_to_window();
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            self.install_hover_tracking();
        }

        // MARK: Interaction overrides (markdown_text_view_interaction)

        #[unsafe(method(mouseMoved:))]
        fn __mouse_moved(&self, event: &NSEvent) {
            let _: () = unsafe { msg_send![super(self), mouseMoved: event] };
            self.mouse_moved(event);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, event: &NSEvent) {
            let _: () = unsafe { msg_send![super(self), mouseExited: event] };
            self.mouse_exited(event);
        }

        #[unsafe(method(cursorUpdate:))]
        fn __cursor_update(&self, event: &NSEvent) {
            self.cursor_update(event);
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            self.mouse_down(event);
        }

        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, sender: Option<&AnyObject>) {
            self.cancel_operation(sender);
        }

        #[unsafe(method_id(menuForEvent:))]
        fn __menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
            self.menu_for_event(event)
        }

        #[unsafe(method(insertText:replacementRange:))]
        fn __insert_text(&self, string: &AnyObject, replacement_range: objc2_foundation::NSRange) {
            self.insert_text(string, replacement_range);
        }

        #[unsafe(method(selectAll:))]
        fn __select_all(&self, sender: Option<&AnyObject>) {
            self.select_all(sender);
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn __set_marked_text(
            &self,
            string: &AnyObject,
            selected_range: objc2_foundation::NSRange,
            replacement_range: objc2_foundation::NSRange,
        ) {
            self.set_marked_text(string, selected_range, replacement_range);
        }

        #[unsafe(method(unmarkText))]
        fn __unmark_text(&self) {
            self.unmark_text();
        }

        #[unsafe(method(deleteBackward:))]
        fn __delete_backward(&self, sender: Option<&AnyObject>) {
            self.delete_backward(sender);
        }

        #[unsafe(method(deleteForward:))]
        fn __delete_forward(&self, sender: Option<&AnyObject>) {
            self.delete_forward(sender);
        }

        #[unsafe(method(deleteBackwardByDecomposingPreviousCharacter:))]
        fn __delete_backward_decomposing(&self, sender: Option<&AnyObject>) {
            self.delete_backward(sender);
        }

        #[unsafe(method(deleteWordBackward:))]
        fn __delete_word_backward(&self, sender: Option<&AnyObject>) {
            self.delete_word_backward(sender);
        }

        #[unsafe(method(deleteWordForward:))]
        fn __delete_word_forward(&self, sender: Option<&AnyObject>) {
            self.delete_word_forward(sender);
        }

        #[unsafe(method(deleteToBeginningOfLine:))]
        fn __delete_to_beginning_of_line(&self, sender: Option<&AnyObject>) {
            self.delete_to_boundary(sender, false, true);
        }

        #[unsafe(method(deleteToEndOfLine:))]
        fn __delete_to_end_of_line(&self, sender: Option<&AnyObject>) {
            self.delete_to_boundary(sender, false, false);
        }

        #[unsafe(method(deleteToBeginningOfParagraph:))]
        fn __delete_to_beginning_of_paragraph(&self, sender: Option<&AnyObject>) {
            self.delete_to_boundary(sender, true, true);
        }

        #[unsafe(method(deleteToEndOfParagraph:))]
        fn __delete_to_end_of_paragraph(&self, sender: Option<&AnyObject>) {
            self.delete_to_boundary(sender, true, false);
        }

        #[unsafe(method(insertNewline:))]
        fn __insert_newline(&self, sender: Option<&AnyObject>) {
            self.insert_newline(sender);
        }

        #[unsafe(method(insertTab:))]
        fn __insert_tab(&self, _sender: Option<&AnyObject>) {
            if !self.isEditable() {
                return;
            }
            self.perform_source_edit(self.source_selected_range(), "\t", "Edit");
        }

        #[unsafe(method(paste:))]
        fn __paste(&self, _sender: Option<&AnyObject>) {
            self.apply_paste(crate::view::markdown_smart_paste::MarkdownPasteMode::Smart);
        }

        #[unsafe(method(pasteAsMarkdown:))]
        fn __paste_as_markdown(&self, _sender: Option<&AnyObject>) {
            self.apply_paste(crate::view::markdown_smart_paste::MarkdownPasteMode::Markdown);
        }

        #[unsafe(method(pasteAndMatchStyle:))]
        fn __paste_and_match_style(&self, _sender: Option<&AnyObject>) {
            self.apply_paste(crate::view::markdown_smart_paste::MarkdownPasteMode::MatchStyle);
        }

        #[unsafe(method(copy:))]
        fn __copy(&self, sender: Option<&AnyObject>) {
            self.copy(sender);
        }

        #[unsafe(method(cut:))]
        fn __cut(&self, sender: Option<&AnyObject>) {
            let range = self.source_selected_range();
            if !(range.length > 0) {
                return;
            }
            self.copy(sender);
            self.perform_source_edit(range, "", "Edit");
        }

        #[unsafe(method(writeSelectionToPasteboard:types:))]
        fn __write_selection(&self, pboard: &NSPasteboard, types: &NSArray<NSString>) -> bool {
            self.write_selection(pboard, types)
        }

        #[unsafe(method(updateDragTypeRegistration))]
        fn __update_drag_type_registration(&self) {
            let _: () = unsafe { msg_send![super(self), updateDragTypeRegistration] };
            self.update_drag_type_registration_additions();
        }

        #[unsafe(method(draggingEntered:))]
        fn __dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.dragging_entered(sender)
        }

        #[unsafe(method(draggingUpdated:))]
        fn __dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.dragging_updated(sender)
        }

        #[unsafe(method(draggingExited:))]
        fn __dragging_exited(&self, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.dragging_exited(sender);
        }

        #[unsafe(method(prepareForDragOperation:))]
        fn __prepare_for_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            self.prepare_for_drag_operation(sender)
        }

        #[unsafe(method(performDragOperation:))]
        fn __perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            self.perform_drag_operation(sender)
        }

        #[unsafe(method(concludeDragOperation:))]
        fn __conclude_drag_operation(&self, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.set_drop_insertion_offset(None);
            self.ivars().claims_active_drag.set(false);
            let _: () = unsafe { msg_send![super(self), concludeDragOperation: sender] };
        }

        #[unsafe(method(quickLookWithEvent:))]
        fn __quick_look(&self, event: &NSEvent) {
            self.quick_look(event);
        }
    }
);

// MARK: - Construction

impl MarkdownTextView {
    /// `init(frame:storage:)`, with the fallback style sheet.
    pub fn with_storage(frame: NSRect, storage: &NSTextStorage, mtm: MainThreadMarker) -> Retained<MarkdownTextView> {
        Self::new(frame, storage, Self::fallback_style_sheet(), mtm)
    }

    /// `init(frame:storage:styleSheet:)`.
    pub fn new(
        frame: NSRect,
        storage: &NSTextStorage,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<MarkdownTextView> {
        // Stored-property initialisers run first, in declaration order.
        let substitution = ParagraphSubstitution::new();
        let engine = DecorationEngine::new((*style_sheet).clone());
        let fragment_context = FragmentContext::new(style_sheet.clone());

        let content_storage = MarkdownContentStorage::new();
        let markdown_layout_manager = NSTextLayoutManager::new();
        let container = NSTextContainer::initWithSize(
            NSTextContainer::alloc(),
            CGSize::new(style_sheet.measure_width, CGFloat::MAX),
        );
        container.setWidthTracksTextView(false);
        container.setLineFragmentPadding(0.0);
        markdown_layout_manager.setTextContainer(Some(&container));
        content_storage.addTextLayoutManager(&markdown_layout_manager);
        content_storage.setTextStorage(Some(storage));
        unsafe { content_storage.setDelegate(Some(ProtocolObject::from_ref(&*substitution))) };

        let this = Self::alloc(mtm).set_ivars(MarkdownTextViewIvars {
            markdown_delegate: RefCell::new(None),
            text_magnification_accumulator: Cell::new(0.0),
            key_event_handler: RefCell::new(None),
            parsed_document: RefCell::new(ParsedDocument::empty()),
            mode: Cell::new(RenderMode::Live),
            configuration: RefCell::new(MarkdownRenderConfiguration::default()),
            source_focus: Cell::new(SourceFocus::None),
            style_sheet: RefCell::new(style_sheet.clone()),
            zoom_level: Cell::new(ZoomLevel::Everything),
            folded_heading_slugs: RefCell::new(HashSet::new()),
            search_hits: RefCell::new(Vec::new()),
            current_search_hit: Cell::new(None),
            speech_highlight: Cell::new(None),
            change_marks: RefCell::new(Vec::new()),
            document_url: RefCell::new(None),
            engine: RefCell::new(engine),
            fragment_context: fragment_context.clone(),
            substitution,
            fragment_provider: RefCell::new(None),
            content_storage,
            markdown_layout_manager,
            base_hidden_ranges: RefCell::new(Vec::new()),
            base_display_map: RefCell::new(DisplayMap::identity()),
            base_layout_map: RefCell::new(DisplayMap::identity()),
            paragraph_index: RefCell::new(ParagraphIndex::empty()),
            display_map: RefCell::new(DisplayMap::identity()),
            hard_wrap_ranges: RefCell::new(Vec::new()),
            hard_wrap_substitutions: RefCell::new(Vec::new()),
            elision: RefCell::new(ElisionPlan::none()),
            expanded_elision_ranges: RefCell::new(Vec::new()),
            is_applying_selection: Cell::new(false),
            is_performing_source_edit: Cell::new(false),
            last_mutation_provenance: RefCell::new("Edit".to_owned()),
            should_follow_caret_after_local_edit: Cell::new(false),
            local_edit_viewport_anchor: Cell::new(None),
            is_tracking_mouse_selection: Cell::new(false),
            suppresses_caret_reveal: Cell::new(false),
            anchored_paragraph: Cell::new(None),
            reveal_paragraph: Cell::new(None),
            elision_was_identity: Cell::new(true),
            overlay_ranges: RefCell::new(Vec::new()),
            object_change_marks: RefCell::new(Vec::new()),
            fragment_accessibility_elements: RefCell::new(Vec::new()),
            path_existence: RefCell::new(HashMap::new()),
            path_refresh_generation: Cell::new(0),
            code_collapse_overrides: RefCell::new(HashMap::new()),
            invisibles_applied: Cell::new(false),
            hover_tracking: RefCell::new(None),
            scroll_observer: RefCell::new(None),
            resign_key_observer: RefCell::new(None),
            pending_resize_request: Cell::new(None),
            resize_work_item: RefCell::new(None),
            copied_code_feedback_work_item: RefCell::new(None),
            motion_driver: RefCell::new(None),
            resize_generation: Cell::new(0),
            viewport_repair_generation: Cell::new(0),
            pending_resize_anchor: Cell::new(None),
            pending_resize_viewport_y: Cell::new(None),
            next_document_update_viewport_y: Cell::new(None),
            resize_needs_repair: Cell::new(false),
            applied_source_focus: Cell::new(SourceFocus::None),
            pending_shrink_repair: Cell::new(false),
            pending_shrink_repair_work_item: RefCell::new(None),
            applying_pending_shrink_repair: Cell::new(false),
            scroll_coalesce_work_item: RefCell::new(None),
            scroll_coalesce_generation: Cell::new(0),
            last_fragment_invalidation_range_for_testing: Cell::new(None),
            hovered_heading_index: Cell::new(None),
            hovered_link_range: Cell::new(None),
            drop_insertion_offset: Cell::new(None),
            claims_active_drag: Cell::new(false),
            composing_paragraph: Cell::new(None),
            update_generation: Cell::new(0),
            gutter_rail: RefCell::new(ObjcWeak::default()),
            word_joiner_runs: RefCell::new(WordJoinerRuns::default()),
            scroll_spring: Cell::new(SpringScalar::with_duration(motion::SPRING_DELIBERATE)),
            scroll_spring_is_active: Cell::new(false),
            scroll_spring_clip: RefCell::new(ObjcWeak::default()),
            pending_scroll_y: Cell::new(None),
            pending_motion_invalidation: Cell::new(None),
        });
        let this: Retained<MarkdownTextView> =
            unsafe { msg_send![super(this), initWithFrame: frame, textContainer: Some(&*container)] };

        let provider = FragmentProvider::new(fragment_context.clone());
        this.ivars()
            .markdown_layout_manager
            .setDelegate(Some(ProtocolObject::from_ref(&*provider)));
        *this.ivars().fragment_provider.borrow_mut() = Some(provider);
        fragment_context.set_text_view(&this);

        this.setVerticallyResizable(true);
        this.setHorizontallyResizable(false);
        this.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);
        this.setAllowsUndo(true);
        this.setRichText(true);
        this.setImportsGraphics(false);
        this.setUsesFontPanel(false);
        this.setUsesFindBar(false);
        this.setDrawsBackground(true);
        // §6.4: typographic substitution is off by default.
        this.setAutomaticQuoteSubstitutionEnabled(false);
        this.setAutomaticDashSubstitutionEnabled(false);
        this.setAutomaticTextReplacementEnabled(false);
        this.setAutomaticSpellingCorrectionEnabled(false);
        this.setSmartInsertDeleteEnabled(false);
        this.setIncrementalSearchingEnabled(false);
        this.setTextContainerInset(NSSize::new(render_metrics::REVEAL_SLACK, render_metrics::VERTICAL_INSET));
        this.setAccessibilityElement(true);
        this.setAccessibilityRole(Some(unsafe { objc2_app_kit::NSAccessibilityTextAreaRole }));
        this.install_accessibility_actions();

        this.ivars().engine.borrow_mut().set_policy(this.effective_policy());
        let threshold = this.ivars().configuration.borrow().code_collapse_threshold() as isize;
        this.ivars().engine.borrow_mut().set_code_collapse_line_count(threshold);
        this.apply_typographic_substitution();
        this.ivars().fragment_context.mode.set(this.mode());
        this.apply_measure();
        this.apply_mode_chrome();
        this.rebuild_paragraph_index();
        this
    }

    fn install_accessibility_actions(&self) {
        let weak_link: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        let weak_copy = weak_link.clone();
        let open_link = RcBlock::new(move || -> Bool {
            Bool::new(weak_link.load().is_some_and(|view| view.activate_link_at_caret()))
        });
        let copy_code = RcBlock::new(move || -> Bool {
            Bool::new(weak_copy.load().is_some_and(|view| view.copy_code_block_for_accessibility()))
        });
        let actions = NSArray::from_retained_slice(&[
            NSAccessibilityCustomAction::initWithName_handler(
                NSAccessibilityCustomAction::alloc(),
                &NSString::from_str("Open link at caret"),
                Some(&open_link),
            ),
            NSAccessibilityCustomAction::initWithName_handler(
                NSAccessibilityCustomAction::alloc(),
                &NSString::from_str("Copy code block"),
                Some(&copy_code),
            ),
        ]);
        self.setAccessibilityCustomActions(Some(&actions));
    }

    /// Used only by the convenience initialiser (§10).
    pub fn fallback_style_sheet() -> Rc<StyleSheet> {
        let appearance = objc2_app_kit::NSAppearance::currentDrawingAppearance();
        Rc::new(StyleSheet::new(crate::render_contracts::Theme::fallback(), &appearance, None))
    }

    // MARK: - Accessors

    pub fn markdown_delegate(&self) -> Option<Rc<dyn MarkdownTextViewDelegate>> {
        self.ivars().markdown_delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub(crate) fn delegate(&self) -> Option<Rc<dyn MarkdownTextViewDelegate>> {
        self.markdown_delegate()
    }

    pub fn set_markdown_delegate(&self, delegate: Option<Weak<dyn MarkdownTextViewDelegate>>) {
        *self.ivars().markdown_delegate.borrow_mut() = delegate;
    }

    pub fn key_event_handler(&self) -> Option<KeyEventHandler> {
        self.ivars().key_event_handler.borrow().clone()
    }

    pub fn set_key_event_handler(&self, handler: Option<KeyEventHandler>) {
        *self.ivars().key_event_handler.borrow_mut() = handler;
    }

    pub fn parsed_document(&self) -> Arc<ParsedDocument> {
        self.ivars().parsed_document.borrow().clone()
    }

    pub fn mode(&self) -> RenderMode {
        self.ivars().mode.get()
    }

    pub fn configuration(&self) -> MarkdownRenderConfiguration {
        *self.ivars().configuration.borrow()
    }

    pub fn source_focus(&self) -> SourceFocus {
        self.ivars().source_focus.get()
    }

    pub(crate) fn set_source_focus_value(&self, focus: SourceFocus) {
        self.ivars().source_focus.set(focus);
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn zoom_level(&self) -> ZoomLevel {
        self.ivars().zoom_level.get()
    }

    pub fn folded_heading_slugs(&self) -> HashSet<String> {
        self.ivars().folded_heading_slugs.borrow().clone()
    }

    pub fn search_hits(&self) -> Vec<NSRange> {
        self.ivars().search_hits.borrow().clone()
    }

    pub fn current_search_hit(&self) -> Option<NSRange> {
        self.ivars().current_search_hit.get()
    }

    pub fn speech_highlight(&self) -> Option<NSRange> {
        self.ivars().speech_highlight.get()
    }

    pub fn change_marks(&self) -> Vec<ChangeMark> {
        self.ivars().change_marks.borrow().clone()
    }

    pub fn document_url(&self) -> Option<String> {
        self.ivars().document_url.borrow().clone()
    }

    pub fn last_mutation_provenance(&self) -> String {
        self.ivars().last_mutation_provenance.borrow().clone()
    }

    pub(crate) fn set_last_mutation_provenance(&self, provenance: &str) {
        *self.ivars().last_mutation_provenance.borrow_mut() = provenance.to_owned();
    }

    pub fn fragment_context(&self) -> &Rc<FragmentContext> {
        &self.ivars().fragment_context
    }


    pub fn paragraph_index(&self) -> ParagraphIndex {
        self.ivars().paragraph_index.borrow().clone()
    }

    /// The live source ⇄ TextKit map.
    pub fn current_display_map(&self) -> DisplayMap {
        self.ivars().display_map.borrow().clone()
    }

    pub fn gutter_rail(&self) -> Option<Retained<GutterRailView>> {
        self.ivars().gutter_rail.borrow().load()
    }

    pub(crate) fn set_gutter_rail(&self, rail: &GutterRailView) {
        *self.ivars().gutter_rail.borrow_mut() = ObjcWeak::from(rail);
    }

    fn gutter_rail_needs_display(&self) {
        if let Some(rail) = self.gutter_rail() {
            rail.setNeedsDisplay(true);
        }
    }

    pub fn hovered_heading_index(&self) -> Option<usize> {
        self.ivars().hovered_heading_index.get()
    }

    pub fn set_hovered_heading_index(&self, index: Option<usize>) {
        self.ivars().hovered_heading_index.set(index);
    }

    /// Where a drag hovering over the surface would land, in source offsets.
    pub fn drop_insertion_offset(&self) -> Option<isize> {
        self.ivars().drop_insertion_offset.get()
    }

    /// True while the host has claimed the drag in flight.
    pub fn claims_active_drag(&self) -> bool {
        self.ivars().claims_active_drag.get()
    }

    pub fn update_generation(&self) -> isize {
        self.ivars().update_generation.get()
    }

    pub fn pending_resize_request_for_testing(&self) -> Option<ContentResizeRequest> {
        self.ivars().pending_resize_request.get()
    }

    pub fn cached_layout_element_count_for_testing(&self) -> usize {
        self.ivars().content_storage.cached_element_count_for_testing()
    }

    /// `lastFragmentInvalidationRangeForTesting`: `None` before any
    /// invalidation, `Some(None)` for a whole-document one.
    pub fn last_fragment_invalidation_range_for_testing(&self) -> Option<Option<NSRange>> {
        self.ivars().last_fragment_invalidation_range_for_testing.get()
    }

    fn text_storage(&self) -> Option<Retained<NSTextStorage>> {
        unsafe { self.textStorage() }
    }

    fn scroll_view(&self) -> Option<Retained<NSScrollView>> {
        self.enclosingScrollView()
    }

    fn effective_policy(&self) -> DecorationPolicy {
        let mut policy = self.mode().policy();
        let reveal = self.ivars().configuration.borrow().reveal_policy;
        policy.reveals_at_caret = reveal != MarkdownRevealPolicy::Never;
        policy.reveals_at_all_cursors = reveal == MarkdownRevealPolicy::AllCursors;
        policy
    }

    // MARK: - Property setters (Swift `didSet`)

    /// Instant, and preserves scroll position and selection (§3.2).
    pub fn set_mode(&self, mode: RenderMode) {
        let old_value = self.mode();
        self.ivars().mode.set(mode);
        if mode == old_value {
            return;
        }
        let anchor = self.capture_viewport_anchor();
        let selection = self.source_selected_ranges();
        if mode == RenderMode::Source {
            self.ivars().source_focus.set(SourceFocus::Document);
        } else if self.source_focus() == SourceFocus::Document {
            self.ivars().source_focus.set(SourceFocus::None);
        }
        let policy = self.effective_policy();
        self.ivars().engine.borrow_mut().set_policy(policy);
        let threshold = self.ivars().configuration.borrow().code_collapse_threshold() as isize;
        self.ivars().engine.borrow_mut().set_code_collapse_line_count(threshold);
        self.ivars().fragment_context.mode.set(mode);
        self.ivars().fragment_context.source_focus_range.set(self.source_focus().range());
        self.apply_mode_chrome();
        self.rebuild_everything();
        self.request_content_resize(ContentResizeRequest::Viewport, Some(anchor), None);
        self.set_source_selected_ranges(&selection);
        self.restore_viewport_to(anchor);
        // Repair once after TextKit's late caret-visible correction and once
        // after the deferred viewport resize settles.
        let generation = self.ivars().viewport_repair_generation.get().wrapping_add(1);
        self.ivars().viewport_repair_generation.set(generation);
        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        let repair = Rc::new(move || {
            let Some(view) = weak.load() else { return };
            if view.ivars().viewport_repair_generation.get() != generation {
                return;
            }
            view.restore_viewport_to(anchor);
        });
        let first = repair.clone();
        main_async(move || first());
        let weak_second: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        main_async(move || {
            if let Some(view) = weak_second.load() {
                view.layoutSubtreeIfNeeded();
                view.prepare_for_display();
            }
            repair();
        });
        if let Some(delegate) = self.delegate() {
            delegate.did_change_source_focus(self, self.source_focus());
        }
    }

    pub fn set_configuration(&self, configuration: MarkdownRenderConfiguration) {
        let old_value = self.configuration();
        if configuration == old_value {
            *self.ivars().configuration.borrow_mut() = configuration;
            return;
        }
        *self.ivars().configuration.borrow_mut() = configuration;
        let anchor = self.capture_viewport_anchor();
        let selection = self.source_selected_ranges();
        let policy = self.effective_policy();
        self.ivars().engine.borrow_mut().set_policy(policy);
        self.ivars().engine.borrow_mut().set_code_collapse_line_count(configuration.code_collapse_threshold() as isize);
        self.apply_typographic_substitution();
        let invisibles_only = configuration.show_invisibles != old_value.show_invisibles
            && configuration.reveal_policy == old_value.reveal_policy
            && configuration.typographic_substitution == old_value.typographic_substitution
            && configuration.typewriter_scrolling == old_value.typewriter_scrolling
            && configuration.reflow_hard_wrapped_paragraphs == old_value.reflow_hard_wrapped_paragraphs
            && configuration.code_collapse_threshold() == old_value.code_collapse_threshold()
            && configuration.large_file_threshold_megabytes() == old_value.large_file_threshold_megabytes();
        if invisibles_only {
            self.apply_invisibles(None);
            self.rebuild_display_map(true, &[]);
            self.invalidate_all_fragments();
        } else {
            self.rebuild_everything();
            self.apply_invisibles(None);
        }
        self.request_content_resize(ContentResizeRequest::Viewport, Some(anchor), None);
        self.set_source_selected_ranges(&selection);
        self.restore_viewport_to(anchor);
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        let old_value = self.style_sheet();
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        let anchor = self.capture_viewport_anchor();
        self.ivars().engine.borrow_mut().set_style_sheet((*style_sheet).clone());
        self.ivars().fragment_context.set_style_sheet(style_sheet.clone());
        self.ivars().fragment_context.invalidate_derived_layout();
        self.apply_measure();
        self.apply_mode_chrome();
        let geometry_changed = old_value.line_height != style_sheet.line_height
            || old_value.measure_width != style_sheet.measure_width
            || old_value.baseline_grid != style_sheet.baseline_grid
            || old_value.math_point_size != style_sheet.math_point_size;
        if geometry_changed {
            self.rebuild_everything();
        } else {
            self.restyle_attributes_preserving_geometry();
        }
        self.request_content_resize(ContentResizeRequest::Viewport, Some(anchor), None);
    }

    /// §5.2. A level change is an elision change, nothing more.
    pub fn set_zoom_level(&self, zoom_level: ZoomLevel) {
        let old_value = self.zoom_level();
        self.ivars().zoom_level.set(zoom_level);
        if zoom_level == old_value {
            return;
        }
        self.ivars().expanded_elision_ranges.borrow_mut().clear();
        let previous_height = self.frame().size.height;
        self.refresh_elision(true);
        self.animate_structural_zoom_height(previous_height);
    }

    pub fn set_folded_heading_slugs(&self, slugs: HashSet<String>) {
        let changed = *self.ivars().folded_heading_slugs.borrow() != slugs;
        *self.ivars().folded_heading_slugs.borrow_mut() = slugs;
        if !changed {
            return;
        }
        self.refresh_elision(true);
    }

    /// Setting hits unfolds any heading whose section contains one.
    pub fn set_search_hits(&self, hits: Vec<NSRange>) {
        *self.ivars().search_hits.borrow_mut() = hits.clone();
        let previous = std::mem::replace(&mut *self.ivars().overlay_ranges.borrow_mut(), hits.clone());
        self.unfold_headings_containing(&hits);
        self.refresh_elision(true);
        let mut invalidating = previous;
        invalidating.extend(hits);
        self.reapply_overlays(&invalidating, false);
    }

    pub fn set_current_search_hit(&self, hit: Option<NSRange>) {
        let old_value = self.current_search_hit();
        self.ivars().current_search_hit.set(hit);
        if hit == old_value {
            return;
        }
        let invalidating: Vec<NSRange> = [old_value, hit].into_iter().flatten().collect();
        self.reapply_overlays(&invalidating, true);
    }

    pub fn set_speech_highlight(&self, highlight: Option<NSRange>) {
        let old_value = self.speech_highlight();
        self.ivars().speech_highlight.set(highlight);
        if highlight == old_value {
            return;
        }
        let invalidating: Vec<NSRange> = [old_value, highlight].into_iter().flatten().collect();
        self.reapply_overlays(&invalidating, true);
    }

    pub fn set_change_marks(&self, marks: Vec<ChangeMark>) {
        let old_value = std::mem::replace(&mut *self.ivars().change_marks.borrow_mut(), marks.clone());
        let mut invalidated: Vec<NSRange> = old_value.iter().map(|mark| mark.range).collect();
        invalidated.extend(marks.iter().map(|mark| mark.range));
        self.reapply_overlays(&invalidated, true);
        self.gutter_rail_needs_display();
    }

    pub fn set_document_url(&self, url: Option<String>) {
        *self.ivars().document_url.borrow_mut() = url.clone();
        *self.ivars().fragment_context.document_url.borrow_mut() = url;
        self.invalidate_all_fragments();
    }

    pub fn set_local_asset_authorizer(&self, authorizer: Option<crate::fragments::fragment_base::LocalAssetAuthorizer>) {
        *self.ivars().fragment_context.local_asset_authorizer.borrow_mut() = authorizer;
        self.invalidate_all_fragments();
    }

    /// Re-evaluate blocked local image fragments after an explicit trust grant.
    pub fn refresh_local_assets(&self) {
        self.invalidate_all_fragments();
        self.setNeedsDisplay(true);
    }

    /// Reveal raw Markdown for a logical selection or block without changing
    /// the presentation of surrounding content.
    pub fn focus_source(&self, requested_range: NSRange) {
        let document_length = self.parsed_document().length;
        if !(self.mode() != RenderMode::Source && document_length > 0) {
            return;
        }
        let anchor = self.capture_viewport_anchor();
        let lower = requested_range.location.min(document_length).max(0);
        let upper = lower.max(requested_range.upper_bound().min(document_length));
        let index = self.paragraph_index();
        let first = index.paragraph_range_containing(lower);
        let last_offset = lower.max(upper - 1);
        let last = index.paragraph_range_containing(last_offset);
        let expanded = first.union(last);
        if self.source_focus() == SourceFocus::Scoped(expanded) {
            return;
        }
        self.ivars().source_focus.set(SourceFocus::Scoped(expanded));
        self.ivars().fragment_context.source_focus_range.set(Some(expanded));
        self.refresh_source_accessibility();
        self.rebuild_everything();
        self.request_content_resize(ContentResizeRequest::Viewport, Some(anchor), None);
        self.set_source_selected_ranges(&[requested_range]);
        post_announcement(self, "Markdown source editor");
        if let Some(delegate) = self.delegate() {
            delegate.did_change_source_focus(self, self.source_focus());
        }
    }

    pub fn focus_entire_source(&self) {
        if self.mode() == RenderMode::Source {
            return;
        }
        self.set_mode(RenderMode::Source);
    }

    pub fn clear_source_focus(&self) {
        match self.source_focus() {
            SourceFocus::None => {}
            SourceFocus::Document => self.set_mode(RenderMode::Live),
            SourceFocus::Scoped(_) => {
                let anchor = self.capture_viewport_anchor();
                self.ivars().source_focus.set(SourceFocus::None);
                self.ivars().fragment_context.source_focus_range.set(None);
                self.refresh_source_accessibility();
                self.rebuild_everything();
                self.request_content_resize(ContentResizeRequest::Viewport, Some(anchor), None);
                if let Some(delegate) = self.delegate() {
                    delegate.did_change_source_focus(self, self.source_focus());
                }
            }
        }
    }

    // MARK: - Document updates

    /// Seeds a second pane over the same attributed storage.
    pub fn adopt_shared_presentation(&self, source: &MarkdownTextView) {
        let (Some(mine), Some(theirs)) = (self.text_storage(), source.text_storage()) else { return };
        if !std::ptr::eq(&*mine, &*theirs) {
            return;
        }
        let si = source.ivars();
        let ivars = self.ivars();
        *ivars.parsed_document.borrow_mut() = source.parsed_document();
        *ivars.paragraph_index.borrow_mut() = source.paragraph_index();
        *ivars.base_hidden_ranges.borrow_mut() = si.base_hidden_ranges.borrow().clone();
        *ivars.hard_wrap_ranges.borrow_mut() = si.hard_wrap_ranges.borrow().clone();
        *ivars.hard_wrap_substitutions.borrow_mut() = si.hard_wrap_substitutions.borrow().clone();
        *ivars.base_display_map.borrow_mut() = si.base_display_map.borrow().clone();
        *ivars.base_layout_map.borrow_mut() = si.base_layout_map.borrow().clone();
        *ivars.display_map.borrow_mut() = si.display_map.borrow().clone();
        ivars.reveal_paragraph.set(si.reveal_paragraph.get());
        ivars.update_generation.set(1.max(si.update_generation.get()));
        *ivars.fragment_context.front_matter_fields.borrow_mut() = si.fragment_context.front_matter_fields.borrow().clone();
        ivars.fragment_context.document_has_h1.set(si.fragment_context.document_has_h1.get());
        ivars.substitution.set_display_map(ivars.base_display_map.borrow().clone());
        ivars.content_storage.configure(
            &ivars.paragraph_index.borrow(),
            &ivars.hard_wrap_ranges.borrow(),
            &ivars.display_map.borrow(),
            None,
        );
        self.setNeedsLayout(true);
        self.setNeedsDisplay(true);
        if let Some(rail) = self.gutter_rail() {
            rail.reload();
        }
    }

    fn source_scopes(&self, dirty: &DirtySet) -> Vec<NSRange> {
        let Some(storage) = self.text_storage() else { return Vec::new() };
        if dirty.is_wholesale {
            return Vec::new();
        }
        let whole = NSRange::new(0, storage.length() as isize);
        let clipped: Vec<NSRange> = dirty
            .ranges
            .iter()
            .filter_map(|range| {
                let intersection = upleft_core::ns_range::ns_intersection_range(*range, whole);
                if intersection.length > 0 { Some(intersection) } else { None }
            })
            .collect();
        RangeSet::normalized(&clipped)
    }

    /// Re-decorates only what the AST diff says changed (§3.5).
    pub fn update(&self, document: Arc<ParsedDocument>, dirty: &DirtySet, preserving_selection: bool) {
        let ivars = self.ivars();
        let is_initial_update = ivars.update_generation.get() == 0;
        let selection = if !preserving_selection || is_initial_update {
            vec![NSRange::new(0, 0)]
        } else {
            self.source_selected_ranges()
        };
        let explicit_viewport_anchor = ivars.local_edit_viewport_anchor.get();
        let anchor = explicit_viewport_anchor.unwrap_or_else(|| self.capture_viewport_anchor());
        ivars.local_edit_viewport_anchor.set(None);
        let locked_viewport_y = ivars.next_document_update_viewport_y.get();
        ivars.next_document_update_viewport_y.set(None);
        let follows_local_edit = ivars.should_follow_caret_after_local_edit.get();
        let follows_caret = follows_local_edit && ivars.configuration.borrow().typewriter_scrolling;
        ivars.should_follow_caret_after_local_edit.set(false);
        let current_mode = self.mode();
        let old_paragraph_count = ivars.paragraph_index.borrow().starts.len();
        let is_wholesale_update = is_initial_update || dirty.is_wholesale;
        let dirty_scopes = if is_wholesale_update { Vec::new() } else { self.source_scopes(dirty) };
        *ivars.parsed_document.borrow_mut() = document.clone();
        ivars.hovered_link_range.set(None);
        ivars.update_generation.set(ivars.update_generation.get().wrapping_add(1));
        if is_wholesale_update {
            ivars.path_existence.borrow_mut().clear();
            ivars.code_collapse_overrides.borrow_mut().clear();
            *ivars.fragment_context.collapse_overrides.borrow_mut() = ivars.code_collapse_overrides.borrow().clone();
        }
        *ivars.fragment_context.front_matter_fields.borrow_mut() = document
            .front_matter
            .as_ref()
            .map(|front_matter| {
                front_matter.fields.iter().map(|field| (field.key.clone(), field.value.clone())).collect()
            })
            .unwrap_or_default();
        ivars.fragment_context.document_has_h1.set(document.headings.iter().any(|heading| heading.level == 1));
        ivars.fragment_context.invalidate_derived_layout();
        self.rebuild_paragraph_index();

        let Some(storage) = self.text_storage() else { return };
        ivars.engine.borrow_mut().decorate(&storage, &document, dirty);
        self.apply_source_presentation(if is_wholesale_update { None } else { Some(&dirty_scopes) });
        if ivars.configuration.borrow().show_invisibles || ivars.invisibles_applied.get() {
            self.apply_invisibles(if is_wholesale_update { None } else { Some(&dirty_scopes) });
        }
        self.rebuild_base_display_map(&document);
        self.refresh_elision(false);
        let decorated_scopes = if is_wholesale_update {
            Vec::new()
        } else {
            ivars.engine.borrow().decorated_bounds(dirty, &document, storage.length() as isize)
        };
        self.rebuild_display_map(is_wholesale_update, &decorated_scopes);
        self.apply_overlays(if is_wholesale_update { None } else { Some(&decorated_scopes) });
        self.apply_path_existence(if is_wholesale_update { None } else { Some(&decorated_scopes) });
        if is_wholesale_update {
            self.invalidate_all_fragments();
        }
        if let Some(rail) = self.gutter_rail() {
            rail.reload();
        }
        self.refresh_fragment_accessibility();
        let resize_request = if is_wholesale_update {
            ContentResizeRequest::Immediate
        } else if ivars.paragraph_index.borrow().starts.len() != old_paragraph_count {
            ContentResizeRequest::LineCount
        } else {
            ContentResizeRequest::Semantic
        };
        let resize_anchor = explicit_viewport_anchor.or(if locked_viewport_y.is_none() { Some(anchor) } else { None });
        let resize_viewport_y = if explicit_viewport_anchor.is_none() { locked_viewport_y } else { None };
        self.request_content_resize(
            resize_request,
            if resize_request == ContentResizeRequest::Immediate || follows_caret { None } else { resize_anchor },
            resize_viewport_y,
        );

        if self.mode() != current_mode {
            self.set_mode(current_mode);
        }
        let bounded_selection: Vec<NSRange> = selection
            .iter()
            .map(|range| {
                let location = range.location.max(0).min(document.length);
                let end = location.max(range.upper_bound()).min(document.length);
                NSRange::new(location, end - location)
            })
            .collect();
        self.set_source_selected_ranges(&bounded_selection);
        if let Some(explicit) = explicit_viewport_anchor {
            self.restore_viewport_to(explicit);
        } else if let Some(locked) = locked_viewport_y {
            self.restore_viewport_y(locked);
        } else if !follows_caret {
            self.restore_viewport_to(anchor);
        }
        let generation = ivars.viewport_repair_generation.get().wrapping_add(1);
        ivars.viewport_repair_generation.set(generation);
        if (explicit_viewport_anchor.is_some() || follows_local_edit) && !follows_caret {
            let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
            let repair = Rc::new(move || {
                let Some(view) = weak.load() else { return };
                if view.ivars().viewport_repair_generation.get() != generation {
                    return;
                }
                view.restore_viewport_to(anchor);
            });
            let first = repair.clone();
            main_async(move || first());
            let weak_second: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
            main_async(move || {
                if let Some(view) = weak_second.load() {
                    view.layoutSubtreeIfNeeded();
                    view.prepare_for_display();
                }
                repair();
            });
        }
    }

    /// Structural children for rendered objects (VoiceOver).
    fn refresh_fragment_accessibility(&self) {
        if self.mode() == RenderMode::Source {
            self.ivars().fragment_accessibility_elements.borrow_mut().clear();
            unsafe { self.setAccessibilityChildren(None) };
            return;
        }
        let mut elements: Vec<Retained<FragmentAccessibilityElement>> = Vec::new();
        let document = self.parsed_document();
        document.root.walk(&mut |block| {
            let descriptor: Option<(&str, &NSString, &str)> = unsafe {
                match &block.content {
                    BlockContent::Table(_) => Some(("Markdown table", objc2_app_kit::NSAccessibilityGroupRole, "Rendered markdown table")),
                    BlockContent::MathBlock { .. } => Some(("Display math", objc2_app_kit::NSAccessibilityGroupRole, "Rendered display math")),
                    BlockContent::Mermaid { .. } => Some(("Mermaid diagram", objc2_app_kit::NSAccessibilityGroupRole, "Rendered mermaid diagram")),
                    BlockContent::FrontMatter(_) => Some((
                        "Edit document metadata",
                        objc2_app_kit::NSAccessibilityButtonRole,
                        "Edit title, author, tags, status, and other front matter",
                    )),
                    _ => None,
                }
            };
            let Some((label, role, help)) = descriptor else { return };
            let element = FragmentAccessibilityElement::new(self, block.range.location);
            element.setAccessibilityRole(Some(role));
            element.setAccessibilityLabel(Some(&NSString::from_str(label)));
            unsafe { element.setAccessibilityParent(Some(self)) };
            element.setAccessibilityHelp(Some(&NSString::from_str(help)));
            element.setAccessibilityEnabled(true);
            if let BlockContent::FrontMatter(_) = block.content {
                let range = block.range;
                let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
                *element.ivars().on_press.borrow_mut() = Some(Box::new(move || {
                    let Some(view) = weak.load() else { return false };
                    if let Some(delegate) = view.delegate() {
                        delegate.did_activate_front_matter_at(&view, range);
                    }
                    true
                }));
            }
            elements.push(element);
        });
        let children: Vec<Retained<AnyObject>> =
            elements.iter().map(|element| Retained::into_super(Retained::into_super(element.clone())).into()).collect();
        *self.ivars().fragment_accessibility_elements.borrow_mut() = elements;
        let array = NSArray::from_retained_slice(&children);
        unsafe { self.setAccessibilityChildren(Some(&array)) };
    }

    /// Keep the current pixel camera through the next parse/decorate commit.
    pub fn preserve_viewport_on_next_document_update(&self) {
        self.ivars()
            .next_document_update_viewport_y
            .set(self.scroll_view().map(|scroll| scroll.contentView().bounds().origin.y));
    }

    /// Capture the source camera before a command mutates shared storage.
    pub fn prepare_for_external_document_edits(&self, edits: &[TextEdit]) {
        let Some(storage) = self.text_storage() else { return };
        if edits.is_empty() {
            return;
        }
        let mut anchor = self.capture_viewport_anchor();
        let mut sorted: Vec<&TextEdit> = edits.iter().collect();
        sorted.sort_by_key(|edit| std::cmp::Reverse(edit.range.location));
        for edit in sorted {
            if !(edit.range.location >= 0 && edit.range.upper_bound() <= storage.length() as isize) {
                continue;
            }
            let inserted = NSString::from_str(&edit.replacement).length() as isize;
            anchor = self.project_viewport_anchor(anchor, edit.range, inserted);
        }
        self.ivars().local_edit_viewport_anchor.set(Some(anchor));
    }

    /// Undo changes both bytes and selection; keep the raw clip coordinate.
    pub fn preserve_viewport_across_undo_redo(&self) {
        let Some(viewport_y) = self.scroll_view().map(|scroll| scroll.contentView().bounds().origin.y) else { return };
        self.ivars().next_document_update_viewport_y.set(Some(viewport_y));
        self.ivars().content_storage.suspend_custom_layout();
    }

    /// Returns a one-shot repair for transient chrome changes.
    pub fn make_viewport_repair(&self) -> impl Fn() + 'static {
        let visible = self.scroll_view().map_or_else(|| self.visibleRect(), |scroll| scroll.documentVisibleRect());
        let viewport_y = self.scroll_view().map_or(0.0, |scroll| scroll.contentView().bounds().origin.y);
        let selection_offset = self.source_selected_range().location;
        let anchor = match self.rect_for_offset(selection_offset) {
            Some(selection_rect) if selection_rect.intersects(visible) => self.capture_viewport_anchor_at(selection_offset),
            _ => self.capture_viewport_anchor(),
        };
        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        move || {
            let Some(view) = weak.load() else { return };
            view.restore_viewport_to(anchor);
            view.restore_viewport_y(viewport_y);
            let weak = ObjcWeak::from(&*view);
            main_async(move || {
                if let Some(view) = weak.load() {
                    view.restore_viewport_to(anchor);
                    view.restore_viewport_y(viewport_y);
                }
            });
        }
    }

    /// Source-targeted variant for rendered controls.
    pub fn make_viewport_repair_at(&self, source_offset: isize) -> impl Fn() + 'static {
        let anchor = self.capture_viewport_anchor_at(source_offset);
        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        move || {
            let Some(view) = weak.load() else { return };
            view.resize_to_fit_content();
            view.restore_viewport_to(anchor);
            let weak = ObjcWeak::from(&*view);
            main_async(move || {
                if let Some(view) = weak.load() {
                    view.resize_to_fit_content();
                    view.restore_viewport_to(anchor);
                }
            });
        }
    }

    fn restore_viewport_y(&self, y: CGFloat) {
        let Some(scroll_view) = self.scroll_view() else { return };
        let clip = scroll_view.contentView();
        let clip_bounds = clip.bounds();
        let content_height = smax(self.frame().size.height, clip_bounds.max_y());
        let max_y = smax(0.0, content_height - clip_bounds.size.height);
        clip.scrollToPoint(NSPoint::new(clip_bounds.origin.x, smin(smax(0.0, y), max_y)));
        scroll_view.reflectScrolledClipView(&clip);
    }

    pub fn capture_viewport_anchor(&self) -> ViewportAnchor {
        self.capture_viewport_anchor_at(self.top_visible_offset())
    }

    pub fn capture_viewport_anchor_at(&self, offset: isize) -> ViewportAnchor {
        let visible = self.scroll_view().map_or_else(|| self.visibleRect(), |scroll| scroll.documentVisibleRect());
        ViewportAnchor {
            offset,
            gap: self.rect_for_offset(offset).map_or(0.0, |rect| visible.min_y() - rect.min_y()),
        }
    }

    /// Puts the anchor line back exactly where it was; a no-op when the
    /// viewport is already there.
    pub fn restore_viewport_to(&self, anchor: ViewportAnchor) {
        let Some(scroll_view) = self.scroll_view() else { return };
        let offset = anchor.offset.max(0).min(self.parsed_document().length);
        let Some(rect) = self.rect_for_offset(offset) else { return };
        let clip = scroll_view.contentView();
        let clip_bounds = clip.bounds();
        let content_height = smax(self.frame().size.height, clip_bounds.max_y());
        let max_y = smax(0.0, content_height - clip_bounds.size.height);
        let y = smin(smax(0.0, rect.min_y() + anchor.gap), max_y);
        if !((y - clip_bounds.origin.y).abs() > 0.5) {
            return;
        }
        clip.scrollToPoint(NSPoint::new(clip_bounds.origin.x, y));
        scroll_view.reflectScrolledClipView(&clip);
    }

    /// Carries a source-coordinate camera through a source edit.
    pub fn project_viewport_anchor(&self, anchor: ViewportAnchor, edit: NSRange, inserted_length: isize) -> ViewportAnchor {
        let mut projected = anchor;
        if anchor.offset >= edit.upper_bound() {
            projected.offset += inserted_length - edit.length;
        } else if anchor.offset > edit.location {
            projected.offset = edit.location + inserted_length;
        }
        projected.offset = projected.offset.max(0);
        projected
    }

    /// Size the document view to the height layout actually used.
    pub fn resize_to_fit_content(&self) {
        self.resize_to_fit_content_scoped(ContentLayoutScope::Document);
    }

    /// Resolves the restored viewport before the first frame is shown.
    pub fn prepare_for_display(&self) {
        self.synchronize_visible_layout();
        let visible = self.scroll_view().map_or_else(|| self.visibleRect(), |scroll| scroll.documentVisibleRect());
        self.displayRect(visible);
    }

    /// Makes TextKit's active fragment set agree with the clip view.
    pub fn synchronize_visible_layout(&self) {
        let Some(layout_manager) = self.textLayoutManager() else { return };
        let visible = self.scroll_view().map_or_else(|| self.visibleRect(), |scroll| scroll.documentVisibleRect());
        let origin = self.textContainerOrigin();
        let viewport = rect(
            smax(0.0, visible.min_x() - origin.x),
            smax(0.0, visible.min_y() - origin.y),
            smax(1.0, visible.width()),
            smax(1.0, visible.height()),
        );
        layout_manager.ensureLayoutForBounds(viewport);
        layout_manager.textViewportLayoutController().layoutViewport();
        self.setNeedsDisplayInRect(visible);
        self.gutter_rail_needs_display();
    }

    /// Paragraphs the eager path may materialise before the estimate takes
    /// over.
    const EAGER_LAYOUT_PARAGRAPH_LIMIT: usize = 4_000;

    fn exceeds_eager_layout_budget(&self) -> bool {
        if let Some(storage) = self.text_storage()
            && (storage.length() as i64) > self.ivars().configuration.borrow().large_file_threshold_megabytes() * 1024 * 1024
        {
            return true;
        }
        self.ivars().paragraph_index.borrow().starts.len() > Self::EAGER_LAYOUT_PARAGRAPH_LIMIT
    }

    fn resize_to_fit_content_scoped(&self, layout_scope: ContentLayoutScope) {
        let Some(layout_manager) = self.textLayoutManager() else { return };
        if layout_scope == ContentLayoutScope::Viewport {
            self.repair_content_height_from_viewport(&layout_manager);
            return;
        }
        let frame = self.frame();
        let style_sheet = self.style_sheet();
        if self.exceeds_eager_layout_budget() {
            let viewport_height = self.scroll_view().map_or(0.0, |scroll| scroll.contentView().bounds().size.height);
            let paragraphs = self.ivars().paragraph_index.borrow().starts.len().max(1) as CGFloat;
            let estimated = smax(
                viewport_height,
                paragraphs * style_sheet.line_height + self.textContainerInset().height * 2.0 + viewport_height * 0.40,
            );
            if !((frame.size.height - estimated).abs() > 0.5) {
                return;
            }
            if estimated < frame.size.height
                && self.viewport_is_pinned_to_bottom()
                && !self.ivars().applying_pending_shrink_repair.get()
            {
                self.defer_shrink_repair();
                return;
            }
            self.set_frame_size_preserving(NSSize::new(frame.size.width, estimated), estimated < frame.size.height);
            return;
        }
        layout_manager.ensureLayoutForRange(&layout_manager.documentRange());

        let used = layout_manager.usageBoundsForTextContainer();
        let viewport_height = self.scroll_view().map_or(0.0, |scroll| scroll.contentView().bounds().size.height);
        let frame = self.frame();
        let height = smax(used.max_y() + self.textContainerInset().height + viewport_height * 0.40, viewport_height);
        if !((frame.size.height - height).abs() > 0.5) {
            return;
        }
        if height < frame.size.height
            && self.viewport_is_pinned_to_bottom()
            && !self.ivars().applying_pending_shrink_repair.get()
        {
            self.defer_shrink_repair();
            return;
        }
        self.set_frame_size_preserving(NSSize::new(frame.size.width, height), height < frame.size.height);
    }

    fn defer_shrink_repair(&self) {
        self.ivars().pending_shrink_repair.set(true);
        if self.ivars().pending_shrink_repair_work_item.borrow().is_some() {
            return;
        }
        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        let work_item = WorkItem::new(move || {
            let Some(view) = weak.load() else { return };
            *view.ivars().pending_shrink_repair_work_item.borrow_mut() = None;
            if !view.ivars().pending_shrink_repair.get() {
                return;
            }
            view.ivars().pending_shrink_repair.set(false);
            view.ivars().applying_pending_shrink_repair.set(true);
            view.resize_to_fit_content_scoped(ContentLayoutScope::Document);
            view.ivars().applying_pending_shrink_repair.set(false);
        });
        *self.ivars().pending_shrink_repair_work_item.borrow_mut() = Some(work_item.clone());
        work_item.dispatch_main();
    }

    /// Pin the anchor line across a shrink.
    fn set_frame_size_preserving(&self, size: NSSize, preserving_reading_position: bool) {
        if !preserving_reading_position {
            self.setFrameSize(size);
            return;
        }
        let anchor = self.capture_viewport_anchor();
        self.setFrameSize(size);
        self.restore_viewport_to(anchor);
    }

    /// Structural zoom (§5.2): spring the document's extent into place.
    fn animate_structural_zoom_height(&self, previous_height: CGFloat) {
        self.resize_to_fit_content_scoped(ContentLayoutScope::Document);
        let target_height = self.frame().size.height;
        if !((target_height - previous_height).abs() > 0.5) {
            return;
        }
        if self.style_sheet().reduce_motion {
            return;
        }
        self.setWantsLayer(true);
        let Some(animation_layer) = self.layer() else { return };
        let key = NSString::from_str("downrightStructuralZoom");
        animation_layer.removeAnimationForKey(&key);
        let settle = objc2_quartz_core::CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str(
            "bounds.size.height",
        )));
        unsafe {
            settle.setFromValue(Some(&NSNumber::new_f64(previous_height)));
            settle.setToValue(Some(&NSNumber::new_f64(target_height)));
        }
        use objc2_quartz_core::CAMediaTiming;
        settle.setDuration(motion::DELIBERATE);
        settle.setTimingFunction(Some(&motion::timing(motion::Curve::Structural)));
        animation_layer.addAnimation_forKey(&settle, Some(&key));
    }

    /// True while the visible region sits within a hair of the bottom.
    fn viewport_is_pinned_to_bottom(&self) -> bool {
        let Some(scroll_view) = self.scroll_view() else { return false };
        let slack = smax(24.0, scroll_view.contentView().bounds().size.height * 0.15);
        scroll_view.documentVisibleRect().max_y() >= self.frame().size.height - slack
    }

    /// Layout only the visible viewport for semantic updates; may grow the
    /// frame but never shrinks it.
    fn repair_content_height_from_viewport(&self, layout_manager: &NSTextLayoutManager) {
        let visible = self.scroll_view().map_or_else(|| self.visibleRect(), |scroll| scroll.documentVisibleRect());
        let viewport_height = smax(
            visible.height(),
            self.scroll_view().map_or(0.0, |scroll| scroll.contentView().bounds().size.height),
        );
        if !(viewport_height > 0.0) {
            return;
        }
        let origin = self.textContainerOrigin();
        let viewport_bounds = rect(
            smax(0.0, visible.min_x() - origin.x),
            smax(0.0, visible.min_y() - origin.y),
            smax(1.0, visible.width()),
            viewport_height,
        );
        layout_manager.ensureLayoutForBounds(viewport_bounds);
        let paragraphs = self.ivars().paragraph_index.borrow().starts.len().max(1) as CGFloat;
        let inset = self.textContainerInset().height;
        let estimated = paragraphs * self.style_sheet().line_height + inset * 2.0 + viewport_height * 0.40;
        let used = layout_manager.usageBoundsForTextContainer().max_y() + inset + viewport_height * 0.40;
        let frame = self.frame();
        let height = smax(smax(smax(frame.size.height, estimated), used), viewport_height);
        if !(height - frame.size.height > 0.5) {
            return;
        }
        self.setFrameSize(NSSize::new(frame.size.width, height));
    }

    /// Schedules a height pass.
    fn request_content_resize(&self, request: ContentResizeRequest, anchor: Option<ViewportAnchor>, viewport_y: Option<CGFloat>) {
        let ivars = self.ivars();
        if let Some(anchor) = anchor {
            ivars.pending_resize_anchor.set(Some(anchor));
        }
        if let Some(viewport_y) = viewport_y {
            ivars.pending_resize_viewport_y.set(Some(viewport_y));
        }
        if request == ContentResizeRequest::ScrollRepair {
            ivars.pending_resize_anchor.set(None);
            ivars.pending_resize_viewport_y.set(None);
        }
        ivars
            .pending_resize_request
            .set(Some(ContentResizePolicy::merge(ivars.pending_resize_request.get(), request)));
        let Some(pending) = ivars.pending_resize_request.get() else { return };

        let generation = ivars.resize_generation.get().wrapping_add(1);
        ivars.resize_generation.set(generation);
        if let Some(item) = ivars.resize_work_item.borrow_mut().take() {
            item.cancel();
        }

        if pending == ContentResizeRequest::Immediate {
            ivars.pending_resize_request.set(None);
            let anchor = ivars.pending_resize_anchor.take();
            let viewport_y = ivars.pending_resize_viewport_y.take();
            ivars.resize_needs_repair.set(false);
            self.resize_to_fit_content();
            if let Some(anchor) = anchor {
                self.restore_viewport_to(anchor);
            } else if let Some(viewport_y) = viewport_y {
                self.restore_viewport_y(viewport_y);
            }
            return;
        }
        ivars.resize_needs_repair.set(true);

        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        let work_item = WorkItem::new(move || {
            let Some(view) = weak.load() else { return };
            let ivars = view.ivars();
            if ivars.resize_generation.get() != generation {
                return;
            }
            *ivars.resize_work_item.borrow_mut() = None;
            if ivars.pending_resize_request.get().is_none() {
                return;
            }
            ivars.pending_resize_request.set(None);
            let anchor = ivars.pending_resize_anchor.take();
            let viewport_y = ivars.pending_resize_viewport_y.take();
            ivars.resize_needs_repair.set(pending == ContentResizeRequest::Semantic);
            view.resize_to_fit_content_scoped(if pending == ContentResizeRequest::Semantic {
                ContentLayoutScope::Viewport
            } else {
                ContentLayoutScope::Document
            });
            if let Some(anchor) = anchor {
                view.restore_viewport_to(anchor);
            } else if let Some(viewport_y) = viewport_y {
                view.restore_viewport_y(viewport_y);
            }
        });
        *ivars.resize_work_item.borrow_mut() = Some(work_item.clone());
        let delay = ContentResizePolicy::idle_delay(pending);
        if delay == 0.0 {
            work_item.dispatch_main();
        } else {
            work_item.dispatch_main_after(delay);
        }
    }

    fn rebuild_everything(&self) {
        let Some(storage) = self.text_storage() else { return };
        self.rebuild_paragraph_index();
        let document = self.parsed_document();
        self.ivars().engine.borrow_mut().decorate(&storage, &document, &DirtySet::wholesale());
        self.apply_source_presentation(None);
        self.apply_invisibles(None);
        self.rebuild_base_display_map(&document);
        self.refresh_elision(false);
        self.rebuild_display_map(true, &[]);
        self.apply_overlays(None);
        self.apply_path_existence(None);
        self.invalidate_all_fragments();
        if let Some(rail) = self.gutter_rail() {
            rail.reload();
        }
        self.refresh_fragment_accessibility();
    }

    /// Theme colour / accent swaps that do not change typography.
    fn restyle_attributes_preserving_geometry(&self) {
        let Some(storage) = self.text_storage() else { return };
        let document = self.parsed_document();
        self.ivars().engine.borrow_mut().decorate(&storage, &document, &DirtySet::wholesale());
        self.apply_source_presentation(None);
        self.apply_overlays(None);
        self.apply_path_existence(None);
        self.rebuild_base_display_map(&document);
        self.rebuild_display_map(true, &[]);
        self.invalidate_all_fragments();
        let layout_manager = &self.ivars().markdown_layout_manager;
        if self.exceeds_eager_layout_budget() {
            layout_manager.textViewportLayoutController().layoutViewport();
        } else {
            layout_manager.ensureLayoutForRange(&layout_manager.documentRange());
        }
        if let Some(rail) = self.gutter_rail() {
            rail.reload();
        }
        self.setNeedsDisplay(true);
    }

    fn refresh_base_layout_map(&self) {
        let base = self.ivars().base_display_map.borrow().clone();
        let layout = self.layout_display_map(&base);
        *self.ivars().base_layout_map.borrow_mut() = layout;
    }

    /// `rebuildBaseDisplayMap(document:)`, through the shared producer in
    /// `view::base_display_map`.
    fn rebuild_base_display_map(&self, document: &ParsedDocument) {
        let Some(storage) = self.text_storage() else { return };
        let paragraph_index = self.paragraph_index();
        let style_sheet = self.style_sheet();
        let maps = {
            let engine = self.ivars().engine.borrow();
            let inputs = BaseDisplayMapInputs {
                document,
                engine: &engine,
                effective_policy: self.effective_policy(),
                source_focus: self.source_focus(),
                reflow_hard_wrapped_paragraphs: self.ivars().configuration.borrow().reflow_hard_wrapped_paragraphs,
                style_sheet: &style_sheet,
                storage: &storage,
                paragraph_index: &paragraph_index,
            };
            base_display_map::rebuild_base_display_map(&inputs, &mut self.ivars().word_joiner_runs.borrow_mut())
        };
        let ivars = self.ivars();
        *ivars.base_hidden_ranges.borrow_mut() = maps.base_hidden_ranges;
        *ivars.hard_wrap_ranges.borrow_mut() = maps.hard_wrap_ranges;
        *ivars.hard_wrap_substitutions.borrow_mut() = maps.hard_wrap_substitutions;
        *ivars.base_display_map.borrow_mut() = maps.base_display_map;
        *ivars.base_layout_map.borrow_mut() = maps.base_layout_map.clone();
        *ivars.display_map.borrow_mut() = maps.base_layout_map;
    }

    /// `layoutDisplayMap(from:)`.
    fn layout_display_map(&self, logical: &DisplayMap) -> DisplayMap {
        let storage: Retained<NSAttributedString> = match self.text_storage() {
            Some(storage) => Retained::into_super(Retained::into_super(storage)),
            None => NSAttributedString::new(),
        };
        base_display_map::layout_display_map(
            logical,
            &self.paragraph_index(),
            &storage,
            &mut self.ivars().word_joiner_runs.borrow_mut(),
        )
    }

    /// Source Focus changes typography and local material, never characters.
    fn apply_source_presentation(&self, scopes: Option<&[NSRange]>) {
        let Some(storage) = self.text_storage() else { return };
        if !(storage.length() > 0) {
            return;
        }
        let whole = NSRange::new(0, storage.length() as isize);
        let source_focus = self.source_focus();
        let applied = self.ivars().applied_source_focus.get();

        let (target, scoped) = match source_focus {
            SourceFocus::None => {
                if applied == SourceFocus::None {
                    return;
                }
                if let Some(previous) = Self::source_presentation_range(applied, whole) {
                    storage.beginEditing();
                    storage.removeAttribute_range(attribute_keys::dr_source_focus(), ns(previous));
                    storage.endEditing();
                }
                self.ivars().applied_source_focus.set(SourceFocus::None);
                return;
            }
            SourceFocus::Document => (whole, false),
            SourceFocus::Scoped(range) => {
                let lower = range.location.min(storage.length() as isize).max(0);
                let upper = lower.max(range.upper_bound().min(storage.length() as isize));
                if !(upper > lower) {
                    return;
                }
                (NSRange::new(lower, upper - lower), true)
            }
        };

        let focus_changed = applied != source_focus;
        if focus_changed && let Some(previous) = Self::source_presentation_range(applied, whole) {
            storage.removeAttribute_range(attribute_keys::dr_source_focus(), ns(previous));
        }
        self.ivars().applied_source_focus.set(source_focus);

        let application_ranges: Vec<NSRange> = match scopes {
            Some(scopes) if !focus_changed => {
                let clipped: Vec<NSRange> = scopes.iter().filter_map(|scope| scope.intersection(target)).collect();
                RangeSet::normalized(&clipped)
            }
            _ => vec![target],
        };
        if application_ranges.is_empty() {
            return;
        }

        storage.beginEditing();
        let style_sheet = self.style_sheet();
        let (mono, ligature) = style_sheet.mono_font_attributes(None);
        let ligature = NSNumber::new_isize(ligature);
        let focus_flag = NSNumber::new_bool(true);

        let source: Retained<NSString> = storage.string();
        let paragraph_ranges: Vec<NSRange> = application_ranges
            .iter()
            .filter_map(|range| {
                let lower = from_ns(source.paragraphRangeForRange(objc2_foundation::NSRange::new(range.location as usize, 0)));
                let upper_offset = range.location.max(range.upper_bound() - 1);
                let upper = from_ns(source.paragraphRangeForRange(objc2_foundation::NSRange::new(upper_offset as usize, 0)));
                lower.union(upper).intersection(target)
            })
            .collect();
        let paragraphs = RangeSet::normalized(&paragraph_ranges);
        let line_height = style_sheet.line_height;
        let character_width = style_sheet.average_character_width;
        let tab_stops: Vec<Retained<NSTextTab>> = (4..=80)
            .step_by(4)
            .map(|column| unsafe {
                NSTextTab::initWithTextAlignment_location_options(
                    NSTextTab::alloc(),
                    NSTextAlignment::Left,
                    column as CGFloat * character_width,
                    &NSDictionary::new(),
                )
            })
            .collect();
        let tab_stops = NSArray::from_retained_slice(&tab_stops);
        let mut style_cache: Vec<(SourceParagraphSpacing, Retained<NSParagraphStyle>)> = Vec::new();
        let mut paragraph_style = |spacing: SourceParagraphSpacing| -> Retained<NSParagraphStyle> {
            if let Some((_, cached)) = style_cache.iter().find(|(key, _)| *key == spacing) {
                return cached.clone();
            }
            let style = NSMutableParagraphStyle::new();
            style.setMinimumLineHeight(line_height);
            style.setMaximumLineHeight(line_height);
            style.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
            style.setParagraphSpacingBefore(spacing.before);
            style.setParagraphSpacing(spacing.after);
            style.setTabStops(Some(&tab_stops));
            style.setDefaultTabInterval(character_width * 4.0);
            let immutable: Retained<NSParagraphStyle> = unsafe { msg_send![&*style, copy] };
            style_cache.push((spacing, immutable.clone()));
            immutable
        };

        let add_attributes = |range: NSRange, paragraph: Option<&NSParagraphStyle>| {
            let mut pairs: Vec<(&NSString, &AnyObject)> = vec![
                (keys::font(), &*mono),
                (keys::ligature(), &*ligature),
                (attribute_keys::dr_source_focus(), &*focus_flag),
            ];
            if let Some(paragraph) = paragraph {
                pairs.push((keys::paragraph_style(), paragraph));
            }
            let key_list: Vec<&NSString> = pairs.iter().map(|(key, _)| *key).collect();
            let value_list: Vec<&AnyObject> = pairs.iter().map(|(_, value)| *value).collect();
            let attributes = NSDictionary::from_slices(&key_list, &value_list);
            unsafe { storage.addAttributes_range(&attributes, ns(range)) };
        };

        if !scoped {
            let style = paragraph_style(SourceParagraphSpacing { before: 0.0, after: 0.0 });
            for range in &paragraphs {
                add_attributes(*range, Some(&style));
            }
            storage.endEditing();
            return;
        }

        for range in &application_ranges {
            add_attributes(*range, None);
        }

        for paragraphs_range in &paragraphs {
            let mut cursor = paragraphs_range.location;
            while cursor < paragraphs_range.upper_bound() {
                let paragraph = from_ns(source.paragraphRangeForRange(objc2_foundation::NSRange::new(cursor as usize, 0)))
                    .intersection(target)
                    .unwrap_or(NSRange::new(cursor, 0));
                if !(paragraph.length > 0) {
                    break;
                }
                let spacing = SourceParagraphSpacing {
                    before: if scoped && cursor == target.location { 28.0 } else { 0.0 },
                    after: if scoped && paragraph.upper_bound() >= target.upper_bound() { 8.0 } else { 0.0 },
                };
                let style = paragraph_style(spacing);
                unsafe { storage.addAttribute_value_range(keys::paragraph_style(), &style, ns(paragraph)) };
                cursor = paragraph.upper_bound();
            }
        }
        storage.endEditing();
    }

    fn source_presentation_range(focus: SourceFocus, whole: NSRange) -> Option<NSRange> {
        match focus {
            SourceFocus::None => None,
            SourceFocus::Document => Some(whole),
            SourceFocus::Scoped(range) => range.intersection(whole),
        }
    }

    pub fn rebuild_paragraph_index(&self) {
        let Some(storage) = self.text_storage() else { return };
        let index = ParagraphIndex::from_text(&storage.string());
        *self.ivars().paragraph_index.borrow_mut() = index.clone();
        *self.ivars().fragment_context.paragraph_index.borrow_mut() = index;
    }

    pub fn paragraph_range_containing(&self, offset: isize) -> NSRange {
        self.ivars().paragraph_index.borrow().paragraph_range_containing(offset)
    }

    pub(crate) fn refresh_display_map_for_composition(&self) {
        self.rebuild_display_map(false, &[]);
    }

    /// Keeps unaffected paragraphs rendered while an asynchronous parse is in
    /// flight. Only the edited paragraph span falls back to literal source.
    pub fn project_display_map_across_edit(
        &self,
        edit: NSRange,
        inserted_length: isize,
        old_paragraphs: &ParagraphIndex,
        old_hidden_ranges: &[NSRange],
        preserves_paragraph_structure: bool,
    ) {
        let ivars = self.ivars();
        let previous_logical_map = ivars.base_display_map.borrow().clone();
        let previous_layout_map = ivars.base_layout_map.borrow().clone();
        let old_display_objects: Vec<DisplaySubstitution> = previous_logical_map
            .substitutions()
            .into_iter()
            .filter(|sub| !sub.is_hidden && !sub.is_hard_wrap_reflow)
            .collect();
        let old_layout_substitutions = previous_layout_map.substitutions();
        let projection = SourceEditProjection::new(edit, inserted_length, old_paragraphs);
        let project_presentation_range = |range: NSRange| -> Option<NSRange> {
            if preserves_paragraph_structure { projection.project_unchanged(range) } else { projection.project(range) }
        };
        let projected_hidden: Vec<NSRange> =
            old_hidden_ranges.iter().filter_map(|range| project_presentation_range(*range)).collect();
        let project_substitution = |sub: &DisplaySubstitution| -> Option<DisplaySubstitution> {
            let range = project_presentation_range(sub.source_range)?;
            let mut projected = sub.clone();
            projected.source_range = range;
            Some(projected)
        };
        let projected_display_objects: Vec<DisplaySubstitution> =
            old_display_objects.iter().filter_map(project_substitution).collect();
        let projected_layout_substitutions: Vec<DisplaySubstitution> =
            old_layout_substitutions.iter().filter_map(project_substitution).collect();
        let projected_hard_wrap_ranges = RangeSet::normalized(
            &ivars
                .hard_wrap_ranges
                .borrow()
                .iter()
                .filter_map(|range| projection.project_container(*range, preserves_paragraph_structure))
                .collect::<Vec<_>>(),
        );
        let projected_hard_wrap_substitutions: Vec<DisplaySubstitution> = ivars
            .hard_wrap_substitutions
            .borrow()
            .iter()
            .filter_map(|sub| {
                let range = projection.project_unchanged(sub.source_range)?;
                if !RangeSet::covers(&projected_hard_wrap_ranges, range.location) {
                    return None;
                }
                let mut projected = sub.clone();
                projected.source_range = range;
                Some(projected)
            })
            .collect();

        *ivars.base_hidden_ranges.borrow_mut() = projected_hidden.clone();
        *ivars.hard_wrap_ranges.borrow_mut() = projected_hard_wrap_ranges.clone();
        *ivars.hard_wrap_substitutions.borrow_mut() = projected_hard_wrap_substitutions.clone();
        let mut projected_logical: Vec<DisplaySubstitution> =
            projected_hidden.iter().map(|range| DisplaySubstitution::hide(*range)).collect();
        projected_logical.extend(projected_display_objects);
        projected_logical.extend(projected_hard_wrap_substitutions);
        let paragraph_index = self.paragraph_index();
        let base_display_map =
            previous_logical_map.projecting_stable_topology(paragraph_index.clone(), projected_logical, projected_hidden.clone());
        let base_layout_map = previous_layout_map.projecting_stable_topology(
            paragraph_index.clone(),
            projected_layout_substitutions,
            projected_hidden.clone(),
        );
        *ivars.base_display_map.borrow_mut() = base_display_map.clone();
        *ivars.base_layout_map.borrow_mut() = base_layout_map.clone();
        *ivars.display_map.borrow_mut() = base_layout_map.clone();
        ivars.substitution.set_display_map(base_display_map);
        ivars.reveal_paragraph.set(None);

        let Some(storage) = self.text_storage() else { return };
        let length = storage.length() as isize;
        if !(length > 0) {
            return;
        }
        let inserted_end = length.min(edit.location + inserted_length);
        let first = paragraph_index.paragraph_range_containing(edit.location.min(length));
        let last = paragraph_index.paragraph_range_containing(edit.location.max(inserted_end - 1));
        let Some(affected) = first.union(last).intersection(NSRange::new(0, length)) else { return };
        if !(affected.length > 0) {
            return;
        }
        let layout_affected = if inserted_length == edit.length {
            affected
        } else {
            NSRange::new(affected.location, length - affected.location)
        };
        ivars.content_storage.configure(
            &paragraph_index,
            &projected_hard_wrap_ranges,
            &base_layout_map,
            Some(&[layout_affected]),
        );
        storage.beginEditing();
        storage.removeAttribute_range(attribute_keys::dr_fragment(), ns(affected));
        storage.removeAttribute_range(attribute_keys::dr_elided(), ns(affected));
        storage.endEditing();
        self.apply_hidden_attribute(&projected_hidden, Some(affected), &[]);
        self.invalidate_fragments(Some(layout_affected));
    }

    pub(crate) fn begin_source_edit(&self) {
        self.ivars().is_performing_source_edit.set(true);
        self.ivars().substitution.set_display_map(DisplayMap::identity());
        self.ivars().content_storage.suspend_custom_layout();
    }

    pub(crate) fn end_source_edit(&self) {
        self.ivars().is_performing_source_edit.set(false);
    }

    // MARK: - Hidden ranges and the display map

    fn same_substitutions(lhs: &[DisplaySubstitution], rhs: &[DisplaySubstitution]) -> bool {
        if lhs.len() != rhs.len() {
            return false;
        }
        lhs.iter().zip(rhs.iter()).all(|(left, right)| {
            let same_replacement = match (&left.replacement, &right.replacement) {
                (None, None) => true,
                (Some(left), Some(right)) => left.isEqualToAttributedString(right),
                _ => false,
            };
            left.source_range == right.source_range
                && left.display_length == right.display_length
                && left.is_hidden == right.is_hidden
                && left.is_hard_wrap_reflow == right.is_hard_wrap_reflow
                && left.preserves_source_offsets == right.preserves_source_offsets
                && same_replacement
        })
    }

    /// Rebuilds the substitution set for the current caret (§12).
    fn rebuild_display_map(&self, full_refresh: bool, additional_scopes: &[NSRange]) {
        let ivars = self.ivars();
        let previous_substitutions = ivars.display_map.borrow().substitutions();
        let caret = if ivars.suppresses_caret_reveal.get() { None } else { self.primary_source_caret() };
        let base_hidden_ranges = ivars.base_hidden_ranges.borrow().clone();
        let base_display_map = ivars.base_display_map.borrow().clone();
        let base_layout_map = ivars.base_layout_map.borrow().clone();
        let paragraph_index = self.paragraph_index();
        let mut hidden = base_hidden_ranges.clone();
        let mut revealed_for_attributes: Vec<NSRange> = Vec::new();
        let mut requires_full_hidden_refresh = false;
        let mut hidden_is_paragraph_scoped = false;
        let mut logical_display_map = base_display_map.clone();
        let mut layout_map = base_layout_map.clone();
        let policy = self.effective_policy();

        if let Some(composing) = ivars.composing_paragraph.get() {
            hidden.retain(|range| range.upper_bound() <= composing.location || range.location >= composing.upper_bound());
            let still_hiding = |map: &DisplayMap| -> Vec<DisplaySubstitution> {
                map.substitutions_in_paragraph_containing(composing.location)
                    .into_iter()
                    .filter(|entry| !base_hidden_ranges.contains(&entry.source_range))
                    .collect()
            };
            logical_display_map = base_display_map.replacing_paragraph(composing.location, still_hiding(&base_display_map));
            layout_map = base_layout_map.replacing_paragraph(composing.location, still_hiding(&base_layout_map));
            requires_full_hidden_refresh = caret.is_none();
        } else if policy.reveals_at_caret {
            let document = self.parsed_document();
            let selections = self.source_selected_ranges();
            let revealed = MarkerPolicy::revealed_marker_ranges(&document, policy, caret, &selections);
            let revealed_math = caret.map(|caret| InlineMathDisplay::ranges_touching(&document, caret)).unwrap_or_default();
            let mut revealed_display_objects = revealed.clone();
            revealed_display_objects.extend(revealed_math);
            let paragraph = caret.map(|caret| paragraph_index.paragraph_range_containing(caret));
            let single_caret = paragraph.is_some_and(|paragraph| {
                selections.len() <= 1
                    && revealed
                        .iter()
                        .all(|range| range.location >= paragraph.location && range.upper_bound() <= paragraph.upper_bound())
            });
            if let Some(paragraph) = paragraph
                && single_caret
            {
                if !revealed_display_objects.is_empty() {
                    logical_display_map =
                        base_display_map.replacing_paragraph_excluding(paragraph.location, &revealed_display_objects);
                    layout_map = base_layout_map.replacing_paragraph_excluding(paragraph.location, &revealed_display_objects);
                    revealed_for_attributes = revealed.clone();
                }
                if !full_refresh && additional_scopes.is_empty() {
                    let affected: Vec<NSRange> = [Some(paragraph), ivars.reveal_paragraph.get()].into_iter().flatten().collect();
                    let scoped: Vec<NSRange> = affected
                        .iter()
                        .flat_map(|range| base_display_map.hidden_ranges_in_paragraph_containing(range.location))
                        .collect();
                    hidden = RangeSet::normalized(&scoped);
                    hidden_is_paragraph_scoped = true;
                }
            } else if !revealed.is_empty() {
                hidden = Self::subtract(&revealed, &hidden);
                let unrevealed = |map: &DisplayMap| -> Vec<DisplaySubstitution> {
                    map.substitutions()
                        .into_iter()
                        .filter(|entry| !revealed_display_objects.contains(&entry.source_range))
                        .collect()
                };
                logical_display_map = DisplayMap::new(paragraph_index.clone(), unrevealed(&base_display_map));
                layout_map = DisplayMap::new(paragraph_index.clone(), unrevealed(&base_layout_map));
                requires_full_hidden_refresh = true;
            }
        }
        ivars.substitution.set_display_map(logical_display_map);
        *ivars.display_map.borrow_mut() = layout_map.clone();

        let current = caret.map(|caret| paragraph_index.paragraph_range_containing(caret));
        let is_full_refresh = full_refresh || (requires_full_hidden_refresh && additional_scopes.is_empty());
        let reveal_paragraph = ivars.reveal_paragraph.get();
        let paragraphs: Vec<NSRange> =
            if is_full_refresh { Vec::new() } else { [reveal_paragraph, current].into_iter().flatten().collect() };
        if !is_full_refresh
            && additional_scopes.is_empty()
            && let Some(current) = current
            && base_display_map.substitutions_in_paragraph_containing(current.location).is_empty()
            && reveal_paragraph.is_none_or(|previous| base_display_map.substitutions_in_paragraph_containing(previous.location).is_empty())
        {
            ivars.reveal_paragraph.set(Some(current));
            return;
        }
        if !is_full_refresh
            && additional_scopes.is_empty()
            && Self::same_substitutions(&layout_map.substitutions(), &previous_substitutions)
        {
            ivars.reveal_paragraph.set(current);
            return;
        }
        let touched: Vec<NSRange> = if hidden_is_paragraph_scoped || paragraphs.len() < 2 {
            paragraphs.clone()
        } else {
            vec![paragraphs[0].union(paragraphs[1])]
        };
        ivars.reveal_paragraph.set(current);
        let mut scopes = additional_scopes.to_vec();
        scopes.extend(touched);
        let invalidated_scopes = RangeSet::normalized(&scopes);
        ivars.content_storage.configure(
            &paragraph_index,
            &ivars.hard_wrap_ranges.borrow(),
            &layout_map,
            if is_full_refresh { None } else { Some(&invalidated_scopes) },
        );
        if is_full_refresh {
            self.apply_hidden_attribute(&hidden, None, &revealed_for_attributes);
            self.invalidate_fragments(None);
        } else {
            for scope in &invalidated_scopes {
                self.apply_hidden_attribute(&hidden, Some(*scope), &revealed_for_attributes);
                self.invalidate_fragments(Some(*scope));
            }
        }
    }

    /// `drHidden` mirrors the map so anything reading the storage sees the
    /// same decision the layout did.
    fn apply_hidden_attribute(&self, hidden: &[NSRange], scope: Option<NSRange>, excluding: &[NSRange]) {
        let Some(storage) = self.text_storage() else { return };
        if !(storage.length() > 0) {
            return;
        }
        let window = scope
            .and_then(|scope| self.clamp_to_storage(scope))
            .unwrap_or(NSRange::new(0, storage.length() as isize));
        if !(window.length > 0) {
            return;
        }
        let flag = NSNumber::new_bool(true);
        storage.beginEditing();
        storage.removeAttribute_range(attribute_keys::dr_hidden(), ns(window));
        for range in RangeSet::intersecting(hidden, window) {
            unsafe { storage.addAttribute_value_range(attribute_keys::dr_hidden(), &flag, ns(range)) };
        }
        for range in RangeSet::intersecting(excluding, window) {
            storage.removeAttribute_range(attribute_keys::dr_hidden(), ns(range));
        }
        storage.endEditing();
    }

    fn subtract(removed: &[NSRange], ranges: &[NSRange]) -> Vec<NSRange> {
        if removed.is_empty() {
            return ranges.to_vec();
        }
        ranges.iter().copied().filter(|range| !removed.iter().any(|gone| gone == range)).collect()
    }

    // MARK: - Elision (§5.2, §7.1, §9.4)

    fn definition_elisions(&self) -> Vec<NSRange> {
        if !self.effective_policy().hides_block_markers {
            return Vec::new();
        }
        let Some(storage) = self.text_storage() else {
            return self.definition_ranges_in(&NSString::from_str(""));
        };
        self.definition_ranges_in(&storage.string())
    }

    fn definition_ranges_in(&self, source: &NSString) -> Vec<NSRange> {
        let document = self.parsed_document();
        document
            .link_references
            .values()
            .map(|reference| reference.range)
            .chain(document.footnotes.values().map(|footnote| footnote.range))
            .map(|range| from_ns(source.paragraphRangeForRange(ns(range))))
            .collect()
    }

    fn refresh_elision(&self, rebuilding_map: bool) {
        let ivars = self.ivars();
        let elision = ElisionPlan::make(
            &self.parsed_document(),
            self.zoom_level(),
            &ivars.folded_heading_slugs.borrow(),
            &ivars.search_hits.borrow(),
            self.primary_source_caret(),
            &ivars.expanded_elision_ranges.borrow(),
        );
        *ivars.elision.borrow_mut() = elision.clone();
        *ivars.fragment_context.cue_elision.borrow_mut() = elision.clone();
        let mut elided = elision.elided_ranges.clone();
        elided.extend(self.definition_elisions());
        *ivars.fragment_context.elision.borrow_mut() =
            ElisionPlan::new(RangeSet::normalized(&elided), elision.forced_visible_ranges.clone());
        self.apply_elided_attribute();
        if rebuilding_map {
            self.rebuild_display_map(true, &[]);
            if let Some(rail) = self.gutter_rail() {
                rail.reload();
            }
        }
    }

    fn apply_elided_attribute(&self) {
        let Some(storage) = self.text_storage() else { return };
        if !(storage.length() > 0) {
            return;
        }
        let definition_elisions = self.definition_elisions();
        let elision = self.ivars().elision.borrow().clone();
        let identity = elision.is_identity() && definition_elisions.is_empty();
        let was_identity = self.ivars().elision_was_identity.get();
        self.ivars().elision_was_identity.set(identity);
        if identity && was_identity {
            return;
        }
        let whole = NSRange::new(0, storage.length() as isize);
        let flag = NSNumber::new_bool(true);
        storage.beginEditing();
        storage.removeAttribute_range(attribute_keys::dr_elided(), ns(whole));
        let mut ranges = elision.elided_ranges.clone();
        ranges.extend(definition_elisions);
        for range in RangeSet::normalized(&ranges) {
            if !(range.upper_bound() <= storage.length() as isize && range.length > 0) {
                continue;
            }
            unsafe { storage.addAttribute_value_range(attribute_keys::dr_elided(), &flag, ns(range)) };
        }
        storage.endEditing();
    }

    fn unfold_headings_containing(&self, hits: &[NSRange]) {
        let folded = self.folded_heading_slugs();
        if folded.is_empty() || hits.is_empty() {
            return;
        }
        let mut unfolded = folded.clone();
        for heading in &self.parsed_document().headings {
            if !folded.contains(&heading.slug) {
                continue;
            }
            let body = ElisionPlan::body_range(heading);
            if hits.iter().any(|hit| hit.location < body.upper_bound() && body.location < hit.upper_bound()) {
                unfolded.remove(&heading.slug);
            }
        }
        if unfolded != folded {
            self.set_folded_heading_slugs(unfolded);
        }
    }

    pub fn expand_elision(&self, offset: isize) {
        let Some(range) = self.ivars().elision.borrow().range_containing(offset) else { return };
        self.ivars().expanded_elision_ranges.borrow_mut().push(range);
        self.refresh_elision(true);
    }

    // MARK: - Code block collapse (§5.1)

    pub fn set_code_block_collapsed(&self, collapsed: bool, offset: isize) {
        self.ivars().code_collapse_overrides.borrow_mut().insert(offset, collapsed);
        *self.ivars().fragment_context.collapse_overrides.borrow_mut() =
            self.ivars().code_collapse_overrides.borrow().clone();
        self.invalidate_all_fragments();
    }

    /// Collapse state for one block, without walking the document.
    pub fn is_collapsed(&self, payload: &FragmentPayload) -> bool {
        self.ivars()
            .code_collapse_overrides
            .borrow()
            .get(&payload.source_range().location)
            .copied()
            .unwrap_or(payload.is_collapsed())
    }

    // MARK: - Overlays: search, changes, paths

    fn reapply_overlays(&self, ranges: &[NSRange], invalidate_fragments: bool) {
        let Some(storage) = self.text_storage() else { return };
        if ranges.is_empty() && self.ivars().overlay_ranges.borrow().is_empty() {
            return;
        }
        let blocks =
            RangeSet::normalized(&ranges.iter().filter_map(|range| self.clamp_to_storage(*range)).collect::<Vec<_>>());
        if !blocks.is_empty() {
            let dirty = DirtySet::new(blocks, false);
            let document = self.parsed_document();
            let rewritten = self.ivars().engine.borrow().decorated_bounds(&dirty, &document, storage.length() as isize);
            self.ivars().engine.borrow_mut().decorate(&storage, &document, &dirty);
            self.rebuild_display_map(false, &rewritten);
        }
        self.apply_overlays(None);
        self.apply_path_existence(None);
        self.refresh_base_layout_map();
        if invalidate_fragments {
            self.invalidate_all_fragments();
        }
    }

    fn apply_overlays(&self, scopes: Option<&[NSRange]>) {
        let Some(storage) = self.text_storage() else { return };
        if !(storage.length() > 0) {
            return;
        }
        let ivars = self.ivars();
        let search_hits = ivars.search_hits.borrow().clone();
        let current_search_hit = ivars.current_search_hit.get();
        let change_marks = ivars.change_marks.borrow().clone();
        let speech_highlight = ivars.speech_highlight.get();
        if search_hits.is_empty() && current_search_hit.is_none() && change_marks.is_empty() && speech_highlight.is_none() {
            ivars.overlay_ranges.borrow_mut().clear();
            ivars.object_change_marks.borrow_mut().clear();
            return;
        }
        let needs_application = |range: NSRange| -> bool {
            match scopes {
                None => true,
                Some(scopes) => {
                    scopes.iter().any(|scope| upleft_core::ns_range::ns_intersection_range(*scope, range).length > 0)
                }
            }
        };
        let style_sheet = self.style_sheet();
        let flag = NSNumber::new_bool(true);
        storage.beginEditing();
        for hit in &search_hits {
            let Some(range) = self.clamp_to_storage(*hit).filter(|range| needs_application(*range)) else { continue };
            unsafe { storage.addAttribute_value_range(attribute_keys::dr_search_hit(), &flag, ns(range)) };
            self.tint(&style_sheet.search_hit, range, &storage);
            self.set_readable_foreground(range, &style_sheet.search_hit, &storage);
        }
        if let Some(current) = current_search_hit
            && let Some(range) = self.clamp_to_storage(current)
            && needs_application(range)
        {
            unsafe { storage.addAttribute_value_range(attribute_keys::dr_current_search_hit(), &flag, ns(range)) };
            self.tint(&style_sheet.search_hit_current, range, &storage);
            self.set_readable_foreground(range, &style_sheet.search_hit_current, &storage);
        }
        if let Some(spoken) = speech_highlight
            && let Some(range) = self.clamp_to_storage(spoken)
            && needs_application(range)
        {
            unsafe { storage.addAttribute_value_range(attribute_keys::dr_speech_highlight(), &flag, ns(range)) };
            self.tint(&style_sheet.search_hit_current, range, &storage);
            self.set_readable_foreground(range, &style_sheet.search_hit_current, &storage);
        }
        let mut object_change_marks: Vec<ChangeMark> = Vec::new();
        for mark in &change_marks {
            let Some(range) = self.clamp_to_storage(mark.range) else { continue };
            if !needs_application(range) {
                if mark.kind != ChangeKind::Deleted && self.glyph_bearing_ranges(range, &storage).is_empty() {
                    object_change_marks.push(mark.clone());
                }
                continue;
            }
            unsafe {
                storage.addAttribute_value_range(
                    attribute_keys::dr_change(),
                    &NSString::from_str(mark.kind.raw_value()),
                    ns(range),
                )
            };
            if mark.kind == ChangeKind::Deleted {
                unsafe {
                    storage.addAttribute_value_range(
                        attribute_keys::dr_change_ghost(),
                        &NSString::from_str(&mark.deleted_text),
                        ns(range),
                    )
                };
                continue;
            }
            let alpha: CGFloat = if mark.visited { 0.045 } else { 0.18 };
            let colour = style_sheet.change_color(mark.kind).colorWithAlphaComponent(alpha);
            let mut painted_anything = false;
            for word in &mark.words {
                let Some(word_range) = self.clamp_to_storage(*word) else { continue };
                painted_anything = self.tint(&colour, word_range, &storage) || painted_anything;
                if !style_sheet.increase_contrast {
                    continue;
                }
                let underline: isize = if mark.kind == ChangeKind::Inserted {
                    NSUnderlineStyle::Single.0
                } else {
                    NSUnderlineStyle::Single.0 | NSUnderlineStyle::PatternDash.0
                };
                let underline = NSNumber::new_isize(underline);
                let color = style_sheet.change_color(mark.kind);
                let attributes = NSDictionary::from_slices(
                    &[keys::underline_style(), keys::underline_color()],
                    &[underline.as_ref() as &AnyObject, color.as_ref()],
                );
                unsafe { storage.addAttributes_range(&attributes, ns(word_range)) };
            }
            if !painted_anything {
                object_change_marks.push(mark.clone());
            }
        }
        *ivars.object_change_marks.borrow_mut() = object_change_marks;
        storage.endEditing();
        let mut overlay = search_hits;
        overlay.extend(change_marks.iter().map(|mark| mark.range));
        overlay.extend(speech_highlight);
        *ivars.overlay_ranges.borrow_mut() = overlay;
    }

    /// Paints `colour` behind the glyph-bearing parts of `range`.
    fn tint(&self, colour: &NSColor, range: NSRange, storage: &NSTextStorage) -> bool {
        let mut painted = false;
        for visible in self.glyph_bearing_ranges(range, storage) {
            unsafe { storage.addAttribute_value_range(keys::background_color(), colour, ns(visible)) };
            painted = true;
        }
        painted
    }

    fn set_readable_foreground(&self, range: NSRange, background: &NSColor, storage: &NSTextStorage) {
        let Some(rgb) = background.colorUsingColorSpace(&objc2_app_kit::NSColorSpace::sRGBColorSpace()) else { return };
        let luminance = 0.2126 * rgb.redComponent() + 0.7152 * rgb.greenComponent() + 0.0722 * rgb.blueComponent();
        let foreground = if luminance > 0.55 { NSColor::blackColor() } else { NSColor::whiteColor() };
        for visible in self.glyph_bearing_ranges(range, storage) {
            unsafe { storage.addAttribute_value_range(keys::foreground_color(), &foreground, ns(visible)) };
        }
    }

    /// `range` minus every run that is hidden syntax or drawn by an object
    /// fragment.
    fn glyph_bearing_ranges(&self, range: NSRange, storage: &NSTextStorage) -> Vec<NSRange> {
        let mut excluded: Vec<NSRange> = RangeSet::intersecting(&self.ivars().base_hidden_ranges.borrow(), range);
        enumerate_attribute(storage, attribute_keys::dr_hidden(), ns(range), false, |value, subrange| {
            if value.is_some() {
                excluded.push(from_ns(subrange));
            }
            true
        });
        enumerate_attribute(storage, attribute_keys::dr_fragment(), ns(range), false, |value, subrange| {
            if let Some(payload) = value.and_then(|value| value.downcast_ref::<FragmentPayload>())
                && payload.kind().replaces_glyphs()
            {
                excluded.push(from_ns(subrange));
            }
            true
        });
        if excluded.is_empty() {
            return vec![range];
        }
        let mut out = Vec::new();
        let mut cursor = range.location;
        for gap in RangeSet::normalized(&excluded) {
            if gap.location > cursor {
                out.push(NSRange::new(cursor, gap.location - cursor));
            }
            cursor = cursor.max(gap.upper_bound());
        }
        if cursor < range.upper_bound() {
            out.push(NSRange::new(cursor, range.upper_bound() - cursor));
        }
        out
    }

    /// Deleted text gets a short wedge in the margin at the join point.
    fn draw_deleted_change_marks(&self, dirty_rect: NSRect) {
        let deletions: Vec<ChangeMark> =
            self.change_marks().into_iter().filter(|mark| mark.kind == ChangeKind::Deleted).collect();
        if deletions.is_empty() {
            return;
        }
        let Some(context) = NSGraphicsContext::currentContext() else { return };
        let cg = context.CGContext();
        let style_sheet = self.style_sheet();
        let colour = style_sheet.change_color(ChangeKind::Deleted);
        let width: CGFloat = 3.0;
        for mark in deletions {
            let Some(rect) = self.rect_for_offset(mark.range.location).filter(|rect| rect.intersects(dirty_rect)) else {
                continue;
            };
            let wedge = crate::appkit_compat::rect(
                smax(0.0, rect.min_x() - width - 2.0),
                rect.min_y(),
                width,
                smax(4.0, smin(rect.height(), style_sheet.line_height)),
            );
            let fill = colour.colorWithAlphaComponent(if mark.visited { 0.35 } else { 0.9 });
            CGContext::set_fill_color_with_color(Some(&cg), Some(&fill.CGColor()));
            CGContext::fill_rect(Some(&cg), wedge);
        }
    }

    /// A rule beside object fragments whose change had no glyphs to tint.
    fn draw_object_change_marks(&self, dirty_rect: NSRect) {
        let marks = self.ivars().object_change_marks.borrow().clone();
        if marks.is_empty() {
            return;
        }
        let Some(context) = NSGraphicsContext::currentContext() else { return };
        let cg = context.CGContext();
        let style_sheet = self.style_sheet();
        let width: CGFloat = 2.0;
        for mark in marks {
            let (Some(start), Some(end)) = (
                self.rect_for_offset(mark.range.location),
                self.rect_for_offset(mark.range.location.max(mark.range.upper_bound() - 1)),
            ) else {
                continue;
            };
            let band = crate::appkit_compat::rect(
                smax(0.0, start.min_x() - width - 6.0),
                start.min_y(),
                width,
                smax(style_sheet.line_height, end.max_y() - start.min_y()),
            );
            if !band.intersects(dirty_rect) {
                continue;
            }
            let fill = style_sheet.change_color(mark.kind).colorWithAlphaComponent(if mark.visited { 0.3 } else { 0.75 });
            CGContext::set_fill_color_with_color(Some(&cg), Some(&fill.CGColor()));
            CGContext::fill_rect(Some(&cg), band);
        }
    }

    /// Re-runs path-existence styling after the resolver has warmed its cache.
    pub fn refresh_path_existence(&self, completion: Option<Box<dyn FnOnce()>>) {
        self.invalidate_path_existence_cache();
        let refresh_generation = self.ivars().path_refresh_generation.get();
        let document_generation = self.update_generation();
        let tokens = self.parsed_document().path_tokens.clone();
        if tokens.is_empty() {
            if let Some(completion) = completion {
                completion();
            }
            return;
        }
        self.refresh_path_existence_batch(Rc::new(tokens), 0, document_generation, refresh_generation, completion);
    }

    /// Split panes share attributed storage but keep independent lookup caches.
    pub fn invalidate_path_existence_cache(&self) {
        let generation = self.ivars().path_refresh_generation.get().wrapping_add(1);
        self.ivars().path_refresh_generation.set(generation);
        self.ivars().path_existence.borrow_mut().clear();
    }

    fn refresh_path_existence_batch(
        &self,
        tokens: Rc<Vec<upleft_core::ResolvableToken>>,
        start: usize,
        document_generation: isize,
        refresh_generation: u64,
        completion: Option<Box<dyn FnOnce()>>,
    ) {
        if self.update_generation() != document_generation
            || self.ivars().path_refresh_generation.get() != refresh_generation
            || start >= tokens.len()
        {
            return;
        }
        let Some(storage) = self.text_storage() else { return };
        let end = (start + 128).min(tokens.len());
        storage.beginEditing();
        for resolvable in &tokens[start..end] {
            let Some(range) = self.clamp_to_storage(resolvable.range) else { continue };
            let exists = self.delegate().is_none_or(|delegate| delegate.path_exists_for(self, &resolvable.token));
            self.ivars().path_existence.borrow_mut().insert(resolvable.token.clone(), exists);
            self.style_path_token(&storage, range, exists);
        }
        storage.endEditing();
        self.setNeedsDisplay(true);

        if end >= tokens.len() {
            if let Some(completion) = completion {
                completion();
            }
            return;
        }
        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        main_async(move || {
            if let Some(view) = weak.load() {
                view.refresh_path_existence_batch(tokens, end, document_generation, refresh_generation, completion);
            }
        });
    }

    fn style_path_token(&self, storage: &NSTextStorage, range: NSRange, exists: bool) {
        let style_sheet = self.style_sheet();
        unsafe {
            storage.addAttribute_value_range(attribute_keys::dr_path_exists(), &NSNumber::new_bool(exists), ns(range));
        }
        if exists {
            unsafe { storage.addAttribute_value_range(keys::foreground_color(), &style_sheet.text_secondary, ns(range)) };
            storage.removeAttribute_range(keys::underline_style(), ns(range));
            storage.removeAttribute_range(keys::underline_color(), ns(range));
        } else {
            let underline = NSNumber::new_isize(NSUnderlineStyle::PatternDot.0 | NSUnderlineStyle::Single.0);
            let attributes = NSDictionary::from_slices(
                &[keys::foreground_color(), keys::underline_style(), keys::underline_color()],
                &[style_sheet.text_secondary.as_ref() as &AnyObject, underline.as_ref(), style_sheet.text_faint.as_ref()],
            );
            unsafe { storage.addAttributes_range(&attributes, ns(range)) };
        }
    }

    /// §8.4's trust instrument.
    fn apply_path_existence(&self, scopes: Option<&[NSRange]>) {
        let Some(storage) = self.text_storage() else { return };
        let document = self.parsed_document();
        if document.path_tokens.is_empty() {
            return;
        }
        storage.beginEditing();
        for resolvable in &document.path_tokens {
            let Some(range) = self.clamp_to_storage(resolvable.range) else { continue };
            if let Some(scopes) = scopes
                && !scopes.iter().any(|scope| upleft_core::ns_range::ns_intersection_range(*scope, range).length > 0)
            {
                continue;
            }
            let cached = self.ivars().path_existence.borrow().get(&resolvable.token).copied();
            let exists = match cached {
                Some(cached) => cached,
                None => {
                    let exists = self.delegate().is_none_or(|delegate| delegate.path_exists_for(self, &resolvable.token));
                    self.ivars().path_existence.borrow_mut().insert(resolvable.token.clone(), exists);
                    exists
                }
            };
            self.style_path_token(&storage, range, exists);
        }
        storage.endEditing();
    }

    pub(crate) fn clamp_to_storage(&self, range: NSRange) -> Option<NSRange> {
        let storage = self.text_storage()?;
        let lo = range.location.max(0);
        let hi = range.upper_bound().min(storage.length() as isize);
        if hi > lo { Some(NSRange::new(lo, hi - lo)) } else { None }
    }

    // MARK: - Selection in source coordinates

    /// Selection converted out of TextKit's hybrid space.
    pub fn source_selected_ranges(&self) -> Vec<NSRange> {
        let Some(storage) = self.text_storage() else { return Vec::new() };
        let full_source = NSRange::new(0, storage.length() as isize);
        let display_map = self.current_display_map();
        let full_text_kit = display_map.text_kit_range_for_source(full_source);
        self.selectedRanges()
            .iter()
            .map(|value| {
                let text_kit_range = from_ns(unsafe { value.rangeValue() });
                if text_kit_range.location <= full_text_kit.location
                    && text_kit_range.upper_bound() >= full_text_kit.upper_bound()
                {
                    return full_source;
                }
                display_map.source_range_for_text_kit(text_kit_range)
            })
            .collect()
    }

    pub fn source_selected_range(&self) -> NSRange {
        self.source_selected_ranges().first().copied().unwrap_or(NSRange::new(0, 0))
    }

    /// Primary caret, or `None` when there is no caret or the selection is
    /// not empty.
    pub fn primary_source_caret(&self) -> Option<isize> {
        if !self.effective_policy().shows_insertion_point {
            return None;
        }
        let first = unsafe { self.selectedRanges().firstObject()?.rangeValue() };
        if first.length != 0 {
            return None;
        }
        Some(self.current_display_map().source_offset_for_text_kit(first.location as isize))
    }

    pub fn set_source_selected_ranges(&self, ranges: &[NSRange]) {
        let display_map = self.current_display_map();
        let converted: Vec<Retained<NSValue>> = ranges
            .iter()
            .map(|range| unsafe_value_with_range(ns(display_map.text_kit_range_for_source(*range))))
            .collect();
        if converted.is_empty() {
            return;
        }
        let array = NSArray::from_retained_slice(&converted);
        self.ivars().is_applying_selection.set(true);
        self.super_set_selected_ranges(&array);
        self.ivars().is_applying_selection.set(false);
    }

    /// `super.setSelectedRanges(_:affinity: .downstream, stillSelecting: false)`.
    fn super_set_selected_ranges(&self, ranges: &NSArray<NSValue>) {
        let _: () = unsafe {
            msg_send![super(self), setSelectedRanges: ranges, affinity: NSSelectionAffinity::Downstream, stillSelecting: false]
        };
    }

    fn key_down(&self, event: &NSEvent) {
        self.interrupt_animated_scroll();
        let modifiers = event.modifierFlags() & objc2_app_kit::NSEventModifierFlags::DeviceIndependentFlagsMask;
        if self.isEditable()
            && modifiers == objc2_app_kit::NSEventModifierFlags::Command
            && event
                .charactersIgnoringModifiers()
                .is_some_and(|characters| characters.lowercaseString().to_string() == "a")
        {
            let _: () = unsafe { msg_send![self, selectAll: None::<&AnyObject>] };
            return;
        }
        if let Some(handler) = self.key_event_handler()
            && handler(event)
        {
            return;
        }
        if self.handle_quick_look_space(event) {
            return;
        }
        let _: () = unsafe { msg_send![super(self), keyDown: event] };
    }

    /// Keep AppKit's own mouse tracking on the same TextKit 2 hit test as the
    /// editor's hover, link, and source-offset paths.
    fn character_index_for_insertion(&self, point: NSPoint) -> usize {
        let super_index = || -> usize { unsafe { msg_send![super(self), characterIndexForInsertionAtPoint: point] } };
        let Some(text_container) = (unsafe { self.textContainer() }) else { return super_index() };
        let origin = self.textContainerOrigin();
        let width = smax(1.0, text_container.size().width);
        let local_point = CGPoint::new(smin(smax(point.x - origin.x, 0.0), width), smax(0.0, point.y - origin.y));

        let line_height = smax(1.0, self.style_sheet().line_height);
        let layout_manager = &self.ivars().markdown_layout_manager;
        layout_manager.ensureLayoutForBounds(rect(
            0.0,
            smax(0.0, local_point.y - line_height * 2.0),
            width,
            line_height * 4.0,
        ));

        let document_location = self.ivars().content_storage.documentRange().location();
        let bounds = rect(0.0, 0.0, width, smax(self.frame().size.height, local_point.y + line_height));
        let selections = layout_manager.textSelectionNavigation().textSelectionsInteractingAtPoint_inContainerAtLocation_anchors_modifiers_selecting_bounds(
            local_point,
            &document_location,
            &NSArray::new(),
            objc2_app_kit::NSTextSelectionNavigationModifier(0),
            false,
            bounds,
        );
        let Some(range) = selections.firstObject().and_then(|selection| selection.textRanges().firstObject()) else {
            return super_index();
        };
        use objc2_app_kit::NSTextSelectionDataSource;
        layout_manager.offsetFromLocation_toLocation(&document_location, &range.location()) as usize
    }

    /// The caret moved, so the reveal set may have.
    /// `handleSelectionChanged(allowTypewriterScrolling:)` with no explicit
    /// viewport anchor, for tests that model a click.
    pub fn handle_selection_changed_for_testing(&self, allow_typewriter_scrolling: bool) {
        self.handle_selection_changed(allow_typewriter_scrolling, None);
    }

    pub(crate) fn handle_selection_changed(&self, allow_typewriter_scrolling: bool, requested_viewport_anchor: Option<ViewportAnchor>) {
        let ivars = self.ivars();
        if requested_viewport_anchor.is_none() {
            ivars.local_edit_viewport_anchor.set(None);
        }
        ivars.pending_resize_anchor.set(None);
        let source_selection = self.source_selected_ranges();
        let viewport_anchor = requested_viewport_anchor.unwrap_or_else(|| {
            source_selection
                .first()
                .and_then(|selection| {
                    if selection.length != 0 {
                        return None;
                    }
                    Some(self.capture_viewport_anchor_at(selection.location))
                })
                .unwrap_or_else(|| self.capture_viewport_anchor())
        });
        ivars
            .fragment_context
            .caret
            .set(if ivars.suppresses_caret_reveal.get() { None } else { self.primary_source_caret() });
        let previous_anchor = ivars.anchored_paragraph.get();

        self.rebuild_display_map(false, &[]);
        self.restore_anchor_shift(previous_anchor);
        self.apply_caret_anchor_shift();

        let display_map = self.current_display_map();
        let restored: Vec<Retained<NSValue>> = source_selection
            .iter()
            .map(|range| unsafe_value_with_range(ns(display_map.text_kit_range_for_source(*range))))
            .collect();
        let current = self.selectedRanges();
        let same = restored.len() == current.count()
            && restored.iter().zip(current.iter()).all(|(a, b)| a.isEqualToValue(&b));
        if !same {
            let array = NSArray::from_retained_slice(&restored);
            ivars.is_applying_selection.set(true);
            self.super_set_selected_ranges(&array);
            ivars.is_applying_selection.set(false);
        }
        self.restore_viewport_to(viewport_anchor);
        self.gutter_rail_needs_display();
        if let Some(delegate) = self.delegate() {
            delegate.did_change_selection(self);
        }
        if allow_typewriter_scrolling
            && !ivars.is_tracking_mouse_selection.get()
            && ivars.configuration.borrow().typewriter_scrolling
            && let Some(caret) = self.primary_source_caret()
            && self.source_selected_range().length == 0
        {
            self.scroll_to_offset(caret, ScrollPosition::Center, true);
        }
    }

    // MARK: - Caret-anchored reveal (§6.1c)

    fn apply_caret_anchor_shift(&self) {
        let ivars = self.ivars();
        if ivars.suppresses_caret_reveal.get() || !self.effective_policy().reveals_at_caret {
            return;
        }
        let Some(caret) = self.primary_source_caret() else { return };
        let Some(storage) = self.text_storage() else { return };
        if !(storage.length() > 0) {
            return;
        }
        let paragraph = self.paragraph_range_containing(caret);
        let revealed: Vec<NSRange> =
            MarkerPolicy::revealed_marker_ranges(&self.parsed_document(), self.effective_policy(), Some(caret), &[])
                .into_iter()
                .filter(|range| range.upper_bound() <= caret && range.location >= paragraph.location)
                .collect();
        if revealed.is_empty() {
            return;
        }
        let mut shift: CGFloat = 0.0;
        for range in revealed {
            let Some(clamped) = self.clamp_to_storage(range) else { continue };
            shift += storage.attributedSubstringFromRange(ns(clamped)).size().width;
        }
        if !(shift > 0.5) {
            return;
        }
        shift = smin(shift, render_metrics::REVEAL_SLACK);

        let Some(base) = attribute_value(&storage, keys::paragraph_style(), paragraph.location as usize)
            .and_then(|value| value.downcast::<NSParagraphStyle>().ok())
        else {
            return;
        };
        let adjusted: Retained<NSMutableParagraphStyle> = unsafe { msg_send![&*base, mutableCopy] };
        adjusted.setFirstLineHeadIndent(base.firstLineHeadIndent() - shift);
        adjusted.setHeadIndent(base.headIndent() - shift);
        storage.beginEditing();
        unsafe { storage.addAttribute_value_range(keys::paragraph_style(), &adjusted, ns(paragraph)) };
        storage.endEditing();
        ivars.anchored_paragraph.set(Some(paragraph));
    }

    fn restore_anchor_shift(&self, previous: Option<NSRange>) {
        self.ivars().anchored_paragraph.set(None);
        let Some(previous) = previous else { return };
        let Some(storage) = self.text_storage() else { return };
        let Some(range) = self.clamp_to_storage(previous) else { return };
        let document = self.parsed_document();
        self.ivars().engine.borrow_mut().decorate(&storage, &document, &DirtySet::new(vec![range], false));
        let hidden = self.current_display_map().hidden_ranges_in_paragraph_containing(range.location);
        self.apply_hidden_attribute(&hidden, Some(range), &[]);
        self.apply_overlays(None);
    }

    // MARK: - Geometry

    /// `rect(forOffset:)`: the first text segment at a source offset, in view
    /// coordinates.
    pub fn rect_for_offset(&self, offset: isize) -> Option<NSRect> {
        let text_kit = self.current_display_map().text_kit_offset_for_source(offset);
        let storage = &self.ivars().content_storage;
        let location = storage.locationFromLocation_withOffset(&storage.documentRange().location(), text_kit)?;
        let range = NSTextRange::initWithLocation(NSTextRange::alloc(), &location);
        let layout_manager = &self.ivars().markdown_layout_manager;
        layout_manager.ensureLayoutForRange(&range);
        let found: Cell<Option<CGRect>> = Cell::new(None);
        let block = block2::StackBlock::new(
            |_range: *mut NSTextRange, frame: CGRect, _baseline: CGFloat, _container: NonNull<NSTextContainer>| -> Bool {
                found.set(Some(frame));
                Bool::NO
            },
        );
        layout_manager.enumerateTextSegmentsInRange_type_options_usingBlock(
            &range,
            objc2_app_kit::NSTextLayoutManagerSegmentType::Standard,
            objc2_app_kit::NSTextLayoutManagerSegmentOptions(0),
            &block,
        );
        let mut rect = found.get()?;
        let origin = self.textContainerOrigin();
        rect.origin.x += origin.x;
        rect.origin.y += origin.y;
        Some(rect)
    }

    /// Whether a point is inside the laid-out glyph segments for a source
    /// range.
    pub fn rendered_text_contains(&self, point: NSPoint, source_range: NSRange) -> bool {
        if !(source_range.length > 0) {
            return false;
        }
        let Some(clamped) = self.clamp_to_storage(source_range) else { return false };
        let text_kit_range = self.current_display_map().text_kit_range_for_source(clamped);
        if !(text_kit_range.length > 0) {
            return false;
        }
        let storage = &self.ivars().content_storage;
        let origin = storage.documentRange().location();
        let (Some(start), Some(end)) = (
            storage.locationFromLocation_withOffset(&origin, text_kit_range.location),
            storage.locationFromLocation_withOffset(&origin, text_kit_range.upper_bound()),
        ) else {
            return false;
        };
        let Some(range) = NSTextRange::initWithLocation_endLocation(NSTextRange::alloc(), &start, Some(&end)) else {
            return false;
        };
        let layout_manager = &self.ivars().markdown_layout_manager;
        layout_manager.ensureLayoutForRange(&range);
        let container_origin = self.textContainerOrigin();
        let point_in_container = NSPoint::new(point.x - container_origin.x, point.y - container_origin.y);
        let Some(fragment) = layout_manager.textLayoutFragmentForPosition(point_in_container) else { return false };
        let fragment_frame = fragment.layoutFragmentFrame();
        for line in fragment.textLineFragments().iter() {
            let character_range = from_ns(line.characterRange());
            let intersection = upleft_core::ns_range::ns_intersection_range(character_range, text_kit_range);
            if !(intersection.length > 0) {
                continue;
            }
            let line_bounds = line.typographicBounds();
            let line_origin = CGPoint::new(fragment_frame.min_x() + line_bounds.min_x(), fragment_frame.min_y() + line_bounds.min_y());
            let start_index = intersection.location - character_range.location;
            let end_index = intersection.upper_bound() - character_range.location;
            let start_point = line.locationForCharacterAtIndex(start_index);
            let end_point = line.locationForCharacterAtIndex(end_index);
            let min_x = line_origin.x + smin(start_point.x, end_point.x);
            let max_x = line_origin.x + smax(start_point.x, end_point.x);
            let min_y = line_origin.y;
            let max_y = line_origin.y + line_bounds.height();
            if point_in_container.x >= min_x
                && point_in_container.x <= max_x
                && point_in_container.y >= min_y
                && point_in_container.y <= max_y
            {
                return true;
            }
        }
        false
    }

    pub fn top_visible_offset(&self) -> isize {
        let visible = self.scroll_view().map_or_else(|| self.visibleRect(), |scroll| scroll.documentVisibleRect());
        let origin = self.textContainerOrigin();
        let sample_y = smax(0.0, visible.min_y() - origin.y) + 1.0;
        let viewport = rect(0.0, smax(0.0, sample_y - 1.0), smax(1.0, visible.width()), smax(1.0, visible.height()));
        let layout_manager = &self.ivars().markdown_layout_manager;
        layout_manager.ensureLayoutForBounds(viewport);
        if let Some(fragment) = layout_manager.textLayoutFragmentForPosition(NSPoint::new(1.0, sample_y)) {
            use objc2_app_kit::NSTextSelectionDataSource;
            let text_kit_offset = layout_manager
                .offsetFromLocation_toLocation(&layout_manager.documentRange().location(), &fragment.rangeInElement().location());
            return self.current_display_map().source_offset_for_text_kit(text_kit_offset);
        }
        let sample_view_y = smax(visible.min_y(), origin.y) + 1.0;
        self.source_offset_at(NSPoint::new(origin.x + 1.0, sample_view_y))
    }

    /// One-based source position for status UI.
    pub fn source_position(&self, offset: isize) -> (isize, isize) {
        let index = self.paragraph_index();
        let clamped = offset.max(0).min(index.length);
        let paragraph = index.index_containing(clamped);
        (paragraph as isize + 1, clamped - index.starts[paragraph] + 1)
    }

    // MARK: - Motion

    /// DESIGN.md: "User scroll must interrupt animated scrolling."
    pub fn interrupt_animated_scroll(&self) {
        let ivars = self.ivars();
        if !ivars.scroll_spring_is_active.get() {
            return;
        }
        ivars.scroll_spring_is_active.set(false);
        *ivars.scroll_spring_clip.borrow_mut() = ObjcWeak::default();
        ivars.pending_scroll_y.set(None);
        if let Some(scroll) = self.scroll_view() {
            let mut spring = ivars.scroll_spring.get();
            spring.snap(scroll.contentView().bounds().origin.y);
            ivars.scroll_spring.set(spring);
        }
    }

    /// Whether this view is somewhere a display link will actually fire.
    pub fn can_drive_motion(&self) -> bool {
        let Some(window) = self.window() else { return false };
        window.isVisible() && window.screen().is_some()
    }

    pub(crate) fn arm_motion_driver(&self) {
        if let Some(driver) = self.ivars().motion_driver.borrow().as_ref() {
            driver.arm();
            return;
        }
        let weak_tick: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        let weak_apply = weak_tick.clone();
        let driver = SpringDriver::new(
            self,
            move |dt| weak_tick.load().is_some_and(|view| view.document_motion_tick(dt)),
            move || {
                if let Some(view) = weak_apply.load() {
                    view.document_motion_apply();
                }
            },
        );
        *self.ivars().motion_driver.borrow_mut() = Some(driver.clone());
        driver.arm();
    }

    pub fn park_motion_driver(&self) {
        if let Some(driver) = self.ivars().motion_driver.borrow().as_ref() {
            driver.park();
        }
        self.ivars().scroll_spring_is_active.set(false);
        *self.ivars().scroll_spring_clip.borrow_mut() = ObjcWeak::default();
        self.ivars().pending_scroll_y.set(None);
    }

    pub fn document_motion_tick(&self, dt: CGFloat) -> bool {
        let ivars = self.ivars();
        let mut moving = false;
        let now = cf_absolute_time_get_current();
        let live: Vec<NSRange> = ivars
            .fragment_context
            .checkbox_pulses
            .borrow()
            .iter()
            .filter(|pulse| now - pulse.started < CheckboxPulse::DURATION)
            .map(|pulse| pulse.source_range)
            .collect();
        if !live.is_empty() {
            ivars.pending_motion_invalidation.set(Some(self.pulse_invalidation_rect(&live)));
            moving = true;
        }
        if ivars.scroll_spring_is_active.get() {
            let clip = ivars.scroll_spring_clip.borrow().load().or_else(|| self.scroll_view().map(|scroll| scroll.contentView()));
            let Some(clip) = clip else {
                ivars.scroll_spring_is_active.set(false);
                return moving;
            };
            let mut spring = ivars.scroll_spring.get();
            let mut alive = spring.advance(dt);
            let document_height = clip.documentView().map_or(0.0, |view| view.frame().size.height);
            let max_y = smax(0.0, document_height - clip.bounds().size.height);
            let mut y = spring.value();
            if y < 0.0 || y > max_y {
                y = smin(max_y, smax(0.0, y));
                spring.snap(y);
                alive = false;
            }
            ivars.scroll_spring.set(spring);
            ivars.pending_scroll_y.set(Some(y));
            ivars.scroll_spring_is_active.set(alive);
            moving = moving || alive;
        }
        moving
    }

    pub fn document_motion_apply(&self) {
        let ivars = self.ivars();
        if let Some(y) = ivars.pending_scroll_y.take() {
            let clip = ivars.scroll_spring_clip.borrow().load().or_else(|| self.scroll_view().map(|scroll| scroll.contentView()));
            if let Some(clip) = clip {
                clip.scrollToPoint(NSPoint::new(clip.bounds().origin.x, y));
                if let Some(scroll) = self.scroll_view() {
                    scroll.reflectScrolledClipView(&clip);
                }
                self.synchronize_visible_layout();
            }
        }
        if let Some(rect) = ivars.pending_motion_invalidation.take() {
            self.setNeedsDisplayInRect(rect);
        }
    }

    fn magnify(&self, event: &NSEvent) {
        self.interrupt_animated_scroll();
        let ivars = self.ivars();
        if event.phase() == NSEventPhase::Began {
            ivars.text_magnification_accumulator.set(0.0);
        }
        ivars
            .text_magnification_accumulator
            .set(ivars.text_magnification_accumulator.get() + event.magnification());
        let step_threshold: CGFloat = 0.075;
        let steps = (ivars.text_magnification_accumulator.get() / step_threshold) as isize;
        if steps != 0 {
            if let Some(delegate) = self.delegate() {
                delegate.did_request_text_size_steps(self, steps);
            }
            ivars
                .text_magnification_accumulator
                .set(ivars.text_magnification_accumulator.get() - steps as CGFloat * step_threshold);
        }
        if event.phase() == NSEventPhase::Ended || event.phase() == NSEventPhase::Cancelled {
            ivars.text_magnification_accumulator.set(0.0);
        }
    }

    /// `scroll(toOffset:position:animated:)`.
    pub fn scroll_to_offset(&self, offset: isize, position: ScrollPosition, animated: bool) {
        if self.ivars().elision.borrow().is_elided(offset) {
            self.unfold_headings_containing(&[NSRange::new(offset, 1)]);
        }
        let Some(rect) = self.rect_for_offset(offset) else { return };
        let Some(scroll_view) = self.scroll_view() else {
            self.scrollRectToVisible(rect);
            return;
        };
        let clip = scroll_view.contentView();
        let clip_bounds = clip.bounds();
        let height = clip_bounds.size.height;

        let mut y = match position {
            ScrollPosition::Top => rect.min_y() - render_metrics::VERTICAL_INSET,
            ScrollPosition::Center => rect.mid_y() - height / 2.0,
            ScrollPosition::Visible => {
                if clip_bounds.intersects(rect) {
                    return;
                }
                rect.min_y() - height / 3.0
            }
        };
        y = smax(0.0, smin(y, smax(0.0, self.frame().size.height - height)));
        let target = NSPoint::new(clip_bounds.origin.x, y);
        let distance = (y - clip_bounds.origin.y).abs();

        let ivars = self.ivars();
        if animated && !self.style_sheet().reduce_motion && distance > 0.5 && self.can_drive_motion() {
            let mut spring = ivars.scroll_spring.get();
            if !ivars.scroll_spring_is_active.get() {
                spring.snap(clip_bounds.origin.y);
            }
            spring.retune(motion::scroll_duration(distance), None);
            spring.target(y);
            ivars.scroll_spring.set(spring);
            *ivars.scroll_spring_clip.borrow_mut() = ObjcWeak::from(&*clip);
            ivars.scroll_spring_is_active.set(true);
            self.arm_motion_driver();
        } else {
            self.interrupt_animated_scroll();
            clip.scrollToPoint(target);
            scroll_view.reflectScrolledClipView(&clip);
            self.synchronize_visible_layout();
        }
    }

    // MARK: - Layout plumbing

    /// Underlines the link under the pointer (§7.1).
    fn draw_hovered_link_underline(&self, _dirty_rect: NSRect) {
        if self.text_storage().is_none() {
            return;
        }
        let Some(range) = self.ivars().hovered_link_range.get() else { return };
        let Some(clamped) = self.clamp_to_storage(range).filter(|range| range.length > 0) else { return };
        let text_kit_range = self.current_display_map().text_kit_range_for_source(clamped);
        if !(text_kit_range.length > 0) {
            return;
        }
        let storage = &self.ivars().content_storage;
        let origin = storage.documentRange().location();
        let (Some(start), Some(end)) = (
            storage.locationFromLocation_withOffset(&origin, text_kit_range.location),
            storage.locationFromLocation_withOffset(&origin, text_kit_range.upper_bound()),
        ) else {
            return;
        };
        let Some(text_range) = NSTextRange::initWithLocation_endLocation(NSTextRange::alloc(), &start, Some(&end)) else {
            return;
        };
        let layout_manager = &self.ivars().markdown_layout_manager;
        layout_manager.ensureLayoutForRange(&text_range);

        let path = NSBezierPath::bezierPath();
        path.setLineWidth(1.0);
        let container_origin = self.textContainerOrigin();
        let block = block2::StackBlock::new(
            |_range: *mut NSTextRange, segment: CGRect, _baseline: CGFloat, _container: NonNull<NSTextContainer>| -> Bool {
                let y = segment.min_y() + container_origin.y - 1.5;
                path.moveToPoint(NSPoint::new(segment.min_x() + container_origin.x, y));
                path.lineToPoint(NSPoint::new(segment.max_x() + container_origin.x, y));
                Bool::YES
            },
        );
        layout_manager.enumerateTextSegmentsInRange_type_options_usingBlock(
            &text_range,
            objc2_app_kit::NSTextLayoutManagerSegmentType::Standard,
            objc2_app_kit::NSTextLayoutManagerSegmentOptions(0),
            &block,
        );
        if !(path.elementCount() > 0) {
            return;
        }
        self.style_sheet().link.setStroke();
        path.stroke();
    }

    fn apply_invisibles(&self, scopes: Option<&[NSRange]>) {
        let Some(storage) = self.text_storage() else { return };
        let ivars = self.ivars();
        if !ivars.configuration.borrow().show_invisibles {
            if !(ivars.invisibles_applied.get() && storage.length() > 0) {
                return;
            }
            storage.removeAttribute_range(attribute_keys::dr_invisible(), objc2_foundation::NSRange::new(0, storage.length()));
            ivars.invisibles_applied.set(false);
            return;
        }
        if !(storage.length() > 0) {
            return;
        }
        let ranges: Vec<NSRange> = match scopes {
            Some(scopes) if !scopes.is_empty() => scopes.to_vec(),
            _ => vec![NSRange::new(0, storage.length() as isize)],
        };
        let flag = NSNumber::new_bool(true);
        storage.beginEditing();
        for range in ranges {
            storage.removeAttribute_range(attribute_keys::dr_invisible(), ns(range));
            let text = storage.string();
            let end = range.upper_bound().min(text.length() as isize);
            let mut offset = range.location.max(0);
            while offset < end {
                let value = text.characterAtIndex(offset as usize);
                if value == 0x20 || value == 0x09 {
                    unsafe {
                        storage.addAttribute_value_range(
                            attribute_keys::dr_invisible(),
                            &flag,
                            objc2_foundation::NSRange::new(offset as usize, 1),
                        )
                    };
                }
                offset += 1;
            }
        }
        storage.endEditing();
        ivars.invisibles_applied.set(true);
    }

    fn draw_scoped_source_background(&self, dirty_rect: NSRect) {
        if !matches!(self.source_focus(), SourceFocus::Scoped(_)) {
            return;
        }
        let Some(band) = self.source_focus_band_rect().filter(|band| band.intersects(dirty_rect)) else { return };
        let style_sheet = self.style_sheet();
        style_sheet.surface.setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(band, 6.0, 6.0).fill();
        style_sheet.rule.setStroke();
        let rule = NSBezierPath::bezierPath();
        rule.moveToPoint(NSPoint::new(band.min_x(), band.min_y() + 24.0));
        rule.lineToPoint(NSPoint::new(band.max_x(), band.min_y() + 24.0));
        rule.setLineWidth(1.0);
        rule.stroke();

        let font = NSFont::systemFontOfSize_weight(11.0, unsafe { NSFontWeightMedium });
        attributed_string("Markdown", &[(keys::font(), &font), (keys::foreground_color(), &style_sheet.text_secondary)])
            .drawAtPoint(NSPoint::new(band.min_x() + 8.0, band.min_y() + 5.0));
        let done = attributed_string("Done", &[(keys::font(), &font), (keys::foreground_color(), &style_sheet.accent)]);
        let size = done.size();
        done.drawAtPoint(NSPoint::new(band.max_x() - size.width - 8.0, band.min_y() + 5.0));
    }

    pub(crate) fn source_focus_band_rect(&self) -> Option<NSRect> {
        let range = self.source_focus().range()?;
        let start = self.rect_for_offset(range.location)?;
        let end = self.rect_for_offset(range.location.max(range.upper_bound() - 1))?;
        let horizontal_inset: CGFloat = 8.0;
        let x = smax(0.0, self.textContainerOrigin().x - horizontal_inset);
        let style_sheet = self.style_sheet();
        Some(rect(
            x,
            smax(0.0, start.min_y() - 28.0),
            smin(smax(0.0, self.bounds().size.width - x), self.column_width() + horizontal_inset * 2.0),
            smax(style_sheet.line_height + 36.0, end.max_y() - start.min_y() + 40.0),
        ))
    }

    pub(crate) fn source_focus_done_rect(&self) -> Option<NSRect> {
        let band = self.source_focus_band_rect()?;
        Some(rect(band.max_x() - 56.0, band.min_y(), 56.0, 24.0))
    }

    /// The container is the reading measure plus a trailing bleed lane.
    pub fn column_width(&self) -> CGFloat {
        self.style_sheet().measure_width + render_metrics::CODE_BLEED
    }

    fn apply_measure(&self) {
        let column_width = self.column_width();
        let style_sheet = self.style_sheet();
        if let Some(container) = unsafe { self.textContainer() } {
            container.setSize(CGSize::new(column_width, CGFloat::MAX));
        }
        self.ivars().fragment_context.content_width.set(column_width);
        self.setMinSize(NSSize::new(column_width, 0.0));
        self.setMaxSize(NSSize::new(column_width + render_metrics::REVEAL_SLACK * 2.0, CGFloat::MAX));
        self.setBackgroundColor(&style_sheet.background);
        self.setInsertionPointColor(Some(if self.mode().policy().shows_insertion_point {
            &style_sheet.accent
        } else {
            &style_sheet.text
        }));
        let attributes = NSDictionary::from_slices(&[keys::background_color()], &[style_sheet.selection.as_ref() as &AnyObject]);
        unsafe { self.setSelectedTextAttributes(&attributes) };
    }

    pub fn apply_responsive_measure(&self, width: CGFloat) {
        if !(width > 100.0) {
            return;
        }
        let previous_width = unsafe { self.textContainer() }.map_or(0.0, |container| container.size().width);
        let anchor = self.capture_viewport_anchor();
        if let Some(container) = unsafe { self.textContainer() } {
            container.setSize(CGSize::new(width, CGFloat::MAX));
        }
        self.ivars().fragment_context.content_width.set(width);
        self.setMinSize(NSSize::new(width, 0.0));
        self.setMaxSize(NSSize::new(width + render_metrics::REVEAL_SLACK * 2.0, CGFloat::MAX));
        if (previous_width - width).abs() > 0.5 {
            self.invalidate_all_fragments();
            self.request_content_resize(ContentResizeRequest::Viewport, Some(anchor), None);
        }
    }

    fn apply_mode_chrome(&self) {
        let style_sheet = self.style_sheet();
        let mode = self.mode();
        self.setEditable(mode.policy().shows_insertion_point);
        let _: () = unsafe { msg_send![self, updateDragTypeRegistration] };
        self.setSelectable(true);
        self.setInsertionPointColor(Some(if mode.policy().shows_insertion_point {
            &style_sheet.accent
        } else {
            &style_sheet.text
        }));
        let font = if mode == RenderMode::Source { style_sheet.mono_font(None) } else { style_sheet.body_font() };
        let attributes = NSDictionary::from_slices(
            &[keys::font(), keys::foreground_color()],
            &[font.as_ref() as &AnyObject, style_sheet.text.as_ref()],
        );
        unsafe { self.setTypingAttributes(&attributes) };
        self.refresh_source_accessibility();
    }

    fn apply_typographic_substitution(&self) {
        let enabled = self.ivars().configuration.borrow().typographic_substitution;
        self.setAutomaticQuoteSubstitutionEnabled(enabled);
        self.setAutomaticDashSubstitutionEnabled(enabled);
    }

    pub(crate) fn refresh_source_accessibility(&self) {
        if self.source_focus() == SourceFocus::None {
            self.setAccessibilityLabel(Some(&NSString::from_str("Document editor")));
            self.setAccessibilityHelp(Some(&NSString::from_str(
                "Rendered Markdown document. Move the caret to edit in place.",
            )));
        } else {
            self.setAccessibilityLabel(Some(&NSString::from_str("Markdown source editor")));
            self.setAccessibilityHelp(Some(&NSString::from_str(
                "Raw Markdown is visible. Press Escape or choose Done to return to the document.",
            )));
        }
    }

    pub fn invalidate_all_fragments(&self) {
        self.invalidate_fragments(None);
    }

    /// Scoped layout invalidation (§12).
    pub fn invalidate_fragments(&self, source_range: Option<NSRange>) {
        self.ivars().last_fragment_invalidation_range_for_testing.set(Some(source_range));
        let layout_manager = &self.ivars().markdown_layout_manager;
        let Some(source_range) = source_range else {
            layout_manager.invalidateLayoutForRange(&layout_manager.documentRange());
            self.setNeedsDisplay(true);
            self.gutter_rail_needs_display();
            return;
        };
        if !(source_range.length > 0) {
            return;
        }
        let text_kit = self.current_display_map().text_kit_range_for_source(source_range);
        let storage = &self.ivars().content_storage;
        let origin = storage.documentRange().location();
        let range = match (
            storage.locationFromLocation_withOffset(&origin, text_kit.location),
            storage.locationFromLocation_withOffset(&origin, text_kit.upper_bound()),
        ) {
            (Some(start), Some(end)) => NSTextRange::initWithLocation_endLocation(NSTextRange::alloc(), &start, Some(&end)),
            _ => None,
        };
        let Some(range) = range else {
            layout_manager.invalidateLayoutForRange(&layout_manager.documentRange());
            self.setNeedsDisplay(true);
            return;
        };
        layout_manager.invalidateLayoutForRange(&range);
        self.setNeedsDisplay(true);
        self.gutter_rail_needs_display();
    }

    fn view_did_move_to_window(&self) {
        let ivars = self.ivars();
        if let Some(driver) = ivars.motion_driver.borrow().as_ref() {
            driver.view_did_move_to_window(self.window().as_deref());
        }
        self.install_hover_tracking();
        if self.window().is_none() {
            self.interrupt_animated_scroll();
        }
        let center = NSNotificationCenter::defaultCenter();
        if let Some(observer) = ivars.scroll_observer.borrow_mut().take() {
            unsafe { center.removeObserver(observer.as_ref()) };
        }
        if let Some(observer) = ivars.resign_key_observer.borrow_mut().take() {
            unsafe { center.removeObserver(observer.as_ref()) };
        }
        if let Some(window) = self.window() {
            let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
            let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
                if let Some(view) = weak.load() {
                    view.clear_hover_state();
                }
            });
            let observer = unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(NSWindowDidResignKeyNotification),
                    Some(&window),
                    Some(&NSOperationQueue::mainQueue()),
                    &block,
                )
            };
            *ivars.resign_key_observer.borrow_mut() = Some(observer);
        }
        if let Some(scroll_view) = self.scroll_view() {
            let clip = scroll_view.contentView();
            clip.setPostsBoundsChangedNotifications(true);
            let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
            let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
                let Some(view) = weak.load() else { return };
                view.scroll_bounds_changed();
            });
            let observer = unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(NSViewBoundsDidChangeNotification),
                    Some(&clip),
                    Some(&NSOperationQueue::mainQueue()),
                    &block,
                )
            };
            *ivars.scroll_observer.borrow_mut() = Some(observer);
        }
    }

    fn scroll_bounds_changed(&self) {
        let ivars = self.ivars();
        let mtm = self.mtm();
        if let Some(event) = objc2_app_kit::NSApplication::sharedApplication(mtm).currentEvent() {
            use objc2_app_kit::NSEventType;
            let kind = event.r#type();
            if [NSEventType::ScrollWheel, NSEventType::KeyDown, NSEventType::LeftMouseDown, NSEventType::LeftMouseDragged]
                .contains(&kind)
            {
                ivars.viewport_repair_generation.set(ivars.viewport_repair_generation.get().wrapping_add(1));
            }
        }
        let generation = ivars.scroll_coalesce_generation.get().wrapping_add(1);
        ivars.scroll_coalesce_generation.set(generation);
        let existing = ivars.scroll_coalesce_work_item.borrow_mut().take();
        let weak: ObjcWeak<MarkdownTextView> = ObjcWeak::from(self);
        let item = WorkItem::new(move || {
            let Some(view) = weak.load() else { return };
            if view.ivars().scroll_coalesce_generation.get() != generation {
                return;
            }
            *view.ivars().scroll_coalesce_work_item.borrow_mut() = None;
            if let Some(delegate) = view.delegate() {
                delegate.did_scroll(&view);
            }
            view.handle_scroll();
        });
        *ivars.scroll_coalesce_work_item.borrow_mut() = Some(item.clone());
        if let Some(existing) = existing {
            existing.cancel();
        }
        item.dispatch_main();
        self.gutter_rail_needs_display();
    }

    /// Called once per coalesced scroll event.
    fn handle_scroll(&self) {
        let Some(scroll_view) = self.scroll_view() else { return };
        let ivars = self.ivars();
        let viewport_height = scroll_view.contentView().bounds().size.height;
        let visible_max_y = scroll_view.documentVisibleRect().max_y();
        let repair_slack = smax(24.0, viewport_height * 0.15);
        let pinned_to_bottom = visible_max_y >= self.frame().size.height - repair_slack;
        if ivars.pending_shrink_repair.get() && !pinned_to_bottom {
            ivars.pending_shrink_repair.set(false);
            if let Some(item) = ivars.pending_shrink_repair_work_item.borrow_mut().take() {
                item.cancel();
            }
            self.request_content_resize(ContentResizeRequest::ScrollRepair, None, None);
            return;
        }
        if !(ivars.resize_needs_repair.get() && pinned_to_bottom) {
            return;
        }
        self.request_content_resize(ContentResizeRequest::ScrollRepair, None, None);
    }

    fn install_hover_tracking(&self) {
        refresh_tracking_area(
            self,
            &self.ivars().hover_tracking,
            NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::CursorUpdate
                | NSTrackingAreaOptions::ActiveInKeyWindow
                | NSTrackingAreaOptions::InVisibleRect,
        );
    }
}

/// `NSAccessibility.post(element:notification: .announcementRequested, …)`.
fn post_announcement(element: &MarkdownTextView, announcement: &str) {
    let key = unsafe { objc2_app_kit::NSAccessibilityAnnouncementKey };
    let value = NSString::from_str(announcement);
    let user_info = NSDictionary::from_slices(&[key], &[value.as_ref() as &AnyObject]);
    unsafe {
        objc2_app_kit::NSAccessibilityPostNotificationWithUserInfo(
            element,
            objc2_app_kit::NSAccessibilityAnnouncementRequestedNotification,
            Some(&user_info),
        )
    };
}

/// `NSValue(range:)`.
fn unsafe_value_with_range(range: objc2_foundation::NSRange) -> Retained<NSValue> {
    // SAFETY: a plain value box.
    unsafe { NSValue::valueWithRange(range) }
}
